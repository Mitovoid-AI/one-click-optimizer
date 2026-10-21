//! Developer junk: dependency and build folders that tools re-create on
//! demand (node_modules, Rust `target`, Gradle `build`, Python virtual
//! environments, .NET `bin`/`obj`, web-framework caches) and big package
//! caches (Gradle, Maven, NuGet, Cargo, pnpm, Bun).
//!
//! Safety rules:
//! - A folder only counts when its project marker sits right next to it:
//!   `node_modules` needs a `package.json` beside it, `target` a `Cargo.toml`,
//!   a virtual environment its `pyvenv.cfg`, and so on. The marker is checked
//!   again right before anything is deleted.
//! - Links (junctions, symlinks) are never followed. Deleting removes the link
//!   itself, never what it points to (pnpm keeps packages behind links).
//! - Windows, Program Files, ProgramData, AppData and other users' folders are
//!   never searched or deleted from.
//! - Nothing here is deleted unless the user ticks it.

use crate::junk::CleanReport;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

/// How deep below a search root projects are looked for.
const MAX_DEPTH: usize = 7;
/// The search stops looking for more folders after this long.
const BUDGET: Duration = Duration::from_secs(30);
/// Projects untouched this long are ticked by default.
pub const STALE_DAYS: u64 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    NodeModules,
    RustTarget,
    GradleBuild,
    GradleProject,
    PythonVenv,
    DotnetBuild,
    WebCache,
    PackageCache,
}

impl Kind {
    /// What it is and how it comes back, in plain words.
    pub fn why(self) -> &'static str {
        match self {
            Kind::NodeModules => "JavaScript packages. npm install (or yarn / pnpm) downloads them again.",
            Kind::RustTarget => "Rust build output. cargo build makes it again.",
            Kind::GradleBuild => "Gradle / Android build output. The next build makes it again.",
            Kind::GradleProject => "Gradle's per-project cache. The next build makes it again.",
            Kind::PythonVenv => "Python virtual environment. Re-create it and pip install the requirements again.",
            Kind::DotnetBuild => ".NET build output. dotnet build makes it again.",
            Kind::WebCache => "Web framework build cache (Next.js, Nuxt, Angular...). The next dev run makes it again.",
            Kind::PackageCache => "Downloaded packages kept for every project. They download again when a build needs them.",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevItem {
    pub path: PathBuf,
    pub kind: Kind,
    /// "node_modules", "Maven repository", ...
    pub label: String,
    /// Project folder name ("" for package caches).
    pub project: String,
    pub bytes: u64,
    pub files: u64,
    /// Days since the project itself was last changed (None for caches).
    pub idle_days: Option<u64>,
}

impl DevItem {
    /// Ticked by default: build folders of projects nobody touched for a month.
    pub fn default_on(&self) -> bool {
        self.kind != Kind::PackageCache && self.idle_days.is_some_and(|d| d >= STALE_DAYS) && self.bytes > 0
    }
}

/// Which developer folder `name` is, given the file names next to it
/// (lower-case) and whether the folder holds a `pyvenv.cfg`.
pub fn classify(name: &str, siblings: &[String], has_pyvenv: impl Fn() -> bool) -> Option<Kind> {
    let has = |f: &str| siblings.iter().any(|s| s == f);
    let has_ext = |ext: &str| siblings.iter().any(|s| s.ends_with(ext));
    match name.to_ascii_lowercase().as_str() {
        "node_modules" if has("package.json") => Some(Kind::NodeModules),
        "target" if has("cargo.toml") => Some(Kind::RustTarget),
        "build" if has("build.gradle") || has("build.gradle.kts") => Some(Kind::GradleBuild),
        ".gradle" if has("build.gradle") || has("build.gradle.kts") || has("settings.gradle") || has("settings.gradle.kts") => Some(Kind::GradleProject),
        "bin" | "obj" if has_ext(".csproj") || has_ext(".fsproj") || has_ext(".vbproj") => Some(Kind::DotnetBuild),
        ".next" | ".nuxt" | ".svelte-kit" | ".parcel-cache" | ".turbo" | ".angular" if has("package.json") => Some(Kind::WebCache),
        ".venv" | "venv" | "env" | ".env" if has_pyvenv() => Some(Kind::PythonVenv),
        _ => None,
    }
}

/// Folders never searched (lower-case names).
fn skip_dir(name: &str) -> bool {
    const SKIP: &[&str] = &[
        "windows",
        "program files",
        "program files (x86)",
        "programdata",
        "appdata",
        "$recycle.bin",
        "system volume information",
        "recovery",
        "perflogs",
        "msocache",
        "$windows.~bt",
        "$windows.~ws",
        "$winreagent",
        "onedrivetemp",
        "node_modules",
    ];
    let n = name.to_ascii_lowercase();
    n.starts_with('.') || SKIP.contains(&n.as_str())
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute())
}

#[link(name = "kernel32")]
extern "system" {
    fn GetLogicalDrives() -> u32;
    fn GetDriveTypeW(root: *const u16) -> u32;
}

/// Where projects are looked for: your user folder and every fixed drive
/// (the Windows drive without its Users folder; your own folder is searched
/// already). `OCO_DEV_ROOTS` (`;`-separated) replaces them for tests.
pub fn search_roots() -> Vec<PathBuf> {
    if let Ok(v) = std::env::var("OCO_DEV_ROOTS") {
        return v.split(';').map(PathBuf::from).filter(|p| p.is_absolute()).collect();
    }
    let mut roots = Vec::new();
    if let Some(home) = env_path("USERPROFILE") {
        roots.push(home);
    }
    let mask = unsafe { GetLogicalDrives() };
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = format!("{}:\\", (b'A' + i as u8) as char);
        // DRIVE_FIXED
        if unsafe { GetDriveTypeW(crate::util::wide(&root).as_ptr()) } == 3 {
            roots.push(PathBuf::from(root));
        }
    }
    roots
}

