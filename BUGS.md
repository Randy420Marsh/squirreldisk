# SquirrelDisk — Code Analysis Findings

This document is the result of a static review of the SquirrelDisk codebase at
commit [`a440815`](../../commit/a440815). It catalogues the bugs, latent
crashes, configuration issues and code-quality problems I found while
preparing the [`build.md`](./build.md) document. Items are grouped by area
and ranked roughly by severity within each group.

Each finding lists:

- **Where** — file and line
- **What** — what is wrong
- **Why it matters** — the user-visible impact
- **Fix** — a concrete suggested change

No source files were modified as part of this analysis.

---

## 1. Correctness bugs

### 1.1 Wrong byte-unit on Linux/macOS due to `window.OS_TYPE` race

**Where:**
[`src/main.tsx:5-7`](src/main.tsx),
[`src/components/FileLine.tsx:15`](src/components/FileLine.tsx),
[`src/components/DiskItem.tsx:19`](src/components/DiskItem.tsx),
[`src/components/ParentFolder.tsx:14`](src/components/ParentFolder.tsx),
[`src/d3chart.ts:218`](src/d3chart.ts).

**What:** `main.tsx` initialises `window.OS_TYPE = 'Windows_NT'` *synchronously*
and only fixes it later via the *asynchronous* `type()` promise:

```ts
window.OS_TYPE = 'Windows_NT';
type().then((type) => window.OS_TYPE = type);
```

Several modules read `window.OS_TYPE` at import time to choose a base-1024 vs
base-1000 byte divisor:

```ts
const mul = window.OS_TYPE === "Windows_NT" ? 1024 : 1000;
```

Because module evaluation is synchronous, `mul` is captured as `1024` on
every platform during the first render. The promise resolves later but the
already-captured `const mul` is never updated.

**Why it matters:** On Linux/macOS every "X GB" label shown in the disk list,
file list, parent-folder breadcrumb and chart tooltip is computed with the
wrong divisor and will be ~7 % too small (1000³/1024³).

**Fix:** Either await `type()` before rendering, or compute `mul` lazily
inside each component:

```ts
const mul = (await type()) === 'Windows_NT' ? 1024 : 1000;
```

or simply always use base-1000 (which is what `pretty-bytes` does and what
all three host OSes display in their file managers).

---

### 1.2 Delete loop pushes "successful" before the deletion actually finishes

**Where:** [`src/components/DiskDetail.tsx:270-301`](src/components/DiskDetail.tsx).

**What:** The delete handler iterates over `deleteList` and calls
`removeDir(...)` / `removeFile(...)` but **never awaits** them:

```ts
for (let node of deleteList) {
  ...
  removeDir(nodePath, { recursive: true })
    .catch((err) => removeFile(nodePath).catch((err2) => console.error(...)));
  successful.push(node);                           // pushed unconditionally
  setDeleteState((prev) => ({ ...prev, current: prev.current + 1 }));
}
d3Chart.current.deleteNodes(successful);           // runs before deletes finish
```

`removeDir` and `removeFile` return promises. Without `await`:

1. `successful` accumulates **every** entry, including ones that ultimately
   fail to delete.
2. The chart is updated before the filesystem operations complete, so the
   sunburst shows the items as gone even when the OS rejected the delete
   (e.g. read-only files, permission denied).
3. The `try/catch` around the call is dead — `removeDir` returns a rejected
   promise rather than throwing synchronously, so the `catch` branch is
   unreachable.

**Fix:** Await the call and only push on success:

```ts
try {
  await removeDir(nodePath, { recursive: true })
    .catch(() => removeFile(nodePath));
  successful.push(node);
  setDeleteState((prev) => ({ ...prev, current: prev.current + 1 }));
} catch (e) {
  console.error(e);
}
```

---

### 1.3 Unreachable code in arc-click handler

**Where:** [`src/components/DiskDetail.tsx:120-130`](src/components/DiskDetail.tsx).

```ts
arcClicked: (_, p) => {
  setFocusedDirectory(p);
  return p;
  // ↓ unreachable
  const curNodePath = buildPath(p);
  const vn = getViewNode(baseData.current!, curNodePath);
  setFocusedDirectory(
    getViewNodeGraph(baseDataD3Hierarchy.current!, curNodePath)
  );
  return vn;
}
```

