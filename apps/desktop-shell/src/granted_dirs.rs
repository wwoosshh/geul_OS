//! AI가 부여받은 *디렉터리 grant* in-memory 캐시 (M10 Phase 1 / ADR-036).
//!
//! 한 dir에 대해 [허용] Dialog를 한 번 처리하면 그 dir 안 후속 write/create/rename은
//! confirm 없이 통과. 세션 = desktop-shell process 한 번 실행 — AI 채팅 세션 (`/ai start
//! ... /exit`) 무관. process 종료 시 자연 reset.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// `..`/`.`를 *어휘적으로*(파일 존재 무관) 해소해 path traversal을 차단한다.
/// `D:\proj\..\..\x` → `D:\x`. 절대 prefix(드라이브/루트)나 첫 Normal 이전의 `..`는 pop하지
/// 않고 그대로 남겨 — 루트 탈출을 시도한 경로는 정규화 후에도 `..`를 포함하므로 정상 granted
/// dir(깨끗한 절대경로)과 prefix 매칭되지 않는다. `canonicalize`와 달리 미존재 경로(새로 만들
/// 파일)에도 동작. 실제 write 시 symlink/canonicalize 방어는 host bridge가 별도 수행.
fn normalize_lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[derive(Default)]
pub struct GrantedDirs {
    /// 활성 grant 전체 (세션 한정 + 영속 워크스페이스 합집합).
    inner: Mutex<HashSet<PathBuf>>,
    /// `~/.geulos/workspaces.json`에 저장되는 영속 워크스페이스 부분집합.
    persistent: Mutex<HashSet<PathBuf>>,
}

impl GrantedDirs {
    pub fn new() -> Self {
        Self::default()
    }

    /// 특정 dir에 대한 grant 여부 — **prefix 매칭** (서버 `is_granted`와 일치).
    ///
    /// dir 자신이 granted거나, granted dir의 하위면 true. 예: grant `/a` → `/a/b` true,
    /// `/c` false. 워크스페이스 모델("한 번 승인하면 하위 전체 무프롬프트")의 전제.
    ///
    /// **보안:** 양쪽을 `normalize_lexical`로 `..`/`.` 해소 후 비교 — 그렇지 않으면
    /// `/a/../../etc` 가 component상 `/a`로 시작해 워크스페이스를 탈출(path traversal).
    /// 정규화 후에도 `..`가 남는(루트 탈출 시도) 경로는 정상 granted dir와 매칭되지 않아
    /// false → Dialog로 떨어진다(안전). 실제 write의 symlink/canonicalize 방어는 host bridge.
    pub fn contains(&self, dir: &Path) -> bool {
        let dir = normalize_lexical(dir);
        self.inner.lock().expect("GrantedDirs poisoned").iter().any(|g| {
            let g = normalize_lexical(g);
            dir == g || dir.starts_with(&g)
        })
    }

    /// 세션 한정 dir grant 추가 (inner만 — 디스크 미저장). 이미 있으면 무동작.
    pub fn insert(&self, dir: PathBuf) {
        self.inner.lock().expect("GrantedDirs poisoned").insert(dir);
    }

    /// 영속 워크스페이스 grant 추가 — inner + persistent에 넣고 디스크에 저장.
    pub fn insert_persistent(&self, dir: PathBuf) {
        self.inner.lock().expect("GrantedDirs poisoned").insert(dir.clone());
        let snapshot = {
            let mut p = self.persistent.lock().expect("GrantedDirs persistent poisoned");
            p.insert(dir);
            p.clone()
        };
        save_persisted(&snapshot);
    }

    /// 현재 grant된 모든 dir 목록 (UI 표시·테스트용).
    pub fn list(&self) -> Vec<PathBuf> {
        let g = self.inner.lock().expect("GrantedDirs poisoned");
        g.iter().cloned().collect()
    }

    /// 영속(워크스페이스) grant 목록.
    pub fn list_persistent(&self) -> Vec<PathBuf> {
        let p = self.persistent.lock().expect("GrantedDirs persistent poisoned");
        p.iter().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("GrantedDirs poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// dir grant 제거 — inner + persistent에서 제거 후 (persistent 변경 시) 디스크에 저장.
    pub fn remove(&self, path: &std::path::Path) {
        self.inner.lock().expect("GrantedDirs poisoned").remove(path);
        let removed = {
            let mut p = self.persistent.lock().expect("GrantedDirs persistent poisoned");
            p.remove(path)
        };
        if removed {
            let snapshot = self.persistent.lock().expect("GrantedDirs persistent poisoned").clone();
            save_persisted(&snapshot);
        }
    }

    /// 시작 시 호출 — `~/.geulos/workspaces.json`을 읽어 inner + persistent를 채운다.
    /// 반환: 로드된 path들 (호출자가 각각 GrantUpdate(Add) wire 송신용). best-effort
    /// (파일 없음/파싱 실패 → 빈 vec).
    pub fn load_persisted(&self) -> Vec<PathBuf> {
        let loaded = load_persisted_from_disk();
        {
            let mut inner = self.inner.lock().expect("GrantedDirs poisoned");
            let mut p = self.persistent.lock().expect("GrantedDirs persistent poisoned");
            for d in &loaded {
                inner.insert(d.clone());
                p.insert(d.clone());
            }
        }
        loaded
    }
}

/// `~/.geulos/workspaces.json` 경로. home 미발견 시 "." fallback (ai_session.rs와 동일 패턴).
fn workspaces_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".geulos").join("workspaces.json")
}

