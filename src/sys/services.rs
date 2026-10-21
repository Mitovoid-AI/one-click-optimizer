use crate::model::ServiceInfo;
use crate::util::{from_wide_ptr, wide};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::Services::{
    ChangeServiceConfigW, CloseServiceHandle, ControlService, EnumServicesStatusExW, OpenSCManagerW, OpenServiceW,
    QueryServiceConfigW, ENUM_SERVICE_STATUS_PROCESSW, QUERY_SERVICE_CONFIGW, SC_HANDLE, SERVICE_STATUS,
};

const SC_MANAGER_CONNECT: u32 = 0x1;
const SC_MANAGER_ENUMERATE_SERVICE: u32 = 0x4;
const SERVICE_QUERY_CONFIG: u32 = 0x1;
const SERVICE_CHANGE_CONFIG: u32 = 0x2;
const SERVICE_QUERY_STATUS: u32 = 0x4;
const SERVICE_STOP: u32 = 0x20;
const SERVICE_NO_CHANGE: u32 = 0xffff_ffff;
const SERVICE_RUNNING: u32 = 4;
const ERROR_MORE_DATA: u32 = 234;
const ERROR_SERVICE_NOT_ACTIVE: u32 = 1062;

struct Sc(SC_HANDLE);
impl Drop for Sc {
    fn drop(&mut self) {
        unsafe {
            CloseServiceHandle(self.0);
        }
    }
}

fn last_err() -> u32 {
    unsafe { GetLastError() }
}

fn open_scm(access: u32) -> Result<Sc, u32> {
    let h = unsafe { OpenSCManagerW(null(), null(), access) };
    if h.is_null() {
        Err(last_err())
    } else {
        Ok(Sc(h))
    }
}

fn open_svc(scm: &Sc, name: &str, access: u32) -> Result<Sc, u32> {
    let h = unsafe { OpenServiceW(scm.0, wide(name).as_ptr(), access) };
    if h.is_null() {
        Err(last_err())
    } else {
        Ok(Sc(h))
    }
}

fn query_config(scm: &Sc, name: &str) -> Result<(u32, String), u32> {
    let s = open_svc(scm, name, SERVICE_QUERY_CONFIG)?;
    unsafe {
        let mut need = 0u32;
        QueryServiceConfigW(s.0, null_mut(), 0, &mut need);
        if need == 0 {
            return Err(last_err());
        }
        let mut buf = vec![0u64; (need as usize).div_ceil(8)];
        if QueryServiceConfigW(s.0, buf.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW, need, &mut need) == 0 {
            return Err(last_err());
        }
        let c = &*(buf.as_ptr() as *const QUERY_SERVICE_CONFIGW);
        Ok((c.dwStartType, from_wide_ptr(c.lpBinaryPathName)))
    }
}

pub fn list_services() -> Vec<ServiceInfo> {
    let Ok(scm) = open_scm(SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE) else {
        return Vec::new();
    };
    let mut buf: Vec<u64> = vec![0; 64 * 1024 / 8];
    let mut count = 0u32;
    unsafe {
        for _ in 0..5 {
            let (mut need, mut resume) = (0u32, 0u32);
            let ok = EnumServicesStatusExW(
                scm.0,
                0,    // SC_ENUM_PROCESS_INFO
                0x30, // SERVICE_WIN32
                3,    // SERVICE_STATE_ALL
                buf.as_mut_ptr() as *mut u8,
                (buf.len() * 8) as u32,
                &mut need,
                &mut count,
                &mut resume,
                null(),
            );
            if ok != 0 {
                break;
            }
            count = 0;
            if last_err() != ERROR_MORE_DATA {
                return Vec::new();
            }
            buf = vec![0; (buf.len() * 8 + need as usize).div_ceil(8) + 512];
        }
        let entries = std::slice::from_raw_parts(buf.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW, count as usize);
        entries
            .iter()
            .map(|e| {
                let name = from_wide_ptr(e.lpServiceName);
                let (start_type, path) = query_config(&scm, &name).unwrap_or((u32::MAX, String::new()));
                ServiceInfo {
                    display: from_wide_ptr(e.lpDisplayName),
                    running: e.ServiceStatusProcess.dwCurrentState == SERVICE_RUNNING,
                    pid: e.ServiceStatusProcess.dwProcessId,
                    start_type,
                    path,
                    name,
                }
            })
            .collect()
    }
}

pub fn query_start_type(name: &str) -> Result<u32, u32> {
    let scm = open_scm(SC_MANAGER_CONNECT)?;
    query_config(&scm, name).map(|(s, _)| s)
}

pub fn set_start_type(name: &str, start: u32) -> Result<(), u32> {
    let scm = open_scm(SC_MANAGER_CONNECT)?;
    let s = open_svc(&scm, name, SERVICE_CHANGE_CONFIG)?;
    let ok = unsafe {
        ChangeServiceConfigW(
            s.0,
            SERVICE_NO_CHANGE,
            start,
            SERVICE_NO_CHANGE,
            null(),
            null(),
            null_mut(),
            null(),
            null(),
            null(),
            null(),
        )
    };
    if ok == 0 {
        Err(last_err())
    } else {
        Ok(())
    }
}

pub fn stop(name: &str) -> Result<(), u32> {
    let scm = open_scm(SC_MANAGER_CONNECT)?;
    let s = open_svc(&scm, name, SERVICE_STOP | SERVICE_QUERY_STATUS)?;
    unsafe {
        let mut st: SERVICE_STATUS = std::mem::zeroed();
        if ControlService(s.0, 1, &mut st) == 0 {
            let e = last_err();
            return if e == ERROR_SERVICE_NOT_ACTIVE { Ok(()) } else { Err(e) };
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_real_services_read_only() {
        let all = list_services();
        assert!(all.len() > 50, "only {} services", all.len());
        let rpc = all.iter().find(|s| s.name.eq_ignore_ascii_case("RpcSs")).expect("RpcSs present");
        assert!(rpc.running);
        assert_eq!(rpc.start_type, crate::model::SERVICE_AUTO_START);
        assert_eq!(query_start_type("RpcSs"), Ok(crate::model::SERVICE_AUTO_START));
        assert_eq!(query_start_type("NoSuchService_OneClick"), Err(1060));
    }
}