**Why it matters:** TypeScript flags this as a warning; more importantly the
dead branch hints at an intended deep-navigation feature that was never
wired up. The lookup via `getViewNodeGraph` would resolve the focused node
from the original hierarchy — without it, navigating into deeply-pruned
sub-trees silently shows incomplete data.

**Fix:** Either delete the dead block or move it above the `return p;` and
test it.

---

### 1.4 `pruneIrrelevant` is a no-op

**Where:** [`src/pruneData.ts:81-118`](src/pruneData.ts).

**What:** The function body is entirely commented out and the function just
returns its input:

```ts
export function pruneIrrelevant(node: DiskItem, ...): DiskItem | null {
  if (node === null) return null;
  // 30 lines of commented-out body
  return node;
}
```

`getViewNode` still calls `pruneIrrelevant(cutted)` in the
non-empty-path branch, so the comment "prune irrelevant nodes" in
`DiskDetail.tsx` is misleading — nothing is pruned. Combined with the
unreachable branch in §1.3, deep navigation produces a tree that is
neither cut nor pruned but is *also* never rendered.

**Fix:** Either delete `pruneIrrelevant` and its call sites, or re-enable
the body. The commented logic references a `count` property that no longer
exists on `DiskItem`, so it would also need updating.

---

### 1.5 `setDeleteState` callback ignores `prev`

**Where:** [`src/components/DiskDetail.tsx:304-308`](src/components/DiskDetail.tsx).

```ts
setDeleteState((prev) => ({
  isDeleting: false,
  total: 0,
  current: 0,
}));
```

**Why it matters:** The `prev` argument is shadowed by the literal object;
the spread `...prev` was clearly intended. Functionally this works *only*
because the explicit fields happen to cover every key in the state shape,
but it is fragile and triggers the `@typescript-eslint/no-unused-vars` rule.

**Fix:** Pass a literal directly:

```ts
setDeleteState({ isDeleting: false, total: 0, current: 0 });
```

---

### 1.6 `version` mismatch between `package.json` and `tauri.conf.json`

**Where:** [`package.json:4`](package.json) (`0.0.0`),
[`src-tauri/tauri.conf.json:10`](src-tauri/tauri.conf.json) (`0.3.4`).

**What:** The npm package version is the boilerplate `0.0.0` while the
Tauri bundle version is `0.3.4`. The Tauri value is what ends up in the
`.deb`'s `Version:` field, so it is the one users see, but the divergence
trips up tooling that reads `package.json` (`npm version`, semantic-release,
etc.).

**Fix:** Set `package.json`'s `version` to match (and consider sourcing
both from one place, e.g. having `tauri.conf.json` read `../package.json`).

---

### 1.7 `removeDir`/`removeFile` paths use `\\` separator munging that no
longer applies

**Where:** [`src/components/DiskDetail.tsx:271-273`](src/components/DiskDetail.tsx),
[`src/pruneData.ts:210`](src/pruneData.ts),
[`src/components/ParentFolder.tsx:40-42`](src/components/ParentFolder.tsx).

```ts
const path = node.data.id.replace("\\/", "/").replace("\\", "/");
```

`.replace(string, string)` replaces **only the first occurrence**. On
Windows a path may contain many backslashes, so this leaves all but the
first one in place and the resulting path is invalid. The Rust side also
uses a regex (`main.rs:69-70`) so the JS-side conversion is partially
redundant.

**Fix:** Use a regex with the global flag:

```ts
const path = node.data.id.replace(/\\\//g, "/").replace(/\\/g, "/");
```

---

## 2. Tauri / packaging issues

### 2.1 Empty `bundle.deb.depends` (mostly cosmetic but loses `xdg-utils`)

**Where:** [`src-tauri/tauri.conf.json:35-37`](src-tauri/tauri.conf.json).

```json
"deb": { "depends": [] }
```

Tauri *auto-detects* `libwebkit2gtk-4.0-37` and `libgtk-3-0` and adds them
to the `.deb`'s `Depends:` field at build time (confirmed by
`dpkg-deb -I`). However, the Rust code shells out to `xdg-open` in
[`src-tauri/src/main.rs:89`](src-tauri/src/main.rs):

```rust
Command::new("xdg-open").arg(&new_path).spawn().unwrap();
```

`xdg-utils` is **not** in the auto-detected list, so on a minimal Debian
install the "right-click → Show in folder" feature crashes the app
(see §3.2).

