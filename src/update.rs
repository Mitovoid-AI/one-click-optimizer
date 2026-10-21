//! Updates from GitHub Releases: check for a newer version, download it with
//! SHA-256 verification, and install it. The "Update now / Skip / Later"
//! window lives in `popup.rs`.
//!
//! Releases are built by .github/workflows/release.yml, which uploads
//! OneClickOptimizer-Setup.exe, OneClickOptimizer.exe and SHA256SUMS.txt.

use crate::install::{self, Layout, VERSION};
use serde::Deserialize;
use std::path::PathBuf;
use std::ptr::null;

pub const REPO: &str = "Mitovoid-AI/one-click-optimizer";
pub const SETUP_ASSET: &str = "OneClickOptimizer-Setup.exe";
pub const PORTABLE_ASSET: &str = "OneClickOptimizer.exe";
const SUMS_ASSET: &str = "SHA256SUMS.txt";
const MAX_DOWNLOAD: usize = 64 << 20;
pub const DIALOG_TITLE: &str = "OneClick Optimizer update";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// Lower-case hex SHA-256 when GitHub reports it.
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub tag: String,
    pub notes: String,
    pub page: String,
    pub assets: Vec<Asset>,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name.eq_ignore_ascii_case(name))
    }

    /// Download size of the file this copy would fetch (0 if unknown).
    pub fn download_size(&self) -> u64 {
        let installed = install::running_installed_copy(&Layout::standard());
        self.asset(if installed { SETUP_ASSET } else { PORTABLE_ASSET }).map_or(0, |a| a.size)
    }
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

/// "v1.2.3" / "1.2" / "1.2.3-beta" -> (1, 2, 3).
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next().unwrap_or("0").parse().ok()?;
    let c = it.next().unwrap_or("0").parse().ok()?;
    Some((a, b, c))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((parse_version(candidate), parse_version(current)), (Some(a), Some(b)) if a > b)
}

pub fn parse_release(json: &str) -> Result<Release, String> {
    let r: GhRelease = serde_json::from_str(json).map_err(|e| format!("unexpected answer from GitHub ({e})"))?;
    if r.draft || r.prerelease {
        return Err("latest release is not final".into());
    }
    parse_version(&r.tag_name).ok_or_else(|| format!("release tag {} is not a version", r.tag_name))?;
    Ok(Release {
        version: r.tag_name.trim_start_matches(['v', 'V']).to_string(),
        tag: r.tag_name.clone(),
        notes: r.body.unwrap_or_default(),
        page: r.html_url,
        assets: r
            .assets
            .into_iter()
            .map(|a| Asset {
                sha256: a.digest.and_then(|d| d.strip_prefix("sha256:").map(|h| h.to_ascii_lowercase())),
                name: a.name,
                url: a.browser_download_url,
                size: a.size,
            })
            .collect(),
    })
}

/// The version this copy reports (OCO_FAKE_VERSION lets tests pretend to be older).
pub fn current_version() -> String {
    std::env::var("OCO_FAKE_VERSION").unwrap_or_else(|_| VERSION.to_string())
}

/// "Skip this version": don't offer this version again.
pub fn skip(version: &str) {
    crate::settings::update(|s| s.skip_version = Some(version.to_string()));
}

pub fn is_skipped(version: &str) -> bool {
    crate::settings::load().skip_version.as_deref() == Some(version)
}

/// Latest final release on GitHub (OCO_UPDATE_URL overrides the API URL for tests).
pub fn latest() -> Result<Release, String> {
    let url = std::env::var("OCO_UPDATE_URL").unwrap_or_else(|_| format!("https://api.github.com/repos/{REPO}/releases/latest"));
    let body = crate::web::get(&url, "application/vnd.github+json", 4 << 20)?;
    parse_release(&String::from_utf8_lossy(&body))
}

/// A newer release the user hasn't skipped (`force` ignores "Skip").
pub fn check(force: bool) -> Result<Option<Release>, String> {
    if std::env::var_os("OCO_NO_UPDATE_CHECK").is_some() && !force {
        return Ok(None);
    }
    let r = latest()?;
    let newer = is_newer(&r.version, &current_version());
    Ok((newer && (force || !is_skipped(&r.version))).then_some(r))
}

// ------------------------------------------------------------ verification

