# Fixed bugs

Bugs found and fixed in this fork, newest first. Each entry gives what went
wrong, the fix, and the test that guards it. Upstream SquirrelDisk 2.6.1 had
all of these unless the entry says otherwise.

## Security

### Double-clicking a scanned file could run it

**Bug.** Open (double-click in the chart, treemap or list, the context menu,
and the duplicate finder's Open button) passed the path straight to
`xdg-open` / `open` / `explorer`. For a program, script, shortcut or
installer, that runs it. A scanned disk, USB stick or network share can hold
anything, so one double-click could start code someone else left there.
Examples: `setup.exe`, `x.bat`, `Shortcut.lnk`, `a.desktop`, `go.sh`, a
`.app` bundle, an extensionless executable on macOS (opens in Terminal), or
`evil.exe.` (Windows drops the trailing dot).

**Fix.** `is_launchable()` recognises such files by extension (Windows,
Linux and macOS lists), by the executable bit together with an ELF, Mach-O
or `#!` header, or by an extensionless executable. A symlink is judged by
both its own name and its target's. Those files are revealed in the file
manager instead of opened, and a toast says why. Web links go through a
separate `open_url()` that accepts only `http(s)://`.
Files: `src/ui/app.rs`, `src/ui/view.rs`, `src/ui/treemap.rs`,
`src/ui/dupes.rs`, `src/ui/cloud.rs`, `assets/i18n/*.json`.
Test: `ui::app::tests::programs_are_revealed_not_run`.

### Secure erase could overwrite a file outside the selection

**Bug.** Secure erase checked each entry with `symlink_metadata`, then
reopened it by name with a plain `open`, which follows symlinks. Anyone who
can write to the folder could swap the file for a symlink in between, and
the three overwrite passes would land on the link's target (for example a
file in your home folder). The read-only fallback also used `metadata` and
`set_permissions`, which follow links. The final truncate reopened the file
by name a third time.

