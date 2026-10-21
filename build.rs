// 1. Embed an application manifest so the window gets modern common controls
//    (checkbox list views, themed header) and never asks for admin on launch.
// 2. Compile resources.rc (the app and tray icons) with the Windows SDK's rc.exe.
use std::path::PathBuf;
use std::process::Command;

fn find_rc() -> Option<PathBuf> {
    if let Some(rc) = std::env::var_os("RC") {
        return Some(PathBuf::from(rc));
    }
    let kits = PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&kits)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path().join(r"x64\rc.exe")))
        .filter(|p| p.exists())
        .collect();
    versions.sort();
    versions.pop()
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=resources.rc");
    println!("cargo:rerun-if-changed=assets");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTUAC:level='asInvoker' uiAccess='false'");
    println!(
        "cargo:rustc-link-arg-bins=/MANIFESTDEPENDENCY:type='win32' \
         name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
         processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
    );

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("resources.res");
    match find_rc() {
        Some(rc) => {
            let ok = Command::new(&rc)
                .args(["/nologo", "/fo"])
                .arg(&out)
                .arg("resources.rc")
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if ok {
                println!("cargo:rustc-link-arg-bins={}", out.display());
            } else {
                println!("cargo:warning=rc.exe failed; building without embedded icons");
            }
        }
        None => println!("cargo:warning=rc.exe not found (install the Windows SDK); building without embedded icons"),
    }
}
