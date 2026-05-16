# Building SquirrelDisk as a `.deb` Package

This document explains how to build SquirrelDisk as a Debian package (`.deb`)
from source on Ubuntu/Debian. It covers prerequisites, building, installing,
running, and troubleshooting. The instructions were verified on
**Ubuntu 22.04 LTS (Jammy)** with **Rust 1.95**, **Node 22**, and
**Tauri 1.2**.

> SquirrelDisk is a Tauri 1.x application. Tauri's Debian bundler is invoked
> by `tauri build` and produces a `.deb` automatically. There is no separate
> `debian/` directory or `dpkg-buildpackage` workflow.

---

## 1. Prerequisites

### 1.1 Supported distributions

Tauri 1.x links against **WebKitGTK 4.0** (`libwebkit2gtk-4.0-37`). Pick a
distro that still ships this package:

| Distribution        | Status                                                      |
| ------------------- | ----------------------------------------------------------- |
| Ubuntu 20.04 LTS    | Supported (matches the CI image used by upstream)           |
| Ubuntu 22.04 LTS    | Supported (verified)                                        |
| Debian 11 (Bullseye)| Supported                                                   |
| Debian 12 (Bookworm)| Supported (`libwebkit2gtk-4.0-37` available via `bookworm`) |
| Ubuntu 24.04+       | **Not supported out of the box** — ships only WebKitGTK 4.1 |

