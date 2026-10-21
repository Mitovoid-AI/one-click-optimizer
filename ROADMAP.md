# Roadmap: what to build next, and why

Research from October 2026: competitor pages, Trustpilot, Hacker News, Microsoft Q&A, GitHub and Microsoft docs. Items marked *(verify)* were not confirmed against a primary source.

## Where we fit

**What US and Western buyers distrust in "PC optimizers":**
- **Snake oil.** Microsoft Q&A answers call paid optimizers snake oil and warn against registry cleaners and "driver boosters".
- **Fake scans.** The FTC took $35M from Office Depot and Support.com over a "PC Health Check" that reported malware it never scanned for.
- **Bundleware and telemetry.** CCleaner was compromised in 2017. Its parent company Avast paid the FTC $16.5M for selling browsing data.
- **Subscriptions that are hard to cancel.** Trustpilot complaints about CCleaner and IObit.

**What they trust:**
- Microsoft's own tools, such as PC Manager and Autoruns.
- Small open-source tools: BleachBit, WizTree, Bulk Crap Uninstaller, and winutil, which has about 56k GitHub stars.
- Changes that can be undone, like O&O ShutUp10++'s one-click revert.
- Paying once.

**Price anchors:**

| Product | Price |
|---|---|
| Process Lasso | $24–40 lifetime |
| Wise Care 365 | $52 lifetime or $22/yr |
| IObit Advanced SystemCare | ~$30/yr |
| CCleaner Pro | ~€45/yr *(verify)* |

At **$5 once** we are an impulse buy.

**Positioning: "the honest optimizer, built for developers too."**
- Every fix shows the exact bytes it frees and can be undone.
- No registry cleaner, no RAM-boost placebo, no ads, no subscription.
- About 1 MB on disk and about 4 MB of RAM.
- It finds what the big suites ignore: idle dev servers, Gradle daemons, Docker and WSL disk bloat, and `node_modules`.

**Timing:** RAM prices are very high in 2026. A 32 GB DDR5 kit costs $350–560. "Free up the RAM you have" beats "buy more".

## Priorities

Value is the value to paying US users; Ease is how easy it is to build. Both are scored 1–5.

| # | Idea | Value | Ease | Tier |
|---|---|---|---|---|
| 1 | Developer cleaner (**built**) | 5 | 3 | Free scan, Pro clean |
| 2 | Autopilot (schedule **built**; per-app idle rules next) | 5 | 3 | Pro |
| 3 | Boot-time tracker + 30-day history + before/after proof | 4 | 5 | Mixed |
| 4 | Browser memory helper | 4 | 5 | Free |
| 5 | Focus mode (**built**; Game / Meeting presets next) | 3 | 3 | Pro |
| 6 | Drive health + Windows Update / driver status | 3 | 5 | Free |
| 7 | Battery health | 3 | 5 | Free |
| 8 | "Why is my PC slow right now?" | 4 | 2 | Pro |
| 9 | Large / old files, duplicates | 3 | 2 | Free list, Pro duplicates |
| 10 | Windows 11 AI / ads off presets, "what grew?" disk alert | 3 | 3 | Pro |
| 11 | Unused apps + leftover sweep | 3 | 2 | Pro |
| 12 | Privacy cleanup | 2 | 3 | Free |
| 13 | RAM / SSD upgrade advisor with affiliate links | 1 | 4 | Free |

**Pro launch bundle (build 1–3 first):**
- Bulk developer clean with quarantine and undo.
- WSL compaction and the `.wslconfig` tuner.
- Autopilot.
- Focus presets.
- 90-day history with before/after reports.

**Free stays genuinely useful:** scan, all manual fixes, boot time, battery, drive health, the update checklist and the browser guide. A free product that helps people is what earns trust and word of mouth.

---

## 1. Developer cleaner (built in v0.4)

Built: project folders and package caches, deleted directly after the project file is checked again. Still to do: Docker/WSL disk compaction, the `.wslconfig` tuner, Android emulator images.


**For the user:** "61 projects, 38 GB you can get back." It finds regenerable developer junk and cleans it safely.

