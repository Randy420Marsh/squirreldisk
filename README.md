<p align="center">
  <img src="assets/icon/icon-256.png" width="128" alt="SquirrelDisk logo">
</p>

<h1 align="center">SquirrelDisk</h1>

<p align="center">
  See what's using your disk space, and clean it up safely.<br>
  For macOS, Windows and Linux. Free, and it makes no network requests of its own.
</p>

<p align="center">
  <a href="https://github.com/Randy420Marsh/squirreldisk/releases/latest"><img src="https://img.shields.io/github/v/release/Randy420Marsh/squirreldisk?color=%238e6bff&label=version&style=flat-square" alt="Latest release"></a>
  <a href="https://github.com/Randy420Marsh/squirreldisk/releases"><img src="https://img.shields.io/github/downloads/Randy420Marsh/squirreldisk/total?color=%23ff7ec8&style=flat-square" alt="Downloads"></a>
  <a href="https://github.com/Randy420Marsh/squirreldisk/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/Randy420Marsh/squirreldisk/ci.yml?branch=main&label=tests&style=flat-square" alt="Tests"></a>
  <img src="https://img.shields.io/badge/built_with-Rust-dca282.svg?style=flat-square" alt="Built with Rust">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0-blue.svg?style=flat-square" alt="License: AGPL-3.0"></a>
</p>

