//! Process list. Memory, start time and CPU time come from one
//! NtQuerySystemInformation snapshot (like Task Manager), so they are known
//! even for SYSTEM services a normal user cannot open. Path and command line
//! need OpenProcess and are filled in where Windows allows it.

use crate::model::ProcInfo;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, FILETIME, HANDLE};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, TerminateProcess,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
};

/// Error value for "the process no longer exists".
pub const GONE: u32 = u32::MAX;
const ERROR_INVALID_PARAMETER: u32 = 87;
const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;
const SYSTEM_PROCESS_INFORMATION_CLASS: u32 = 5;
const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;

#[link(name = "ntdll")]
extern "system" {
    fn NtQueryInformationProcess(h: HANDLE, class: u32, info: *mut c_void, len: u32, ret: *mut u32) -> i32;
    fn NtQuerySystemInformation(class: u32, info: *mut c_void, len: u32, ret: *mut u32) -> i32;
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn open(pid: u32, access: u32) -> Result<Handle, u32> {
    let h = unsafe { OpenProcess(access, 0, pid) };
    if h.is_null() {
        Err(unsafe { GetLastError() })
    } else {
        Ok(Handle(h))
    }
}

fn ft(f: &FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

fn created_via_handle(h: &Handle) -> Option<u64> {
    unsafe {
        let (mut c, mut e, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) =
            (std::mem::zeroed(), std::mem::zeroed(), std::mem::zeroed(), std::mem::zeroed());
        (GetProcessTimes(h.0, &mut c, &mut e, &mut k, &mut u) != 0).then(|| ft(&c))
    }
}

fn image_path(h: &Handle) -> String {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    unsafe {
        if QueryFullProcessImageNameW(h.0, 0, buf.as_mut_ptr(), &mut len) != 0 {
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            String::new()
        }
    }
}

fn command_line(h: &Handle) -> String {
    #[repr(C)]
    struct UnicodeString {
        len: u16,
        max: u16,
        buf: *const u16,
    }
    unsafe {
        let mut need = 0u32;
        NtQueryInformationProcess(h.0, PROCESS_COMMAND_LINE_INFORMATION, null_mut(), 0, &mut need);
        if need < std::mem::size_of::<UnicodeString>() as u32 || need > (1 << 20) {
            return String::new();
        }
        let mut buf = vec![0u64; (need as usize).div_ceil(8)];
        if NtQueryInformationProcess(h.0, PROCESS_COMMAND_LINE_INFORMATION, buf.as_mut_ptr() as *mut c_void, need, &mut need) < 0 {
            return String::new();
        }
        let us = &*(buf.as_ptr() as *const UnicodeString);
        if us.buf.is_null() || us.len == 0 {
            return String::new();
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(us.buf, us.len as usize / 2))
    }
}

/// One process from the system snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawProc {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub private_bytes: u64,
    pub working_set: u64,
    pub created: u64,
    pub cpu: u64,
    pub handles: u32,
}

// x64 SYSTEM_PROCESS_INFORMATION field offsets.
const OFF_HANDLES: usize = 0x60;
const OFF_NEXT: usize = 0x00;
const OFF_CREATE: usize = 0x20;
const OFF_USER: usize = 0x28;
const OFF_KERNEL: usize = 0x30;
const OFF_NAME_LEN: usize = 0x38;
const OFF_NAME_PTR: usize = 0x40;
const OFF_PID: usize = 0x50;
const OFF_PPID: usize = 0x58;
const OFF_WORKING_SET: usize = 0x90;
const OFF_PRIVATE: usize = 0xC8;
const MIN_ENTRY: usize = 0xD0;

fn rd<T: Copy>(buf: &[u8], off: usize) -> Option<T> {
    let end = off.checked_add(std::mem::size_of::<T>())?;
    let bytes = buf.get(off..end)?;
    Some(unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const T) })
}