**Evidence of demand:**
- npkill has 9.5k stars; kondo cleans 20+ ecosystems.
- One developer reclaimed about 353 GB from bloated Docker VHDX files.
- Docker's forum reports the VHDX growing about 1.5 GB per update.
- WSL takes up to 50% of host RAM by default.
- Paid Windows dev-cleaners (K8 Cruft, SpaceFree) only appeared in 2026, and no mainstream suite covers this.

**How to build:**
- **Project folders:**
  - Walk the project roots and match `node_modules` (next to `package.json`), `target` (`Cargo.toml`), `build`, `.venv`, `.next`, `dist`.
  - Measure age from the newest file *outside* the dependency folder, so a project untouched for 90 days is "stale".
- **Caches:** use each tool's own command where one exists:
  - `npm cache clean --force`, `yarn cache clean`, `pnpm store prune`
  - `pip cache purge`, `dotnet nuget locals all --clear`
  - Gradle: run `gradle --stop` first, then clear `~/.gradle/caches`.
  - `~/.m2/repository` and `~/.cargo/registry` are plain folders.
- **Android:**
  - Look at `%USERPROFILE%\.android\avd`, `Sdk\system-images` and old `AndroidStudio*` folders. One write-up freed 80 GB of 220 GB.
  - Delete through `avdmanager` and `sdkmanager`.
- **Docker and WSL:**
  - Run `docker system prune` (never volumes by default) and `wsl --shutdown`, then compact the VHDX:
    - Windows Pro: `Optimize-VHD -Mode Full` (needs the Hyper-V module).
    - Windows Home: `diskpart compact vdisk`.
  - Compaction needs admin.
  - Find distros under `HKCU\...\Lxss\{guid}\BasePath` *(verify)*.
- **`.wslconfig` tuner:**
  - Set `memory`, `processors` and `swap` under `[wsl2]`, and `autoMemoryReclaim` and `sparseVhd` under `[experimental]`.
  - Back up the file first. Changes need `wsl --shutdown`.
- **JDKs:** list them and send the user to the uninstaller. Never delete JDK folders.

**Effort:** M for scanning; L for compaction.

**Risks:**
- Offline-only or patched `node_modules` won't come back. Default to quarantine (move to a dated folder, auto-purge after 7 days) rather than delete.
- pnpm hardlinks overstate the space you get back.
- Docker volumes can hold databases.
- Unregistering a WSL distro destroys its data, so never do it.

**Bonus:** suggest a Dev Drive (ReFS), which Microsoft benchmarks as up to 30% faster for builds.

## 2. Autopilot: scheduled clean + per-app idle rules (schedule built in v0.4)

Built: daily/weekly clean-up from the tray when idle, on power, not presenting. Still to do: per-app idle rules.


**For the user:** "Every Sunday at 3 am, clean junk and close idle dev tools", plus "close or throttle X after 30 idle minutes", without opening the app.

**How to build:**
- **Schedule:**
  - Register a per-user Task Scheduler task through the Task Scheduler 2.0 COM API (`ITaskService`).
  - Use a weekly trigger plus a logon trigger, running `oneclick.exe --auto <profile>`.
  - Set `StartWhenAvailable` so missed runs catch up, and don't start on battery.
- **Idle rules:** these run in the tray, because they need live state:
  - no foreground window (`GetForegroundWindow`);
  - no input (`GetLastInputInfo`);
  - low CPU (`GetProcessTimes` deltas);
  - no open TCP connections (`GetExtendedTcpTable`);
  - no audio session.
- **Actions, gentlest first:**
  1. EcoQoS: `SetProcessInformation(ProcessPowerThrottling)`.
  2. Lower priority.
  3. `WM_CLOSE`.
  4. `TerminateProcess`, only for headless daemons.
- **Safety:** make the first run a dry run that shows a notification of what *would* happen. Write every action to the undo journal.

**Effort:** S–M for the schedule; M–L for the rules.

**Competitors:**
- CCleaner, IObit and Wise schedule only in their paid tiers *(verify)*.
- Process Lasso puts rules in its $40 Pro.

