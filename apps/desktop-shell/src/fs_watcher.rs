//! 외부 파일시스템 변경 감지 — `notify-rs` cross-platform watcher (M10 Phase 2 / ADR-036).
//!
//! mounted folder별로 watcher를 *공유*하지 않고 *글로벌 단일 watcher*에 path 추가/제거.
//! notify::RecommendedWatcher가 Windows의 ReadDirectoryChangesW / Linux inotify / macOS
//! FSEvents를 추상화. 이벤트 종류:
//! - Create — 새 파일/폴더 등장 (외부 또는 우리가 만든 것 — echo 구분은 echo_cache로)
//! - Modify(data) — 파일 내용 변경
//! - Remove — 파일/폴더 사라짐
//! - Modify(name) — rename (notify는 Remove + Create 시퀀스 또는 Both로 전달)
//!
//! main loop는 100ms 주기로 `drain()`을 호출해 누적 이벤트를 받아 적절한 mount/destroy/
//! SetState broadcast로 변환. echo_cache가 우리 자신의 fs op (create_file/save 등) 직후
//! 같은 path 이벤트를 1초 동안 무시 — *외부* 변경만 처리되도록 보장.
//!
//! **동기 mpsc vs tokio 통합:** notify-rs는 OS thread에서 callback을 호출하므로 std mpsc
//! sender가 자연스럽다. main loop는 *interval tick + try_recv* 패턴으로 폴링 — 별도
//! tokio mpsc forwarder thread를 띄우지 않아 구조가 단순. 100ms 폴링 지연은 사용자 인식
//! 가능 임계 (보통 200ms+) 아래라 UX 영향 미미.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// 외부 fs 변경의 정규화된 분류 — notify::EventKind를 main이 다루기 쉽게 단순화.
#[derive(Debug, Clone)]
pub enum FsChange {
    Created(PathBuf),
    Modified(PathBuf),
    Removed(PathBuf),
}

/// 글로벌 watcher + rx + echo 방지 캐시.
///
/// `watcher`는 Drop 시 자동으로 모든 watch를 해제 — 명시적 unwatch 불필요.
/// `rx`는 std::sync::mpsc — notify callback이 OS thread에서 send하므로 sync 채널이 자연.
/// `echo_cache`는 자체 fs op 직후 같은 path 이벤트를 ECHO_WINDOW 동안 무시.
pub struct FsWatcher {
    /// notify의 RecommendedWatcher — Drop 시 모든 watch 자동 해제.
    /// 필드는 *살아있어야* watcher가 동작하므로 명시 보존 (사용 안 한다는 경고는 #[allow]).
    #[allow(dead_code)]
    watcher: RecommendedWatcher,
    rx: mpsc::Receiver<notify::Result<Event>>,
    /// 자체 fs op 직후 *같은 path* 이벤트를 무시하기 위한 캐시.
    /// 1초 이내면 echo로 간주 — 우리가 막 만든 파일을 notify가 알려도 무시.
    echo_cache: Mutex<Vec<(PathBuf, Instant)>>,
}

/// echo 무시 윈도우. notify가 OS에서 이벤트를 받아 우리에게 전달하기까지의 지연 + drain
/// 폴링 주기 (100ms)를 합쳐 *대부분 1초 이내*에 echo가 도착. 더 길게 잡으면 *외부* 변경
/// 중 일부가 echo로 오인될 위험 ↑.
const ECHO_WINDOW: Duration = Duration::from_millis(1000);

