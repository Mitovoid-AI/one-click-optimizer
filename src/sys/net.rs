//! Listening TCP ports per process (IPv4 + IPv6; Vite often binds ::1 only),
//! and which processes have an open connection (something is talking to them).

use std::collections::HashSet;
use std::ffi::c_void;

const TCP_TABLE_OWNER_PID_LISTENER: i32 = 3;
const TCP_TABLE_OWNER_PID_CONNECTIONS: i32 = 4;
const MIB_TCP_STATE_ESTAB: u32 = 5;
const AF_INET: u32 = 2;
const AF_INET6: u32 = 23;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

#[link(name = "iphlpapi")]
extern "system" {
    fn GetExtendedTcpTable(table: *mut c_void, size: *mut u32, order: i32, af: u32, class: i32, reserved: u32) -> u32;
}

/// The low 16 bits of `dwLocalPort` hold the port in network byte order.
pub fn port_from_dword(v: u32) -> u16 {
    (((v & 0xff) << 8) | ((v >> 8) & 0xff)) as u16
}

fn read_u32(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

fn table(af: u32, class: i32) -> Vec<u8> {
    let mut size = 0u32;
    for _ in 0..4 {
        let mut buf = vec![0u8; size as usize];
        let ptr = if buf.is_empty() { std::ptr::null_mut() } else { buf.as_mut_ptr() as *mut c_void };
        let r = unsafe { GetExtendedTcpTable(ptr, &mut size, 0, af, class, 0) };
        match r {
            0 => return buf,
            ERROR_INSUFFICIENT_BUFFER => size += 1024,
            _ => return Vec::new(),
        }
    }
    Vec::new()
}

/// Parses a MIB_TCPTABLE_OWNER_PID / MIB_TCP6TABLE_OWNER_PID buffer.
pub fn parse_table(buf: &[u8], v6: bool) -> Vec<(u32, u16)> {
    let (row, port_off, pid_off) = if v6 { (56, 20, 52) } else { (24, 8, 20) };
    let n = read_u32(buf, 0).unwrap_or(0) as usize;
    (0..n)
        .filter_map(|i| {
            let base = 4 + i * row;
            Some((read_u32(buf, base + pid_off)?, port_from_dword(read_u32(buf, base + port_off)?)))
        })
        .collect()
}

pub fn listening_ports() -> Vec<(u32, u16)> {
    let mut v = parse_table(&table(AF_INET, TCP_TABLE_OWNER_PID_LISTENER), false);
    v.extend(parse_table(&table(AF_INET6, TCP_TABLE_OWNER_PID_LISTENER), true));
    v.sort_unstable();
    v.dedup();
    v
}

/// (pid, TCP state) rows of a MIB_TCPTABLE_OWNER_PID / MIB_TCP6TABLE_OWNER_PID buffer.
pub fn parse_states(buf: &[u8], v6: bool) -> Vec<(u32, u32)> {
    let (row, state_off, pid_off) = if v6 { (56, 48, 52) } else { (24, 0, 20) };
    let n = read_u32(buf, 0).unwrap_or(0) as usize;
    (0..n)
        .filter_map(|i| {
            let base = 4 + i * row;
            Some((read_u32(buf, base + pid_off)?, read_u32(buf, base + state_off)?))
        })
        .collect()
}

/// Processes with an established TCP connection: a browser tab on a dev
/// server, a build client talking to its daemon, a phone on a test server.
pub fn connected_pids() -> HashSet<u32> {
    let mut rows = parse_states(&table(AF_INET, TCP_TABLE_OWNER_PID_CONNECTIONS), false);
    rows.extend(parse_states(&table(AF_INET6, TCP_TABLE_OWNER_PID_CONNECTIONS), true));
    rows.into_iter().filter(|&(pid, state)| state == MIB_TCP_STATE_ESTAB && pid > 4).map(|(pid, _)| pid).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_byte_order() {
        // 5300 = 0x14B4, stored as B4 14 in the low word.
        assert_eq!(port_from_dword(0xB414), 5300);
        assert_eq!(port_from_dword(0x5000), 80);
    }

    #[test]
    fn parses_v4_rows() {
        let mut b = vec![];
        b.extend(2u32.to_le_bytes());
        for (port, pid) in [(5300u16, 111u32), (80, 222)] {
            let be = port.to_be_bytes();
            b.extend(2u32.to_le_bytes()); // state
            b.extend(0u32.to_le_bytes()); // local addr
            b.extend(u32::from_le_bytes([be[0], be[1], 0, 0]).to_le_bytes());
            b.extend(0u32.to_le_bytes());
            b.extend(0u32.to_le_bytes());
            b.extend(pid.to_le_bytes());
        }
        assert_eq!(parse_table(&b, false), vec![(111, 5300), (222, 80)]);
        assert!(parse_table(&b[..10], false).is_empty());
    }

    #[test]
    fn sees_an_open_connection() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let before = connected_pids().contains(&std::process::id());
        let _client = std::net::TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (_server, _) = l.accept().unwrap();
        assert!(connected_pids().contains(&std::process::id()), "connected now (before: {before})");
    }

    #[test]
    fn finds_a_real_listener() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let me = std::process::id();
        assert!(listening_ports().contains(&(me, port)));
    }
}
