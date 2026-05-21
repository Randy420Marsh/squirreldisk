---
name: testing-squirreldisk
description: Test SquirrelDisk's Tauri app end-to-end on Linux. Use when verifying disk-scan UI, sunburst rendering, bulk delete, show-in-folder, network isolation, byte-unit math, or Rust panic-resilience fixes.
---

# Testing SquirrelDisk (Tauri 1.x + React + WebKit2GTK)

This skill captures hard-won knowledge from running the adversarial test
plan against PR #1. The codebase is a Tauri 1.x desktop app: React +
Vite frontend, Rust backend, packaged as `.deb` on Linux.

## Build & launch

- **Release build:** from repo root, `npx tauri build --bundles deb`
  produces `src-tauri/target/release/bundle/deb/squirrel-disk_*.deb`
  and the unbundled binary at
  `src-tauri/target/release/squirrel-disk` (14.5 MB, ELF 64-bit).
- **Dev build with DevTools:** `npx tauri dev` from repo root. Use
  this whenever you need the WebView console (recommended for any
  IPC-driven test). Compile is ~2-3 min on a clean cache. Logs the
  Tauri side to stdout/stderr of the shell that started it.
- Build needs `libwebkit2gtk-4.0-dev libsoup2.4-dev libjavascriptcoregtk-4.0-dev`
  + the usual `pkg-config build-essential`. See `build.md` for the
  full apt list.

## Critical limitation: react-beautiful-dnd in WebKit2GTK

**Symptom:** the bulk-delete flow uses react-beautiful-dnd to drag
folder rows from the file list into the delete dropzone. Synthetic
mouse events (xdotool, `computer.left_click_drag`, or manual
`left_mouse_down` + multi-step `mouse_move` + `left_mouse_up`) all
fail to trigger a cross-droppable drop — they re-order entries
within the source list instead. This is a known interaction issue
between react-beautiful-dnd's pointer-event sensor and WebKit2GTK's
event stream; it might be fixed in a future webkit2gtk update, but
do not assume it works.

**Workaround for tests that need to exercise the JS-layer delete
code (e.g. verifying the §1.2 `await removeDir` fix):** call the
underlying Tauri commands directly from the WebView DevTools
console. The §1.2 fix is purely JS-layer (adding `await` around the
filesystem ops), so calling `removeFile` / `removeDir` from the
console exercises the exact code path the React loop runs.

## Tauri 1.x internal IPC pattern

For filesystem ops the high-level wrapper is `@tauri-apps/api/fs`,
but the *internal* IPC (which is what the dev console can reach
without an import map) looks like:

```js
await window.__TAURI_INVOKE__('tauri', {
  __tauriModule: 'Fs',
  message: { cmd: 'removeFile', path: '/tmp/scratch/file.bin' }
});
await window.__TAURI_INVOKE__('tauri', {
  __tauriModule: 'Fs',
  message: { cmd: 'removeDir', path: '/tmp/scratch/sub', options: { recursive: true } }
});
```

For custom commands exposed in `src-tauri/src/main.rs`, use the
standard top-level invoke:

```js
await window.__TAURI_INVOKE__('show_in_folder', { path: '/etc/hostname' });
await window.__TAURI_INVOKE__('stop_scanning', { path: '/tmp/scratch' });
```

Don't try the `@tauri-apps/api/tauri` ES-module import from the
DevTools console — it's not resolvable without a dev-server URL
that the WebView treats specially.

## Network-isolation verification

The original codebase pulled in `cdn.headwayapp.co/widget.js` on
every launch and had `updater.active: true` which pinged
`www.squirreldisk.com/api/updates/...`. To verify those calls are
gone:

```bash
strace -ff -e trace=connect,sendto,sendmmsg,sendmsg \
  -o /tmp/strace-squirreldisk.log \
  src-tauri/target/release/squirrel-disk &
sleep 15
kill %1

# Expect zero matches in all three:
grep -E 'headwayapp|squirreldisk\.com|api/updates' /tmp/strace-squirreldisk.log.*
grep -E 'connect\(.*AF_INET' /tmp/strace-squirreldisk.log.*
grep -E 'connect\(.*AF_INET6' /tmp/strace-squirreldisk.log.*
```

All observed traffic should be on AF_UNIX (X11, DBus, Wayland) only.

## Byte-unit verification

Linux/macOS should use base-1000 (1 GB = 10^9 B); Windows should use
base-1024. To verify on Linux: compare the displayed `X.X GB` to
`df --block-size=1 / | awk 'NR==2 {print $2}'`. Divide by 1000³ — it
should match the displayed value to ~4 sig figs. Divide by 1024³ —
it should differ by ~7%.

## GTK folder picker gotcha

The Tauri folder picker uses GTK's native dialog. On WebKit2GTK
1.18+, typing a path and pressing `Tab` or trailing-`/` triggers
auto-completion that selects the first matching CHILD rather than
confirming the typed-in folder. To navigate to `/tmp/scratch`,
either (a) click through the file tree manually, or (b) bypass the
picker entirely by scripting Tauri's `get_size`/scan IPC directly.

## Useful smoke commands

```bash
# Process still alive?
pgrep -af "squirrel-disk|squirreldisk-tauri"

# Any Rust panic on the Tauri side?
grep -iE "panic|RUST_BACKTRACE|aborted" /tmp/tauri-dev.log

# Built-bundle audit for the §4.4 debugger;-statement deletion:
grep -E "debugger" dist/assets/*.js dist/index.html
```

## Devin secrets needed

None. All tests run locally against a release / dev build; no
credentials required.
