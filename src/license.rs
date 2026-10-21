//! Pro license: $20 once ($10 launch price until 1 February 2027), no account.
//!
//! The buyer pays on the store's hosted checkout (email only) and gets a
//! license key. The app activates it once with the store's PUBLIC license API
//! (no secret key is ever built into the app), then saves it encrypted with
//! Windows DPAPI and tied to this PC. It re-checks quietly every 14 days;
//! being offline never locks a paying user out.
//!
//! Until `POLAR` is filled in, Pro isn't launched and every feature is free.

use crate::util::{data_dir, unix_now, wide};
use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use std::ptr::{null, null_mut};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    /// https://polar.sh/docs/features/benefits/license-keys (`sandbox` = sandbox host).
    Polar { sandbox: bool },
    /// https://docs.dodopayments.com/features/license-keys (`test` = test-mode host).
    Dodo { test: bool },
    /// https://docs.lemonsqueezy.com/api/license-api
    LemonSqueezy,
    #[cfg(feature = "license-mock")]
    Mock,
}

pub struct Store {
    pub provider: Provider,
    /// Hosted checkout page opened by "Buy Pro".
    pub checkout_url: &'static str,
    /// Only keys for this product unlock Pro (a key from another product is
    /// refused). For Polar this is the license-key benefit ID.
    pub product_id: &'static str,
    /// Polar organization ID (other stores leave it empty).
    pub org_id: &'static str,
    pub price: &'static str,
}

/// Pro's full price.
pub const PRICE: &str = "$20";
/// Charged until `LAUNCH_ENDS`: the checkout link carries a 50% "Launch price"
/// discount that Polar ends at the same time (see SELLING.md).
pub const LAUNCH_PRICE: &str = "$10";
/// 1 February 2027, 00:00 UTC.
pub const LAUNCH_ENDS: u64 = 1_801_440_000;
pub const LAUNCH_ENDS_TEXT: &str = "1 February 2027";

/// The price to show at `now`, and the full price while the launch price runs.
pub fn price_at(full: &'static str, now: u64) -> (&'static str, Option<&'static str>) {
    if now < LAUNCH_ENDS {
        (LAUNCH_PRICE, Some(full))
    } else {
        (full, None)
    }
}

/// The live store. Paste the three values from the Polar dashboard (see
/// SELLING.md); Pro switches on by itself once they are filled in.
#[cfg_attr(feature = "license-mock", allow(dead_code))]
const POLAR: Store = Store {
    provider: Provider::Polar { sandbox: false },
    checkout_url: "https://buy.polar.sh/polar_cl_8V5m8NArA6JURSM8Ssb7jkqSwR8aZ4Q1n6Nmp0Kor9M",
    // The "OneClick Optimizer Pro license" benefit (License Keys).
    product_id: "b86d596c-d342-483d-80a3-45d10dae015a",
    org_id: "6ffc6006-b048-449b-9d05-50897e9096d4",
    price: PRICE,
};

#[cfg_attr(feature = "license-mock", allow(dead_code))]
const fn ready(s: &Store) -> bool {
    !s.checkout_url.is_empty() && !s.product_id.is_empty() && !s.org_id.is_empty()
}

#[cfg(not(feature = "license-mock"))]
pub const STORE: Option<Store> = if ready(&POLAR) { Some(POLAR) } else { None };

/// Test builds (`--features license-mock`) use an offline fake store.
#[cfg(feature = "license-mock")]
pub const STORE: Option<Store> = Some(Store {
    provider: Provider::Mock,
    checkout_url: "https://example.com/buy-oneclick-optimizer-pro",
    product_id: "mock-product",
    org_id: "mock-org",
    price: PRICE,
});

pub const FREE_RUNS: u32 = 3;
const RECHECK_SECS: u64 = 14 * 86_400;
const ENTROPY: &[u8] = b"OneClickOptimizer.license.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct License {
    pub key: String,
    pub instance_id: String,
    /// MachineGuid of the PC it was activated on.
    pub machine: String,
    pub activated: u64,
    pub checked: u64,
    #[serde(default)]
    pub email: String,
}

impl License {
    /// "ABCD…WXYZ" for display.
    pub fn masked_key(&self) -> String {
        mask(&self.key)
    }
}

