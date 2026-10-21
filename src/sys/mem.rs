use crate::model::MemInfo;
use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows_sys::Win32::Storage::FileSystem::FileTimeToLocalFileTime;
use windows_sys::Win32::System::SystemInformation::{GetSystemTimeAsFileTime, GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Time::FileTimeToSystemTime;

pub fn read_mem() -> MemInfo {
    unsafe {
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut m) == 0 {
            return MemInfo::default();
        }
        MemInfo {
            total_phys: m.ullTotalPhys,
            avail_phys: m.ullAvailPhys,
            commit_limit: m.ullTotalPageFile,
            commit_used: m.ullTotalPageFile.saturating_sub(m.ullAvailPageFile),
        }
    }
}

pub fn now_filetime() -> u64 {
    unsafe {
        let mut ft: FILETIME = std::mem::zeroed();
        GetSystemTimeAsFileTime(&mut ft);
        ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
    }
}

/// FILETIME ticks (UTC) -> "21 Oct 14:05" in local time.
pub fn fmt_local(ft: u64) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    unsafe {
        let utc = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
        let mut local: FILETIME = std::mem::zeroed();
        let mut st: SYSTEMTIME = std::mem::zeroed();
        if FileTimeToLocalFileTime(&utc, &mut local) == 0 || FileTimeToSystemTime(&local, &mut st) == 0 {
            return String::from("?");
        }
        format!(
            "{} {} {:02}:{:02}",
            st.wDay,
            MONTHS[(st.wMonth.clamp(1, 12) - 1) as usize],
            st.wHour,
            st.wMinute
        )
    }
}