/// Walks a SystemProcessInformation buffer. Names point inside the buffer.
pub fn parse_system_processes(buf: &[u8]) -> Vec<RawProc> {
    let mut out = Vec::new();
    let base = buf.as_ptr() as usize;
    let mut off = 0usize;
    loop {
        if off + MIN_ENTRY > buf.len() {
            break;
        }
        let name_len = rd::<u16>(buf, off + OFF_NAME_LEN).unwrap_or(0) as usize;
        let name_ptr = rd::<usize>(buf, off + OFF_NAME_PTR).unwrap_or(0);
        let name = if name_ptr >= base && name_ptr + name_len <= base + buf.len() && name_len > 0 {
            let start = name_ptr - base;
            let words: Vec<u16> = buf[start..start + name_len]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&words)
        } else {
            String::new()
        };
        let pid = rd::<usize>(buf, off + OFF_PID).unwrap_or(0) as u32;
        out.push(RawProc {
            pid,
            ppid: rd::<usize>(buf, off + OFF_PPID).unwrap_or(0) as u32,
            name: if name.is_empty() && pid == 4 { "System".into() } else { name },
            private_bytes: rd::<usize>(buf, off + OFF_PRIVATE).unwrap_or(0) as u64,
            working_set: rd::<usize>(buf, off + OFF_WORKING_SET).unwrap_or(0) as u64,
            created: rd::<i64>(buf, off + OFF_CREATE).unwrap_or(0).max(0) as u64,
            cpu: (rd::<i64>(buf, off + OFF_USER).unwrap_or(0) + rd::<i64>(buf, off + OFF_KERNEL).unwrap_or(0)).max(0) as u64,
            handles: rd::<u32>(buf, off + OFF_HANDLES).unwrap_or(0),
        });
        let next = rd::<u32>(buf, off + OFF_NEXT).unwrap_or(0) as usize;
        if next == 0 {
            break;
        }
        off += next;
    }
    out
}

pub fn system_processes() -> Vec<RawProc> {
    let mut len: u32 = 512 * 1024;
    for _ in 0..6 {
        let mut buf = vec![0u64; len as usize / 8];
        let mut ret = 0u32;
        let st = unsafe {
            NtQuerySystemInformation(SYSTEM_PROCESS_INFORMATION_CLASS, buf.as_mut_ptr() as *mut c_void, len, &mut ret)
        };
        if st == STATUS_INFO_LENGTH_MISMATCH {
            len = ret.max(len) + 64 * 1024;
            continue;
        }
        if st < 0 {
            return Vec::new();
        }
        let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, len as usize) };
        return parse_system_processes(bytes);
    }
    Vec::new()
}

pub fn list_processes(cpu_sample_ms: u64, with_details: bool) -> Vec<ProcInfo> {
    let first = system_processes();
    let mut procs: Vec<ProcInfo> = first
        .iter()
        .filter(|r| r.pid != 0)
        .map(|r| ProcInfo {
            pid: r.pid,
            ppid: r.ppid,
            name: r.name.clone(),
            private_bytes: r.private_bytes,
            working_set: r.working_set,
            created: r.created,
            handles: r.handles,
            ..Default::default()
        })
        .collect();
    if with_details {
        for p in &mut procs {
            if let Ok(h) = open(p.pid, PROCESS_QUERY_LIMITED_INFORMATION) {
                p.path = image_path(&h);
                p.cmdline = command_line(&h);
            }
        }
    }
    if cpu_sample_ms > 0 {
        let start = std::time::Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(cpu_sample_ms));
        let elapsed_ticks = start.elapsed().as_nanos() as f64 / 100.0;
        let cpus = std::thread::available_parallelism().map_or(1, |n| n.get()) as f64;
        let before: HashMap<(u32, u64), &RawProc> = first.iter().map(|r| ((r.pid, r.created), r)).collect();
        let second = system_processes();
        let after: HashMap<(u32, u64), &RawProc> = second.iter().map(|r| ((r.pid, r.created), r)).collect();
        for p in &mut procs {
            let key = (p.pid, p.created);
            if let (Some(b), Some(a)) = (before.get(&key), after.get(&key)) {
                p.cpu_pct = (a.cpu.saturating_sub(b.cpu) as f64 / (elapsed_ticks * cpus) * 100.0) as f32;
                p.growth_bytes = a.private_bytes as i64 - b.private_bytes as i64;
                p.private_bytes = a.private_bytes;
                p.working_set = a.working_set;
                p.handles = a.handles;
            }
        }
    }
    procs
}

