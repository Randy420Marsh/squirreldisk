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

### Layout tests

`cargo test --release layout` runs `src/ui/layout_tests.rs`. It draws
every screen and dialog with the demo data at each size in `SIZES`, both
ways up, and lists every text that runs out of its box or over another
text. Add a `Scene` when you add a screen or dialog. When it fails, take a
screenshot at that size (below) to see what it means.

### Developer hooks

`src/ui/debug.rs` lists the `SQD_*` environment variables. The most useful
ones:

| Variable | Effect |
|---|---|
| `MARKETING_DEMO=1` | made-up disks and a scan played back from fake data. Delete only pretends. |
| `SQD_WINDOW=WxH` | start at this size (and skip the automatic placement) |
| `SQD_SCAN=/` | scan at launch |
| `SQD_SCAN=smb://user@host:port/share?v=3&enc=1` with `SQD_SMB_PASSWORD=pw` | scan a share at launch |
| `SQD_MODAL=settings\|delete\|ssh\|smb\|rclone` | open a dialog |
| `SQD_SCREEN=dupes` with `SQD_DUPES_PICK=1` | duplicate finder with mock results |
| `SQD_SCREEN=dupes` with `SQD_DUPES_ADD=1` | duplicate finder with the "Add place" menu open |
| `SQD_SHOT=prefix`, `SQD_SHOT_AT=3`, `SQD_QUIT=1` | screenshot after 3 s, then quit |
| `SQD_ZOOM=2` | interface scale |
| `SQD_FPS=1`, `SQD_FPS_WHY=1` | print frames per second to stderr (and what asked for each frame) |

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

### SMB test servers

`src/smb` is the app's own SMB client. To test it against real servers,
`scripts/smb-test-servers.sh start /tmp/sqs` starts four Samba servers as
you, on 127.0.0.1 only:

| Port | Server |
|---|---|
| 4451 | SMB 1 only |
| 4452 | SMB 2.0.2 to 2.1 |
| 4453 | SMB 3 only, encryption required |
| 4454 | SMB 1 with signing required |

Each has a read-only share `test` and a writable share `rw`. The account is
your user name with the password `sqdtest`. Keep the folder path short:
smbd puts its sockets there, and a path over about 60 characters fails
with "File name too long". Then:

```bash
SQD_SMB_SHARE=/tmp/sqs/share cargo test smb_ -- --ignored   # all four servers
SQD_SCAN="smb://$USER@127.0.0.1:4453/test?v=3&enc=1" SQD_SMB_PASSWORD=sqdtest \
  cargo run --release                                        # the app on one
```

For screenshots of a share, run `scripts/test-screen.sh` with
`TEST_SCREEN_DEMO=` (empty) so it uses real data instead of the demo.
`SQD_COLLECT=1 SQD_AUTODELETE=1` deletes over SMB, but only in a folder
whose path contains `sqd-fake`. Stop the servers with
`scripts/smb-test-servers.sh stop /tmp/sqs` when you're done.

### Frame rate

The app draws only while something moves, and at most 60 frames a second
(`FRAME` in `src/ui/app.rs`), 30 while a scan reshapes the chart. That also
holds where vsync doesn't, as in Xephyr. After a change to drawing or
animation, check it: run with `SQD_FPS=1` on a test screen and leave the
window alone. Every screen should drop to about 3 fps within a second or
two of the last change. `SQD_FPS_WHY=1` also prints where each repaint was
asked for. egui takes one frame off every `request_repaint_after` delay,
so a delay under one frame is the same as `request_repaint()`.

### Screenshots and demo GIFs

`scripts/make-gifs.sh [scripts...]` records the README GIFs from the demo
scripts in `src/ui/director.rs` (`hero`, `collect`, `themes`, `treemap`)
and frames them in `packaging/gif/`. With `FRAMES=<dir>` it keeps the
frames, which is where the still screenshots come from. On Linux, record
in a Xephyr screen with `SQD_WINDOW=1180x760`: the app otherwise sizes its
window to 80% of the screen. It needs ffmpeg and `rsvg-convert`. Check
every frame you keep for personal data.

### Icons

`assets/icon/icon-1024.png` is the master, a painting with a hand-cut
transparent edge. `squirreldisk --render-icons` makes every other size
from it, plus the `.ico` and `.icns`. Below 64 px the icon is drawn from
circles instead (`icon::small` in `src/icon.rs`), its rim, crown and
acorns getting bolder as the size shrinks, and that shape list is also
written as `icon-small.svg`. Run it again after changing either.

### Window placement

`src/ui/placement.rs` handles it. macOS and Windows centre on the screen
under the mouse, less the menu bar, Dock or taskbar. X11 uses XRandR plus
`_NET_WORKAREA`. On Wayland the compositor decides.

## Packaging and CI

- `.github/workflows/ci.yml`: clippy, the tests (including the layout
  tests) and the headless build on Ubuntu 24.04/26.04 x86_64, Ubuntu arm64,
  macOS, Windows and Arch, on every push and pull request.
- `.github/workflows/release.yml`: macOS universal DMG, Windows MSI, Linux
  AppImage and archive for x86_64 and aarch64 (built in an ubuntu:22.04
  container for an old glibc), `.deb` for Ubuntu 24.04/26.04 and for Debian
  trixie arm64 (Raspberry Pi), and an Arch package.
- `scripts/build-appimage.sh`, `scripts/package-deb.sh`,
  `packaging/arch/PKGBUILD` and `scripts/bundle-macos.sh` can also be run
  locally. Build the AppImage on the oldest system it should run on (glibc
  2.35, Ubuntu 22.04): it needs that glibc or newer.

## Where things stand (2026-10-23)

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

- Landscape and portrait layout tests
  (`src/ui/layout_tests.rs`). They run the real `App` headless through
  `egui::Context`, for every screen and dialog at phone, Pi, CRT, laptop
  and 1080p sizes both ways up. They fail if any text leaves its box or the
  window, or overlaps other text. They are part of `cargo test`.
- Narrow and short layouts for the duplicate finder and the delete dialog.

- CI on every platform, and arm64 Linux and Raspberry Pi packages.

- The new icon, drawn small sizes and sharp home-screen logo; the AppImage
  menu entry, Wayland title bar and the AppImage fixes in BUGS.md; frame
  rate capped and idle windows idle; screenshots and GIFs recreated.

- Windows shares over SMB 1, 2 and 3 with the app's own client
  (`src/smb`): NTLMv2 sign-in, signing, and SMB 3 encryption (AES-CCM and
  AES-GCM). Scans, deletes, previews and the duplicate finder all work on
  shares. Tested against `scripts/smb-test-servers.sh`.
- Previews of pictures, text and video frames; Open and Show folder; a
  folder picker of the app's own; hold-to-pick-up on touch screens.
- An Android build (arm64, API 36, `scripts/build-android.sh`), not yet in
  CI or the release. It has no Trash and no rclone, and SSH uses the app's
  own client (`src/sshc.rs`) with no agent upload.

Planned:
- The Wayland title bar check on more compositors.
