//! Windows JobObject 래퍼 — long-running process를 *손주 process 포함* cascade kill.
//!
//! Windows에서 `TerminateProcess`는 *부모만* kill → npm.cmd → node → esbuild 사슬에서
//! 손주 process가 orphan화. `JobObject + JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`로
//! job handle close 또는 `TerminateJobObject` 시 *모든 descendant* 동시 kill.
//!
//! Unix는 stub (KI-027 — v2에서 setsid + killpg).

#[cfg(windows)]
mod windows_impl {
    use std::io;
    use std::ptr;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

    /// JobObject + 강제 kill 정책.
    ///
    /// Drop 시 CloseHandle → JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE 효력으로 descendant 전체 kill.
    /// 명시 `terminate()`는 즉시 kill이 필요할 때 (사용자 X 닫기 / AI terminate invoke).
    pub struct JobHandle(HANDLE);

    impl JobHandle {
        /// 새 JobObject 생성 + KILL_ON_JOB_CLOSE 플래그 설정.
        pub fn create() -> io::Result<Self> {
            unsafe {
                let h = CreateJobObjectW(ptr::null(), ptr::null());
                if h.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    h,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    let e = io::Error::last_os_error();
                    CloseHandle(h);
                    return Err(e);
                }
                Ok(JobHandle(h))
            }
        }

        /// 주어진 PID의 process를 이 job에 attach.
        ///
        /// child가 *CREATE_SUSPENDED*로 spawn된 직후 호출하고 ResumeThread해야
        /// child가 spawn 후 즉시 fork한 손주가 job에 포함된다.
        pub fn assign_process(&self, pid: u32) -> io::Result<()> {
            unsafe {
                let proc_handle = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
                if proc_handle.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let ok = AssignProcessToJobObject(self.0, proc_handle);
                CloseHandle(proc_handle);
                if ok == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            }
        }

        /// 즉시 모든 process kill (exit code 1).
        pub fn terminate(&self) -> io::Result<()> {
            unsafe {
                if TerminateJobObject(self.0, 1) == 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        }
    }

    impl Drop for JobHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    unsafe impl Send for JobHandle {}
    unsafe impl Sync for JobHandle {}
}

#[cfg(not(windows))]
mod unix_stub {
    use std::io;

    /// Unix stub — M13 v1은 Windows 전용 (KI-027).
    pub struct JobHandle;

    impl JobHandle {
        pub fn create() -> io::Result<Self> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "M13 v1 ConsoleWindow JobObject는 Windows 전용 — Unix는 v2 setsid+killpg (KI-027)",
            ))
        }
        pub fn assign_process(&self, _pid: u32) -> io::Result<()> {
            unreachable!("JobHandle::create가 이미 실패해야 함")
        }
        pub fn terminate(&self) -> io::Result<()> {
            unreachable!("JobHandle::create가 이미 실패해야 함")
        }
    }
    unsafe impl Send for JobHandle {}
    unsafe impl Sync for JobHandle {}
}

#[cfg(windows)]
pub use windows_impl::JobHandle;
#[cfg(not(windows))]
pub use unix_stub::JobHandle;

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn create_and_drop_job_handle() {
        let job = JobHandle::create().expect("JobObject 생성 실패");
        drop(job);
    }

    #[test]
    fn terminate_empty_job_returns_ok() {
        let job = JobHandle::create().expect("JobObject 생성 실패");
        job.terminate().expect("빈 job terminate OK");
    }

    #[test]
    fn assign_real_process_then_terminate() {
        use std::process::Command;
        let job = JobHandle::create().expect("JobObject 생성 실패");
        let child = Command::new("cmd")
            .args(["/c", "ping", "-n", "30", "127.0.0.1"])
            .spawn()
            .expect("spawn 실패");
        let pid = child.id();
        job.assign_process(pid).expect("assign 실패");
        std::thread::sleep(std::time::Duration::from_millis(200));
        job.terminate().expect("terminate 실패");
        let mut c = child;
        let status = c.wait().expect("wait 실패");
        assert!(!status.success(), "terminate 후 exit code 0 안 됨");
    }
}