pub fn mask(key: &str) -> String {
    let k: Vec<char> = key.chars().collect();
    if k.len() <= 8 {
        return "\u{2022}".repeat(k.len());
    }
    format!("{}\u{2026}{}", k[..4].iter().collect::<String>(), k[k.len() - 4..].iter().collect::<String>())
}

pub fn store() -> Option<&'static Store> {
    STORE.as_ref()
}

/// Is the paid version switched on in this build?
pub fn launched() -> bool {
    store().is_some()
}

/// Pro features available (always, until Pro is launched).
pub fn is_pro() -> bool {
    !launched() || load().is_some()
}

pub fn free_runs_left() -> u32 {
    if is_pro() {
        return u32::MAX;
    }
    FREE_RUNS.saturating_sub(crate::settings::load().free_runs_used)
}

/// May the user run one-click Optimise now (Pro, or a free run left)?
pub fn can_one_click() -> bool {
    is_pro() || free_runs_left() > 0
}

/// Counts a free run (no-op for Pro users).
pub fn use_free_run() {
    if !is_pro() {
        crate::settings::update(|s| s.free_runs_used += 1);
    }
}

// ------------------------------------------------------------- this PC

fn machine_id() -> String {
    use windows_sys::Win32::System::Registry::{RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE};
    unsafe {
        let mut k: HKEY = null_mut();
        // KEY_READ | KEY_WOW64_64KEY
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, wide(r"SOFTWARE\Microsoft\Cryptography").as_ptr(), 0, 0x20019 | 0x100, &mut k) != 0 {
            return String::new();
        }
        let mut buf = [0u16; 128];
        let mut len = (buf.len() * 2) as u32;
        let mut ty = 0u32;
        let r = RegQueryValueExW(k, wide("MachineGuid").as_ptr(), null(), &mut ty, buf.as_mut_ptr() as *mut u8, &mut len);
        RegCloseKey(k);
        if r == 0 {
            crate::util::from_wide(&buf)
        } else {
            String::new()
        }
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn LocalFree(h: *mut c_void) -> *mut c_void;
}

/// Label for this activation in the store. PC names often carry the owner's
/// name, so only an anonymous tag derived from the MachineGuid is sent.
pub fn instance_name() -> String {
    let tag = crate::update::sha256_hex(format!("oco-pc:{}", machine_id()).as_bytes()).unwrap_or_default();
    format!("Windows PC {}", tag.get(..8).unwrap_or("").to_uppercase())
}

// ------------------------------------------------------------ storage (DPAPI)

#[repr(C)]
struct Blob {
    len: u32,
    data: *mut u8,
}

#[link(name = "crypt32")]
extern "system" {
    fn CryptProtectData(input: *const Blob, descr: *const u16, entropy: *const Blob, reserved: *mut c_void, prompt: *mut c_void, flags: u32, out: *mut Blob) -> i32;
    fn CryptUnprotectData(input: *const Blob, descr: *mut *mut u16, entropy: *const Blob, reserved: *mut c_void, prompt: *mut c_void, flags: u32, out: *mut Blob) -> i32;
}

const CRYPTPROTECT_UI_FORBIDDEN: u32 = 1;