**Risks:**
- Never close an app that may hold unsaved work, like editors and Office. Only close known daemons and dev servers.
- Energy Saver can skip scheduled tasks, so log missed runs.

## 3. Boot-time tracker, 30-day history, proof reports

**For the user:** "Boot went from 48 s to 31 s after you disabled 4 startup apps", plus a 30-day memory-pressure and health trend. This makes our claims verifiable, which is the opposite of snake oil.

**How to build:**
- **Boot events:** read `Microsoft-Windows-Diagnostics-Performance/Operational` with `EvtQuery`.
  - Event 100 carries `BootTime` in ms.
  - Events 101–110 name the slow app, driver or service with its `DegradationTime`.
  - Check that non-admin users can read this log *(verify)*.
- **Medians:** store (time, BootTime, what was disabled) and compare medians over several boots, because single boots are noisy.
- **History:**
  - The tray samples `GlobalMemoryStatusEx` and PDH counters every few minutes into a binary ring file. At about 16 bytes per sample, 30 days is about 140 KB.
  - Draw it with GDI polylines. No SQLite, to keep the 1 MB exe.
- **Edge cases:**
  - Fast Startup makes "boot" a hybrid resume, so check `HiberbootEnabled`.
  - Post-update reboots are slow; mark them.
  - Times when the tray wasn't running are "unknown", not zero.

**Effort:** S for the number and trend; M for naming culprits.

**Tier:**
- Free: last boot, a 24-hour sparkline.
- Pro: culprit list, 90-day history, the before/after report.

## 4. Browser memory helper

**For the user:** "Chrome is using 3.1 GB across 42 processes. Turn on Memory Saver: here's how."

**How to build:**
- Sum `PrivateUsage` per browser and classify processes by `--type=renderer|gpu-process` from the command line.
- Deep-link to `chrome://settings/performance` or `edge://settings/system`.
- **Don't write browser policies.** They need admin, show "Managed by your organization" and lock the user's own toggle.

**Effort:** S.

**Risk:** never kill browser processes, because people lose tabs.

## 5. Focus / Game / Meeting mode (Focus built in v0.4)

Built: efficiency mode for busy background programs, Best performance power mode, apps on screen and their children never slowed, restore after crash or restart. Still to do: Game and Meeting presets, pausing notifications.


**For the user:** one click quiets everything in the background, and a second click puts it all back.

**How to build:**
- Lower or EcoQoS background processes, and stop only the whitelisted dev servers.
- Switch the power plan with `PowerSetActiveScheme`, which is documented. The Windows 11 power-mode overlay has no documented API.
- Detect fullscreen or presentation with `SHQueryUserNotificationState`. It has to be polled.
- Toast suppression has only community registry routes. Avoid the undocumented ones.
- Save the "before" state to the journal and restore it on the next launch after a crash.

**Effort:** M.

**Tier:** Pro.

**Competitors:** Razer Cortex is free, so plain "game boost" is commoditised. Our angle is a **Dev Focus** preset.

**Risk:** touching anti-cheat or launcher processes. Keep a never-touch list.

## 6. Drive health + Windows Update / driver status (read-only)

**Drive health:**
- Read `MSFT_StorageReliabilityCounter`: Wear, Temperature, PowerOnHours, ReadErrorsTotal.
- Not every drive reports every counter. Show "not reported" rather than guessing.

**Update status:**
- Pending updates: the Windows Update Agent COM API with `IsInstalled=0 and IsHidden=0`.
- Pending reboot: check `RebootRequired`, `RebootPending` and `PendingFileRenameOperations`.
- Problem devices: `Win32_PnPEntity.ConfigManagerErrorCode != 0`.
- Old GPU, network and storage drivers: link to the PC maker's page.

**Never build a driver updater.** Trustpilot has BSOD complaints about Driver Booster and a non-working paid updater from CCleaner, and Microsoft advises against these tools.

**Effort:** S. **Tier:** Free.

## 7. Battery health

**For the user:** "Your battery holds 71% of its design capacity (wear 29%), 412 cycles."

