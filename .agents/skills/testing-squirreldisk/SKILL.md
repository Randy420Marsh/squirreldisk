---
name: testing-squirreldisk
description: Test SquirrelDisk 2 (native Rust + egui) end-to-end on Linux. Use when verifying scans, the sunburst/treemap chart, delete/trash, duplicates, SSH/rclone scans, translations, or the fork's no-network policy.
---

# Testing SquirrelDisk 2 (Rust + eframe/egui)

The app is one native binary: eframe/egui 0.36 with the glow (OpenGL)
backend, no web view. Everything the UI draws is painted by hand in
`src/ui/*.rs`, so there is no DOM or accessibility tree to script. Drive it
with the `SQD_*` environment hooks below and take screenshots.

The old Tauri/React version and its testing notes are on the
`tauri-legacy` branch.

## Build & launch

- `cargo build --release` → `target/release/squirreldisk` (~11 MB ELF).
- Linux build deps: `libasound2-dev libxkbcommon-dev libwayland-dev libx11-dev`
  (see `build.md`).
- `cargo build --release --no-default-features` builds the headless SSH
  agent **to the same path**. Rebuild the GUI before GUI tests.
- `target/release/squirreldisk <folder>` opens on a folder.
  `squirreldisk scan <folder> [--depth N --top N --json]` prints a report
  without a window, which is the quickest scanner check.

## Unit tests

```bash
cargo test --release      # expect: all pass, 1 ignored
```

The i18n test (`src/i18n.rs`) collects every `tr("…")` key from the UI
sources and fails if a language is missing more than 10% of them, or if a
translation drops a `{placeholder}`. If you add UI strings, run it.

## Developer hooks (`src/ui/debug.rs`)

| Variable | Effect |
| --- | --- |
| `SQD_SCAN=<path>` | start scanning at launch |
| `SQD_SHOT=<prefix>` + `SQD_SHOT_AT=3,8` | save `<prefix>-<n>.bmp` at those seconds |
| `SQD_QUIT=1` | quit after the last shot |
| `SQD_WINDOW=1180x760` | window size (also skips auto-placement) |
| `SQD_ZOOM=2` | interface scale (see the UI scale note below) |
| `SQD_MODAL=settings\|settings-end\|ssh\|sshauth\|rclone\|delete` | open a modal |
| `SQD_SCREEN=dupes`, `SQD_DUPES_PICK=1`, `SQD_DUPES=<a>,<b>` | duplicate finder |
| `SQD_UPDATE=available\|downloading\|ready` | fake an update offer (UI only; nothing is downloaded) |
| `SQD_TEST_TRASH`, `SQD_AUTODELETE` | delete-flow testing, see the source |
| `MARKETING_DEMO=1` | fake disks/folders, never touches the filesystem |

A typical screenshot run:

```bash
SQD_SCAN=$HOME/Downloads SQD_SHOT=/tmp/sqd SQD_SHOT_AT=4 SQD_QUIT=1 \
  target/release/squirreldisk
```

## Network-isolation verification (fork policy)

This fork must make **no network requests on its own**: no sponsors, update
checks, self-update, rclone download, or remote `curl`. Check it like this:

```bash
# 1) no HTTP client in the dependency tree
cargo tree -e normal | grep -iE 'ureq|reqwest|hyper|rustls|native-tls|openssl'   # expect nothing

# 2) no hard-coded endpoints left in the source
grep -rnE 'https?://' src | grep -vE 'example\.com|127\.0\.0\.1|rclone\.org/install|x\.y/z|a\.b|github\.com/a/b'   # expect nothing

# 3) runtime: only AF_UNIX sockets (X11/Wayland, D-Bus, portal)
strace -f -qq -e trace=socket,connect,sendto,sendmsg,sendmmsg \
  -o /tmp/sqd.strace timeout 25 target/release/squirreldisk
grep -oE 'socket\(AF_[A-Z0-9]+' /tmp/sqd.strace | sort | uniq -c
grep -E 'AF_INET6?' /tmp/sqd.strace                                   # expect nothing
```

Also open Settings and check that it has no "Check for updates" button and
no sponsor toggles, and that home and the scan view show no sponsor banner.

User-started SSH and rclone scans *do* use the network (your own `ssh`/`rclone`
processes). That is allowed. Test them against your own host only.

## UI scale

The interface scales with the window: drag the window bigger and every
element grows. Ctrl + / Ctrl − / Ctrl 0 change the size on top of that, and
the setting is saved as `ui_scale`. Check small windows (`SQD_WINDOW=700x450`)
and large ones (`SQD_WINDOW=2400x1500`) for clipped text.

## Byte units

Sizes use base 1000 on Linux and macOS (1 GB = 10⁹ B) and base 1024 on
Windows. Compare the home screen's disk size with `df --block-size=1 /`.

## Useful smoke commands

```bash
pgrep -af squirreldisk
RUST_BACKTRACE=1 target/release/squirreldisk 2>&1 | grep -iE 'panic|error'
target/release/squirreldisk scan / --depth 1 --top 5
```

## Secrets needed

None.