/// 영속 set을 JSON 배열(문자열)로 저장. best-effort — 실패는 log만 (grant 자체는 메모리 유지).
fn save_persisted(set: &HashSet<PathBuf>) {
    let path = workspaces_path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("[granted_dirs] workspaces dir 생성 실패: {} — 영속화 skip", e);
            return;
        }
    }
    let list: Vec<String> = set.iter().map(|p| p.to_string_lossy().to_string()).collect();
    match serde_json::to_vec_pretty(&list) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                eprintln!("[granted_dirs] workspaces.json 쓰기 실패: {}", e);
            }
        }
        Err(e) => eprintln!("[granted_dirs] workspaces.json 직렬화 실패: {}", e),
    }
}

/// `~/.geulos/workspaces.json`을 읽어 PathBuf vec로. best-effort (없으면 빈 vec).
fn load_persisted_from_disk() -> Vec<PathBuf> {
    let path = workspaces_path();
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return Vec::new(), // 파일 없음 = 정상 (최초 실행).
    };
    match serde_json::from_slice::<Vec<String>>(&bytes) {
        Ok(list) => list.into_iter().map(PathBuf::from).collect(),
        Err(e) => {
            eprintln!("[granted_dirs] workspaces.json 파싱 실패: {} — 빈 목록으로 진행", e);
            Vec::new()
        }
    }
}

// ───── M11: wire 동기화 helper ─────

use geulos_core::ActorId;
use geulos_proto::{encode_frame, GrantOp, GrantUpdate};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

/// granted_dirs에 path 추가 + server-host에 GrantUpdate(Add) 송신.
///
/// 호출자 책임: actor는 grant를 받을 AI session actor (Dialog로 동의한 시점의
/// 활성 ai-bridge connection의 actor_id). stream은 desktop-shell의 server connection.
///
/// wire 송신 실패는 *경고만* 출력하고 local insert는 진행 — server-host가 재시작되면
/// desktop-shell도 곧 끊겨 재시작될 가능성 큼. 회복 정책은 v2.
pub async fn grant_dir(
    granted: &GrantedDirs,
    stream: &mut TcpStream,
    actor: &ActorId,
    path: std::path::PathBuf,
) -> std::io::Result<()> {
    granted.insert(path.clone());
    let msg = GrantUpdate {
        actor: actor.as_str().to_string(),
        path: path.to_string_lossy().to_string(),
        op: GrantOp::Add,
    };
    let body = serde_json::to_vec(&msg).map_err(std::io::Error::other)?;
    if let Err(e) = stream.write_all(&encode_frame(&body)).await {
        eprintln!("[granted_dirs] GrantUpdate(Add) wire 송신 실패: {} — local만 반영", e);
    }
    Ok(())
}

/// 영속 워크스페이스 grant — `grant_dir`의 wire 송신 + `insert_persistent`(디스크 저장).
///
/// `/workspace add` 전용 (사용자 CLI 경로). 세션 한정 `grant_dir`과 달리 desktop-shell
/// 재시작 후에도 유지된다.
pub async fn grant_dir_persistent(
    granted: &GrantedDirs,
    stream: &mut TcpStream,
    actor: &ActorId,
    path: std::path::PathBuf,
) -> std::io::Result<()> {
    granted.insert_persistent(path.clone());
    let msg = GrantUpdate {
        actor: actor.as_str().to_string(),
        path: path.to_string_lossy().to_string(),
        op: GrantOp::Add,
    };
    let body = serde_json::to_vec(&msg).map_err(std::io::Error::other)?;
    if let Err(e) = stream.write_all(&encode_frame(&body)).await {
        eprintln!(
            "[granted_dirs] GrantUpdate(Add, persistent) wire 송신 실패: {} — local만 반영",
            e
        );
    }
    Ok(())
}