**Fix:**

```json
"deb": { "depends": ["xdg-utils"] }
```

---

### 2.2 Updater is enabled but not bundled

**Where:** [`src-tauri/tauri.conf.json:59,69-76`](src-tauri/tauri.conf.json).

`bundle.targets = "all"` but Tauri still emits a warning during `tauri
build`:

```
Warn The updater is enabled but the bundle target list does not contain
`updater`, so the updater artifacts won't be generated.
```

**Why it matters:** The Linux build will never produce the signed
`*.tar.gz` + `*.sig` pair that Tauri's update server expects to publish,
so Linux users will never receive updates even though the in-app updater
is on.

**Fix:** Either disable the updater (`"active": false`) or explicitly add
the `updater` target:

```json
"targets": ["deb", "appimage", "updater"]
```

---

### 2.3 `bundle.identifier` is a wildcard `.dev` namespace

**Where:** [`src-tauri/tauri.conf.json:48`](src-tauri/tauri.conf.json).

`"identifier": "com.squirreldisk.dev"` — `*.dev` is owned by Google as
a public-suffix TLD. Tauri uses the identifier to derive macOS bundle IDs,
DBus names and Linux app IDs, so a real reverse-DNS identifier
(`com.squirreldisk.app` or similar) is preferable.

---

### 2.4 Maintainer and description fields are blank in the produced `.deb`

**Where:** [`src-tauri/Cargo.toml:1-9`](src-tauri/Cargo.toml),
[`src-tauri/tauri.conf.json:31-65`](src-tauri/tauri.conf.json).

Inspecting the produced package:

```
Package: squirrel-disk
Maintainer: you
Description: (none)
```

Both `Cargo.toml` (`authors = ["you"]`, `description = "A Tauri App"`,
`license = ""`, `repository = ""`) and `tauri.conf.json`
(`shortDescription = ""`, `longDescription = ""`, `copyright = ""`) are
left at their `tauri init` defaults.

**Fix:** Populate at minimum:

```toml
[package]
authors = ["Adileo Barone <…>"]
description = "Disk usage visualizer built with Tauri."
license = "MIT"        # or whatever applies
repository = "https://github.com/adileo/squirreldisk"
```

```json
"copyright": "© 2024 Adileo Barone",
"shortDescription": "Disk usage visualizer.",
"longDescription": "Open-source alternative to WinDirStat / DaisyDisk."
```

---

### 2.5 Overly permissive Tauri allowlist

**Where:** [`src-tauri/tauri.conf.json:13-30`](src-tauri/tauri.conf.json).

```json
"allowlist": {
  "all": true,
  "fs": {
    "all": false,
    ...
    "scope": ["**"]
  }
}
```

`"all": true` enables every Tauri command (shell, http, process, dialog,
clipboard, …). Combined with `fs.scope = ["**"]` this gives the WebView
unrestricted filesystem reach.

**Why it matters:**

1. Bloats the binary by ~600 KB of unused Tauri command stubs.
2. The Tauri security model recommends an explicit allowlist — Tauri 1.4+
   prints a warning when `"all": true` is set, and Tauri 2.0 removes it
   entirely.

**Fix:** Enable only the APIs the app actually uses (`dialog.open`,
`fs.removeDir`, `fs.removeFile`, `process.command`, `os.platform`,
`os.type`, `app.getVersion`, `event`, `window`).

---

### 2.6 Missing ARM Linux sidecar binary

**Where:** [`src-tauri/bin/`](src-tauri/bin/).

The `pdu` (parallel-disk-usage) sidecar is shipped for:

- `pdu-x86_64-unknown-linux-gnu`
- `pdu-x86_64-pc-windows-msvc.exe`
- `pdu-x86_64-apple-darwin`
- `pdu-aarch64-apple-darwin` (a copy of the x86_64 macOS binary, per the
  README in that folder)

There is **no** `pdu-aarch64-unknown-linux-gnu`. Cross-building the
`.deb` for `arm64` (i.e. `--target aarch64-unknown-linux-gnu`) fails at
bundle time. This is worth a comment in the README or a CI check.

---

### 2.7 Linux icon set is missing `512x512` and `icon.png`

**Where:** [`src-tauri/icons/`](src-tauri/icons/),
[`src-tauri/tauri.conf.json:41-47`](src-tauri/tauri.conf.json).