impl FsWatcher {
    /// 새 watcher + 채널. 호출자는 watch(path)로 감시 시작.
    pub fn new() -> notify::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |res| {
            // 채널이 닫혔으면 (main loop 종료) silently drop — 종료 시점 노이즈 방지.
            let _ = tx.send(res);
        })?;
        Ok(Self { watcher, rx, echo_cache: Mutex::new(Vec::new()) })
    }

    /// 특정 디렉터리 감시 시작 (non-recursive — 직계 children만).
    ///
    /// 재귀 감시는 cwd가 매우 크면 (예: node_modules) 비현실적이라 *lazy_expand된 폴더만*
    /// 등록. 같은 path를 중복 watch하면 notify가 자체적으로 idempotent 처리.
    pub fn watch(&mut self, dir: &Path) -> notify::Result<()> {
        self.watcher.watch(dir, RecursiveMode::NonRecursive)
    }

    /// 자체 fs op 직후 호출 — 이 path의 이벤트는 echo로 간주해 무시.
    ///
    /// folder_ops::create_file_in / file_ops::delete_file / file_write::save 등 우리가
    /// 디스크를 건드린 *직후* 호출. notify가 그 변경을 ECHO_WINDOW 안에 보고하면 자동 skip.
    pub fn mark_self_op(&self, path: PathBuf) {
        let mut cache = self.echo_cache.lock().expect("echo_cache poisoned");
        cache.push((path, Instant::now()));
        // 1초 지난 entry 정리 — 무한 누적 방지.
        cache.retain(|(_, t)| t.elapsed() < ECHO_WINDOW);
    }

    /// path가 최근 self_op로 표시되었으면 true (= 이벤트 무시).
    pub fn is_echo(&self, path: &Path) -> bool {
        let mut cache = self.echo_cache.lock().expect("echo_cache poisoned");
        cache.retain(|(_, t)| t.elapsed() < ECHO_WINDOW);
        cache.iter().any(|(p, _)| p == path)
    }

    /// non-blocking poll — 누적된 이벤트를 가져온다. Echo는 자동 필터.
    ///
    /// main loop가 tokio::time::interval (100ms)로 호출. 반환 Vec는 자연 순서 (수신 순).
    /// EventKind::Access 등 *우리가 신경 쓰지 않는* 이벤트는 skip.
    pub fn drain(&self) -> Vec<FsChange> {
        let mut out = Vec::new();
        while let Ok(res) = self.rx.try_recv() {
            let event = match res {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("[fs_watcher] event error: {}", e);
                    continue;
                }
            };
            for path in event.paths {
                if self.is_echo(&path) {
                    continue;
                }
                let change = match event.kind {
                    EventKind::Create(_) => FsChange::Created(path),
                    EventKind::Modify(_) => FsChange::Modified(path),
                    EventKind::Remove(_) => FsChange::Removed(path),
                    _ => continue, // Access/Other — 무시.
                };
                out.push(change);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn watcher_detects_create() {
        let mut watcher = FsWatcher::new().expect("watcher 생성");
        let dir = tempdir().unwrap();
        watcher.watch(dir.path()).expect("watch 등록");
        // notify 초기화 직후 약간의 settle time 필요 — OS에 따라 다름.
        std::thread::sleep(Duration::from_millis(200));

        let p = dir.path().join("new.txt");
        std::fs::write(&p, "hi").unwrap();
        // OS event 지연 — 짧게 대기. ReadDirectoryChangesW는 보통 100ms 내.
        std::thread::sleep(Duration::from_millis(500));

        let events = watcher.drain();
        // 일부 백엔드는 새 파일 등장을 Created가 아닌 Modified로 보고 — 둘 다 허용.
        // canonical path 일치 검사 (Windows는 short path / long path 차이 가능 — file_name만 비교).
        let p_name = p.file_name().unwrap();
        let matched = events.iter().any(|e| match e {
            FsChange::Created(p2) | FsChange::Modified(p2) => p2.file_name() == Some(p_name),
            _ => false,
        });
        assert!(matched, "Created/Modified 이벤트가 도착해야 — got: {:?}", events);
    }

    #[test]
    fn echo_cache_filters_self_op() {
        let watcher = FsWatcher::new().expect("watcher 생성");
        let p = PathBuf::from("D:/test/echo/x");
        assert!(!watcher.is_echo(&p));
        watcher.mark_self_op(p.clone());
        assert!(watcher.is_echo(&p));
    }

    #[test]
    fn echo_cache_expires() {
        let watcher = FsWatcher::new().expect("watcher 생성");
        let p = PathBuf::from("D:/test/echo/expire");
        watcher.mark_self_op(p.clone());
        assert!(watcher.is_echo(&p));
        std::thread::sleep(Duration::from_millis(1100));
        assert!(!watcher.is_echo(&p));
    }
}