On Ubuntu 24.04 you must either install `libwebkit2gtk-4.0-37` from a backport
or upgrade the project to Tauri 2.x (which uses WebKitGTK 4.1). See
[Troubleshooting](#7-troubleshooting).

### 1.2 System packages

```bash
sudo apt-get update
sudo apt-get install -y \
    build-essential \
    curl \
    wget \
    file \
    libssl-dev \
    libgtk-3-dev \
    libwebkit2gtk-4.0-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    pkg-config
```

`libayatana-appindicator3-dev` is only required if you want system-tray
support; SquirrelDisk does not currently use the tray but Tauri's default
allowlist pulls it in, so install it to avoid build warnings.

### 1.3 Rust toolchain

Install Rust via `rustup` and pin to a stable release (`>= 1.69` is required
by Tauri 1.2; the project was verified with `1.83`):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustup default stable
rustc --version
```

### 1.4 Node.js & npm

Tauri's `beforeBuildCommand` is `npm run build`, which runs `tsc && vite
build`. Use Node 18 LTS or newer (verified with Node 22):

```bash
# Option A: via NodeSource
curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash -
sudo apt-get install -y nodejs

# Option B: via nvm
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.7/install.sh | bash
nvm install --lts
```

### 1.5 Tauri CLI

The Tauri CLI is already declared as a dev-dependency in `package.json`, so
once `npm install` has run it is invoked via `npx tauri ...`. There is no
need to install `tauri-cli` globally.

---

## 2. Get the source

```bash
git clone https://github.com/Randy420Marsh/squirreldisk.git
cd squirreldisk
```

---

## 3. Install JavaScript dependencies

```bash
npm install
```

This installs the React + Vite + Tauri JS toolchain. Expect ~240 packages and
a one-time warning about npm audit findings; none of them block the build.

---

## 4. Build the frontend

Tauri will invoke this for you, but it is useful to run it once on its own to
catch TypeScript errors before kicking off the longer Rust compile:

```bash
npm run build
```

Output is emitted to `dist/`.

---

## 5. Build the `.deb`

From the repository root:

```bash
npx tauri build --bundles deb
```

What this does, in order:

1. Runs the `beforeBuildCommand` (`npm run build`) to produce `dist/`.
2. Compiles the Rust crate in `src-tauri/` in release mode
   (`cargo build --release`). First-time compilation downloads ~400 crates
   and takes **2–5 minutes** on a typical 4-core machine; subsequent
   builds are cached by `target/`.
3. Bundles the binary, the `dist/` web assets, the `pdu` sidecar and the
   icons into a Debian package.

The resulting artefact is written to:

```
src-tauri/target/release/bundle/deb/squirrel-disk_<version>_amd64.deb
```

where `<version>` is the value of `package.version` in
`src-tauri/tauri.conf.json` (currently `0.3.4`).

Inspect the produced package:

```bash
dpkg-deb -I src-tauri/target/release/bundle/deb/squirrel-disk_0.3.4_amd64.deb
```

You should see something like:

```
 Package: squirrel-disk
 Version: 0.3.4
 Architecture: amd64
 Depends: libwebkit2gtk-4.0-37, libgtk-3-0
```

> The `Depends:` line is auto-populated by Tauri because
> `tauri.conf.json` declares an empty `bundle.deb.depends` array.
> If you ever need to declare additional runtime dependencies
> (for example `xdg-utils` for "Show in folder" to work) add them to
> that list and rebuild — see also [BUGS.md](./BUGS.md).

---

## 6. Install and run

### 6.1 Install with apt (recommended)

`apt` resolves missing dependencies automatically:

```bash
sudo apt install ./src-tauri/target/release/bundle/deb/squirrel-disk_0.3.4_amd64.deb
```

### 6.2 Install with dpkg

```bash
sudo dpkg -i src-tauri/target/release/bundle/deb/squirrel-disk_0.3.4_amd64.deb
# resolve any missing deps reported by dpkg
sudo apt-get install -f
```

### 6.3 Launch

From a terminal:

```bash
squirrel-disk
```

Or from the desktop menu — the bundler installs a `.desktop` entry at
`/usr/share/applications/squirrel-disk.desktop` and an icon at
`/usr/share/icons/hicolor/`.

### 6.4 Uninstall

```bash
sudo apt remove squirrel-disk      # or: sudo dpkg -r squirrel-disk
```

---

## 7. Troubleshooting

### `error: failed to run custom build command for openssl-sys`

Install `libssl-dev` and `pkg-config` (covered in
[section 1.2](#12-system-packages)).

### `failed to load shared libraries: libwebkit2gtk-4.0.so.37`

The runtime library is missing. On Ubuntu 22.04 / Debian 12:

```bash
sudo apt-get install -y libwebkit2gtk-4.0-37
```

On Ubuntu 24.04 the 4.0 series is no longer published. Workarounds:

- Use a 22.04/Debian 12 chroot or container to **build**; the resulting
  `.deb` still installs fine on 24.04 once the 4.0 runtime is added from
  a backport PPA.
- Or migrate the project to Tauri 2.x (out of scope for this document).

### `xdg-open: command not found` when clicking "Show in folder"

`xdg-open` is invoked via `std::process::Command` from the Rust side
(`src-tauri/src/main.rs`) and is not declared as a runtime dependency.
Install it explicitly:

```bash
sudo apt-get install -y xdg-utils
```

A permanent fix is tracked in [BUGS.md](./BUGS.md).

### `The updater is enabled but the bundle target list does not contain 'updater'`

This warning is emitted by Tauri because `tauri.conf.json` enables the
auto-updater but `bundles` does not contain `"updater"`. The `.deb` is
still produced and works; it just will not generate the signed
`*.tar.gz.sig` update artefacts. Add `"updater"` (alongside `"deb"`) if
you also want to ship updates. See [BUGS.md](./BUGS.md).

### `cannot find sidecar binary 'bin/pdu-aarch64-unknown-linux-gnu'`

The repository only ships an x86_64 build of the `pdu` sidecar
(`src-tauri/bin/pdu-x86_64-unknown-linux-gnu`). Building for ARM Linux
requires you to drop an `aarch64` build of [parallel-disk-usage] into
`src-tauri/bin/` first. Otherwise pass `--target x86_64-unknown-linux-gnu`
explicitly:

```bash
npx tauri build --target x86_64-unknown-linux-gnu --bundles deb
```

[parallel-disk-usage]: https://github.com/KSXGitHub/parallel-disk-usage

---

## 8. Other bundle formats

Tauri's Linux bundler also supports AppImage. To build both at once:

```bash
npx tauri build --bundles deb,appimage
```

AppImage requires `libfuse2`:

```bash
sudo apt-get install -y libfuse2
```

---

## 9. Reproducing the upstream CI build

`.github/workflows/main.yml` builds the `.deb` on `ubuntu-20.04` via
[`tauri-apps/tauri-action@v0`]. To reproduce its environment locally:

```bash
docker run --rm -it \
    -v "$PWD":/workspace -w /workspace \
    ubuntu:20.04 bash
# inside the container:
apt-get update && apt-get install -y \
    build-essential curl wget file libssl-dev pkg-config \
    libgtk-3-dev libwebkit2gtk-4.0-dev \
    libayatana-appindicator3-dev librsvg2-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
curl -fsSL https://deb.nodesource.com/setup_20.x | bash -
apt-get install -y nodejs
npm install
npx tauri build --bundles deb
```

[`tauri-apps/tauri-action@v0`]: https://github.com/tauri-apps/tauri-action