fn has_exited(h: &Handle) -> bool {
    const STILL_ACTIVE: u32 = 259;
    let mut code = STILL_ACTIVE;
    unsafe { GetExitCodeProcess(h.0, &mut code) != 0 && code != STILL_ACTIVE }
}

/// Ends a process, but only if it is still the same process (same creation time).
pub fn terminate(pid: u32, created: u64) -> Result<(), u32> {
    let h = match open(pid, PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION) {
        Ok(h) => h,
        Err(ERROR_INVALID_PARAMETER) => return Err(GONE),
        Err(e) => return Err(e),
    };
    if created != 0 && created_via_handle(&h) != Some(created) {
        return Err(GONE); // PID now belongs to a different process
    }
    if has_exited(&h) {
        return Err(GONE); // exited, but someone still holds a handle to it
    }
    if unsafe { TerminateProcess(h.0, 1) } == 0 {
        let e = unsafe { GetLastError() };
        return Err(if has_exited(&h) { GONE } else { e });
    }
    Ok(())
}

/// Creation time of a live process.
pub fn creation_time(pid: u32) -> Option<u64> {
    created_via_handle(&open(pid, PROCESS_QUERY_LIMITED_INFORMATION).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    /// Cross-checks the hand-decoded snapshot layout against the documented APIs.
    #[test]
    fn snapshot_layout_matches_documented_apis() {
        let me = std::process::id();
        let raw = system_processes();
        assert!(raw.len() > 20);
        let r = raw.iter().find(|r| r.pid == me).expect("own process in snapshot");
        let exe = std::env::current_exe().unwrap();
        assert!(exe.to_string_lossy().ends_with(&r.name), "name {:?}", r.name);
        assert_eq!(Some(r.created), creation_time(me));
        let mut c: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS, c.cb) };
        let diff = (r.private_bytes as i64 - c.PrivateUsage as i64).abs();
        assert!(diff < 16 << 20, "private {} vs {}", r.private_bytes, c.PrivateUsage);
        assert!(r.working_set > 0);
        // Parent is a real process (cargo test harness).
        assert!(raw.iter().any(|p| p.pid == r.ppid));
        // Services running as SYSTEM report memory too.
        let lsass = raw.iter().find(|p| p.name.eq_ignore_ascii_case("lsass.exe")).expect("lsass");
        assert!(lsass.private_bytes > 0, "SYSTEM process memory visible");
    }

    #[test]
    fn parser_survives_truncated_buffers() {
        assert!(parse_system_processes(&[]).is_empty());
        assert!(parse_system_processes(&[0u8; 0x40]).is_empty());
        let one = parse_system_processes(&[0u8; MIN_ENTRY]);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0], RawProc::default());
    }

    #[test]
    fn sees_itself_with_details() {
        let me = std::process::id();
        let procs = list_processes(0, true);
        let p = procs.iter().find(|p| p.pid == me).expect("own process listed");
        assert!(p.private_bytes > 0);
        assert!(p.created > 0);
        assert!(p.path.to_ascii_lowercase().ends_with(".exe"));
        assert!(!p.cmdline.is_empty());
    }

    #[test]
    fn terminate_gone_and_mismatch() {
        let mut child = std::process::Command::new("cmd")
            .args(["/c", "ping -n 30 127.0.0.1 >nul"])
            .spawn()
            .unwrap();
        let pid = child.id();
        let created = creation_time(pid).unwrap();
        // Wrong creation time: must refuse (PID reuse guard).
        assert_eq!(terminate(pid, created + 1), Err(GONE));
        assert_eq!(terminate(pid, created), Ok(()));
        let _ = child.wait();
        assert_eq!(terminate(pid, created), Err(GONE));
    }
}
