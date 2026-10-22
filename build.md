# Building SquirrelDisk from source

SquirrelDisk 2 is a native Rust app (eframe/egui, OpenGL via glow). There is
no web front end, no Node toolchain and no WebKitGTK. One `cargo build`
produces a single executable.

This fork builds with **no network code at all**. It has no HTTP client, no
update checks, no sponsor banner, no rclone downloader, and nothing is fetched
on remote hosts. See [Network policy](#5-network-policy).

> The old Tauri 1.x + React version (0.3.x) lives on the `tauri-legacy`
> branch, with its own `build.md` for making `.deb` packages.

---

## 1. Prerequisites

### 1.1 Rust

Install a current stable toolchain with [rustup](https://rustup.rs):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup update stable
```

### 1.2 Linux system packages

On Debian/Ubuntu, install the ALSA (sound effects), X11 and Wayland
development packages:

```bash
sudo apt-get install -y build-essential pkg-config \
    libasound2-dev libxkbcommon-dev libwayland-dev libx11-dev
```

At run time the app needs an OpenGL driver. If GL is missing it falls back to
CPU rendering, and Settings shows `CPU renderer (...)`. The folder picker uses
the XDG desktop portal (`xdg-desktop-portal` plus a GTK/KDE backend).

macOS and Windows need nothing beyond Rust (Xcode command-line tools on macOS,
MSVC build tools on Windows).

---

## 2. Building

```bash
cargo build --release          # desktop app → target/release/squirreldisk
cargo run --release            # build and launch
cargo run --release -- ~/Downloads        # open on a folder
cargo run --release -- scan ~/Projects    # terminal report, no window
```

### Headless remote agent

`--no-default-features` drops the `gui` feature and builds the small agent
that remote (SSH) scans use:

```bash
cargo build --release --no-default-features
```

It writes to the **same path** (`target/release/squirreldisk`), so rebuild the
GUI afterwards or keep the agent somewhere else, for example with
`--target-dir target/agent`.

---

## 3. Testing

```bash
cargo test --release
```

Expected result: all tests pass, with 1 ignored. The i18n test checks that
every translation in `assets/i18n/*.json` parses and keeps its placeholders.

---

## 4. Packaging

`packaging/` holds the Linux AppImage files (`AppRun`, `.desktop`), the
macOS DMG and Quick Action, and the Windows WiX installer.
`.github/workflows/release.yml` shows the exact steps for each platform. For a
simple local install on Linux:

```bash
install -Dm755 target/release/squirreldisk ~/.local/bin/squirreldisk
install -Dm644 packaging/linux/squirreldisk.desktop ~/.local/share/applications/squirreldisk.desktop
install -Dm644 assets/icon/icon-256.png ~/.local/share/icons/hicolor/256x256/apps/squirreldisk.png
```

---

## 5. Network policy

The fork removes everything that would make the app contact the internet on
its own:

| Upstream feature                           | In this fork                                        |
| ------------------------------------------ | --------------------------------------------------- |
| Sponsor banner (`squirreldisk.com/ads.json`, `/impressions`, click redirect) | Removed (`src/sponsor/`, `src/ui/ads.rs`) |
| Update check (GitHub API) and self-update  | Removed: `Updater::check`/`install` do nothing      |
| One-click rclone download (`downloads.rclone.org`) | Removed: install rclone with your package manager |
| Remote host `curl`s the agent from GitHub  | Removed: the agent is only uploaded over SSH        |
| `ureq`, `self-replace`, `ring`, `miniz_oxide` crates | Removed from `Cargo.toml`                  |

Features **you** start still use the network, because that is what they do:

- SSH scans of your own servers (through your `ssh`).
- Cloud scans through your own `rclone`.
- The "Other ways to install" button, which opens rclone.org in your browser.

To check this yourself:

```bash
cargo tree -e normal | grep -iE 'ureq|reqwest|hyper|rustls|native-tls|openssl'   # no output
strace -f -qq -e trace=socket,connect,sendto,sendmsg,sendmmsg \
    -o /tmp/sqd.strace timeout 25 target/release/squirreldisk
grep -oE 'socket\(AF_[A-Z0-9]+' /tmp/sqd.strace | sort | uniq -c        # only AF_UNIX
```

---

## 6. Troubleshooting

- **`alsa-sys` build error.** Install `libasound2-dev`.
- **`wayland-sys`/`xkbcommon` errors.** Install `libwayland-dev libxkbcommon-dev`.
- **Blank or black window on old GPUs.** Turn off "Shader effects" in Settings.
  If GL fails completely, the CPU renderer is used.
- **The folder picker doesn't open.** Install `xdg-desktop-portal-gtk` (or
  `-kde`). Dropping a folder onto the window works too.
