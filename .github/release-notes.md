## SquirrelDisk 2.6.2 (no-network fork)

This is [Randy420Marsh/squirreldisk](https://github.com/Randy420Marsh/squirreldisk), a modified version of [SquirrelDisk](https://github.com/adileo/squirreldisk) 2.6.1 by Adileo Barone. It is licensed under the AGPL-3.0, like the original. [NOTICE.md](https://github.com/Randy420Marsh/squirreldisk/blob/main/NOTICE.md) lists what was changed.

- **No network requests of its own.** There is no update check, no sponsor list, no usage reporting and no rclone download. The app only connects to a server when you start a scan over SSH, SMB or rclone. It never updates itself: new versions are on this page.
- **The interface scales with the window.** Ctrl + / Ctrl − change the size, and Ctrl 0 resets it (⌘ on macOS). Your choice is remembered.
- **Network shares.** Mounted SMB 1/2/3 and NFS shares are listed on the home screen, and an SMB share can be added as an rclone account.
- **Windows shares, built in.** Scan a share on a Windows PC, a NAS or a Samba server over SMB 1, 2 or 3, without mounting it or installing anything: type `\\nas\share` and sign in. SMB 3 encrypts everything sent, and you can require it. SMB 1 can't encrypt, and the app says so. The password is kept in memory for the session only. Deleting and the duplicate finder work on shares too (with no Trash on a share, so deleted files are gone).
- **Previews.** Pictures, text files and a frame of a video show in the side panel. Open and Show folder are one click away.
- **Find duplicates anywhere.** Compare any folder, disk, SSH server or share.
- **Finnish.** 26 languages now.
- **Safer with files you didn't make.** Programs, scripts and shortcuts found in a scan are shown in the file manager instead of being run. Secure erase can no longer be redirected through a symlink and leaves hard-linked files alone. File names from SSH scans can't add fake entries or point outside the scanned folder. [BUGS.md](https://github.com/Randy420Marsh/squirreldisk/blob/main/BUGS.md) has the details.
- Fixed: backup over SSH failed with OpenSSH 9 and later, and on names with `[` or `*`.
- **Small screens, portrait and touch.** The window can be as small as 320x320 and turned either way up, from a phone-sized screen or a Raspberry Pi HyperPixel to a monitor on its side. Narrow windows get their own arrangement of the home page, the duplicate finder and the dialogs, and lists scroll with a finger. Automated tests check every screen at those sizes on every platform.
- **The window opens in the middle of your screen** on Linux X11 with more than one monitor.
- **A new icon**, with simpler versions drawn for small sizes so it still reads at 16 px.
- **Easier on the GPU and battery.** The window draws only while something moves, at most 60 frames a second, and almost nothing while idle.
- **The AppImage joins the menu and the dock.** On first run it adds itself to the applications menu, follows the file when you move it, and keeps the `squirreldisk` command working after a move or an update. It has a title bar on GNOME Wayland, and programs it opens no longer get its bundled libraries.
- New packages for Ubuntu 24.04, Ubuntu 26.04, Arch Linux, 64-bit ARM Linux and the Raspberry Pi (Debian 13 trixie, arm64).

Everything else is as in upstream 2.6.1.

### Downloads
- **macOS 11 or later:** `SquirrelDisk-macOS.dmg` (Apple silicon and Intel). Open it and drag SquirrelDisk to Applications.
- **Windows 10 / 11:** `SquirrelDisk-Windows.msi` (installer) or `squirreldisk-x86_64-pc-windows-msvc.exe` (portable).
- **Linux:** `SquirrelDisk-x86_64.AppImage` (any distribution with glibc 2.35 or later), `SquirrelDisk-Ubuntu-24.04-amd64.deb`, `SquirrelDisk-Ubuntu-26.04-amd64.deb`, `SquirrelDisk-Arch-x86_64.pkg.tar.zst`, or `SquirrelDisk-Linux-x86_64.tar.gz`.
- **Linux on 64-bit ARM:** `SquirrelDisk-aarch64.AppImage`, `SquirrelDisk-Linux-aarch64.tar.gz`, or `SquirrelDisk-Debian-trixie-arm64.deb` for the Raspberry Pi (64-bit Raspberry Pi OS) and Debian 13.
- **Android 11 or later, arm64 (exploratory):** `SquirrelDisk-Android-arm64.apk`. It scans local storage and SMB shares, without SSH, cloud storage or the Trash.

`SHA256SUMS` lists the checksum of every file. Check a download with `sha256sum -c SHA256SUMS --ignore-missing`.

The builds are made by GitHub Actions from the tagged commit. They are not signed.

### First launch
**macOS:** if macOS says SquirrelDisk can't be opened, open it once, then go to **System Settings → Privacy & Security** and click **Open Anyway**. If it says the app is damaged, run this in Terminal:

```
xattr -dr com.apple.quarantine /Applications/SquirrelDisk.app
```

**Windows:** if you see "Windows protected your PC", click **More info → Run anyway**. For the portable `.exe` you can also right-click it, choose **Properties** and tick **Unblock**.

**Linux:** `chmod +x SquirrelDisk-x86_64.AppImage` and run it. Install the packages with `sudo apt install ./SquirrelDisk-Ubuntu-24.04-amd64.deb` `sudo apt install ./SquirrelDisk-Debian-trixie-arm64.deb` on a Raspberry Pi, or `sudo pacman -U SquirrelDisk-Arch-x86_64.pkg.tar.zst`.
