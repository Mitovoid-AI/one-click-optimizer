# OneClick Optimizer

A tiny Windows app that makes your PC faster in one click. It finds what is
slowing the PC (memory hogs, idle developer tools, junk files, apps that start
by themselves, health problems), explains each one in plain words, and fixes
them with a single **Optimise now** button.

Written in Rust with raw Win32 / GDI+: no browser engine and no UI framework.

| Part | What it is | Memory it uses |
|---|---|---|
| `OneClickOptimizer.exe` | The window | ~4 MB private |
| `OneClickOptimizer.exe agent` | Tray watch + quick panel | ~2 MB idle, ~4 MB with the panel open |
| `oneclick-cli.exe` | Text / JSON report | runs and exits |

## Download

Get **`OneClickOptimizer-Setup.exe`** from the [latest release](https://github.com/Mitovoid-AI/one-click-optimizer/releases/latest). It's about 1.2 MB. The website is at https://mitovoid-ai.github.io/one-click-optimizer/.

| File | What it is |
|---|---|
| `OneClickOptimizer-Setup.exe` | Installer: Start menu, desktop shortcut, tray watch, uninstall from Settings |
| `OneClickOptimizer.exe` | Portable app that runs without installing |
| `oneclick-cli.exe` | Command-line report |
| `SHA256SUMS.txt` | Checksums (the app uses them to verify updates) |

## Installing (for a new user)

1. The user downloads `OneClickOptimizer-Setup.exe` and double-clicks it.
   - Windows SmartScreen may say "Windows protected your PC" because the file isn't code-signed yet. Click **More info**, then **Run anyway**.
2. The setup screen offers two options:
   - Desktop shortcut
   - Watch memory from the tray (starts with Windows)
3. They press **Install**.
   - It's per-user, so there's no admin prompt.
   - It takes about a second.
4. The "All set!" screen lists where to find the app. **Open OneClick Optimizer** starts it.

What they'll find afterwards:

| Where | What |
|---|---|
| Start menu | "OneClick Optimizer" (search works) |
| Desktop | OneClick Optimizer icon (if chosen) |
| Next to the clock | Tray icon with the coloured status dot; click it for the quick panel (if chosen) |
| Settings → Apps → Installed apps | "OneClick Optimizer" with its version and an **Uninstall** button |
| Task Manager → Startup apps | "OneClickOptimizer" (only if the tray option is on) |
| `%LOCALAPPDATA%\Programs\OneClickOptimizer` | The app itself |
| `%LOCALAPPDATA%\OneClickOptimizer` | History (undo journal) and the tray log |

**Updates are automatic.** The app checks GitHub Releases each time it opens, and the tray watch checks every 6 hours. When there's a newer version, it asks:

- **Update now:** downloads the new version and checks its SHA-256 against the release before running it. A file that doesn't match is never run. Then it installs the update (keeping your choices and history) and restarts the app.
- **Skip this version:** doesn't ask again until an even newer version comes out.
- **Later:** asks again next time.

From the tray you can also right-click → **Check for updates**. Portable copies update themselves in place.

**Uninstalling:** Settings → Apps → OneClick Optimizer → Uninstall. This removes the app, the shortcuts, the tray icon, the startup entry and the history. Changes the app made to the PC stay as they are; undo them first in History & undo if wanted.

**Portable use:** "Just run it without installing" on the setup screen, or rename the file so its name doesn't contain "Setup".

## Using it

1. **Open `OneClickOptimizer.exe` and press the big Scan now button.** The deep scan takes about 3–5 seconds and shows each step as it runs.
2. **Read the result:**
   - A **health score** out of 100, with an estimate of what it will be after Optimise.
   - Four cards: memory you can free, junk files, apps that start by themselves, and health warnings.
   - **What Optimise will do**: the ticked rows are safe. Click a row to read why. Untick anything you want to keep.
3. **Press Optimise now.** It closes idle programs, cleans junk files, and stops unneeded apps and services from starting by themselves.
   - If anything needs admin rights, Windows asks once.
   - It doesn't ask for confirmation unless you ticked something extra.
4. The **Done** screen shows what was freed and the new health score.

The sidebar holds the detail pages:
- **Junk files**: every category with its size. The risky ones (browser cache, developer caches, Recycle Bin, Windows Update downloads) are unticked.
- **Developer junk**: `node_modules`, Rust `target`, Gradle `build` and `.gradle`, Python virtual environments, .NET `bin`/`obj`, Next.js/Nuxt/Angular caches, and the shared Gradle, Maven, NuGet, Cargo, pnpm and Bun caches.
  - It searches your user folder and every fixed drive (never Windows, Program Files, ProgramData, AppData or other users' folders).
  - A folder only counts when its project file sits next to it: `node_modules` needs a `package.json`, `target` needs a `Cargo.toml`, and so on. The project file is checked again right before deleting.
  - Links are never followed, so pnpm's linked packages stay safe.
  - Projects nobody changed for 30+ days are ticked. Package caches are never ticked for you.
- **Apps using memory**: close any app.
- **Start with Windows**: switch apps on or off at sign-in.
- **Background services**: set services to start only when needed.
- **Autopilot & Focus**: scheduled clean-ups and Focus mode (see below).
- **History & undo**: everything the app changed, with undo for settings changes.

### Autopilot (scheduled clean-up)

Runs from the tray watch, daily or weekly, once the PC has been idle for 5 minutes.

- **What it does.** It deletes temporary files older than 7 days and crash dumps, and closes Gradle and Kotlin build daemons that sat idle. Dev servers (Vite, Next.js, Expo, nodemon...) and adb are closed only after you turn on **Also close idle dev servers and adb**.
- **When a program counts as idle.** It closes a program only if all of these hold:
  - The tray watch recorded it using under 2 seconds of CPU over the last 30 minutes. It samples every 5 minutes; a 2-second check isn't enough.
  - Nothing is connected to it, such as a browser tab on a dev server, a build talking to its daemon, or a phone on a test server.
  - You haven't picked it in History and chosen **Never close it automatically**. Kept programs are also never ticked for one-click Optimise. Clear the list on the Autopilot & Focus page.
- **What it never does.** It never closes apps you use, never touches startup apps or services, and never asks for admin rights.
- **When it waits.** It skips its turn on a low battery, during presentations and full-screen games, and while Focus mode is on.
- **Afterwards.** Everything goes into History, and a notification says what was freed. "Run now" on the page runs it right away.

### Focus mode

One switch, on the Autopilot & Focus page, in the tray panel or in the tray menu, gives the apps you're using all of the PC's power:

- **What slows down.** Busy background programs go into Windows' efficiency mode, the same as Task Manager's: lower priority and EcoQoS.
- **Power mode.** It switches to Best performance.
- **Never slowed:**
  - apps with a window on screen, and everything they started (a build run from your editor, a game's helpers);
  - browsers, calls, music, recording and anti-cheat programs;
  - Windows itself.
- **While it's on.** Every 30 seconds the tray watch gives full speed back to apps you switch to, and slows programs that became busy.
- **Turning it off.** Everything goes back. The state is saved in `focus.json`, so a crash or restart never leaves anything slowed or the power mode changed.

### Notifications

The tray watch shows its own small notifications above the tray, each with one button:
- memory is almost full (**Optimise**)
- idle dev tools (**Free it**)
- a new version (**See what's new**)
- Autopilot results (**See details**)

They wait while you present or play full-screen. A new version opens the update window: what's new, then **Update now**, **Later** or **Skip this version**, with a progress bar while it downloads.

### Tray watch and quick panel

Flip **Watch in tray** in the sidebar, or run `OneClickOptimizer.exe agent`.

- **Left-click** the tray icon to open the quick panel. It shows memory use, the 3 biggest apps, what one click will do, and an **Optimise now** button.
- **Right-click** the icon for Quick optimise, Open the app, Focus mode, Start with Windows, Check for updates, or Exit.
- It warns you once when memory has been critical for 2 minutes, and once when dev tools sit idle.
- Its activity is logged to `%LOCALAPPDATA%\OneClickOptimizer\agent.log`.

### Pro ($10 once, no account)

Pro switches on by itself once the Polar IDs are filled in (`POLAR` in `src/license.rs`). Until then every feature is free.

| | Free | Pro |
|---|---|---|
| Scan, health score, every detail page, fixes one by one, undo | Yes | Yes |
| One-click Optimise (window and tray) | 3 times | Unlimited |
| Developer junk | See it | Clean it |
| Autopilot (scheduled clean-ups) | | Yes |
| Focus mode | | Yes |

- **Buy Pro** opens Polar's checkout in the browser. Polar is the merchant of record and handles payment, tax and receipts. The buyer only gives an email address and gets a license key.
- **Activating** means pasting the key on the Pro page. The app checks it once with the store's public license API, which needs no secret in the app.
- **Storage and re-checks.** The key is saved encrypted with Windows DPAPI and tied to this PC. It is re-checked quietly about every 14 days. Being offline never switches Pro off; only a refunded or disabled key does.
- **Moving PCs.** "Move Pro to another PC" frees the activation. One key works on up to 3 PCs.
- **What goes to the store.** Only the key and an anonymous PC tag (derived from the Windows installation ID). The PC name is not sent.

See [SELLING.md](SELLING.md) for setting up the store and launching.

### Command line

```
oneclick-cli scan            # deep-scan report (read-only)
oneclick-cli scan --json     # same, machine-readable
oneclick-cli dev             # developer junk on this PC (read-only)
oneclick-cli bench --json    # measurements used by the website (read-only scans + a ~20 s Focus test)
oneclick-cli history         # changes made so far
```

## Website

`docs/` is the website, live at https://oneclickoptimizer.vercel.app/. It has real screenshots, charts drawn from `docs/data/benchmarks.json`, and the installer itself (`docs/download/OneClickOptimizer-Setup.exe`), so visitors never land on GitHub.

- **Hosting:** Vercel is connected to this repository with **Root Directory `docs`**, so only that folder is published. Every push to `main` redeploys it.
- **Downloads:** the release workflow copies each new installer into `docs/download` on `main` (one small bot commit per release), and Vercel redeploys. To refresh it by hand, run `tools\deploy-site.ps1`. It fetches the latest release, checks it against the release's SHA-256 checksums, and copies it in. Commit and push the result.
- **Visitor counts:** turn on **Analytics → Web Analytics** in the Vercel project. The page loads Vercel's script, which sets no cookies.
- **Legal pages:** `docs/terms.html`, `privacy.html`, `refund.html` and `licenses.html` (and the site footer) are built from `legal/` and `THIRD-PARTY-NOTICES.txt`. Edit the wording there, then run `python tools/legal-pages.py`. Your seller name, city and contact email live at the top of that script.
- **Refresh the measurements on your own PC:**
  ```
  cargo build --release
  powershell -ExecutionPolicy Bypass -File tools\bench.ps1
  ```
  The script runs `oneclick-cli bench`. It also measures the window and tray watch from a throwaway data folder, and rewrites `docs/data/benchmarks.json` and `.js`.

## What the deep scan checks

| Area | What it finds |
|---|---|
| Memory | Idle Gradle/Kotlin daemons and adb, forgotten or duplicate Vite / Next / webpack / Expo dev servers (with their npx launcher and esbuild helpers), background helpers (Lenovo Vantage, Xbox, Widgets, Edge Game Assist, Intel DSA tray) |
| Junk files | Old temp files (7+ days), crash dumps and error reports, Windows temp (admin), browser cache, npm/pip/Yarn caches, Windows Update downloads, Recycle Bin |
| Startup | Run-key apps that start at sign-in (same switch as Task Manager) and always-on services that can start only when needed |
| Health | Days since restart, low space on C:, page file turned off, handle leaks (20,000+ handles), memory growing fast during the scan, piles of open terminals, heavy browser use, and **RAM upgrade advice** read from the memory chips (soldered vs. slot, type, speed) |

## Safety rules

- **Nothing changes until you press a button.** Scanning is read-only.
- **Junk cleaning only touches known junk folders.**
  - Every folder must end in a known junk-folder name.
  - Links and junctions are never followed, so a link can't lead outside the folder.
  - Temp files must be untouched for 7+ days.
  - Files in use are skipped.
- **Protected lists.** Windows core processes, Defender, audio, Wi-Fi and hardware services (Fn keys, thermal control) are never offered.
- **No placebo "RAM boosting".** It does not trim working sets or flush caches; those make PCs slower, not faster. Memory is freed only by closing things that are really idle.
- **Process safety.** It never ends itself, and never ends a recycled PID: every kill checks the process start time first.
- **Undo journal.** Every change is recorded in `%LOCALAPPDATA%\OneClickOptimizer\journal.json`. Settings changes can be undone; closed apps simply start again, and deleted junk is gone.
- **Admin only when needed.** Admin changes run in a short-lived elevated copy of the exe. The plan is passed on its command line, so there is no temp file to tamper with.

## Publishing a release

Bump `version` in `Cargo.toml`, commit, then push a matching tag:

```
git tag v0.3.0
git push origin v0.3.0
```

`.github/workflows/release.yml` then runs on GitHub:
1. Checks that the tag matches `Cargo.toml`.
2. Runs all the tests.
3. Builds the exes.
4. Publishes a release with the installer, the portable exe, the CLI and `SHA256SUMS.txt`.

Installed copies see the new release and offer the update.


## Building and testing

```
cargo build --release      # target\release\OneClickOptimizer.exe, oneclick-cli.exe
cargo test --release       # unit + end-to-end tests (no admin needed)
python tools/make_icon.py  # regenerate assets\*.ico (Pillow)
```

The tests never touch your real files or settings:
- The junk cleaner is tested on throwaway folders, including a junction that must not be followed.
- Kill tests use a throwaway `ping` process.
- Startup tests flip a made-up startup entry name and then undo it.

Test switches:
- `OCO_DRY_RUN=1` makes Optimise simulate everything and change nothing.
- `OCO_OFFSCREEN=1` opens the window off-screen for screenshot tests.
- `OCO_TICK_MS` speeds up the tray timer.
- `OCO_TRACE=1` prints per-category junk scan times.
- `OCO_FAKE_VERSION=0.1.0` makes the app think it's older, to try the update prompt.
- `OCO_NO_UPDATE_CHECK=1` turns the automatic update check off.
- `OCO_INSTALL_ROOT` and `OCO_DATA_DIR` install into, and keep history, settings and the license in, throwaway folders.
- `cargo build --release --features license-mock --target-dir target/mock` builds a copy with an offline fake store (key `OCO-TEST-VALID-KEY`) to try the Pro flow. It is never used for releases.
- With `OCO_DRY_RUN=1`, Focus mode only works out what it would slow down and changes nothing, and Autopilot reports what it would clean.
- `OCO_DEV_ROOTS=<folder>;<folder>` makes Developer junk search only those folders (and skip the shared caches).
- `OCO_IDLE_SECS` fakes how long the PC has been idle, for Autopilot tests.

The release workflow runs all tests before it builds a release, so a version that fails them is never published.

## Layout

```
src/
  optimize.rs   deep scan -> one-click plan -> run -> summary (shared by window and tray)
  junk.rs       junk categories, safe measuring and cleaning
  health.rs     health checks, RAM upgrade advice, health score
  analyze.rs    memory pressure, per-app totals, fix recommendations
  kb.rs         knowledge base: what each process/service/startup entry is, what is protected
  actions.rs    actions, undo journal, executor (defers admin work)
  elevate.rs    UAC helper (`OneClickOptimizer.exe apply-plan <hex>`)
  sys/          Windows access: processes, ports, services, startup, memory, SMBIOS RAM info
  ui.rs         the window
  agent.rs      tray watch + quick panel
  paint.rs      shared drawing (palette, fonts, GDI+ shapes, hover buttons)
  icon.rs       app and tray icons
  install.rs    per-user install/uninstall (shortcuts, Settings > Apps entry, startup entry)
  setup.rs      setup screen, update mode (--update), uninstall flow (--uninstall)
  update.rs     GitHub Releases check, Update now / Skip / Later prompt, verified download
  license.rs    Pro license: Polar (and Dodo / Lemon Squeezy) license API, DPAPI storage
  settings.rs   small settings file (skipped version, free runs used, Autopilot schedule)
  devclean.rs   developer junk: find, measure and safely delete
  autopilot.rs  scheduled clean-up: when it is due, what it cleans
  focus.rs      Focus mode: efficiency mode for background programs, power mode, restore
  popup.rs      the app's notifications and the update window
  web.rs        HTTPS GET/POST (WinINet)
legal/          EULA, privacy and refund policy (published by tools/legal-pages.py)
  report.rs     text/JSON report
```