Only `32x32.png`, `128x128.png`, `256x256.png`, `icon.icns` and `icon.ico`
are referenced. Tauri's Linux bundler uses `icon.png` (preferred) or
`512x512.png` for `/usr/share/icons/hicolor/512x512/`. The current build
succeeds but the desktop entry has no high-DPI icon, so on HiDPI displays
the launcher icon is upscaled and blurry.

---

## 3. Rust-side latent panics

The Rust code uses `.unwrap()` and `.expect()` liberally on operations
that can legitimately fail at runtime. Each is a path to a process crash
that the frontend cannot recover from.

### 3.1 `show_in_folder` panics on missing path

**Where:** [`src-tauri/src/main.rs:81`](src-tauri/src/main.rs).

```rust
let new_path = match metadata(&path).unwrap().is_dir() {
```

If the user right-clicks an entry whose file has already been deleted
(common after the bulk-delete in §1.2 finishes), `metadata` returns
`Err(NotFound)` and the whole Tauri process crashes.

**Fix:** Return early with a `Result`:

```rust
let md = match metadata(&path) {
    Ok(md) => md,
    Err(e) => { eprintln!("show_in_folder: {e}"); return; }
};
```

### 3.2 `show_in_folder` panics if `xdg-open` is missing

**Where:** [`src-tauri/src/main.rs:89`](src-tauri/src/main.rs).

```rust
Command::new("xdg-open").arg(&new_path).spawn().unwrap();
```

`Command::spawn` returns `Err(ENOENT)` if the binary is not in `$PATH`.
Combined with §2.1 (where `xdg-utils` is not declared as a dependency)
this is a real crash on minimal Debian installs.

**Fix:** `.spawn().ok()` and log, or declare `xdg-utils` in
`bundle.deb.depends`.

### 3.3 `stop_scanning` panics if called without an active scan

**Where:** [`src-tauri/src/scan.rs:191-200`](src-tauri/src/scan.rs).

```rust
pub fn stop(state: tauri::State<'_, MyState>) {
    state
        .0
        .lock()
        .unwrap()
        .take()
        .unwrap()                   // panic if no scan running
        .kill()
        .expect("State is None");   // misleading message
}
```

`DiskDetail.tsx` calls `invoke("stop_scanning", ...)` from its `useEffect`
cleanup. React 18 will run cleanup twice in `StrictMode`; the second
call hits `None` and panics. (`StrictMode` is currently commented out in
`main.tsx`, which masks the bug.)

**Fix:**

```rust
if let Some(child) = state.0.lock().unwrap().take() {
    let _ = child.kill();
}
```

### 3.4 `unimplemented!()` on unknown `CommandEvent`

**Where:** [`src-tauri/src/scan.rs:85`](src-tauri/src/scan.rs).

```rust
_ => unimplemented!(),
```

`tauri::api::process::CommandEvent` is marked `#[non_exhaustive]`. Any
future Tauri release that introduces a new variant (e.g. `Error`) will
panic the scan task. Replace with a no-op or a `tracing::warn!`.

### 3.5 `get_disks` panics on non-UTF-8 disk names

**Where:** [`src-tauri/src/main.rs:121`](src-tauri/src/main.rs).

```rust
name: disk.name().to_str().unwrap(),
```

Disk labels may be arbitrary bytes (`/dev/disk/by-label/...`). Use
`.to_string_lossy()` or filter out non-UTF8 disks.

### 3.6 Other unwraps worth fixing

- `src-tauri/src/main.rs:36` — `app.get_window("main").unwrap()`
- `src-tauri/src/main.rs:74` — `Command::new("explorer").spawn().unwrap()` (Windows)
- `src-tauri/src/main.rs:128` — `serde_json::to_string(&vec).unwrap()`
- `src-tauri/src/main.rs:86` — `path2.into_os_string().into_string().unwrap()`
- `src-tauri/src/scan.rs:34` — `fs::read_dir("/").unwrap()`
- `src-tauri/src/scan.rs:51,54` — `expect("failed to create…")`
- `src-tauri/src/scan.rs:202-227` — three chained unwraps in `emit_scan_status`
  (the regex guarantees groups 1 and 2; group 3 can be `None`, but
  `map_or("0", …).parse::<u64>().unwrap()` is still fragile if pdu's
  progress format ever changes).

---

## 4. Stale / leftover code

