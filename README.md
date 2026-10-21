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

## Installing (for a new user)

Share one file: **`dist\OneClickOptimizer-Setup.exe`** (about 900 KB). Make it with `tools\package.ps1`.

1. The user double-clicks it.
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
| Settings → Apps → Installed apps | "OneClick Optimizer 0.2.0" with an **Uninstall** button |
| Task Manager → Startup apps | "OneClickOptimizer" (only if the tray option is on) |
| `%LOCALAPPDATA%\Programs\OneClickOptimizer` | The app itself |
| `%LOCALAPPDATA%\OneClickOptimizer` | History (undo journal) and the tray log |

**Updating:** run a newer `OneClickOptimizer-Setup.exe`. It closes the running app, replaces it, and keeps the history.

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
- **Apps using memory**: close any app.
- **Start with Windows**: switch apps on or off at sign-in.
- **Background services**: set services to start only when needed.
- **History & undo**: everything the app changed, with undo for settings changes.

### Tray watch and quick panel

Flip **Watch in tray** in the sidebar, or run `OneClickOptimizer.exe agent`.

- **Left-click** the tray icon to open the quick panel. It shows memory use, the 3 biggest apps, what one click will do, and an **Optimise now** button.
- **Right-click** the icon for Quick optimise, Open the app, Start with Windows, or Exit.
- It warns you once when memory has been critical for 2 minutes, and once when dev tools sit idle.
- Its activity is logged to `%LOCALAPPDATA%\OneClickOptimizer\agent.log`.

### Command line

```
oneclick-cli scan            # deep-scan report (read-only)
oneclick-cli scan --json     # same, machine-readable
oneclick-cli history         # changes made so far
```

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
  report.rs     text/JSON report
```
