# Changelog

The release workflow copies the section for each version into the GitHub
release notes, and the app shows it under "What's new" in the update prompt.

## v0.4.0
- Developer junk: finds node_modules, build folders, Python environments and package caches, and shows how much space they take. Projects nobody changed for a month are ticked
- Autopilot: a daily or weekly clean-up that runs on its own when your PC is idle. By default it closes only Gradle and Kotlin build daemons; dev servers and adb need a switch. A program must have been idle for 30 minutes with nothing connected to it
- "Never close this" in History: one-click Optimise won't tick that program and Autopilot won't close it
- Focus mode: one switch gives the apps you're using full power, and turning it off puts everything back
- New notifications and a new update window with what's new and a progress bar
- Pro ($10 once, no account): unlimited one-click Optimise, cleaning developer junk, Autopilot and Focus mode. It stays switched off, with everything free, until the store is set up
- Cleaner look: solid buttons, calmer colours, no decorative icons
- Command line: `oneclick-cli dev` lists developer junk, `oneclick-cli bench` measures your PC
- The installer adds the open-source license notices (THIRD-PARTY-NOTICES.txt)

## v0.3.1
- The update prompt shows the real download size (it said "0 MB" for files under 1 MB)
- "What's new" in the update prompt shows these notes as a clean list

## v0.3.0
- Releases on GitHub with the installer, the portable app, the command-line tool and checksums
- Automatic update check when the app opens and every 6 hours from the tray: Update now, Skip this version or Later
- Updates are verified against the release's SHA-256 before they are installed
- Tray menu: Check for updates
- The version number is shown in the sidebar

## v0.2.0
- Per-user installer with Start menu and desktop shortcuts, tray watch at sign-in, and uninstall from Settings > Apps
- Start screen with one Scan button, deep scan with a health score, and one-click Optimise
- Junk file cleaning, health checks and RAM upgrade advice
- Tray quick panel with one-click Optimise