fn dpapi(data: &[u8], protect: bool) -> Option<Vec<u8>> {
    unsafe {
        let input = Blob { len: data.len() as u32, data: data.as_ptr() as *mut u8 };
        let entropy = Blob { len: ENTROPY.len() as u32, data: ENTROPY.as_ptr() as *mut u8 };
        let mut out = Blob { len: 0, data: null_mut() };
        let ok = if protect {
            CryptProtectData(&input, wide("OneClick Optimizer license").as_ptr(), &entropy, null_mut(), null_mut(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        } else {
            CryptUnprotectData(&input, null_mut(), &entropy, null_mut(), null_mut(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        };
        if ok == 0 || out.data.is_null() {
            return None;
        }
        let v = std::slice::from_raw_parts(out.data, out.len as usize).to_vec();
        LocalFree(out.data as *mut c_void);
        Some(v)
    }
}

fn path() -> std::path::PathBuf {
    data_dir().join("license.dat")
}

/// The saved license, if it decrypts and belongs to this PC.
pub fn load() -> Option<License> {
    let raw = std::fs::read(path()).ok()?;
    let plain = dpapi(&raw, false)?;
    let lic: License = serde_json::from_slice(&plain).ok()?;
    (lic.machine == machine_id()).then_some(lic)
}

fn save(l: &License) -> Result<(), String> {
    let plain = serde_json::to_vec(l).map_err(|e| e.to_string())?;
    let enc = dpapi(&plain, true).ok_or("couldn't protect the license file")?;
    std::fs::write(path(), enc).map_err(|e| format!("couldn't save the license: {e}"))
}

fn forget() {
    let _ = std::fs::remove_file(path());
}

// --------------------------------------------------------------- the API

/// What the store said about a key.
#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    Ok { instance_id: String, email: String },
    /// Definitely not valid (wrong key, refunded, disabled, other product, limit reached).
    Refused(String),
}

fn s(v: &serde_json::Value, path: &[&str]) -> String {
    let mut cur = v;
    for p in path {
        cur = &cur[*p];
    }
    match cur {
        serde_json::Value::String(x) => x.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// Lemon Squeezy /v1/licenses/activate answer.
pub fn parse_ls_activate(body: &str, product_id: &str) -> Result<Answer, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|_| "unexpected answer from the store".to_string())?;
    if v["activated"].as_bool() != Some(true) {
        let err = s(&v, &["error"]);
        return Ok(Answer::Refused(if err.is_empty() { "this key couldn't be activated".into() } else { err }));
    }
    if s(&v, &["meta", "product_id"]) != product_id {
        return Ok(Answer::Refused("this key is for a different product".into()));
    }
    Ok(Answer::Ok { instance_id: s(&v, &["instance", "id"]), email: s(&v, &["meta", "customer_email"]) })
}

/// Lemon Squeezy /v1/licenses/validate answer.
pub fn parse_ls_validate(body: &str, product_id: &str) -> Result<bool, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|_| "unexpected answer from the store".to_string())?;
    Ok(v["valid"].as_bool() == Some(true) && s(&v, &["meta", "product_id"]) == product_id)
}

/// Dodo /licenses/activate answer (201 on success; errors are plain HTTP codes).
pub fn parse_dodo_activate(code: u32, body: &str, product_id: &str) -> Result<Answer, String> {
    match code {
        200 | 201 => {}
        403 => return Ok(Answer::Refused("this key isn't active (refunded or disabled)".into())),
        404 => return Ok(Answer::Refused("license key not found".into())),
        422 => return Ok(Answer::Refused("this key is already in use on its maximum number of PCs \u{2014} free one of them first".into())),
        _ => return Err(format!("store unavailable (HTTP {code})")),
    }
    let v: serde_json::Value = serde_json::from_str(body).map_err(|_| "unexpected answer from the store".to_string())?;
    if s(&v, &["product", "product_id"]) != product_id {
        return Ok(Answer::Refused("this key is for a different product".into()));
    }
    let id = s(&v, &["id"]);
    if id.is_empty() {
        return Err("unexpected answer from the store".into());
    }
    Ok(Answer::Ok { instance_id: id, email: s(&v, &["customer", "email"]) })
}

/// Dodo /licenses/validate answer: Ok(false) only when the store says so.
pub fn parse_dodo_validate(code: u32, body: &str) -> Result<bool, String> {
    match code {
        200 => {
            let v: serde_json::Value = serde_json::from_str(body).map_err(|_| "unexpected answer from the store".to_string())?;
            v["valid"].as_bool().ok_or_else(|| "unexpected answer from the store".to_string())
        }
        403 | 404 => Ok(false),
        _ => Err(format!("store unavailable (HTTP {code})")),
    }
}

fn dodo_post(test: bool, action: &str, body: serde_json::Value) -> Result<(u32, String), String> {
    let host = if test { "test.dodopayments.com" } else { "live.dodopayments.com" };
    let (code, bytes) = crate::web::post(&format!("https://{host}/licenses/{action}"), "application/json", body.to_string().as_bytes(), 1 << 20)?;
    Ok((code, String::from_utf8_lossy(&bytes).into_owned()))
}

/// Text of a Polar error answer (`detail` is a string for 403/404, a list for 422).
fn polar_detail(body: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    match &v["detail"] {
        serde_json::Value::String(d) => d.trim_end_matches('.').to_string(),
        _ => String::new(),
    }
}

/// Polar /customer-portal/license-keys/activate answer.
pub fn parse_polar_activate(code: u32, body: &str, benefit_id: &str) -> Result<Answer, String> {
    match code {
        200 | 201 => {}
        403 => {
            let d = polar_detail(body);
            return Ok(Answer::Refused(if d.is_empty() { "this key was refunded, disabled, or is already used on its maximum number of PCs".into() } else { d }));
        }
        404 => return Ok(Answer::Refused("license key not found".into())),
        422 => return Ok(Answer::Refused("that doesn't look like a valid key".into())),
        _ => return Err(format!("store unavailable (HTTP {code})")),
    }
    let v: serde_json::Value = serde_json::from_str(body).map_err(|_| "unexpected answer from the store".to_string())?;
    if s(&v, &["license_key", "benefit_id"]) != benefit_id {
        return Ok(Answer::Refused("this key is for a different product".into()));
    }
    let id = s(&v, &["id"]);
    if id.is_empty() {
        return Err("unexpected answer from the store".into());
    }
    Ok(Answer::Ok { instance_id: id, email: s(&v, &["license_key", "customer", "email"]) })
}

/// Polar /customer-portal/license-keys/validate answer: 404 means revoked,
/// disabled, expired or not found; anything else unexpected is "couldn't ask".
pub fn parse_polar_validate(code: u32, body: &str, benefit_id: &str) -> Result<bool, String> {
    match code {
        200 => {
            let v: serde_json::Value = serde_json::from_str(body).map_err(|_| "unexpected answer from the store".to_string())?;
            Ok(s(&v, &["status"]) == "granted" && s(&v, &["benefit_id"]) == benefit_id)
        }
        404 => Ok(false),
        _ => Err(format!("store unavailable (HTTP {code})")),
    }
}

fn polar_post(sandbox: bool, action: &str, body: serde_json::Value) -> Result<(u32, String), String> {
    let host = if sandbox { "sandbox-api.polar.sh" } else { "api.polar.sh" };
    let url = format!("https://{host}/v1/customer-portal/license-keys/{action}");
    let (code, bytes) = crate::web::post(&url, "application/json", body.to_string().as_bytes(), 1 << 20)?;
    Ok((code, String::from_utf8_lossy(&bytes).into_owned()))
}

const LS_API: &str = "https://api.lemonsqueezy.com/v1/licenses";

fn ls_post(action: &str, form: &[(&str, &str)]) -> Result<(u32, String), String> {
    let body = crate::web::form_encode(form);
    let (code, bytes) = crate::web::post(&format!("{LS_API}/{action}"), "application/x-www-form-urlencoded", body.as_bytes(), 1 << 20)?;
    Ok((code, String::from_utf8_lossy(&bytes).into_owned()))
}

fn api_activate(st: &Store, key: &str) -> Result<Answer, String> {
    match st.provider {
        Provider::Polar { sandbox } => {
            let body = serde_json::json!({ "key": key, "organization_id": st.org_id, "label": instance_name() });
            let (code, body) = polar_post(sandbox, "activate", body)?;
            parse_polar_activate(code, &body, st.product_id)
        }
        Provider::Dodo { test } => {
            let (code, body) = dodo_post(test, "activate", serde_json::json!({ "license_key": key, "name": instance_name() }))?;
            parse_dodo_activate(code, &body, st.product_id)
        }
        Provider::LemonSqueezy => {
            let (_, body) = ls_post("activate", &[("license_key", key), ("instance_name", &instance_name())])?;
            parse_ls_activate(&body, st.product_id)
        }
        #[cfg(feature = "license-mock")]
        Provider::Mock => Ok(if key == "OCO-TEST-VALID-KEY" {
            Answer::Ok { instance_id: "mock-instance".into(), email: "buyer@example.com".into() }
        } else {
            Answer::Refused("license key not found".into())
        }),
    }
}

/// Ok(true) valid, Ok(false) definitely invalid, Err = couldn't ask (offline).
fn api_validate(st: &Store, l: &License) -> Result<bool, String> {
    match st.provider {
        Provider::Polar { sandbox } => {
            let body = serde_json::json!({ "key": l.key, "organization_id": st.org_id, "activation_id": l.instance_id });
            let (code, body) = polar_post(sandbox, "validate", body)?;
            parse_polar_validate(code, &body, st.product_id)
        }
        Provider::Dodo { test } => {
            let (code, body) = dodo_post(test, "validate", serde_json::json!({ "license_key": l.key, "license_key_instance_id": l.instance_id }))?;
            parse_dodo_validate(code, &body)
        }
        Provider::LemonSqueezy => {
            let (code, body) = ls_post("validate", &[("license_key", &l.key), ("instance_id", &l.instance_id)])?;
            if code >= 500 {
                return Err(format!("store unavailable (HTTP {code})"));
            }
            parse_ls_validate(&body, st.product_id)
        }
        #[cfg(feature = "license-mock")]
        Provider::Mock => Ok(l.key == "OCO-TEST-VALID-KEY"),
    }
}

fn api_deactivate(st: &Store, l: &License) -> Result<(), String> {
    match st.provider {
        Provider::Polar { sandbox } => {
            let body = serde_json::json!({ "key": l.key, "organization_id": st.org_id, "activation_id": l.instance_id });
            let (code, _) = polar_post(sandbox, "deactivate", body)?;
            match code {
                // 404: the activation is already gone, which is what we want.
                200 | 204 | 404 => Ok(()),
                _ => Err(format!("store unavailable (HTTP {code}) \u{2014} try again in a minute")),
            }
        }
        Provider::Dodo { test } => {
            let (code, _) = dodo_post(test, "deactivate", serde_json::json!({ "license_key": l.key, "license_key_instance_id": l.instance_id }))?;
            match code {
                // 403/404: the activation is already gone, which is what we want.
                200 | 204 | 403 | 404 => Ok(()),
                _ => Err(format!("store unavailable (HTTP {code}) \u{2014} try again in a minute")),
            }
        }
        Provider::LemonSqueezy => {
            let (_, body) = ls_post("deactivate", &[("license_key", &l.key), ("instance_id", &l.instance_id)])?;
            let v: serde_json::Value = serde_json::from_str(&body).map_err(|_| "unexpected answer from the store".to_string())?;
            if v["deactivated"].as_bool() == Some(true) {
                Ok(())
            } else {
                Err(s(&v, &["error"]))
            }
        }
        #[cfg(feature = "license-mock")]
        Provider::Mock => Ok(()),
    }
}

/// Activates `key` on this PC and saves it.
pub fn activate(key: &str) -> Result<License, String> {
    let st = store().ok_or("Pro isn't available in this version")?;
    let key = key.trim();
    if key.len() < 8 || key.len() > 200 {
        return Err("that doesn't look like a license key \u{2014} copy it from your purchase email".into());
    }
    match api_activate(st, key)? {
        Answer::Refused(why) => Err(format!("that key didn't work: {why}. Copy it again from your purchase email.")),
        Answer::Ok { instance_id, email } => {
            let now = unix_now();
            let lic = License { key: key.to_string(), instance_id, machine: machine_id(), activated: now, checked: now, email };
            save(&lic)?;
            Ok(lic)
        }
    }
}

/// Frees this PC's activation (to move Pro to another PC) and forgets the key.
pub fn deactivate() -> Result<(), String> {
    let st = store().ok_or("Pro isn't available in this version")?;
    let lic = load().ok_or("Pro isn't active on this PC")?;
    api_deactivate(st, &lic)?;
    forget();
    Ok(())
}

/// Quiet background re-check every 14 days. Removes the license only when the
/// store definitely says no (refund, disabled key); offline keeps it.
pub fn recheck_if_due() {
    let (Some(st), Some(mut lic)) = (store(), load()) else { return };
    if unix_now().saturating_sub(lic.checked) < RECHECK_SECS {
        return;
    }
    match api_validate(st, &lic) {
        Ok(true) => {
            lic.checked = unix_now();
            let _ = save(&lic);
        }
        Ok(false) => forget(),
        Err(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACTIVATED: &str = r#"{"activated":true,"error":null,"license_key":{"id":1,"status":"active","key":"38b1460a-5104-4067-a91d-77b872934d51","activation_limit":3,"activation_usage":1},"instance":{"id":"47596ad9-a811-4ebf-ac8a-03fc7b6d2a17","name":"PC (OneClick Optimizer)"},"meta":{"store_id":1,"product_id":12345,"customer_email":"buyer@example.com"}}"#;

    #[test]
    fn lemon_squeezy_activation_answers() {
        assert_eq!(
            parse_ls_activate(ACTIVATED, "12345").unwrap(),
            Answer::Ok { instance_id: "47596ad9-a811-4ebf-ac8a-03fc7b6d2a17".into(), email: "buyer@example.com".into() }
        );
        assert_eq!(parse_ls_activate(ACTIVATED, "999").unwrap(), Answer::Refused("this key is for a different product".into()));
        let limit = r#"{"activated":false,"error":"This license key has reached the activation limit.","license_key":{},"meta":{}}"#;
        assert_eq!(parse_ls_activate(limit, "12345").unwrap(), Answer::Refused("This license key has reached the activation limit.".into()));
        assert!(parse_ls_activate("<html>", "12345").is_err());
    }

    #[test]
    fn lemon_squeezy_validation_answers() {
        let ok = r#"{"valid":true,"error":null,"meta":{"product_id":12345}}"#;
        assert!(parse_ls_validate(ok, "12345").unwrap());
        assert!(!parse_ls_validate(ok, "1").unwrap());
        assert!(!parse_ls_validate(r#"{"valid":false,"error":"license_key not found."}"#, "12345").unwrap());
    }

    #[test]
    fn keys_are_masked() {
        assert_eq!(mask("38b1460a-5104-4067-a91d-77b872934d51"), "38b1\u{2026}4d51");
        assert_eq!(mask("short"), "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}");
    }

    #[test]
    fn dpapi_roundtrip_and_machine_id() {
        let enc = dpapi(b"secret license", true).unwrap();
        assert_ne!(enc, b"secret license");
        assert_eq!(dpapi(&enc, false).unwrap(), b"secret license");
        assert!(dpapi(b"not encrypted", false).is_none());
        assert_eq!(machine_id().len(), 36, "MachineGuid looks like a GUID");
        let name = instance_name();
        assert!(name.starts_with("Windows PC ") && name.len() == 19, "{name}");
        assert_eq!(name, instance_name(), "stable for this PC");
    }

    #[test]
    fn polar_answers() {
        let ok = r#"{"id":"0b6f8c9e-1111-4c1a-9d1e-aaaaaaaaaaaa","license_key_id":"lk-1","label":"Windows PC 1A2B3C4D","meta":{},"created_at":"2026-10-21T10:00:00Z","modified_at":null,
            "license_key":{"id":"lk-1","organization_id":"org-1","customer_id":"cus-1","customer":{"id":"cus-1","email":"ann@example.com","name":"Ann"},"benefit_id":"ben-42","key":"OCO-ABCD","display_key":"****-ABCD","status":"granted","limit_activations":3,"usage":0,"validations":0,"expires_at":null}}"#;
        assert_eq!(
            parse_polar_activate(200, ok, "ben-42").unwrap(),
            Answer::Ok { instance_id: "0b6f8c9e-1111-4c1a-9d1e-aaaaaaaaaaaa".into(), email: "ann@example.com".into() }
        );
        assert_eq!(parse_polar_activate(200, ok, "ben-other").unwrap(), Answer::Refused("this key is for a different product".into()));
        let limit = r#"{"error":"NotPermitted","detail":"License key only supports 3 activations."}"#;
        assert_eq!(parse_polar_activate(403, limit, "ben-42").unwrap(), Answer::Refused("License key only supports 3 activations".into()));
        assert!(matches!(parse_polar_activate(403, "", "ben-42").unwrap(), Answer::Refused(m) if m.contains("refunded")));
        assert!(matches!(parse_polar_activate(404, "{}", "ben-42").unwrap(), Answer::Refused(m) if m.contains("not found")));
        assert!(matches!(parse_polar_activate(422, r#"{"detail":[{"msg":"bad"}]}"#, "ben-42").unwrap(), Answer::Refused(_)));
        assert!(parse_polar_activate(502, "", "ben-42").is_err());

        let valid = r#"{"id":"lk-1","status":"granted","benefit_id":"ben-42","customer":{"email":"ann@example.com"},"activation":{"id":"act"}}"#;
        assert!(parse_polar_validate(200, valid, "ben-42").unwrap());
        assert!(!parse_polar_validate(200, valid, "ben-x").unwrap());
        assert!(!parse_polar_validate(404, "{}", "ben-42").unwrap(), "revoked or refunded");
        // Outages never revoke a paying user.
        assert!(parse_polar_validate(503, "", "ben-42").is_err());
        assert!(parse_polar_validate(422, "{}", "ben-42").is_err());
    }

    #[test]
    fn store_switches_on_only_when_filled_in() {
        let empty = Store { provider: Provider::Polar { sandbox: false }, checkout_url: "", product_id: "", org_id: "", price: PRICE };
        assert!(!ready(&empty), "an empty store keeps Pro switched off");
        let filled = Store { provider: Provider::Polar { sandbox: true }, checkout_url: "https://buy.polar.sh/x", product_id: "b", org_id: "o", price: PRICE };
        assert!(ready(&filled));
        assert_eq!(PRICE, "$20");
    }

    #[test]
    fn launch_price_ends_on_1_february_2027() {
        assert_eq!(LAUNCH_ENDS, (365 * 57 + 14 + 31) * 86_400, "57 years, 14 leap days, then January");
        assert_eq!(price_at(PRICE, LAUNCH_ENDS - 1), ("$10", Some("$20")));
        assert_eq!(price_at(PRICE, LAUNCH_ENDS), ("$20", None));
    }

    #[test]
    fn dodo_answers() {
        let ok = r#"{"id":"lki_123","business_id":"bus_1","name":"Windows PC 1A2B3C4D","license_key_id":"lic_9","created_at":"2026-10-21T10:00:00Z","customer":{"customer_id":"cus_1","name":"Ann","email":"ann@example.com"},"product":{"product_id":"pdt_42","name":"Pro"}}"#;
        assert_eq!(parse_dodo_activate(201, ok, "pdt_42").unwrap(), Answer::Ok { instance_id: "lki_123".into(), email: "ann@example.com".into() });
        assert_eq!(parse_dodo_activate(201, ok, "pdt_other").unwrap(), Answer::Refused("this key is for a different product".into()));
        assert!(matches!(parse_dodo_activate(404, "", "pdt_42").unwrap(), Answer::Refused(m) if m.contains("not found")));
        assert!(matches!(parse_dodo_activate(422, "{}", "pdt_42").unwrap(), Answer::Refused(m) if m.contains("maximum")));
        assert!(matches!(parse_dodo_activate(403, "{}", "pdt_42").unwrap(), Answer::Refused(m) if m.contains("refunded")));
        assert!(parse_dodo_activate(500, "", "pdt_42").is_err());
        assert!(parse_dodo_activate(201, "<html>", "pdt_42").is_err());

        assert!(parse_dodo_validate(200, r#"{"valid":true}"#).unwrap());
        assert!(!parse_dodo_validate(200, r#"{"valid":false}"#).unwrap());
        assert!(!parse_dodo_validate(404, "").unwrap());
        // Outages and odd answers never revoke a paying user.
        assert!(parse_dodo_validate(503, "").is_err());
        assert!(parse_dodo_validate(422, "{}").is_err());
        assert!(parse_dodo_validate(200, "{}").is_err());
    }

    #[cfg(not(feature = "license-mock"))]
    #[test]
    fn pro_is_sold_through_live_polar() {
        assert!(launched());
        let st = store().expect("Pro is on sale");
        assert!(matches!(st.provider, Provider::Polar { sandbox: false }), "never ship the sandbox store");
        assert!(st.checkout_url.starts_with("https://buy.polar.sh/polar_cl_"));
        for id in [st.product_id, st.org_id] {
            assert_eq!(id.len(), 36, "{id} should be a Polar ID");
        }
    }
}