> [!IMPORTANT]
> **This is the Randy420Marsh fork of [SquirrelDisk](https://github.com/adileo/squirreldisk), a no-network build.** It never connects anywhere by itself: no sponsor banner, no usage counts, no update checks or self-update, and no automatic downloads. Scans you start over SSH or through rclone connect only to the servers and accounts you choose. The changes are listed in [NOTICE.md](NOTICE.md), fixed bugs in [BUGS.md](BUGS.md). The old Tauri version is on the [`tauri-legacy`](../../tree/tauri-legacy) branch.

<p align="center">
  <img src="screenshots/hero.gif" alt="Scanning a disk and exploring the chart" width="860">
</p>

## Download

| Platform | Download |
|----------|----------|
| macOS 11 or later (Apple silicon and Intel) | [SquirrelDisk-macOS.dmg](https://github.com/Randy420Marsh/squirreldisk/releases/latest/download/SquirrelDisk-macOS.dmg) |
| Windows 10 / 11 | [Installer (.msi)](https://github.com/Randy420Marsh/squirreldisk/releases/latest/download/SquirrelDisk-Windows.msi) · [Portable (.exe)](https://github.com/Randy420Marsh/squirreldisk/releases/latest/download/squirreldisk-x86_64-pc-windows-msvc.exe) |
| Linux (x86_64), any distribution | [AppImage](https://github.com/Randy420Marsh/squirreldisk/releases/latest/download/SquirrelDisk-x86_64.AppImage) · [Archive (.tar.gz)](https://github.com/Randy420Marsh/squirreldisk/releases/latest/download/SquirrelDisk-Linux-x86_64.tar.gz) |
| Ubuntu 24.04 / 26.04 | [24.04 (.deb)](https://github.com/Randy420Marsh/squirreldisk/releases/latest/download/SquirrelDisk-Ubuntu-24.04-amd64.deb) · [26.04 (.deb)](https://github.com/Randy420Marsh/squirreldisk/releases/latest/download/SquirrelDisk-Ubuntu-26.04-amd64.deb) |
| Arch Linux | [Package (.pkg.tar.zst)](https://github.com/Randy420Marsh/squirreldisk/releases/latest/download/SquirrelDisk-Arch-x86_64.pkg.tar.zst) |

Every build is on the [releases page](https://github.com/Randy420Marsh/squirreldisk/releases), made by [GitHub Actions](.github/workflows/release.yml) from the tagged source, with a `SHA256SUMS` file to check the downloads against. The builds are not signed, so macOS and Windows warn the first time (see below). SquirrelDisk never updates itself: download a new release when you want one. The AppImage also works with AppImageUpdate and Gear Lever, which fetch new releases from this fork when you ask them to.

<details>
<summary>macOS says the app can't be opened</summary>

SquirrelDisk is not signed with an Apple developer ID, so macOS asks for confirmation the first time.

1. Open SquirrelDisk once and close the warning.
2. Go to **System Settings → Privacy & Security**, scroll down and click **Open Anyway** next to the SquirrelDisk message, then confirm.

If macOS reports the app as damaged, run this in Terminal and open it again:

```bash
xattr -dr com.apple.quarantine /Applications/SquirrelDisk.app
```
</details>

<details>
<summary>Windows shows "Windows protected your PC"</summary>

Click **More info**, then **Run anyway**. For the portable `.exe` you can also right-click the file, choose **Properties**, tick **Unblock** and press **OK**.
</details>

<details>
<summary>Installing on Linux</summary>

- **AppImage:** `chmod +x SquirrelDisk-x86_64.AppImage`, then run it. It works on any distribution from 2022 on (glibc 2.35 or newer).
- **Ubuntu:** `sudo apt install ./SquirrelDisk-Ubuntu-24.04-amd64.deb` (or the 26.04 file). It adds SquirrelDisk to the app menu and the `squirreldisk` command.
- **Arch:** `sudo pacman -U SquirrelDisk-Arch-x86_64.pkg.tar.zst`. To build the package yourself from a clone, run `makepkg -si` in [packaging/arch](packaging/arch).
- **Archive:** unpack it anywhere and run `SquirrelDisk/squirreldisk`.
</details>

<details>
<summary>Portable mode (Windows and Linux)</summary>

To keep everything on a USB stick or in a single folder, create an empty folder named `portable` next to the Windows `.exe`, the `.AppImage` file or the `squirreldisk` binary from the archive:

```
SquirrelDisk/
├── squirreldisk-x86_64-pc-windows-msvc.exe
└── portable/
```

SquirrelDisk then keeps its settings there instead of in your user profile, along with your cloud accounts (`rclone.conf`). If you set `RCLONE_CONFIG` yourself, that file is used instead. To carry rclone along too, put its executable in `portable/bin/`: SquirrelDisk prefers that copy over one installed on the computer.

To use a specific folder instead, set the `SQUIRRELDISK_CONFIG_DIR` environment variable. It takes priority over the `portable` folder and works on macOS too.
</details>

## Features

**The whole disk at a glance.** Every folder becomes a slice of a colourful sunburst, sized by the space it takes. The chart builds up live while the scan runs, so the big offenders show up in seconds.

**Explore by clicking.** Click a slice to zoom in, click the centre to go back. Hover any slice and the side list shows what's inside, biggest first. Tiny files are grouped so the chart stays readable, and you can open any group to see everything in it.

<p align="center"><img src="screenshots/explore.png" alt="Exploring a folder" width="820"></p>

**Sunburst or treemap.** Prefer boxes to rings? Switch to the treemap in Settings: every folder becomes a box inside its parent, with names and sizes, and everything else works the same way.

<p align="center"><img src="screenshots/treemap.gif" alt="Exploring a disk in the treemap view" width="860"></p>

**Fits any window.** The whole interface scales with the window: shrink it to a corner of the screen or stretch it across a 4K monitor, and text, buttons and the chart grow or shrink to fit. Ctrl + and Ctrl − (⌘ on macOS) make everything bigger or smaller on top of that, and Ctrl 0 goes back to normal. Your choice is remembered.

<p align="center"><img src="screenshots/scaling.gif" alt="The interface scaling with the window and with Ctrl + and Ctrl −" width="860"></p>

**Collect, then clean up.** Drag slices or list rows into the collector at the bottom. When you're ready, move everything to the Trash, delete it for good, or copy it to an external drive or to the cloud first. A progress view keeps you posted, with a little celebration at the end.

<p align="center"><img src="screenshots/collect.gif" alt="Dragging folders into the collector and deleting them" width="860"></p>

**Find duplicates.** A tool on the home screen finds identical files across folders, disks, servers and cloud storage, compared by content, so renamed copies are found too. One copy of every set is always kept.

**Secure erase.** When you delete for good, you can have every file overwritten three times with zeros, ones and random data (DoD 5220.22-M) before it's removed. On SSDs some old data can survive; full-disk encryption protects it completely.

**Safe by design.** System folders, your home folder, whole drives and other essential locations are protected. App and settings folders ask for an extra confirmation before anything happens. Programs, scripts and shortcuts found in a scan are never run: opening one shows it in your file manager instead.

**Accurate numbers.** SquirrelDisk shows the space files really take on disk. Files that live only in the cloud (iCloud, Dropbox, Google Drive, OneDrive) count as zero, and linked folders are counted once.

**Always up to date.** Delete something in Finder, Explorer or your file manager and the chart updates on its own.

**Servers, shares and cloud storage too.** Scan any machine you can reach over SSH. Mounted network shares (SMB 1, 2 and 3, NFS) appear on the home screen next to your disks. S3 buckets, Google Drive, Dropbox, FTP, SMB and many more cloud services work through [rclone](https://rclone.org): install rclone yourself, then add, edit and remove your accounts right from the app.

**Right from your folders.** Turn on "Folder menu" in Settings and right-clicking a folder offers "Scan with SquirrelDisk": in Finder's Quick Actions on macOS, in Explorer on Windows (under "Show more options" on Windows 11), and in Dolphin, Nemo, Nautilus (Scripts) or any file manager's "Open with" on Linux.

**From the terminal too.** `squirreldisk <folder>` opens the app on that folder, and `squirreldisk scan <folder>` prints the biggest items without opening a window, as a tree (`--depth`, `--top`) or as JSON (`--json`) for scripts. The Windows installer puts the command on your PATH; on macOS and Linux SquirrelDisk sets it up on first launch when it can, or from Settings → Command-line tool.

```
$ squirreldisk scan ~/Projects --depth 2 --top 3
/home/me/Projects  48.2 GB · 912,044 files · 3.1s

  21.4 GB   44%  squirreldisk/
  19.8 GB   41%  ├─ target/
   1.2 GB    2%  ├─ node_modules/
   402 MB    1%  └─ … 38 more
  12.0 GB   25%  website/
  …
```

**Light on your machine.** The app is a single file of about 12 MB, starts instantly, and maps millions of files with a small amount of memory. It runs smoothly on older computers too.

**Speaks your language.** SquirrelDisk is available in 26 languages, including English, 简体中文, हिन्दी, Español, Français, العربية, Português, Русский, Deutsch, 日本語, 한국어, Italiano and Suomi. It picks your system language automatically, and you can change it in Settings.

**Make it yours.** Seven colour themes, optional sound effects, and an adjustable chart depth.

<p align="center"><img src="screenshots/themes.gif" alt="Switching colour themes" width="860"></p>

## Screenshots

<p align="center">
  <img src="screenshots/home.png" alt="Home screen" width="420">
  <img src="screenshots/overview.png" alt="Disk overview" width="420">
  <img src="screenshots/delete.png" alt="Delete confirmation" width="420">
  <img src="screenshots/theme-sunset.png" alt="Sunset theme" width="420">
</p>

## Privacy

This fork sends nothing anywhere. There is no sponsor banner, no analytics, no crash reporting and no update check, and the app never downloads anything by itself. The only connections it makes are the ones you ask for: an SSH scan or backup to a server you name, and rclone talking to the cloud accounts you set up. [build.md](build.md#5-network-policy) shows how to check this yourself.

## Issues

Report bugs and suggest ideas in this fork's [issues](https://github.com/Randy420Marsh/squirreldisk/issues). Problems that also happen in the original SquirrelDisk can go [upstream](https://github.com/adileo/squirreldisk/issues).

## Build from source

With [Rust](https://rustup.rs) installed:

```bash
cargo run --release
```

On Linux you also need the ALSA, X11 and Wayland development packages (`libasound2-dev libxkbcommon-dev libwayland-dev libx11-dev` on Debian and Ubuntu, `alsa-lib libxkbcommon wayland libx11` on Arch). See [build.md](build.md) for the headless agent, packaging, the tests, and how to check that the build makes no network requests. [DEVELOPMENT.md](DEVELOPMENT.md) covers the workflow: the checks before a commit, layout testing at small sizes and in portrait, and the current state of work.

## How this fork is made

The changes in this fork were written with the help of an AI coding assistant (Claude Code) and reviewed by the fork's maintainer. Every change is in the git history, the tests run on every push, and fixed bugs are written up with their tests in [BUGS.md](BUGS.md).

## License

[AGPL-3.0](LICENSE). SquirrelDisk is © Adileo Barone. This repository is a
**modified version**: the changes, with dates, are listed in
[NOTICE.md](NOTICE.md). The complete source of every build of this fork is
here, at <https://github.com/Randy420Marsh/squirreldisk>, under the same
license.
