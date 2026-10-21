//! Knowledge base: what known processes, services and startup entries are,
//! whether they are safe to touch, and how to explain them in plain words.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcKind {
    GradleDaemon,
    KotlinDaemon,
    Adb,
    /// A front-end dev server (Vite, Next.js, ...).
    DevServer(&'static str),
    /// `npx` / `npm run` launcher that only waits on a dev server child.
    NpmWrapper,
    Helper {
        label: &'static str,
        why: &'static str,
    },
}

const DEV_SERVERS: &[(&str, &str)] = &[
    (r"\vite\bin\vite", "Vite"),
    (r"\next\dist\bin\next", "Next.js"),
    ("webpack-dev-server", "webpack"),
    (r"\webpack\bin\webpack.js serve", "webpack"),
    (r"react-scripts\scripts\start", "Create React App"),
    (r"\astro\astro.js", "Astro"),
    (r"\nuxi\", "Nuxt"),
    (r"\@angular\cli\bin\ng", "Angular"),
    (r"\@expo\cli", "Expo"),
    (r"\expo\bin\cli", "Expo"),
    (r"\nodemon\", "nodemon"),
    (r"\@sveltejs\kit", "SvelteKit"),
    (r"\parcel\lib\bin", "Parcel"),
];

const HELPERS: &[(&str, &str, &str)] = &[
    (
        "xboxpctray.exe",
        "Xbox app tray",
        "Keeps the Xbox app loaded in the background even when you aren't gaming.",
    ),
    (
        "edgegameassist.exe",
        "Edge Game Assist",
        "Edge's in-game browser overlay. Only useful while playing PC games.",
    ),
    (
        "widgets.exe",
        "Widgets board",
        "Loads news and weather in a hidden browser. It comes back if you open Widgets (Win+W).",
    ),
    (
        "messagingplugin.exe",
        "Lenovo UDC messaging plugin",
        "Lenovo telemetry and messaging helper with no visible feature.",
    ),
    (
        "lenovovantage-",
        "Lenovo Vantage add-ins",
        "Background add-ins for Lenovo Vantage (telemetry, offers, diagnostics). Vantage starts them again when you open it.",
    ),
    (
        "phoneexperiencehost.exe",
        "Phone Link",
        "Keeps Phone Link connected in the background. It opens again when you launch Phone Link.",
    ),
    (
        "gamebar.exe",
        "Xbox Game Bar",
        "Game overlay (Win+G). It starts again when you press Win+G.",
    ),
    (
        "gamebarftserver.exe",
        "Xbox Game Bar",
        "Game overlay (Win+G). It starts again when you press Win+G.",
    ),
    (
        "dsatray.exe",
        "Intel Driver & Support Assistant tray",
        "Checks for Intel driver updates in the background. Open Intel DSA yourself when you want to update drivers.",
    ),
    (
        "mumuplayerremotebackend.exe",
        "MuMuPlayer remote backend",
        "Android-emulator helper that runs even while the emulator is closed.",
    ),
];

fn norm(s: &str) -> String {
    s.to_ascii_lowercase().replace('/', "\\")
}

fn matches_entry(name: &str, pattern: &str) -> bool {
    if pattern.ends_with('-') || pattern.ends_with('*') {
        name.starts_with(pattern.trim_end_matches('*'))
    } else {
        name == pattern
    }
}

pub fn is_npm_wrapper(name: &str, cmdline: &str) -> bool {
    let c = norm(cmdline);
    is_js_runtime(name)
        && ["npx-cli.js", "npm-cli.js", "pnpm.cjs", "pnpm.js", "yarn.js", "yarn.cjs"]
            .iter()
            .any(|w| c.contains(w))
}

fn is_js_runtime(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "node.exe" | "bun.exe" | "deno.exe"
    )
}

pub fn classify_process(name: &str, cmdline: &str) -> Option<ProcKind> {
    let n = name.to_ascii_lowercase();
    let c = norm(cmdline);
    match n.as_str() {
        "java.exe" | "javaw.exe" => {
            if c.contains("gradledaemon") || c.contains("org.gradle.launcher.daemon") {
                Some(ProcKind::GradleDaemon)
            } else if c.contains("kotlincompiledaemon") || c.contains("kotlin-daemon") {
                Some(ProcKind::KotlinDaemon)
            } else {
                None
            }
        }
        "adb.exe" => Some(ProcKind::Adb),
        _ if is_js_runtime(&n) => {
            if is_npm_wrapper(&n, cmdline) {
                return Some(ProcKind::NpmWrapper);
            }
            DEV_SERVERS
                .iter()
                .find(|(needle, _)| c.contains(needle))
                .map(|(_, tool)| ProcKind::DevServer(tool))
        }
        _ => HELPERS
            .iter()
            .find(|(pat, _, _)| matches_entry(&n, pat))
            .map(|&(_, label, why)| ProcKind::Helper { label, why }),
    }
}

/// `...\my-shop\node_modules\.bin\..\vite\bin\vite.js` -> `my-shop`.
pub fn project_from_cmdline(cmdline: &str) -> Option<String> {
    let c = cmdline.replace('/', "\\");
    let idx = c.to_ascii_lowercase().find("\\node_modules\\")?;
    let before = &c[..idx];
    let name = before.rsplit(['\\', '"', '\'']).next()?.trim();
    if name.is_empty() || name.ends_with(':') {
        None
    } else {
        Some(name.to_string())
    }
}

/// Processes Windows (or the user's safety) depends on. Never offered for killing.
pub fn is_protected_process(name: &str) -> bool {
    const P: &[&str] = &[
        "system",
        "system idle process",
        "registry",
        "secure system",
        "memory compression",
        "smss.exe",
        "csrss.exe",
        "wininit.exe",
        "winlogon.exe",
        "services.exe",
        "lsass.exe",
        "lsaiso.exe",
        "svchost.exe",
        "dwm.exe",
        "explorer.exe",
        "fontdrvhost.exe",
        "sihost.exe",
        "ctfmon.exe",
        "audiodg.exe",
        "spoolsv.exe",
        "searchindexer.exe",
        "msmpeng.exe",
        "mpdefendercoreservice.exe",
        "nissrv.exe",
        "securityhealthservice.exe",
        "securityhealthsystray.exe",
        "smartscreen.exe",
        "wudfhost.exe",
        "taskhostw.exe",
        "runtimebroker.exe",
        "startmenuexperiencehost.exe",
        "shellexperiencehost.exe",
        "textinputhost.exe",
        "conhost.exe",
        "dllhost.exe",
        "wmiprvse.exe",
        "oneclickoptimizer.exe",
        "oneclick-cli.exe",
        "ramdoctor.exe",
        "ramdoctor-cli.exe",
    ];
    let n = name.to_ascii_lowercase();
    P.contains(&n.as_str())
}

const SERVICES: &[(&str, &str)] = &[
    ("lenovovantageservice", "Runs Lenovo Vantage's add-ins all day (telemetry, offers, update checks). Vantage can still be opened when you need it."),
    ("udcservice", "Lenovo Universal Device Client: telemetry and messaging, no visible feature."),
    ("imcontrollerservice", "Lenovo System Interface Foundation: loads Vantage plugins in the background."),
    ("elevocservice", "Lenovo AI microphone noise cancelling. The microphone still works without it."),
    ("agmservice", "Adobe Genuine Service: a licence checker. Useless if you have no Adobe apps."),
    ("adobearmservice", "Adobe updater. Adobe apps check for updates when you open them."),
    ("anydesk", "Keeps AnyDesk listening for connections all day. AnyDesk still opens normally."),
    ("mumuplayerremoteservice", "MuMuPlayer helper that runs even while the emulator is closed."),
    ("clicktorunsvc", "Office's background updater. Word, Excel and the other Office apps start it when you open them."),
    ("postgresql-*", "PostgreSQL database server. Start it only when you work on a project that needs it."),
    ("mysql*", "MySQL database server. Start it only when you work on a project that needs it."),
    ("mongodb", "MongoDB database server. Start it only when you work on a project that needs it."),
    ("mssql*", "SQL Server database engine. Start it only when you work on a project that needs it."),
    ("com.docker.service", "Docker Desktop's background service. Docker Desktop starts it when you open it."),
    ("teamviewer", "Keeps TeamViewer listening for connections all day. TeamViewer still opens normally."),
    ("dsaservice", "Intel Driver & Support Assistant background checker. Open Intel DSA when you want to update drivers."),
    ("dsaupdateservice", "Keeps Intel Driver & Support Assistant itself updated in the background."),
];

pub fn service_advice(name: &str) -> Option<&'static str> {
    let n = name.to_ascii_lowercase();
    SERVICES
        .iter()
        .find(|(pat, _)| matches_entry(&n, pat))
        .map(|(_, why)| *why)
}