/// Package caches shared by all projects (only those that exist).
pub fn cache_dirs() -> Vec<(PathBuf, &'static str)> {
    if std::env::var_os("OCO_DEV_ROOTS").is_some() {
        return Vec::new();
    }
    let mut v = Vec::new();
    if let Some(home) = env_path("USERPROFILE") {
        v.push((home.join(r".gradle\caches"), "Gradle download cache"));
        v.push((home.join(r".m2\repository"), "Maven repository"));
        v.push((home.join(r".nuget\packages"), "NuGet packages"));
        v.push((home.join(r".cargo\registry"), "Cargo crates (downloaded)"));
        v.push((home.join(r".bun\install\cache"), "Bun package cache"));
    }
    if let Some(local) = env_path("LOCALAPPDATA") {
        v.push((local.join(r"pnpm\store"), "pnpm store"));
    }
    v.into_iter().filter(|(p, _)| p.is_dir()).collect()
}

fn is_reparse(md: &std::fs::Metadata) -> bool {
    md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// Folders that only exist for Windows or live in the cloud are not searched.
fn searchable(md: &std::fs::Metadata) -> bool {
    let a = md.file_attributes();
    a & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_SYSTEM | FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) == 0
}

/// Under Windows, Program Files, ProgramData or another user's profile?
fn forbidden(p: &Path) -> bool {
    let lower = p.to_string_lossy().to_lowercase();
    let windir = env_path("SystemRoot").map_or(r"c:\windows".to_string(), |w| w.to_string_lossy().to_lowercase());
    let mut bad = vec![windir, r":\program files".into(), r":\programdata".into(), r"\appdata\".into()];
    if let (Some(home), Some(drive)) = (env_path("USERPROFILE"), env_path("SystemDrive").or_else(|| Some(PathBuf::from("C:")))) {
        let users = format!("{}\\users\\", drive.to_string_lossy().to_lowercase().trim_end_matches('\\'));
        let home = format!("{}\\", home.to_string_lossy().to_lowercase());
        if lower.starts_with(&users) && !lower.starts_with(&home) {
            return true;
        }
    }
    bad.retain(|b| !b.is_empty());
    bad.iter().any(|b| lower.contains(b.as_str()))
}

struct Candidate {
    path: PathBuf,
    kind: Kind,
    project: String,
}

fn walk(dir: &Path, depth: usize, deadline: Instant, out: &mut Vec<Candidate>) {
    if Instant::now() > deadline {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for e in rd.flatten() {
        let Ok(md) = e.metadata() else { continue };
        let name = e.file_name().to_string_lossy().to_string();
        if md.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0 {
            dirs.push((name, md, e.path()));
        } else {
            files.push(name.to_ascii_lowercase());
        }
    }
    let project = dir.file_name().map_or(String::new(), |n| n.to_string_lossy().to_string());
    for (name, md, path) in dirs {
        if is_reparse(&md) {
            continue;
        }
        if let Some(kind) = classify(&name, &files, || path.join("pyvenv.cfg").is_file()) {
            out.push(Candidate { path, kind, project: project.clone() });
        } else if depth < MAX_DEPTH && searchable(&md) && !skip_dir(&name) && !forbidden(&path) {
            walk(&path, depth + 1, deadline, out);
        }
    }
}

/// (files, bytes) under `root`, never following links.
pub fn measure(root: &Path) -> (u64, u64) {
    let (mut files, mut bytes) = (0, 0);
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(md) = e.metadata() else { continue };
            if is_reparse(&md) {
                continue;
            }
            if md.is_dir() {
                stack.push(e.path());
            } else {
                files += 1;
                bytes += md.len();
            }
        }
    }
    (files, bytes)
}

