//! Runs admin-only changes in a short-lived elevated copy of OneClickOptimizer.exe.
//! The plan travels hex-encoded on the command line, so there is no temp file
//! another program could tamper with between writing and elevation.

use crate::actions::{self, Plan};
use crate::util::{hex_decode, hex_encode, wide};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};

const ERROR_CANCELLED: u32 = 1223;

pub enum ElevateError {
    Cancelled,
    Failed(String),
}

/// Path of the GUI exe (the CLI hands admin work to it as well).
pub fn gui_exe() -> std::path::PathBuf {
    let me = std::env::current_exe().unwrap_or_default();
    let gui = me.with_file_name("OneClickOptimizer.exe");
    if gui.exists() {
        gui
    } else {
        me
    }
}

pub fn run_elevated(plan: &Plan) -> Result<u32, ElevateError> {
    let json = serde_json::to_vec(plan).map_err(|e| ElevateError::Failed(e.to_string()))?;
    let params = wide(&format!("apply-plan {}", hex_encode(&json)));
    let file = wide(&gui_exe().to_string_lossy());
    let verb = wide("runas");
    unsafe {
        let mut sei: SHELLEXECUTEINFOW = std::mem::zeroed();
        sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        sei.fMask = SEE_MASK_NOCLOSEPROCESS;
        sei.lpVerb = verb.as_ptr();
        sei.lpFile = file.as_ptr();
        sei.lpParameters = params.as_ptr();
        sei.nShow = 0; // SW_HIDE
        if ShellExecuteExW(&mut sei) == 0 {
            let e = GetLastError();
            return Err(if e == ERROR_CANCELLED {
                ElevateError::Cancelled
            } else {
                ElevateError::Failed(format!("could not start the admin helper (Windows error {e})"))
            });
        }
        if sei.hProcess.is_null() {
            return Err(ElevateError::Failed("admin helper did not start".into()));
        }
        WaitForSingleObject(sei.hProcess, INFINITE);
        let mut code = 1u32;
        GetExitCodeProcess(sei.hProcess, &mut code);
        CloseHandle(sei.hProcess);
        Ok(code)
    }
}

/// Entry point of the elevated helper: `OneClickOptimizer.exe apply-plan <hex>`.
pub fn run_plan_arg(hex: &str) -> i32 {
    let Some(bytes) = hex_decode(hex) else { return 3 };
    let Ok(plan) = serde_json::from_slice::<Plan>(&bytes) else { return 4 };
    let left = actions::execute(&plan.journal, plan.ops, true);
    if left.is_empty() {
        0
    } else {
        2
    }
}
