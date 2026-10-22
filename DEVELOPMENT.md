# Development

How this fork is worked on: the rules, the checks before a commit, how
layouts are tested, and where things stand. For build prerequisites see
[build.md](build.md); for fixed bugs see [BUGS.md](BUGS.md).

## Ground rules

- **No network on its own.** The app makes no requests unless the user
  starts them: an SSH scan, an rclone remote or an SMB share. It has no
  update checks, telemetry, ads or sponsor banner. New dependencies must not
  pull in an HTTP client (see build.md, *Network policy*).
- **AGPL-3.0.** Keep `LICENSE` and `NOTICE.md` in every package, and keep
  the source link (`SOURCE_URL` in `src/main.rs`) in the Settings footer.
- **No personal data in the repo.** Packages name the project's issue
  tracker as the contact, never a person's email. Screenshots use
  `MARKETING_DEMO=1` data only. Check every image for real paths, user
  names and host names before you commit it.
- **Releases.** CI builds a release when a `v*` tag is pushed. A
  maintainer pushes the tag by hand, for example
  `git tag v2.6.2 && git push origin v2.6.2`.

## Before a commit

```bash
cargo test --release
cargo clippy --release --all-targets -- -D warnings
cargo clippy --release --no-default-features -- -D warnings   # headless CLI/agent
cargo check --release --target x86_64-pc-windows-gnu          # if you touched cfg(windows) code
```

All four must pass with no warnings. Add each fixed bug to BUGS.md with
what went wrong, the fix, the files and the guarding test.

## Code style

- Write code like the code around it: the same density of short comments
  that say *why*, plain names, and one-line `if`/`match` arms where they
  fit.
- Layout is done by hand with `Rect` arithmetic in points, not egui
  layouts. Every text that can grow, such as names, paths and
  translations, goes through `widgets::truncate` with the room it actually
  has.
- A narrow window gets its own arrangement, not just a squeezed one. Look
  for `compact`, `stacked` and `narrow` in `src/ui/`.
- User-facing strings go through `tr()`/`trf()`, and new keys go into all of
  `assets/i18n/*.json`. The i18n test checks that placeholders match.

## Window sizes and orientations

The layout must work from a phone held upright (360x792 points) to a 4K
desktop, in landscape and portrait. That includes a Raspberry Pi with a
HyperPixel 4.0 (800x480 / 480x800) and old 640x480 CRTs.
`App::auto_scale` zooms so that the short side gets at least `MIN_SHORT` and
the long side at least `MIN_LONG` points. The minimum window size is
320x320.

### Developer hooks

`src/ui/debug.rs` lists the `SQD_*` environment variables. The most useful
ones:

| Variable | Effect |
|---|---|
| `MARKETING_DEMO=1` | made-up disks and a scan played back from fake data. Delete only pretends. |
| `SQD_WINDOW=WxH` | start at this size (and skip the automatic placement) |
| `SQD_SCAN=/` | scan at launch |
| `SQD_MODAL=settings\|delete\|ssh\|rclone` | open a dialog |
| `SQD_SCREEN=dupes` with `SQD_DUPES_PICK=1` | duplicate finder with mock results |
| `SQD_SHOT=prefix`, `SQD_SHOT_AT=3`, `SQD_QUIT=1` | screenshot after 3 s, then quit |
| `SQD_ZOOM=2` | interface scale |

### Screenshots at many sizes

`scripts/test-screen.sh <list> [out-dir]` starts a small Xephyr screen in the
middle of the primary monitor. It shoots each line of the list (`WxH name
ENV...`) with demo data and a throwaway HOME, then always closes the screen
again. Look at the PNGs before you trust a layout change. Keep test screens
small (at most about 1200x900), and make sure none are left behind:
`pgrep -a Xephyr` should print nothing.

Xephyr has no window manager. With `SQD_WINDOW` the app does not place
itself, so a window at the top left of a test screen is expected there. To
check placement, run without `SQD_WINDOW` (see BUGS.md, *The window opened
at the top left*).

### Window placement

`src/ui/placement.rs` handles it. macOS and Windows centre on the screen
under the mouse, less the menu bar, Dock or taskbar. X11 uses XRandR plus
`_NET_WORKAREA`. On Wayland the compositor decides.

## Packaging and CI

- `.github/workflows/release.yml`: macOS universal DMG, Windows MSI, Linux
  AppImage (built in an ubuntu:22.04 container for an old glibc), `.deb` for
  Ubuntu 24.04/26.04, and an Arch package.
- `scripts/package-deb.sh`, `packaging/arch/PKGBUILD` and
  `scripts/bundle-macos.sh` can also be run locally.

## Where things stand (2026-10-22)

Done, release 2.6.2:
- Upstream merge with the no-network policy.
- SMB 1/2/3 and SSH scans.
- AGPL notices.
- Finnish.
- Security fixes (BUGS.md).
- deb and Arch packages.
- UI that scales with the window.
- Small-screen and touch support on the home page, the session view and
  the settings.
- X11 window placement.

In progress:
- Automated **landscape and portrait layout tests** that run headless on
  every CI platform. They drive `App` frames through `egui::Context` and
  check that no text leaves the screen or overlaps other text.
- **Narrow layouts** for the duplicate finder and the delete dialog, which
  still overlap below about 640 points wide.

Planned:
- CI workflow matrix: Ubuntu 24.04/26.04 x86_64 and arm64, macOS,
  Windows, Arch.
- arm64 Linux and Raspberry Pi (Debian trixie) packages.
- An Android build (arm64, API 36). This is exploratory: the file pickers,
  trash, SSH, rclone and desktop integration would have to be left out.
