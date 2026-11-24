//! Small Windows helpers: a job object so recorders die with the app, the recycle bin,
//! opening files/links, Explorer "show in folder", and the autostart Run value.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

/// CREATE_NO_WINDOW: child console programs (yt-dlp, ffmpeg) never flash a window.
pub const NO_WINDOW: u32 = 0x0800_0000;

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

/// Every recorder process is put in this job. When Rewind exits (or crashes), Windows
/// closes the job and kills the recorders with it, so nothing keeps downloading in the dark.
pub struct Job(isize);

unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    pub fn new() -> Job {
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Job(0);
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            Job(handle as isize)
        }
    }

    pub fn adopt(&self, process: std::os::windows::io::RawHandle) {
        if self.0 == 0 {
            return;
        }
        unsafe {
            windows_sys::Win32::System::JobObjects::AssignProcessToJobObject(self.0 as _, process as _);
        }
    }
}

/// Kills a process and all of its children (yt-dlp starts ffmpeg).
pub fn kill_tree(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(NO_WINDOW)
        .output();
}

/// Moves files to the recycle bin (can be restored from there).
pub fn recycle(paths: &[&Path]) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::*;
    let existing: Vec<&Path> = paths.iter().copied().filter(|p| p.exists()).collect();
    if existing.is_empty() {
        return Ok(());
    }
    let mut from: Vec<u16> = Vec::new();
    for path in existing {
        from.extend(path.as_os_str().encode_wide());
        from.push(0);
    }
    from.push(0);
    let mut op: SHFILEOPSTRUCTW = unsafe { std::mem::zeroed() };
    op.wFunc = FO_DELETE as _;
    op.pFrom = from.as_ptr();
    op.fFlags = (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI) as _;
    let result = unsafe { SHFileOperationW(&mut op) };
    if result == 0 && op.fAnyOperationsAborted == 0 {
        Ok(())
    } else {
        Err(format!("Windows could not move the file to the recycle bin (code {result})"))
    }
}

/// Opens a file with its default program, or a web link in the default browser.
pub fn shell_open(target: &str) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let verb = wide("open");
    let file = wide(target);
    let result = unsafe {
        ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL)
    };
    if result as isize > 32 {
        Ok(())
    } else {
        Err("Windows could not open it".into())
    }
}

/// Explorer with the file selected. Rust's normal argument quoting breaks `/select,`.
pub fn reveal(path: &Path) {
    let _ = Command::new("explorer").raw_arg(format!("/select,\"{}\"", path.display())).spawn();
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Rewind";

/// Writes the Run value ourselves: the exe path is quoted (the project path has a space).
pub fn set_autostart(on: bool) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::*;
    let key = wide(RUN_KEY);
    let name = wide(RUN_VALUE);
    unsafe {
        if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let data = wide(format!("\"{}\" --autostart", exe.display()));
            let status = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                data.as_ptr() as *const core::ffi::c_void,
                (data.len() * 2) as u32,
            );
            if status != 0 {
                return Err(format!("Could not turn on start with Windows (code {status})"));
            }
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr());
        }
    }
    Ok(())
}
