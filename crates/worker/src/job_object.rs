//! Windows Job Object confinement for the worker process.
//!
//! The worker puts itself into a job with a per-process memory cap, a per-process CPU time cap,
//! kill-on-close and die-on-unhandled-exception. Sidecar engines (LibreOffice, Tesseract, ...)
//! are started by their own adapters under their own jobs, so no active-process limit is set here.
//!
//! The job allows breakaway, but only for a child that asks for it at creation. The language model
//! server asks (see [`SidecarSpawner`]): a per-process CPU time cap of ten minutes would end it
//! after about a minute of answering on eight threads, and a model needs more memory than a PDF
//! parser. It gets a job of its own that ends it when the worker goes.
#![allow(unsafe_code)]

use quirewise_engine_ai_llamacpp::Spawner;
use std::process::{Child, Command};

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
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
        JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOB_OBJECT_LIMIT_PROCESS_TIME,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
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
            | JOB_OBJECT_LIMIT_BREAKAWAY_OK
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

/// A job of its own for the language model server: it ends the server when it is closed, which
/// happens when the worker goes, however it goes.
#[derive(Debug)]
pub struct SidecarJob {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
}

// SAFETY: a job handle is only closed, and closing it from another thread is allowed.
#[cfg(windows)]
unsafe impl Send for SidecarJob {}

#[cfg(windows)]
impl Drop for SidecarJob {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        // SAFETY: `handle` was returned by CreateJobObjectW and is closed exactly once, here.
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// Starts the language model server outside the limits of the worker's job, in a job that ends it
/// with the worker.
#[derive(Debug, Clone, Copy, Default)]
pub struct SidecarSpawner;

#[cfg(windows)]
fn own_job(child: &Child) -> Option<SidecarJob> {
    use core::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    // SAFETY: plain Win32 calls. `info` outlives the call and its size is passed exactly; the
    // job handle is handed to `SidecarJob`, which closes it, or closed by it on every early return.
    unsafe {
        let job = CreateJobObjectW(None, None).ok()?;
        let guard = SidecarJob { handle: job };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw const info).cast::<c_void>(),
            u32::try_from(core::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).ok()?,
        )
        .ok()?;
        AssignProcessToJobObject(job, HANDLE(child.as_raw_handle())).ok()?;
        Some(guard)
    }
}

#[cfg(windows)]
impl Spawner for SidecarSpawner {
    fn spawn(&self, mut command: Command) -> std::io::Result<(Child, Box<dyn Send>)> {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x0800_0000;
        const BREAKAWAY: u32 = 0x0100_0000;
        // Asking to break away fails when the job does not allow it (a test runner's, say); the
        // server then starts inside the job like any other program.
        let child = match command.creation_flags(NO_WINDOW | BREAKAWAY).spawn() {
            Ok(child) => child,
            Err(_) => command.creation_flags(NO_WINDOW).spawn()?,
        };
        let hold: Box<dyn Send> = match own_job(&child) {
            Some(job) => Box::new(job),
            None => Box::new(()),
        };
        Ok((child, hold))
    }
}

#[cfg(not(windows))]
impl Spawner for SidecarSpawner {
    fn spawn(&self, mut command: Command) -> std::io::Result<(Child, Box<dyn Send>)> {
        command.spawn().map(|c| (c, Box::new(()) as Box<dyn Send>))
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    #[test]
    fn a_server_is_stopped_when_its_hold_is_dropped() {
        let mut command = Command::new("ping.exe");
        command
            .args(["-n", "60", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let (mut child, hold) = SidecarSpawner.spawn(command).unwrap();
        assert!(child.try_wait().unwrap().is_none(), "it runs");
        drop(hold);
        let started = Instant::now();
        while child.try_wait().unwrap().is_none() {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "it was not stopped"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