/// Services that keep hardware or security working. Never offered.
pub fn is_protected_service(name: &str) -> bool {
    const P: &[&str] = &[
        "lenovofnandfunctionkeys",
        "litssvc",
        "windefend",
        "mdcoresvc",
        "wdnissvc",
        "securityhealthservice",
        "audiosrv",
        "audioendpointbuilder",
        "rtkaudiouniversalservice",
        "intelaudioservice",
        "esifsvc",
        "wlansvc",
        "dhcp",
        "dnscache",
        "eventlog",
        "rpcss",
        "lsm",
        "samss",
        "keyiso",
        "vaultsvc",
        "cryptsvc",
        "bfe",
        "mpssvc",
        "wuauserv",
        "trustedinstaller",
    ];
    let n = name.to_ascii_lowercase();
    P.contains(&n.as_str())
}

const STARTUP: &[&str] = &[
    "mumuplayer",
    "free download manager",
    "googlechromeautolaunch",
    "microsoftedgeautolaunch",
    "onedrivesetup",
    "lenovovantage",
    "discord",
    "spotify",
    "steam",
    "epicgameslauncher",
    "teams",
    "skype",
    "utorrent",
    "bittorrent",
    "anydesk",
    "zoom",
    "ccleaner",
    "opera",
    "adobe",
    "eadm",
    "origin",
];