### 4.1 Electron-era global declarations

**Where:** [`src/components/DiskList.tsx:12-19`](src/components/DiskList.tsx).

```ts
declare global {
  interface Window {
    electron: any;
    analytics: any;
    configStore: any;
    licver: any;
  }
}
```

None of these globals are ever assigned (and three of them are never
*read*). They are leftovers from the pre-Tauri Electron version and
should be removed. `window.electron` was previously referenced in
commented-out code in `DiskDetail.tsx`.

### 4.2 Headway widget initialised with someone else's account ID

**Where:** [`src/components/DiskList.tsx:65-73`](src/components/DiskList.tsx),
[`index.html:12`](index.html).

```ts
var config = { selector: ".inject_here", account: "xYZ8B7" };
if (window.Headway) { window.Headway.init(config); }
```

`xYZ8B7` looks like a placeholder Headway account ID copied from a
tutorial — clicking the "What's new" indicator opens an unrelated
changelog (or a 404 if the placeholder account no longer exists). The
widget script is also loaded unconditionally from
`https://cdn.headwayapp.co/widget.js`, which leaks a request to a
third-party host every time the app starts.

**Fix:** Either replace with a real account ID and document it, or
remove the Headway integration entirely (including the `<script>` tag in
`index.html`, the `Window.Headway: any` declaration in
`src/window.d.ts`, and the `.HW_badge_cont` CSS in `src/style.css`).

### 4.3 Unused module-level `genId`

**Where:** [`src/pruneData.ts:5`](src/pruneData.ts).

```ts
let genId = 0;          // declared, never referenced
```

### 4.4 Multiple `debugger;` statements in production code

**Where:**

- [`src/d3chart.ts:107`](src/d3chart.ts)
- [`src/d3chart.ts:122`](src/d3chart.ts)
- [`src/pruneData.ts:127`](src/pruneData.ts)
- [`src/pruneData.ts:41`](src/pruneData.ts) (commented `// debugger;`)
- [`src/pruneData.ts:46`](src/pruneData.ts) (commented `// debugger;`)

Vite leaves `debugger;` statements in the production bundle (it strips
`console.*` but not `debugger`). Whenever a user has DevTools open the
WebView pauses execution. Strip them or replace with structured logging.

### 4.5 Large blocks of commented-out code

`src/d3chart.ts`, `src/pruneData.ts`, `src-tauri/src/scan.rs` and
several components contain >50-line comment blocks of the previous
Electron / synchronous-walkdir implementation. The repo README itself
already calls the code "spaghetti", but for any future refactor it would
help to delete these blocks (they live in git history if needed).

### 4.6 Unused Rust imports & functions

`cargo build` emits four warnings on Linux:

```
warning: unused import: `regex::Regex`       --> src/main.rs:8
warning: unused variable: `window`           --> src/main.rs:36
warning: function `set_window_styles` is never used   --> src/window_style.rs:30
warning: variant `UnsupportedPlatform` is never constructed --> src/window_style.rs:77
```

All four are guarded by `#[cfg(target_os = …)]` blocks that fire only on
Windows/macOS. Wrap the imports/items in matching `#[cfg]` guards (or
`#[allow(dead_code)]` on the enum variant).

---

## 5. Type-safety / lint issues

### 5.1 `any` everywhere

The TypeScript code uses `any` to escape strict typing in many places:

- `obj: any, parent: any = null` in `itemMap` (`src/pruneData.ts:6`)
- `obj["children"].forEach((element: any) => …)` (same file, line 22)
- `d3Chart.current = useRef(null) as any` (`DiskDetail.tsx:43`)
- `state: any` in route locations (`DiskDetail.tsx:25`, `TitleBar.tsx:32`)
- `setStatus] : any = useState()` (`DiskDetail.tsx:46`)
- the `disk: any` prop in `DiskItem`
- the `d3Chart: any` prop in `FileLine`, `ParentFolder`
- `(accumulator as any).value` and friends in `d3chart.ts`

`DiskItem` already has a typed interface in `src/index.d.ts`; tightening
these would catch bugs like §1.5 at compile time.

### 5.2 Non-null assertions on possibly-null refs

`d3Chart.current.deleteNodes(...)` and `d3Chart.current.focusDirectory(...)`
in `DiskDetail.tsx` and `FileLine.tsx` assume `d3Chart.current` is
populated. It is not until the first `useEffect` runs, so a fast click
on a `<FileLine>` produced before the effect ran will throw.

