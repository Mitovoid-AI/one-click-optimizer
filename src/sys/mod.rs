//! Everything that reads from or writes to Windows.

pub mod hw;
pub mod mem;
pub mod net;
pub mod procs;
pub mod services;
pub mod startup;

use crate::model::Snapshot;
use std::ffi::c_void;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// Full picture of the machine. `cpu_sample_ms` > 0 measures CPU use per process.
pub fn snapshot(cpu_sample_ms: u64) -> Snapshot {
    let services = services::list_services();
    let startup = startup::list_startup();
    let mut procs = procs::list_processes(cpu_sample_ms, true);
    for (pid, port) in net::listening_ports() {
        if let Some(p) = procs.iter_mut().find(|p| p.pid == pid) {
            if !p.ports.contains(&port) {
                p.ports.push(port);
            }
        }
    }
    for p in &mut procs {
        p.ports.sort_unstable();
    }
    Snapshot {
        mem: mem::read_mem(),
        procs,
        services,
        startup,
        now: mem::now_filetime(),
        self_pid: std::process::id(),
    }
}

pub fn is_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut c_void,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}