pub fn startup_advice(name: &str) -> Option<&'static str> {
    let n = name.to_ascii_lowercase();
    STARTUP.iter().any(|p| n.contains(p)).then_some(
        "Starts with Windows and uses memory before you open anything. The app still works when you open it yourself.",
    )
}

pub fn is_protected_startup(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["securityhealth", "rtkaud", "windowsdefender", "ramdoctor", "oneclickoptimizer"]
        .iter()
        .any(|p| n.contains(p))
}

/// Friendly name for the "Apps by memory" view.
pub fn app_label(exe: &str) -> &'static str {
    match exe.to_ascii_lowercase().trim_end_matches(".exe") {
        "chrome" => "Google Chrome",
        "msedge" => "Microsoft Edge",
        "msedgewebview2" => "Edge WebView (browser inside other apps)",
        "brave" => "Brave browser",
        "firefox" => "Firefox",
        "claude" => "Claude",
        "code" => "VS Code",
        "cursor" => "Cursor editor",
        "zed" => "Zed editor",
        "node" => "Node.js (dev servers, tools)",
        "python" | "pythonw" => "Python",
        "java" | "javaw" => "Java (Gradle, Android tools)",
        "ffmpeg" => "FFmpeg (video processing)",
        "svchost" => "Windows services (shared)",
        "msmpeng" => "Microsoft Defender antivirus",
        "dwm" => "Desktop Window Manager",
        "explorer" => "File Explorer and taskbar",
        "memory compression" => "Windows memory compression",
        "searchhost" | "searchindexer" => "Windows Search",
        "textinputhost" => "Touch keyboard and emoji panel",
        "powershell" | "pwsh" => "PowerShell",
        "bash" => "Git Bash",
        "conhost" => "Console windows",
        "discord" => "Discord",
        "spotify" => "Spotify",
        "ms-teams" | "teams" => "Microsoft Teams",
        "vmmem" | "vmmemwsl" => "WSL / Docker virtual machine",
        "docker desktop" => "Docker Desktop",
        "adb" => "Android Debug Bridge",
        "studio64" => "Android Studio",
        "postgres" => "PostgreSQL database",
        "audiodg" => "Windows audio engine (includes sound enhancements)",
        "esbuild" => "esbuild (bundler used by Vite dev servers)",
        "startmenuexperiencehost" => "Start menu",
        "shellexperiencehost" => "Taskbar and notification panels",
        "systemsettings" => "Settings app",
        "applicationframehost" => "Window frames for Store apps",
        "dsatray" => "Intel Driver & Support Assistant tray",
        "lenovovantageservice" => "Lenovo Vantage",
        "wsl" | "wslservice" => "Windows Subsystem for Linux",
        "msmpengcp" | "mpdefendercoreservice" => "Microsoft Defender",
        "onedrive" => "OneDrive",
        "ms-teams_modulehost" => "Microsoft Teams",
        "phoneexperiencehost" => "Phone Link",
        "widgets" | "widgetservice" => "Windows Widgets",
        "oneclickoptimizer" | "ramdoctor" => "OneClick Optimizer (this app)",
        "wlanext" => "Wi-Fi driver helper (Windows)",
        "esrv_svc" | "esrv" => "Intel energy/telemetry service",
        "mumunxmain" | "mumuplayer" => "MuMuPlayer Android emulator",
        "vlc" => "VLC media player",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VITE: &str = r#""node"   "D:\projects\my-shop\node_modules\.bin\\..\vite\bin\vite.js" --port 5300 --strictPort"#;
    const NPX: &str = r#""C:\Program Files\nodejs\node.exe" C:\Users\a\AppData\Roaming\npm/node_modules/npm/bin/npx-cli.js vite --port 5300"#;

    #[test]
    fn classifies_build_daemons() {
        let gradle = r"java.exe -cp gradle-launcher-8.2.1.jar org.gradle.launcher.daemon.bootstrap.GradleDaemon 8.2.1";
        assert_eq!(classify_process("java.exe", gradle), Some(ProcKind::GradleDaemon));
        assert_eq!(
            classify_process("java.exe", "java -cp kotlin-compiler.jar org.jetbrains.kotlin.daemon.KotlinCompileDaemon"),
            Some(ProcKind::KotlinDaemon)
        );
        assert_eq!(classify_process("java.exe", "java -jar app.jar"), None);
        assert_eq!(classify_process("adb.exe", "adb -L tcp:5037 fork-server server"), Some(ProcKind::Adb));
    }

    #[test]
    fn classifies_dev_servers_and_wrappers() {
        assert_eq!(classify_process("node.exe", VITE), Some(ProcKind::DevServer("Vite")));
        assert_eq!(classify_process("node.exe", NPX), Some(ProcKind::NpmWrapper));
        assert_eq!(
            classify_process("node.exe", r"node C:\app\node_modules\next\dist\bin\next dev"),
            Some(ProcKind::DevServer("Next.js"))
        );
        assert_eq!(classify_process("node.exe", "node server.js"), None);
        assert_eq!(classify_process("chrome.exe", "chrome.exe --type=renderer"), None);
    }

    #[test]
    fn classifies_helpers_by_prefix() {
        match classify_process("LenovoVantage-(GenericTelemetryAddin).exe", "") {
            Some(ProcKind::Helper { label, .. }) => assert_eq!(label, "Lenovo Vantage add-ins"),
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(
            classify_process("XboxPcTray.exe", ""),
            Some(ProcKind::Helper { .. })
        ));
    }

    #[test]
    fn extracts_project_name() {
        assert_eq!(project_from_cmdline(VITE).as_deref(), Some("my-shop"));
        assert_eq!(
            project_from_cmdline("node /home/me/shop/node_modules/vite/bin/vite.js").as_deref(),
            Some("shop")
        );
        assert_eq!(project_from_cmdline("node server.js"), None);
    }

    #[test]
    fn protection_lists() {
        assert!(is_protected_process("svchost.exe"));
        assert!(is_protected_process("MsMpEng.exe"));
        assert!(!is_protected_process("node.exe"));
        assert!(is_protected_service("LenovoFnAndFunctionKeys"));
        assert!(is_protected_service("LITSSVC"));
        assert!(!is_protected_service("UDCService"));
        assert!(is_protected_startup("SecurityHealth"));
        assert!(is_protected_startup("RtkAudUService"));
    }

    #[test]
    fn service_and_startup_advice() {
        assert!(service_advice("AGMService").is_some());
        assert!(service_advice("postgresql-x64-18").is_some());
        assert!(service_advice("Spooler").is_none());
        assert!(startup_advice("GoogleChromeAutoLaunch_244F4A54").is_some());
        assert!(startup_advice("MuMuPlayerGlobal").is_some());
        assert!(startup_advice("SecurityHealth").is_none());
    }
}