/// Days since anything directly inside the project (other than its
/// developer folders) changed.
fn idle_days(project: &Path, now: SystemTime) -> Option<u64> {
    let newest = std::fs::read_dir(project)
        .ok()?
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_ascii_lowercase();
            !matches!(n.as_str(), "node_modules" | "target" | "build" | ".gradle" | "bin" | "obj" | ".next" | ".nuxt" | ".svelte-kit" | ".parcel-cache" | ".turbo" | ".angular" | ".venv" | "venv")
        })
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()?;
    Some(now.duration_since(newest).map_or(0, |d| d.as_secs() / 86_400))
}

/// Runs `f` over `items` on a few threads, keeping the order.
fn par_map<T: Sync, R: Send + Default + Clone>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let n = items.len();
    let threads = std::thread::available_parallelism().map_or(4, |p| p.get()).clamp(2, 8).min(n.max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(vec![R::default(); n]);
    std::thread::scope(|sc| {
        for _ in 0..threads {
            sc.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if i >= n {
                    break;
                }
                let r = f(&items[i]);
                results.lock().unwrap_or_else(|e| e.into_inner())[i] = r;
            });
        }
    });
    results.into_inner().unwrap_or_else(|e| e.into_inner())
}

/// Finds developer folders under `roots` (plus package caches when
/// `with_caches`), measured, biggest first.
pub fn scan_roots(roots: &[PathBuf], with_caches: bool) -> Vec<DevItem> {
    let deadline = Instant::now() + BUDGET;
    // Search each top-level folder on its own thread.
    let mut tops = Vec::new();
    let mut found = Vec::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(root) else { continue };
        let mut files = Vec::new();
        let mut subdirs = Vec::new();
        for e in rd.flatten() {
            let Ok(md) = e.metadata() else { continue };
            let name = e.file_name().to_string_lossy().to_string();
            if md.file_attributes() & FILE_ATTRIBUTE_DIRECTORY == 0 {
                files.push(name.to_ascii_lowercase());
            } else if !is_reparse(&md) {
                subdirs.push((name, md, e.path()));
            }
        }
        let project = root.file_name().map_or(String::new(), |n| n.to_string_lossy().to_string());
        for (name, md, path) in subdirs {
            if let Some(kind) = classify(&name, &files, || path.join("pyvenv.cfg").is_file()) {
                found.push(Candidate { path, kind, project: project.clone() });
            } else if searchable(&md) && !skip_dir(&name) && !forbidden(&path) && !(name.eq_ignore_ascii_case("users") && path.parent().is_some_and(|p| p.parent().is_none())) {
                tops.push(path);
            }
        }
    }
    tops.sort();
    tops.dedup();
    for mut part in par_map(&tops, |top| {
        let mut out = Vec::new();
        walk(top, 2, deadline, &mut out);
        out.into_iter().map(|c| (c.path, c.kind, c.project)).collect::<Vec<_>>()
    }) {
        found.extend(part.drain(..).map(|(path, kind, project)| Candidate { path, kind, project }));
    }
    // The user folder is also inside C:\Users, so the same project can be seen twice.
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found.dedup_by(|a, b| a.path == b.path);

    let mut items: Vec<DevItem> = found
        .into_iter()
        .map(|c| DevItem {
            label: c.path.file_name().map_or(String::new(), |n| n.to_string_lossy().to_string()),
            path: c.path,
            kind: c.kind,
            project: c.project,
            bytes: 0,
            files: 0,
            idle_days: None,
        })
        .collect();
    if with_caches {
        for (path, label) in cache_dirs() {
            items.push(DevItem { path, kind: Kind::PackageCache, label: label.to_string(), project: String::new(), bytes: 0, files: 0, idle_days: None });
        }
    }
    let now = SystemTime::now();
    let measured = par_map(&items, |it| {
        let (files, bytes) = measure(&it.path);
        let idle = if it.kind == Kind::PackageCache { None } else { it.path.parent().and_then(|p| idle_days(p, now)) };
        (files, bytes, idle)
    });
    for (it, (files, bytes, idle)) in items.iter_mut().zip(measured) {
        it.files = files;
        it.bytes = bytes;
        it.idle_days = idle;
    }
    items.retain(|it| it.bytes > 0);
    items.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    items
}

