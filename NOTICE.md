# Notice of modifications

SquirrelDisk is © Adileo Barone and contributors, licensed under the GNU
Affero General Public License version 3 ([LICENSE](LICENSE)).
Original source: <https://github.com/adileo/squirreldisk>.

This repository, <https://github.com/Randy420Marsh/squirreldisk>, is a
**modified version** maintained by Randy420Marsh. It is distributed under the
same license, AGPL-3.0, and with no warranty. The complete source code of
every build of this fork is in this repository. The git history records
every change in detail, with its date.

## Changes in this fork

### 2026-10-23

- Windows (SMB 1/2/3) shares can be scanned directly, with the app's own
  SMB client: NTLMv2 sign-in, signing and SMB 3 encryption (`src/smb/`,
  `src/ui/smb.rs`). Passwords stay in memory only.
- Previews, Open and Show folder, a built-in folder picker, and hold to pick
  up on touch screens (`src/ui/preview.rs`, `src/ui/pick.rs`, `src/ui/`).
- The duplicate finder can compare any folder, disk, server or share.
- An Android build (`src/android.rs`, `src/lib.rs`, `src/sshc.rs`,
  `packaging/android/`, `scripts/build-android.sh`).
- Translations for the new texts in all languages (`assets/i18n/*`).

### 2026-10-22: no-network build (based on upstream 2.6.1)

- Removed the sponsor banner and everything behind it: the sponsor list
  download, click and view reporting, interest categories, and their settings
  and texts (`src/sponsor/`, `src/ui/ads.rs`, `assets/i18n/*`).
- Removed the update check and self-update (`src/update.rs`, Settings).
- Removed the one-click rclone download (`src/rclone.rs`, `src/ui/cloud.rs`).
- Removed the step where remote servers `curl` the agent from GitHub. The
  agent is only uploaded over SSH (`src/scan/remote.rs`).
- Removed the `ureq`, `self-replace`, `ring` and `miniz_oxide` dependencies.
- The interface scales with the window, and Ctrl + / − / 0 adjust it
  (`src/ui/app.rs`, `src/settings.rs`, `src/main.rs`).
- Mounted network shares (SMB 1/2/3, NFS) are listed on the home screen on
  Linux, macOS and Windows, and SMB can be added as an rclone account
  (`src/disks.rs`, `src/ui/home.rs`, `src/ui/cloud.rs`).
- Added a Finnish translation (`assets/i18n/fi.json`).
- Settings shows the license and where to get the source (`src/ui/modals.rs`).
- Rewrote the build and testing docs (`build.md`,
  `.agents/skills/testing-squirreldisk/SKILL.md`), and updated the README.

### Earlier: Tauri version

The `tauri-legacy` branch keeps the old Tauri/React version (0.3.x), with the
same no-network changes applied.
