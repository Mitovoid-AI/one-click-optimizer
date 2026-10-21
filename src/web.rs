//! Tiny HTTPS client on WinINet (part of Windows, no extra DLLs or crates):
//! GET for the update check, form POST for license activation.

use crate::install::VERSION;
use crate::util::wide;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

#[link(name = "wininet")]
extern "system" {
    fn InternetOpenW(agent: *const u16, access: u32, proxy: *const u16, bypass: *const u16, flags: u32) -> *mut c_void;
    fn InternetOpenUrlW(h: *mut c_void, url: *const u16, headers: *const u16, headers_len: u32, flags: u32, ctx: usize) -> *mut c_void;
    fn InternetConnectW(h: *mut c_void, server: *const u16, port: u16, user: *const u16, pass: *const u16, service: u32, flags: u32, ctx: usize) -> *mut c_void;
    fn HttpOpenRequestW(conn: *mut c_void, verb: *const u16, object: *const u16, version: *const u16, referrer: *const u16, accept: *const *const u16, flags: u32, ctx: usize) -> *mut c_void;
    fn HttpSendRequestW(req: *mut c_void, headers: *const u16, headers_len: u32, body: *const c_void, body_len: u32) -> i32;
    fn InternetReadFile(h: *mut c_void, buf: *mut u8, to_read: u32, read: *mut u32) -> i32;
    fn InternetCloseHandle(h: *mut c_void) -> i32;
    fn InternetSetOptionW(h: *mut c_void, option: u32, buf: *const c_void, len: u32) -> i32;
    fn HttpQueryInfoW(h: *mut c_void, level: u32, buf: *mut c_void, len: *mut u32, index: *mut u32) -> i32;
}

const FLAG_RELOAD: u32 = 0x8000_0000;
const FLAG_NO_CACHE_WRITE: u32 = 0x0400_0000;
const FLAG_NO_UI: u32 = 0x0000_0200;
const FLAG_SECURE: u32 = 0x0080_0000;

struct Inet(*mut c_void);
impl Drop for Inet {
    fn drop(&mut self) {
        unsafe {
            InternetCloseHandle(self.0);
        }
    }
}

fn session() -> Result<Inet, String> {
    unsafe {
        let agent = wide(&format!("OneClickOptimizer/{VERSION}"));
        let net = InternetOpenW(agent.as_ptr(), 0, null(), null(), 0);
        if net.is_null() {
            return Err("no internet connection".into());
        }
        let timeout: u32 = 20_000;
        for opt in [2u32, 5, 6] {
            // connect, send, receive timeouts
            InternetSetOptionW(net, opt, &timeout as *const u32 as *const c_void, 4);
        }
        Ok(Inet(net))
    }
}

fn status(req: &Inet) -> u32 {
    let mut status: u32 = 0;
    let mut len = 4u32;
    // HTTP_QUERY_STATUS_CODE | HTTP_QUERY_FLAG_NUMBER
    unsafe { HttpQueryInfoW(req.0, 19 | 0x2000_0000, &mut status as *mut u32 as *mut c_void, &mut len, null_mut()) };
    status
}

fn read_all(req: &Inet, limit: usize, progress: &dyn Fn(u64)) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let mut read = 0u32;
        if unsafe { InternetReadFile(req.0, buf.as_mut_ptr(), buf.len() as u32, &mut read) } == 0 {
            return Err("download interrupted".into());
        }
        if read == 0 {
            return Ok(out);
        }
        out.extend_from_slice(&buf[..read as usize]);
        progress(out.len() as u64);
        if out.len() > limit {
            return Err("response is larger than expected".into());
        }
    }
}

/// HTTPS GET with redirects and no cache; errors unless the answer is 200.
pub fn get(url: &str, accept: &str, limit: usize) -> Result<Vec<u8>, String> {
    get_with_progress(url, accept, limit, &|_| {})
}

/// `get` that reports the bytes received so far (for download progress).
pub fn get_with_progress(url: &str, accept: &str, limit: usize, progress: &dyn Fn(u64)) -> Result<Vec<u8>, String> {
    if !url.starts_with("https://") {
        return Err("only HTTPS is allowed".into());
    }
    let net = session()?;
    let headers = wide(&format!("Accept: {accept}\r\n"));
    let req = unsafe {
        InternetOpenUrlW(net.0, wide(url).as_ptr(), headers.as_ptr(), u32::MAX, FLAG_RELOAD | FLAG_NO_CACHE_WRITE | FLAG_NO_UI, 0)
    };
    if req.is_null() {
        return Err("couldn't connect (check your internet connection)".into());
    }
    let req = Inet(req);
    let code = status(&req);
    if code != 200 {
        return Err(format!("the server answered with HTTP {code}"));
    }
    read_all(&req, limit, progress)
}

/// `application/x-www-form-urlencoded` encoding.
pub fn form_encode(fields: &[(&str, &str)]) -> String {
    fn enc(s: &str) -> String {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
                b' ' => "+".to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect()
    }
    fields.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&")
}

/// `https://host/path` -> (host, "/path").
pub fn split_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    (!host.is_empty() && !host.contains(':')).then(|| (host.to_string(), path.to_string()))
}

/// HTTPS POST. Returns (status, body) for any HTTP status, because APIs
/// explain errors (e.g. "license key not found") in non-200 answers.
pub fn post(url: &str, content_type: &str, body: &[u8], limit: usize) -> Result<(u32, Vec<u8>), String> {
    let (host, path) = split_url(url).ok_or("only HTTPS is allowed")?;
    let net = session()?;
    unsafe {
        // INTERNET_DEFAULT_HTTPS_PORT, INTERNET_SERVICE_HTTP
        let conn = InternetConnectW(net.0, wide(&host).as_ptr(), 443, null(), null(), 3, 0, 0);
        if conn.is_null() {
            return Err("couldn't connect (check your internet connection)".into());
        }
        let conn = Inet(conn);
        let req = HttpOpenRequestW(conn.0, wide("POST").as_ptr(), wide(&path).as_ptr(), null(), null(), null(), FLAG_SECURE | FLAG_RELOAD | FLAG_NO_CACHE_WRITE | FLAG_NO_UI, 0);
        if req.is_null() {
            return Err("couldn't start the request".into());
        }
        let req = Inet(req);
        let headers = wide(&format!("Content-Type: {content_type}\r\nAccept: application/json\r\n"));
        if HttpSendRequestW(req.0, headers.as_ptr(), u32::MAX, body.as_ptr() as *const c_void, body.len() as u32) == 0 {
            return Err("couldn't reach the license server (check your internet connection)".into());
        }
        let code = status(&req);
        Ok((code, read_all(&req, limit, &|_| {})?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_encoding() {
        assert_eq!(form_encode(&[("license_key", "AB12-cd34"), ("instance_name", "My PC #1")]), "license_key=AB12-cd34&instance_name=My+PC+%231");
    }

    #[test]
    fn url_splitting() {
        assert_eq!(split_url("https://api.example.com/v1/licenses/activate"), Some(("api.example.com".into(), "/v1/licenses/activate".into())));
        assert_eq!(split_url("https://example.com"), Some(("example.com".into(), "/".into())));
        assert_eq!(split_url("http://example.com/x"), None);
        assert_eq!(split_url("https://example.com:8443/x"), None);
    }

    #[test]
    fn only_https() {
        assert!(get("http://example.com", "*/*", 10).is_err());
        assert!(post("http://example.com", "text/plain", b"", 10).is_err());
    }
}
