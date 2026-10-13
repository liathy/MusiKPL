use std::{
    process::{Child, Command, Output, Stdio},
    sync::{Arc, Mutex},
};

use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(crate) fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(windows)]
struct ProcessJob {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl ProcessJob {
    fn new() -> Option<Self> {
        use std::mem::{size_of, zeroed};
        use std::ptr::{null, null_mut};
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };

        unsafe {
            let handle = CreateJobObjectW(null_mut(), null());
            if handle == 0 {
                return None;
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &mut limits as *mut _ as *mut _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if configured == 0 {
                windows_sys::Win32::Foundation::CloseHandle(handle);
                return None;
            }
            Some(Self { handle })
        }
    }

    fn assign(&self, child: &Child) {
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
        unsafe {
            let _ = AssignProcessToJobObject(self.handle, child.as_raw_handle() as isize);
        }
    }

    fn terminate(&self) {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        unsafe {
            let _ = TerminateJobObject(self.handle, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(not(windows))]
struct ProcessJob;

#[cfg(not(windows))]
impl ProcessJob {
    fn new() -> Option<Self> {
        Some(Self)
    }

    fn assign(&self, _child: &Child) {}

    fn terminate(&self) {}
}

#[derive(Clone)]
pub(crate) struct ProcessRegistry {
    pids: Arc<Mutex<Vec<u32>>>,
    closing: Arc<AtomicBool>,
    job: Arc<ProcessJob>,
}

impl Default for ProcessRegistry {
    fn default() -> Self {
        Self {
            pids: Arc::new(Mutex::new(Vec::new())),
            closing: Arc::new(AtomicBool::new(false)),
            job: Arc::new(ProcessJob::new().expect("failed to create child process job")),
        }
    }
}

impl ProcessRegistry {
    pub(crate) fn add(&self, pid: u32) {
        if let Ok(mut pids) = self.pids.lock() {
            pids.push(pid);
        }
    }

    pub(crate) fn remove(&self, pid: u32) {
        if let Ok(mut pids) = self.pids.lock() {
            pids.retain(|registered| *registered != pid);
        }
    }

    pub(crate) fn assign_for_player(&self, child: &Child) {
        self.job.assign(child);
    }

    pub(crate) fn terminate_all(&self) {
        self.closing.store(true, Ordering::SeqCst);
        self.job.terminate();
        let pids = self
            .pids
            .lock()
            .map(|pids| pids.clone())
            .unwrap_or_default();
        for pid in pids {
            let mut command = Command::new("taskkill");
            hide_console(&mut command);
            let _ = command
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .status();
        }
        if let Ok(mut registered) = self.pids.lock() {
            registered.clear();
        }
    }

    pub(crate) fn is_closing(&self) -> bool {
        self.closing.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.pids.lock().map(|pids| pids.is_empty()).unwrap_or(true)
    }
}

pub(crate) fn run_registered(
    mut command: Command,
    registry: &ProcessRegistry,
) -> Result<Output, String> {
    if registry.is_closing() {
        return Err("Application is closing".into());
    }
    hide_console(&mut command);
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start yt-dlp: {e}"))?;
    let pid = child.id();
    registry.add(pid);
    registry.assign_for_player(&child);
    if registry.is_closing() {
        let _ = child.kill();
        let _ = child.wait();
        registry.remove(pid);
        return Err("Application is closing".into());
    }
    let result = child
        .wait_with_output()
        .map_err(|e| format!("Could not read yt-dlp output: {e}"));
    registry.remove(pid);
    result
}
