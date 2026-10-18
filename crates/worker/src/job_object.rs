//! Windows Job Object confinement for the worker process.
//!
//! The worker puts itself into a job with a per-process memory cap, a per-process CPU time cap,
//! kill-on-close and die-on-unhandled-exception. Sidecar engines (LibreOffice, Tesseract, ...)
//! are started by their own adapters under their own jobs, so no active-process limit is set here.
#![allow(unsafe_code)]

/// Limits applied to the worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobLimits {
    /// Memory cap per process in bytes.
    pub memory_bytes: u64,
    /// CPU time cap per process in seconds.
    pub cpu_seconds: u64,
}

/// Keeps the job handle open for the life of the process. Dropping it closes the job, and with
/// kill-on-close that ends the process, so hold it in `main` until exit.
#[derive(Debug)]
pub struct JobGuard {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl Drop for JobGuard {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        // SAFETY: `handle` was returned by CreateJobObjectW and is closed exactly once, here.
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// Confines the current process. Returns an error message (never shown raw to users) on failure.
#[cfg(windows)]
pub fn confine_current_process(limits: JobLimits) -> Result<JobGuard, String> {
    use core::ffi::c_void;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
        JOB_OBJECT_LIMIT_PROCESS_TIME, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;

    // PerProcessUserTimeLimit counts 100 ns ticks.
    let cpu_ticks =
        i64::try_from(limits.cpu_seconds.saturating_mul(10_000_000)).unwrap_or(i64::MAX);
    let memory = usize::try_from(limits.memory_bytes).unwrap_or(usize::MAX);

    // SAFETY: plain Win32 calls. `info` outlives the SetInformationJobObject call and its size is
    // passed exactly; the handle is checked by `?` and either handed to `JobGuard` or closed.
    unsafe {
        let job = CreateJobObjectW(None, None).map_err(|e| format!("CreateJobObjectW: {e}"))?;
        let guard = JobGuard { handle: job };

        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_PROCESS_TIME
            | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
        info.BasicLimitInformation.PerProcessUserTimeLimit = cpu_ticks;
        info.ProcessMemoryLimit = memory;

        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw const info).cast::<c_void>(),
            u32::try_from(core::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                .map_err(|_| "limit structure too large".to_owned())?,
        )
        .map_err(|e| format!("SetInformationJobObject: {e}"))?;

        AssignProcessToJobObject(job, GetCurrentProcess())
            .map_err(|e| format!("AssignProcessToJobObject: {e}"))?;
        Ok(guard)
    }
}

/// Job Objects exist only on Windows. Other platforms (used for CI tooling) run unconfined.
#[cfg(not(windows))]
pub fn confine_current_process(_limits: JobLimits) -> Result<JobGuard, String> {
    Ok(JobGuard {})
}

/// True when the current process is inside a job object. Used by the `--check-confinement`
/// self-test so the safety control is verified, not assumed.
#[cfg(windows)]
pub fn is_confined() -> bool {
    use windows::Win32::System::JobObjects::IsProcessInJob;
    use windows::Win32::System::Threading::GetCurrentProcess;
    use windows::core::BOOL;

    let mut in_job = BOOL(0);
    // SAFETY: `in_job` is a valid out pointer for the duration of the call, and a null job
    // handle asks about membership in any job.
    let queried = unsafe { IsProcessInJob(GetCurrentProcess(), None, &mut in_job) };
    queried.is_ok() && in_job.as_bool()
}

/// Without Job Objects nothing is confined.
#[cfg(not(windows))]
pub fn is_confined() -> bool {
    false
}