/// Everything on this PC (takes a few seconds; read-only).
pub fn scan() -> Vec<DevItem> {
    scan_roots(&search_roots(), true)
}

/// Is `path` still a developer folder we may delete? Returns its kind.
pub fn validate(path: &Path) -> Result<Kind, String> {
    if !path.is_absolute() || path.components().count() < 3 {
        return Err("not a developer folder".into());
    }
    let md = std::fs::symlink_metadata(path).map_err(|_| "the folder is already gone".to_string())?;
    if !md.is_dir() || is_reparse(&md) {
        return Err("not a developer folder".into());
    }
    let lower = path.to_string_lossy().to_lowercase();
    if cache_dirs().iter().any(|(p, _)| p.to_string_lossy().to_lowercase() == lower) {
        return Ok(Kind::PackageCache);
    }
    if forbidden(path) {
        return Err("that folder belongs to Windows or another user".into());
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).ok_or("not a developer folder")?;
    let parent = path.parent().ok_or("not a developer folder")?;
    let siblings: Vec<String> = std::fs::read_dir(parent)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.metadata().is_ok_and(|m| !m.is_dir()))
        .map(|e| e.file_name().to_string_lossy().to_ascii_lowercase())
        .collect();
    classify(&name, &siblings, || path.join("pyvenv.cfg").is_file()).ok_or_else(|| "its project file is missing, so it was left alone".to_string())
}

fn remove_tree(dir: &Path, rep: &mut CleanReport) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let path = e.path();
        let Ok(md) = e.metadata() else {
            rep.skipped += 1;
            continue;
        };
        if is_reparse(&md) {
            // Remove the link itself; whatever it points to is left alone.
            let r = if md.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0 { std::fs::remove_dir(&path) } else { std::fs::remove_file(&path) };
            if r.is_err() {
                rep.skipped += 1;
            }
        } else if md.is_dir() {
            remove_tree(&path, rep);
            let _ = std::fs::remove_dir(&path);
        } else {
            let len = md.len();
            let mut ok = std::fs::remove_file(&path).is_ok();
            if !ok && md.permissions().readonly() {
                let mut p = md.permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                p.set_readonly(false);
                ok = std::fs::set_permissions(&path, p).is_ok() && std::fs::remove_file(&path).is_ok();
            }
            if ok {
                rep.files += 1;
                rep.bytes += len;
            } else {
                rep.skipped += 1;
            }
        }
    }
}