**Fix.** `open_same_file()` opens with `O_NOFOLLOW | O_NONBLOCK` on Unix
(a symlink fails, and a FIFO can't block the erase) and with
`FILE_FLAG_OPEN_REPARSE_POINT` on Windows. It then checks that the open
handle is a regular file with the same device and inode as the scanned
entry. All passes and the truncate use that one handle.
File: `src/delete.rs`.
Test: `delete::tests::erase_never_writes_through_a_swapped_file`.

### Secure erase on Windows didn't notice a swapped file

**Bug.** The fix above checked device and inode on Unix only. On Windows,
`open_same_file()` accepted any regular file at the path, so a different file
renamed into place after the scan would still be overwritten three times.
Found when the test failed on the Windows CI runner. This fork only; upstream
had the older, wider bug above.

**Fix.** On Windows the open handle must also match the scanned entry's
creation time, last write time and size (stable Rust has no file id there).
A file renamed into place keeps its own creation time.
File: `src/delete.rs`.
Test: `delete::tests::erase_never_writes_through_a_swapped_file`, now on
Windows too.

### Secure erase emptied hard-linked files

**Bug.** A file with other hard links is meant to be only unlinked, since
overwriting it would destroy the data seen through its other names. The
overwrite was skipped, but the code still truncated the file to zero bytes
afterwards. That emptied the shared data for every other name.

**Fix.** Shared files are not opened at all, only renamed and unlinked.
File: `src/delete.rs`.
Test: `delete::tests::erase_leaves_other_hard_links_intact`.

### A file name with a newline could add fake entries to an SSH scan

**Bug.** When the agent can't run on the server, the scan falls back to
`find -printf`, one entry per line. A file name can contain a newline, so a
file named `x\nf\t0\t1\t1\tfake/huge` made up an entry of its own: any name
and size, anywhere in the tree. Its path could then be selected for delete or
backup, and those commands run on the server.

**Fix.** `find` ends each record with `\0`, which no name can hold, and the
reader splits on it (`pump_lines` takes the delimiter).
File: `src/scan/remote.rs`.
Test: `scan::remote::tests::find_records_split_at_nul`.

### `du` lines from outside the scanned folder were counted

**Bug.** The `du` fallback matched lines to the root with a plain string
prefix, so scanning `/srv/data` also took in `/srv/data-old`. A newline in a
name split a line in two, and the second half was read as a path of its own.

**Fix.** `du_rel()` accepts a line only if it is the root itself or starts
with the root followed by `/`. Anything else is dropped.
File: `src/scan/remote.rs`.
Test: `scan::remote::tests::du_paths_must_be_under_the_root`.

### Remote paths with `..` could point outside the scan

**Bug.** Paths from rclone, `find` and `du` went into the tree as given.
Cloud object keys are free text: an S3 key like `a/../../etc/passwd` is
valid, and `find` or `du` output can be faked as above. The tree stored
`..`, `.` and empty components, so a later delete or backup of that entry
could name a path outside the folder that was scanned.

**Fix.** `safe_rel()` rejects any path with a `..`, `.` or empty component,
or a NUL byte. Such entries are skipped. Names like `..b` or `c..` are kept.
File: `src/scan/pathtree.rs`.
Test: `scan::pathtree::tests::ignores_paths_that_leave_the_root`.

### Agent output could name a file `..` or `a/b`

**Bug.** The SSH agent's output was trusted for file names. A changed or
broken agent, or a man-in-the-middle on a server you don't control, could
send the name `..`, `.`, an empty name, or one containing `/`, and the tree
would join it into paths outside the scan.

**Fix.** The parser skips such an entry and everything under it.
File: `src/scan/remote.rs`.
Test: `scan::remote::tests::agent_names_stay_inside_the_root`.

### Backup over SSH failed with OpenSSH 9 and on names with `[` or `*`

**Bug.** Backup copies remote files with `scp`, passing the path quoted for
a shell. Since OpenSSH 9.0, `scp` uses the SFTP protocol by default, which
takes the quotes as part of the name, so every backup failed with "No such
file or directory". The old protocol (`-O`) failed too on names with glob
characters ("protocol error: filename does not match request").

**Fix.** Backup first tries SFTP mode (`-s`) with only glob characters
escaped (`glob_escape()`), so no shell sees the name. If that fails, as it
does with OpenSSH before 8.7 such as the one in Windows 10, it falls back to
the old protocol with the shell quoting. Tested by hand against
`sftp-server` with a folder named `dir \ with 'odd' [x] ? $(touch PWNED)`:
the copy worked and nothing ran.
File: `src/delete.rs`.
Test: `delete::tests::scp_paths_are_not_globbed`.

### Known limit

Secure erase checks the file it opens, but not the folders above it. Someone
who can write to a parent folder could still swap that folder between the
scan and the erase. Only regular files that are not hard-linked elsewhere
are ever overwritten, so this can't reach a symlink target or a device.
Closing it needs `openat`-style traversal from the scan root.

## Linux desktop and AppImage

### The AppImage release jobs failed after a good build

**Bug.** `scripts/build-appimage.sh` ended with `"$OUT" --version` as a smoke
test. The release workflow passes a bare file name (`SquirrelDisk-x86_64.AppImage`),
so bash looked it up in `PATH` and stopped with "command not found" (exit
127), although the AppImage had been built. This fork only.

**Fix.** Run it by its full path, with `APPIMAGE_EXTRACT_AND_RUN=1` so it
needs no FUSE.
File: `scripts/build-appimage.sh`.
Test: the release workflow's Linux jobs.

### Programs started from the AppImage loaded its libraries

**Bug.** AppRun always added the bundled `usr/lib` (xkbcommon) to
`LD_LIBRARY_PATH`. The app passed that on to everything it started: the file
manager, `ssh`, `rclone`, `xdg-open`. Those then loaded the AppImage's copies
instead of the system's own, which can crash or misbehave when the versions
differ.

**Fix.** AppRun adds the bundled libraries only when the system has no
xkbcommon of its own, and saves the original value in
`SQUIRRELDISK_LD_LIBRARY_PATH`. The app restores it first thing in `main`,
after the loader has read it and before anything is started.
Files: `packaging/linux/AppRun`, `src/main.rs`.

### Starting the AppImage from a terminal could fail

**Bug.** Run from a terminal, the app starts a detached copy of itself and
exits. For an AppImage it started the executable inside the AppImage's
mount, which goes away as soon as the first process exits. The new copy
could lose its own files and fail to start.

**Fix.** It starts the `.AppImage` file again (`$APPIMAGE`), which mounts
afresh.
File: `src/main.rs`.

### The AppImage's mount stayed after the app closed

**Bug.** The detached copy inherited the open pipe the AppImage runtime
uses to know when to unmount. One mount was left behind until that copy
exited, and each launch from a terminal added another.

**Fix.** Before the new copy starts, every inherited descriptor above
stderr is set to close on exec (`close_range`). There is now one mount
while the app runs and none after it closes.
File: `src/main.rs`.

### No title bar or dock icon on Wayland

**Bug.** GNOME on Wayland draws no title bars for apps (no server-side
decorations), so the window had none: it couldn't be moved or closed with
the mouse. Without an app id the dock and task bars showed a generic icon
and couldn't match the window to the menu entry.

**Fix.** winit's own Adwaita-style title bar (`wayland-csd-adwaita`, drawn
by winit, no extra system libraries). The app id and X11 `WM_CLASS` are
`squirreldisk`, and the desktop entries carry `StartupWMClass=squirreldisk`.
Files: `Cargo.toml`, `src/main.rs`, `packaging/linux/squirreldisk.desktop`.

