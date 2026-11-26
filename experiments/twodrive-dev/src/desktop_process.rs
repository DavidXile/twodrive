//! Owned child-process lifetime on Windows.
#[cfg(windows)]
use anyhow::{Context, ensure};
#[cfg(windows)]
use std::process::Child;
#[cfg(windows)]
pub struct WindowsJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl WindowsJob {
    pub fn attach(child: &mut Child) -> anyhow::Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        // Non-inheritable handle: closing/crashing the controller kills all
        // processes in this job, including a test candidate's descendants.
        let job = Self(unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) });
        ensure!(
            !job.0.is_null(),
            "create owned Windows process job: {}",
            std::io::Error::last_os_error()
        );
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        ensure!(
            unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&limits) as u32,
                )
            } != 0,
            "configure owned Windows process job: {}",
            std::io::Error::last_os_error()
        );
        if unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle()) } == 0 {
            let error = std::io::Error::last_os_error();
            if child.try_wait()?.is_none() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error).context("attach child to owned Windows process job");
            }
        }
        Ok(job)
    }
}
#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.0);
            }
        }
    }
}
// Jobs can be transferred with the owning IPC worker, never shared.
#[cfg(windows)]
unsafe impl Send for WindowsJob {}
