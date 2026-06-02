//! AI가 부여받은 *디렉터리 grant* in-memory 캐시 (M10 Phase 1 / ADR-036).
//!
//! 한 dir에 대해 [허용] Dialog를 한 번 처리하면 그 dir 안 후속 write/create/rename은
//! confirm 없이 통과. 세션 = desktop-shell process 한 번 실행 — AI 채팅 세션 (`/ai start
//! ... /exit`) 무관. process 종료 시 자연 reset.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 경로를 비교용 정규화 컴포넌트 리스트로. **호스트 경로(`D:\`)와 Linux 경로(`/`) 모두 인지** —
/// `std::path`는 VM(Linux)에서 `D:\a\b`를 *단일 컴포넌트*로 취급해 prefix 매칭·parent가 깨지므로
/// 직접 문자열 분해한다. 호스트 경로는 `\`/`/` 둘 다 구분자 + 소문자화(Windows 대소문자 무시),
/// Linux는 `/` 구분자 + 그대로. `.`/`..` 어휘적 해소로 path traversal 차단(탈출 경로는 컴포넌트가
/// 달라져 granted와 prefix 불일치 → Dialog). 실제 write의 symlink/canonicalize 방어는 host bridge.
fn norm_components(path: &str) -> Vec<String> {
    let host = crate::host_bridge_client::is_host_path(path);
    let normalized = if host { path.to_lowercase().replace('\\', "/") } else { path.to_string() };
    let mut out: Vec<String> = Vec::new();
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            p => out.push(p.to_string()),
        }
    }
    out
}

/// 경로의 부모 디렉터리 — 호스트 경로(`\`)와 Linux 경로(`/`) 모두 인지. `std::path::parent()`는
/// VM(Linux)에서 `D:\a\b`를 단일 컴포넌트로 봐 빈 부모를 반환하므로 마지막 구분자에서 직접 자른다.
/// grant-on-approve가 *파일의 부모 dir*을 grant할 때 사용.
pub fn parent_of(path: &Path) -> Option<PathBuf> {
    let s = path.to_string_lossy();
    let cut = s.rfind(['/', '\\'])?;
    // 루트/드라이브 직속은 부모 없음 — bare `D:`나 `/` *전체*를 워크스페이스로 grant하지 않는다
    // (권한 과확장 방지): `/x`(cut==0), `D:\x`(cut==2, 드라이브 designator 직후).
    if cut == 0 || (cut == 2 && s.as_bytes().get(1) == Some(&b':')) {
        return None;
    }
    Some(PathBuf::from(&s[..cut]))
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
    /// **보안:** 양쪽을 `norm_components`로 정규화(`..`/`.` 해소 + 호스트 경로 인지) 후 컴포넌트
    /// prefix 비교 — 그렇지 않으면 `/a/../../etc`나 `D:\a\..\..\x`가 워크스페이스를 탈출(traversal).
    /// `dir`은 파일 경로여도 됨 — granted dir의 하위면 true(prefix). 실제 write의 symlink 방어는 host bridge.
    pub fn contains(&self, dir: &Path) -> bool {
        let dir_c = norm_components(&dir.to_string_lossy());
        if dir_c.is_empty() {
            return false;
        }
        self.inner.lock().expect("GrantedDirs poisoned").iter().any(|g| {
            let g_c = norm_components(&g.to_string_lossy());
            !g_c.is_empty() && dir_c.len() >= g_c.len() && dir_c[..g_c.len()] == g_c[..]
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
    fn contains_host_path_prefix() {
        // 호스트 경로(D:\) — VM(Linux)에서 std::path가 단일 컴포넌트로 봐도 host-aware 매칭.
        let g = GrantedDirs::new();
        g.insert(PathBuf::from("D:\\react_project1"));
        assert!(g.contains(Path::new("D:\\react_project1")));
        // 하위 파일/폴더 — 워크스페이스 핵심 케이스.
        assert!(g.contains(Path::new("D:\\react_project1\\src\\App.css")));
        assert!(g.contains(Path::new("D:\\react_project1\\src")));
        // 대소문자/구분자 무시 (Windows).
        assert!(g.contains(Path::new("d:/react_project1/src/main.jsx")));
        // 형제·무관 경로는 false.
        assert!(!g.contains(Path::new("D:\\other")));
        assert!(!g.contains(Path::new("C:\\react_project1")));
        // traversal 탈출 차단.
        assert!(!g.contains(Path::new("D:\\react_project1\\..\\..\\Windows\\System32")));
    }

    #[test]
    fn parent_of_handles_host_and_linux() {
        assert_eq!(
            parent_of(Path::new("D:\\react_project1\\src\\App.css")),
            Some(PathBuf::from("D:\\react_project1\\src"))
        );
        assert_eq!(parent_of(Path::new("/a/b/c")), Some(PathBuf::from("/a/b")));
        // 루트 직속은 부모 없음.
        assert_eq!(parent_of(Path::new("/x")), None);
        // 보안: 드라이브 직속 파일의 부모는 None — bare D:를 워크스페이스로 grant하지 않음.
        assert_eq!(parent_of(Path::new("D:\\file.txt")), None);
        assert_eq!(parent_of(Path::new("C:\\App.js")), None);
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
