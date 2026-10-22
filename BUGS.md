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

## Interface

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