**How to build:**
- Parse `powercfg /batteryreport /xml`, or use `root\wmi` `BatteryStaticData` and `BatteryFullChargedCapacity`.
- Wear = 1 − full charge capacity ÷ design capacity.
- Cycle count is missing on many laptops.
- Hide the card on desktops and virtual machines.

**Effort:** S. **Tier:** Free. It is a good reason for laptop owners to install the app.

## 8. "Why is my PC slow right now?"

**For the user:** press a button and get plain-English culprits from the last 60 seconds, each with a one-click action. For example: "Windows Search is indexing (35% CPU): it will finish on its own" or "Chrome tab using 1.4 GB".

**How to build:**
- Sample CPU, disk, memory and hard faults per process for 60 seconds.
- Match the results against the knowledge base (`kb.rs`).

**Effort:** M–L. **Tier:** Pro.

**Why it matters:** it answers exactly what people ask on Microsoft Q&A, and it avoids the "Boost" placebo.

## 9. Large / old files and duplicates

**How to build:**
- **Fast walk:** use `FindFirstFileExW` with `FindExInfoBasic` and `FIND_FIRST_EX_LARGE_FETCH` over the user's folders. No admin needed.
- **Optional whole-disk scan:** an MFT scan needs admin. The `ntfs-reader` and `usn-journal-rs` crates exist.
- **Duplicates:**
  1. Group by size.
  2. Skip hardlinks by file ID.
  3. Hash the first bytes, then the last bytes, then the whole file.
  4. Skip files under 1 MB.
  5. Send matches to the Recycle Bin, never straight to deletion.
- Use last-write time for "old", because last-access is often turned off.
- OneDrive placeholder files report sizes they don't really use.

**Effort:** M.

**Competitors:** WizTree is the speed king and free for personal use. Don't compete on the treemap.

## 10. Windows 11 AI / ads-off presets, "what grew?" alert

**Presets:**
- Turn off Copilot, Recall, Widgets, Start suggestions and the lock-screen ads.
- Every change is journalled and has one-click undo.
- Feature updates can reset them, so re-check after updates.

**"What grew?":**
- Snapshot folder sizes weekly.
- When C: suddenly fills, show what grew.
- Offer the system reclaimers: hibernation file, `Windows.old`, `DISM /StartComponentCleanup`.

**Effort:** M. **Tier:** Pro.

## 11. Unused apps + leftovers

**How to build:**
- **Inventory:** read the Uninstall keys (HKLM, HKCU, WOW6432Node) plus Store packages.
- **Last used:** UserAssist (ROT13 names, run count and last-run time, no admin needed) shows GUI launches only.
- **Exclusions:** leave out runtimes, drivers and developer tools, because UserAssist can't see command-line use.
- **Leftovers:**
  1. Snapshot ProgramFiles, AppData and `HKCU/HKLM\Software\<vendor>` before the uninstall.
  2. Diff after the uninstall.
  3. Show the matches for review, then quarantine them.

**Effort:** M–L.

**Competitors:** Bulk Crap Uninstaller is free and excellent. Don't compete on deep registry sweeps; registry deletion is what gave optimizers their bad name.

## 12. Privacy cleanup

**What to clear:**
- Recent files (`%APPDATA%\Microsoft\Windows\Recent` and jump lists).
- Clipboard history.

**Telemetry:** don't rebuild ShutUp10++. `AllowTelemetry=0` does nothing on Home and Pro, and turning diagnostics fully off hurts Windows Update failure reporting. Link to O&O ShutUp10++ instead.

**Effort:** S–M. **Tier:** Free.

## 13. RAM / SSD upgrade advisor (affiliate)

**For the user:** "You have one free DDR4 SODIMM slot; a compatible 16 GB stick is here." We already read the RAM chips through SMBIOS.

**Rules:**
- **Amazon bans affiliate links inside desktop software.** The app must link to a page on *your website*, which holds the links.
- **FTC disclosure:** "paid link" must sit right next to each link.
- Show advice only on sustained memory pressure, never as a pop-up, and never in Pro.

**Reality check:**
- RAM is very expensive in 2026, and Crucial's consumer line has ended.
- Expect a low conversion rate.

**Effort:** S.
