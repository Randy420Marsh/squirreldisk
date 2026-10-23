#!/usr/bin/env bash
# Builds an arm64 Android APK (exploratory: no SSH, cloud or Trash there).
# Needs the Android SDK with build-tools and platform 36, the NDK, a JDK for
# apksigner and `rustup target add aarch64-linux-android`.
# Usage: scripts/build-android.sh [output.apk]
# ANDROID_HOME (default ~/Android/Sdk), ANDROID_NDK (default: newest in the
# SDK), JAVA_HOME (default: Android Studio's) and ANDROID_KEYSTORE (default:
# a debug key made in target/android, kept so updates install over the app).
set -euo pipefail
umask 022
OUT="${1:-target/SquirrelDisk-arm64.apk}"
SDK="${ANDROID_HOME:-$HOME/Android/Sdk}"
NDK="${ANDROID_NDK:-$(ls -d "$SDK"/ndk/* | sort -V | tail -1)}"
export JAVA_HOME="${JAVA_HOME:-$HOME/android-studio/jbr}"
export PATH="$JAVA_HOME/bin:$PATH"
API=36
BT="$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)"
JAR="$SDK/platforms/android-$API/android.jar"
CLANG="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
TARGET=aarch64-linux-android

export CC_aarch64_linux_android="$CLANG/$TARGET$API-clang"
export AR_aarch64_linux_android="$CLANG/llvm-ar"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CLANG/$TARGET$API-clang"
# 16 KB pages, which newer phones use (and Google Play requires)
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384"
cargo rustc --lib --release --target "$TARGET" --crate-type cdylib
# the headless build: the SSH client (src/sshc.rs), named like a library so
# that Android unpacks it where apps may run programs from
cargo build --bin squirreldisk --no-default-features --release --target "$TARGET"

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
IFS=. read -r MA MI PA <<<"$VERSION"
CODE=$((MA * 10000 + MI * 100 + PA))
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

sed -e "s/@VERSION@/$VERSION/" -e "s/@VERSION_CODE@/$CODE/" packaging/android/AndroidManifest.xml > "$WORK/AndroidManifest.xml"
# the launcher scales it to each screen density
install -Dm644 assets/icon/icon-256.png "$WORK/res/mipmap/ic_launcher.png"
"$BT/aapt2" compile --dir "$WORK/res" -o "$WORK/res.zip"
"$BT/aapt2" link -I "$JAR" --manifest "$WORK/AndroidManifest.xml" -o "$WORK/app.apk" "$WORK/res.zip"
mkdir -p "$WORK/apk/lib/arm64-v8a"
cp "target/$TARGET/release/libsquirreldisk_android.so" "$WORK/apk/lib/arm64-v8a/"
cp "target/$TARGET/release/squirreldisk" "$WORK/apk/lib/arm64-v8a/libsquirreldisk_ssh.so"
cp LICENSE NOTICE.md THIRD-PARTY-LICENSES.txt "$WORK/apk/"
# stored, not compressed: Android maps the library straight from the APK
(cd "$WORK/apk" && zip -q -0 -r ../app.apk lib && zip -q -r ../app.apk LICENSE NOTICE.md THIRD-PARTY-LICENSES.txt)
"$BT/zipalign" -f -P 16 4 "$WORK/app.apk" "$WORK/aligned.apk"

KEY="${ANDROID_KEYSTORE:-target/android/debug.keystore}"
if [ ! -f "$KEY" ]; then
  mkdir -p "$(dirname "$KEY")"
  "$JAVA_HOME/bin/keytool" -genkeypair -keystore "$KEY" -storepass android -keypass android -alias debug \
    -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=SquirrelDisk debug" >/dev/null
fi
mkdir -p "$(dirname "$OUT")"
"$BT/apksigner" sign --ks "$KEY" --ks-pass pass:android --key-pass pass:android --out "$OUT" "$WORK/aligned.apk"
rm -f "$OUT.idsig"
echo "$OUT"