### 5.3 `pretty-bytes` imported but unused

`src/components/FileLine.tsx:1` and `src/components/ParentFolder.tsx:2`
both `import prettyBytes from "pretty-bytes";` but never call it. The
manual `(value / mul / mul / mul).toFixed(2) + " GB"` formatting is used
instead (which is the source of bug §1.1).

### 5.4 `getIconForFolder` returns `string | undefined` but is concatenated unguarded

**Where:** [`src/components/FileLine.tsx:56-58`](src/components/FileLine.tsx).

```tsx
src={
  item.data.isDirectory
    ? "/fileicons/" + getIconForFolder(item.data.name)
    : "/fileicons/" + (getIconForFile(item.data.name) || "default_file.svg")
}
```

When `getIconForFolder` returns `undefined` the `src` becomes
`/fileicons/undefined`, producing a broken-image icon in the file list.
The file branch already falls back to `default_file.svg`; the folder
branch needs the same `|| "default_folder.svg"` guard.

---

## 6. Build / CI

### 6.1 GitHub Actions release name is the boilerplate `App Name v__VERSION__`

**Where:** [`.github/workflows/main.yml:53`](.github/workflows/main.yml).

```yaml
releaseName: 'App Name v__VERSION__'
```

Releases on the `Randy420Marsh/squirreldisk` fork (and upstream) will be
titled "App Name v0.3.4". Replace with `SquirrelDisk v__VERSION__`.

### 6.2 No `lint` / `test` / `check` scripts

`package.json` exposes only `dev`, `build`, `preview`, `tauri`. There is
no `tsc --noEmit`-only target, no ESLint, no Prettier config, no Cargo
clippy invocation. Adding at minimum:

```json
"scripts": {
  "lint": "tsc --noEmit && eslint src --ext .ts,.tsx",
  "lint:rust": "cd src-tauri && cargo clippy --all-targets -- -D warnings"
}
```

would have caught most issues in §1, §3.6, §4.4, §4.6 and §5.

### 6.3 `npm audit` reports 17 vulnerabilities (7 moderate, 9 high, 1 critical)

A fresh `npm install` against this lockfile reports a critical
vulnerability and several highs. Most are transitive
(`vite`, `postcss`, `d3` chain), but `npm audit fix` resolves the
majority without breaking changes.

---

## 7. Minor / cosmetic

- `src/components/DiskDetail.tsx:21` — `(window as any).LockDNDEdgeScrolling = () => true;`
  is a runtime patch for `react-beautiful-dnd`; document why it's there or
  upgrade to `@hello-pangea/dnd` (the maintained fork).
- `src/components/DiskDetail.tsx:143-153` — the progress bar percentage is
  computed twice and capped to 100 % via `Math.min(status.total, used)`.
  When `used` is 0 (folder scan from §1.6 of `build.md`) the division by
  zero yields `NaN%` for the entire scan.
- `src/style.css:50-53` — `.HW_badge_cont` styles only the Headway widget
  badge from §4.2; dead if Headway is removed.
- `index.html:5` — favicon still points at the Vite default
  (`/vite.svg`). Should point at one of the SquirrelDisk icons.
- `README.md:6-8` — the badges still link to the original
  `adileo/squirreldisk` repository rather than this fork.
- The repo has no `LICENSE` in `Cargo.toml`/`tauri.conf.json` metadata
  even though the project includes an MIT `LICENSE` file at the root.

---

## Summary

The build itself works — a fresh `npx tauri build --bundles deb` on
Ubuntu 22.04 produces a 9.2 MB `squirrel-disk_0.3.4_amd64.deb` that
installs and launches correctly. The issues above are mostly latent:
they manifest on slower deletes, non-UTF8 disks, missing optional tools
or non-Windows hosts.

Priority for fixes, in my opinion:

1. **§1.1** — wrong byte units on Linux/macOS (user-visible, every label).
2. **§1.2** — un-awaited deletes (data-loss-adjacent UX bug).
3. **§3.2 + §2.1** — `xdg-open` crash on minimal Debian (silent app death).
4. **§3.3** — `stop_scanning` panic (will hit when StrictMode is re-enabled).
5. **§2.2** — broken auto-updater on Linux.
6. Everything else can ride along with a general refactor.