### The AppImage wasn't in the menu or the dock

**Bug.** An AppImage is one downloaded file. Nothing added it to the
applications menu, and docks had no icon for it.

**Fix.** At launch the app writes `~/.local/share/applications/squirreldisk.desktop`
and the hicolor icons, pointing at the AppImage it runs from. It follows
the file when it moves. `TryExec` hides the entry once the file is gone. It
is left out in portable mode, when the app is installed as a package, when
AppImageLauncher or Gear Lever already added that AppImage, and when the
user has an entry of their own there.
File: `src/ui/integration.rs`.
Test: `ui::integration::tests::appimage_menu_entry`.

### The `squirreldisk` command broke when the AppImage moved

**Bug.** The command is a link to the AppImage. After moving it or
downloading a newer version, the link pointed at nothing (or at the old
download), and Settings couldn't set it up again because the link was in
the way.

**Fix.** A link that points at nothing, or at another SquirrelDisk
AppImage, counts as missing. At launch it is replaced by one to the copy
being run, and Settings can replace it too (`ln -sf` on macOS).
File: `src/ui/integration.rs`.
Test: `ui::integration::tests::stale_command_links`.

### The running AppImage could be deleted from inside the app

**Bug.** The safety rules protected the running executable. Inside an
AppImage that is a file in the read-only mount, not the `.AppImage` the user
sees, so the app would offer to collect and delete its own AppImage.

**Fix.** The file named by `$APPIMAGE` is protected as well.
File: `src/safety.rs`.

## Interface

### The icon had no transparency, and was a blob at 16 px

**Bug.** The new icon painting came with a white background, not a
transparent one, so it showed a white square in docks and on dark panels.
At 16 to 48 px the painting shrank to a coloured blob.

**Fix.** The disc was cut out by hand with a softened edge. That cut-out is
`assets/icon/icon-1024.png`, and every size is made from it. Below 64 px the
icon is drawn from circles instead (`icon::small`): rim, crown and acorns
get bolder as the size shrinks. `--render-icons` also writes the same shapes
as `icon-small.svg`.
File: `src/icon.rs`.
Test: `icon::tests::small_icons_are_drawn`.

### The home-screen logo looked grainy

**Bug.** The logo was rendered at 184 px and egui shrank it to about 46 on
screen, at a position between pixels.

**Fix.** It is rendered at its size in physical pixels and placed on whole
pixels.
File: `src/ui/home.rs`.

### The window drew as many frames as the GPU allowed

**Bug.** With the sunburst effects on (the default), a folder view asked for
a new frame every frame, although the effects don't move. So did the
duplicate finder's growing "space to win back" counter, the Cloud storage
spinner and the toasts for their whole five seconds. Where vsync isn't
honoured (nested X servers like Xephyr, some drivers) that meant hundreds of
frames a second and a fully loaded GPU on an idle window.

**Fix.** Frames come only while something moves: chart transitions, drags,
particles, a toast sliding in or fading, rows sliding in. The hatch over
collected items redraws at 15 fps and an idle window polls at 2 fps. A frame
limiter keeps to at most 60 frames a second (30 while a scan reshapes the
chart) even without vsync; egui takes one frame off every timed repaint, so
the limiter, not the delay, sets the pace. Measured under Xephyr with
`SQD_FPS=1`: 3 fps idle on every screen, 30 fps during a scan.
Files: `src/ui/app.rs`, `src/ui/fx.rs`, `src/ui/dupes.rs`,
`src/ui/cloud.rs`, `src/ui/view.rs`.

### The window opened at the top left, partly off the screen

**Bug.** On Linux with more than one monitor, the window opened at the top
left of the whole desktop instead of in the middle of the screen in use.
eframe's `centered` option centres on the desktop's first output and ignores
the monitor's offset. The old fallback in `App::place_window` made the same
mistake. With a 1920-pixel screen on the left and a 2560x1440 screen at
+1920+0, the window landed on the wrong screen or ran past its edge. Panels
and docks were not allowed for either.