/// 철회 — local + wire 동시.
pub async fn revoke_dir(
    granted: &GrantedDirs,
    stream: &mut TcpStream,
    actor: &ActorId,
    path: std::path::PathBuf,
) -> std::io::Result<()> {
    granted.remove(&path);
    let msg = GrantUpdate {
        actor: actor.as_str().to_string(),
        path: path.to_string_lossy().to_string(),
        op: GrantOp::Remove,
    };
    let body = serde_json::to_vec(&msg).map_err(std::io::Error::other)?;
    if let Err(e) = stream.write_all(&encode_frame(&body)).await {
        eprintln!("[granted_dirs] GrantUpdate(Remove) wire 송신 실패: {}", e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_then_contains() {
        let g = GrantedDirs::new();
        let d = PathBuf::from("/tmp/x");
        assert!(!g.contains(&d));
        g.insert(d.clone());
        assert!(g.contains(&d));
    }

    #[test]
    fn insert_duplicate_is_no_op() {
        let g = GrantedDirs::new();
        let d = PathBuf::from("/tmp/x");
        g.insert(d.clone());
        g.insert(d.clone());
        assert_eq!(g.len(), 1);
    }

    #[test]
    fn different_dirs_independent() {
        let g = GrantedDirs::new();
        g.insert(PathBuf::from("/a"));
        g.insert(PathBuf::from("/b"));
        assert!(g.contains(Path::new("/a")));
        assert!(g.contains(Path::new("/b")));
        assert!(!g.contains(Path::new("/c")));
        assert_eq!(g.len(), 2);
    }

    #[test]
    fn contains_is_prefix_match() {
        // grant /a → /a 자신과 모든 하위는 true, 형제 /c 는 false (서버 is_granted와 일치).
        let g = GrantedDirs::new();
        g.insert(PathBuf::from("/a"));
        assert!(g.contains(Path::new("/a")));
        assert!(g.contains(Path::new("/a/b")));
        assert!(g.contains(Path::new("/a/b/c")));
        assert!(!g.contains(Path::new("/c")));
        // prefix 문자열 우연 일치 회피 — component 단위 starts_with라 /ab 는 /a 하위 아님.
        assert!(!g.contains(Path::new("/ab")));
    }

    #[test]
    fn contains_blocks_dotdot_traversal() {
        // 보안: grant /a 후 /a/../../etc 같은 탈출 경로는 false여야 (path traversal 차단).
        let g = GrantedDirs::new();
        g.insert(PathBuf::from("/a"));
        // /a/x/../y 는 /a/y 로 정규화 → 여전히 granted.
        assert!(g.contains(Path::new("/a/x/../y")));
        // /a/../../etc 는 /etc 로 정규화 → granted 아님.
        assert!(!g.contains(Path::new("/a/../../etc")));
        assert!(!g.contains(Path::new("/a/../b")));
        // granted dir 자체가 .. 포함해도 정규화되어 비교 — /a/sub/.. == /a.
        assert!(g.contains(Path::new("/a/sub/..")));
    }

    #[test]
    fn insert_persistent_then_list_persistent() {
        // env var 격리 — 테스트 간 간섭/실제 HOME 오염 방지.
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("geulos-test-ws-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        with_temp_home(&tmp, || {
            let g = GrantedDirs::new();
            g.insert_persistent(PathBuf::from("/ws1"));
            g.insert_persistent(PathBuf::from("/ws2"));
            let mut got = g.list_persistent();
            got.sort();
            assert_eq!(got, vec![PathBuf::from("/ws1"), PathBuf::from("/ws2")]);
            // inner에도 반영 (contains true).
            assert!(g.contains(Path::new("/ws1/sub")));
        });
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn save_load_round_trip() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("geulos-test-ws-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        with_temp_home(&tmp, || {
            let g1 = GrantedDirs::new();
            g1.insert_persistent(PathBuf::from("/proj"));
            // 새 인스턴스에서 디스크 로드 → 같은 path 복원.
            let g2 = GrantedDirs::new();
            let loaded = g2.load_persisted();
            assert_eq!(loaded, vec![PathBuf::from("/proj")]);
            assert!(g2.contains(Path::new("/proj")));
            assert!(g2.contains(Path::new("/proj/src")));
            // remove → 디스크에서도 사라짐.
            g2.remove(Path::new("/proj"));
            let g3 = GrantedDirs::new();
            assert!(g3.load_persisted().is_empty());
        });
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── 테스트 헬퍼 ──

    // HOME/USERPROFILE env var는 프로세스 전역 — 동시 변경 시 race. 영속 테스트는 직렬화.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// HOME/USERPROFILE을 `base`로 임시 설정하고 클로저 실행 후 복원.
    /// `dirs::home_dir()`는 Windows에선 USERPROFILE, unix에선 HOME을 본다.
    fn with_temp_home<F: FnOnce()>(base: &Path, f: F) {
        std::fs::create_dir_all(base).unwrap();
        let old_home = std::env::var_os("HOME");
        let old_profile = std::env::var_os("USERPROFILE");
        std::env::set_var("HOME", base);
        std::env::set_var("USERPROFILE", base);
        f();
        match old_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match old_profile {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
    }
}