#[link(name = "bcrypt")]
extern "system" {
    fn BCryptHash(alg: usize, secret: *const u8, secret_len: u32, input: *const u8, input_len: u32, out: *mut u8, out_len: u32) -> i32;
}

/// SHA-256 with Windows CNG (BCRYPT_SHA256_ALG_HANDLE).
pub fn sha256_hex(data: &[u8]) -> Option<String> {
    let mut out = [0u8; 32];
    let st = unsafe { BCryptHash(0x41, null(), 0, data.as_ptr(), data.len() as u32, out.as_mut_ptr(), 32) };
    (st >= 0).then(|| crate::util::hex_encode(&out))
}

/// `<sha256>  <file name>` lines (sha256sum / Get-FileHash style).
pub fn find_in_sums(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let file = parts.next()?.trim_start_matches('*');
        (file.eq_ignore_ascii_case(name) && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

fn expected_hash(r: &Release, a: &Asset) -> Result<String, String> {
    if let Some(h) = &a.sha256 {
        return Ok(h.clone());
    }
    let sums = r.asset(SUMS_ASSET).ok_or("this release has no checksums, so it can't be verified")?;
    let text = crate::web::get(&sums.url, "application/octet-stream", 1 << 20)?;
    find_in_sums(&String::from_utf8_lossy(&text), &a.name).ok_or_else(|| format!("no checksum listed for {}", a.name))
}

/// Downloads one release file to %TEMP% and checks size, exe header and SHA-256.
/// `progress(received, total)` follows the download (total 0 = unknown).
pub fn download(r: &Release, name: &str, progress: &dyn Fn(u64, u64)) -> Result<PathBuf, String> {
    let a = r.asset(name).ok_or_else(|| format!("release {} has no {name}", r.tag))?;
    let want = expected_hash(r, a)?;
    let total = a.size;
    let bytes = crate::web::get_with_progress(&a.url, "application/octet-stream", MAX_DOWNLOAD, &|got| progress(got, total))?;
    if a.size > 0 && bytes.len() as u64 != a.size {
        return Err("the download was incomplete".into());
    }
    if bytes.len() < 2 || &bytes[..2] != b"MZ" {
        return Err("the download isn't a Windows program".into());
    }
    let got = sha256_hex(&bytes).ok_or("couldn't verify the download")?;
    if got != want {
        return Err("the download failed its security check (checksum mismatch), so it was not installed".into());
    }
    let path = std::env::temp_dir().join(format!("OneClickOptimizer-{}-{}", r.version, name));
    std::fs::write(&path, &bytes).map_err(|e| format!("couldn't save the update: {e}"))?;
    Ok(path)
}

/// Installs the update. Returns Ok when the caller should now exit:
/// an installed copy hands over to the new setup (`--update`), a portable
/// copy replaces its own exe and restarts.
pub fn apply(r: &Release, progress: &dyn Fn(u64, u64)) -> Result<(), String> {
    let l = Layout::standard();
    if install::running_installed_copy(&l) {
        let setup = download(r, SETUP_ASSET, progress)?;
        std::process::Command::new(&setup).arg("--update").spawn().map_err(|e| format!("couldn't start the update: {e}"))?;
        return Ok(());
    }
    let new = download(r, PORTABLE_ASSET, progress)?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let aside = me.with_extension(format!("{}.old", std::process::id()));
    std::fs::rename(&me, &aside).map_err(|e| format!("couldn't replace the app: {e}"))?;
    if let Err(e) = std::fs::copy(&new, &me) {
        let _ = std::fs::rename(&aside, &me);
        return Err(format!("couldn't replace the app: {e}"));
    }
    let _ = std::process::Command::new(&me).spawn();
    Ok(())
}

// -------------------------------------------------------------- what's new

/// Plain-text "What's new" from the release notes: the `## What's new`
/// section the release workflow copies from CHANGELOG.md (or, failing that,
/// the readable lines of the notes), without Markdown symbols or tables.
pub fn whats_new(notes: &str) -> String {
    let lines: Vec<&str> = notes.lines().map(str::trim).collect();
    let section: Vec<&str> = match lines.iter().position(|l| l.trim_start_matches('#').trim().eq_ignore_ascii_case("what's new")) {
        Some(start) => lines[start + 1..].iter().take_while(|l| !l.starts_with('#') && !l.starts_with("---")).copied().collect(),
        None => lines,
    };
    let mut s = section
        .iter()
        .filter(|l| !l.is_empty() && !l.starts_with('|') && !l.to_ascii_lowercase().contains("full changelog"))
        .map(|l| {
            let l = l.trim_start_matches('#').trim();
            let l = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")).map_or(l.to_string(), |rest| format!("\u{2022} {rest}"));
            l.replace("**", "").replace('`', "")
        })
        .take(12)
        .collect::<Vec<_>>()
        .join("\n");
    if s.chars().count() > 900 {
        s = s.chars().take(900).collect::<String>() + "\u{2026}";
    }
    s
}

/// The "What's new" items without bullets, for the update window.
pub fn whats_new_items(notes: &str) -> Vec<String> {
    whats_new(notes)
        .lines()
        .map(|l| l.trim_start_matches('\u{2022}').trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "tag_name": "v0.4.0", "html_url": "https://github.com/x/y/releases/tag/v0.4.0",
        "draft": false, "prerelease": false, "body": "**New**\n- Faster scan\n\n- Bug fixes",
        "assets": [
          {"name": "OneClickOptimizer-Setup.exe", "browser_download_url": "https://github.com/x/y/releases/download/v0.4.0/OneClickOptimizer-Setup.exe", "size": 925000, "digest": "sha256:ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789"},
          {"name": "SHA256SUMS.txt", "browser_download_url": "https://github.com/x/y/releases/download/v0.4.0/SHA256SUMS.txt", "size": 300}
        ]}"#;

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(parse_version("v0.10.2"), Some((0, 10, 2)));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("2.0.0-beta.1"), Some((2, 0, 0)));
        assert_eq!(parse_version("latest"), None);
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(is_newer("0.3.1", "0.3.0"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.9", "0.3.0"));
        assert!(!is_newer("garbage", "0.3.0"));
    }

    #[test]
    fn parses_github_release_json() {
        let r = parse_release(SAMPLE).unwrap();
        assert_eq!(r.version, "0.4.0");
        let a = r.asset("oneclickoptimizer-setup.exe").unwrap();
        assert_eq!(a.size, 925000);
        assert_eq!(a.sha256.as_deref(), Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"));
        assert_eq!(r.asset(SUMS_ASSET).unwrap().sha256, None);
        assert!(whats_new(&r.notes).starts_with("New\n\u{2022} Faster scan"), "{}", whats_new(&r.notes));
        assert!(parse_release(&SAMPLE.replace("\"prerelease\": false", "\"prerelease\": true")).is_err());
        assert!(parse_release("{}").is_err());
    }

    #[test]
    fn whats_new_uses_the_changelog_section() {
        let notes = "## What's new\n- Shows the real download size\n- Cleaner notes\n\n---\n**Download OneClickOptimizer-Setup.exe**\n| File | What |\n|---|---|\n## What's Changed\n* commit by @x\n**Full Changelog**: https://x";
        assert_eq!(whats_new(notes), "\u{2022} Shows the real download size\n\u{2022} Cleaner notes");
        // No section: readable lines only, no tables or changelog links.
        assert_eq!(whats_new("Hello\n| a | b |\n**Full Changelog**: u\n* fix `x`"), "Hello\n\u{2022} fix x");
    }

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(sha256_hex(b"abc").unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(sha256_hex(b"").unwrap(), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn reads_checksum_files() {
        let sums = "AAA  other.exe\nba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  OneClickOptimizer-Setup.exe\n";
        assert_eq!(find_in_sums(sums, "OneClickOptimizer-Setup.exe").as_deref(), Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
        assert_eq!(find_in_sums(sums, "other.exe"), None, "short hashes are rejected");
        assert_eq!(find_in_sums(sums, "missing.exe"), None);
    }

    #[test]
    fn skip_is_remembered() {
        let dir = std::env::temp_dir().join(format!("oco-update-test-{}", std::process::id()));
        std::env::set_var("OCO_DATA_DIR", &dir);
        assert!(!is_skipped("9.9.9"));
        skip("9.9.9");
        assert!(is_skipped("9.9.9"));
        assert!(!is_skipped("9.9.10"));
        std::env::remove_var("OCO_DATA_DIR");
        let _ = std::fs::remove_dir_all(dir);
    }
}