This showed up first in the small-screen test windows. Those are Xephyr
displays with no window manager, started with `SQD_WINDOW=WxH`, which also
skipped placement altogether. So there it looked like a test problem, but
the same offset bug hit real multi-monitor X11 desktops.

**Fix.** On X11 the app now places the window itself.
`placement::x11_work_area` finds the monitor under the mouse with XRandR (or
the primary monitor) and trims it to `_NET_WORKAREA`. `placement::fit` then
sizes the window to that area, and it is centred there in physical pixels.
`centered` is now used only on macOS and Windows, which already centre on
the active screen (AppKit `visibleFrame`, Win32 `rcWork`). Wayland doesn't
let apps place windows, so the compositor decides. x11-dl is loaded at run
time, so there is no link-time X11 dependency. Checked in a 1200x900
Xephyr: the window came up at 960x765+120+68.
Files: `src/ui/placement.rs`, `src/ui/app.rs`, `src/main.rs`, `Cargo.toml`.
Tests: `ui::placement::tests::work_area_is_cut_to_the_monitor`,
`ui::placement::tests::window_fits_the_screen`.

### The delete dialog ran into its buttons on short windows

**Bug.** The delete dialog had a fixed height. Its four mode cards were two
by two even on a phone held upright. On a short window (a landscape phone,
the Pi at 800x480) the caution checkbox and the hints were drawn over the
Cancel and Delete buttons, and could not be scrolled to.

**Fix.** Below 400 points of card width the modes go one under the other.
The part between the title and the buttons is a clipped area that scrolls
with the wheel or a finger. Long titles and subtitles of every dialog are
truncated to the card.
File: `src/ui/modals.rs`.
Test: `ui::app::layout_tests::every_screen_fits_landscape_and_portrait`.

### The duplicate finder overlapped on narrow and short windows

**Bug.** The duplicate finder always put the preview panel beside the list,
and every row assumed a wide window. Below about 900 points the places
card, the search and filter row, the keep rules and the selection bar ran
over each other, and on a landscape phone the list had no height left at
all.

**Fix.** Narrow windows get one column. The preview opens in place of the
list with a Back button, and Esc goes back too. The places card, the
toolbar, the rules and the selection bar each have a stacked arrangement,
texts are truncated, and on very short windows the rules row is left out
so the list keeps its room.
File: `src/ui/dupes.rs`.
Test: `ui::app::layout_tests::every_screen_fits_landscape_and_portrait`.

### The window couldn't be made small or turned to portrait

**Bug.** The minimum window size was 640x400 points, which is bigger than a
phone held upright or a Raspberry Pi HyperPixel in portrait (480x800), and
the zoom was worked out from the width alone.

**Fix.** The minimum is now 320x320. `zoom_for` scales so that the short
side gets at least `MIN_SHORT` (360) points and the long side at least
`MIN_LONG` (600), whichever way up the screen is.
Files: `src/main.rs`, `src/ui/app.rs`.

### Touch screens couldn't scroll

**Bug.** Lists and pages scrolled only with a mouse wheel or touchpad
(`smooth_scroll_delta`), so on a touch screen a finger drag did nothing.

**Fix.** `scroll_delta` also counts a deliberate touch drag. The home page,
the settings, the cloud picker and the duplicate list use it.
File: `src/ui/app.rs`.

### The home page overlapped on narrow windows

**Bug.** Below about 520 points wide, the three source cards were squeezed
side by side and their titles ran over each other. The volume name, the
usage bar and the Scan button overlapped, and long session and tool titles
ran under their buttons.

**Fix.** On a narrow window the source cards go one under the other in a
row layout. A narrow volume card puts the bar under the name. Every title
is truncated to the space it has.
File: `src/ui/home.rs`.

### A broken interface size in the settings file

**Bug.** A `ui_scale` of NaN (from a hand-edited or damaged settings file)
went straight into the zoom, which made the whole UI unusable.

**Fix.** A value that isn't finite counts as 1.0.
File: `src/ui/app.rs`.

### Long translations ran under the Settings controls

**Bug.** Setting descriptions were drawn at full width, so longer
translations (Finnish, German and others) ran underneath the toggle, the
language picker or the chart-style buttons.

**Fix.** Each description is truncated to the space left of its control.
File: `src/ui/modals.rs`.

## Tests

### The layout tests panicked after an egui update

**Bug.** The layout tests run frames with no renderer, so the font atlas
was never uploaded. Newer epaint panics when a frame's textures are dropped
unused ("Dropped TexturesDelta with 1 unapplied deltas"), and every layout
test failed.

**Fix.** The test frame clears the textures it was given.
File: `src/ui/layout_tests.rs` (`widget_frame`).