/// Deletes one developer folder after checking it again.
pub fn delete(path: &Path) -> Result<CleanReport, String> {
    validate(path)?;
    let mut rep = CleanReport::default();
    remove_tree(path, &mut rep);
    let _ = std::fs::remove_dir(path);
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn folders_need_their_project_file() {
        assert_eq!(classify("node_modules", &names(&["package.json"]), || false), Some(Kind::NodeModules));
        assert_eq!(classify("node_modules", &names(&["readme.md"]), || false), None);
        assert_eq!(classify("target", &names(&["cargo.toml", "cargo.lock"]), || false), Some(Kind::RustTarget));
        assert_eq!(classify("target", &names(&["pom.xml"]), || false), None, "Maven target dirs are not ours to judge");
        assert_eq!(classify("build", &names(&["build.gradle.kts"]), || false), Some(Kind::GradleBuild));
        assert_eq!(classify("build", &names(&["makefile"]), || false), None);
        assert_eq!(classify("obj", &names(&["app.csproj"]), || false), Some(Kind::DotnetBuild));
        assert_eq!(classify(".next", &names(&["package.json"]), || false), Some(Kind::WebCache));
        assert_eq!(classify(".venv", &[], || true), Some(Kind::PythonVenv));
        assert_eq!(classify("env", &[], || false), None, "a folder called env without pyvenv.cfg is left alone");
        assert_eq!(classify("src", &names(&["package.json"]), || true), None);
    }

    fn file(p: &Path, bytes: usize) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![1u8; bytes]).unwrap();
    }

    #[test]
    fn finds_measures_and_deletes_safely() {
        // Not under %TEMP%: AppData is off limits to the scanner by design.
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(r"target\test-fixtures\oco-dev-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("code");
        let outside = base.join("precious");
        // A Node project with a link inside node_modules (like pnpm).
        file(&root.join(r"web\package.json"), 10);
        file(&root.join(r"web\node_modules\a\index.js"), 1000);
        file(&root.join(r"web\node_modules\b\deep\x.js"), 2000);
        file(&outside.join("keep.txt"), 300);
        let st = std::process::Command::new("cmd").args(["/c", "mklink", "/J"]).arg(root.join(r"web\node_modules\linked")).arg(&outside).output().unwrap();
        assert!(st.status.success(), "{st:?}");
        // A Rust project two levels down, with a read-only file in target.
        file(&root.join(r"tools\cli\Cargo.toml"), 10);
        file(&root.join(r"tools\cli\target\debug\cli.exe"), 500);
        let ro = root.join(r"tools\cli\target\debug\lock");
        file(&ro, 5);
        let mut perm = fs::metadata(&ro).unwrap().permissions();
        perm.set_readonly(true);
        fs::set_permissions(&ro, perm).unwrap();
        // node_modules without package.json, and one hidden in .git: ignored.
        file(&root.join(r"notes\node_modules\x.js"), 100);
        file(&root.join(r"web\.git\node_modules\y.js"), 100);

        let items = scan_roots(&[root.clone()], false);
        let got: Vec<(String, Kind, u64)> = items.iter().map(|i| (i.project.clone(), i.kind, i.bytes)).collect();
        assert_eq!(got, vec![("web".into(), Kind::NodeModules, 3000), ("cli".into(), Kind::RustTarget, 505)], "{items:#?}");
        assert_eq!(items[0].idle_days, Some(0));
        assert!(!items[0].default_on(), "a project used today is not ticked");

        // Refused: no project file, outside folder, relative path.
        assert!(delete(&root.join(r"notes\node_modules")).is_err());
        assert!(delete(&outside).is_err());
        assert!(delete(Path::new("node_modules")).is_err());

        let rep = delete(&items[0].path).unwrap();
        assert_eq!((rep.files, rep.bytes), (2, 3000));
        assert!(!root.join(r"web\node_modules").exists());
        assert!(outside.join("keep.txt").exists(), "the link's target is untouched");
        assert!(root.join(r"web\package.json").exists());
        let rep = delete(&items[1].path).unwrap();
        assert_eq!((rep.files, rep.bytes, rep.skipped), (2, 505, 0), "read-only files are deleted too");
        assert!(root.join(r"tools\cli\Cargo.toml").exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn system_folders_are_off_limits() {
        assert!(skip_dir("Windows") && skip_dir("AppData") && skip_dir(".git") && skip_dir("Program Files (x86)"));
        assert!(!skip_dir("projects"));
        assert!(forbidden(Path::new(r"C:\Program Files\app\node_modules")));
        if let Some(windir) = env_path("SystemRoot") {
            assert!(forbidden(&windir.join("node_modules")));
        }
    }
}
