//! The Android app: a library that Android's NativeActivity loads (built as
//! a cdylib by scripts/build-android.sh). Everywhere else this crate is
//! empty and the app is main.rs, which declares the same modules.
#![cfg(all(target_os = "android", feature = "gui"))]
#![allow(dead_code)]

mod agent;
mod android;
mod cli;
mod delete;
mod ipc;
mod rclone;
mod safety;
mod scan;
mod settings;
mod smb;
mod tree;
mod disks;

/// Where the source of this modified version is (AGPL-3.0 §6 and §13).
pub const SOURCE_URL: &str = "github.com/Randy420Marsh/squirreldisk";
mod dupes;
mod i18n;
mod icon;
mod sound;
mod ui;
mod update;
mod watch;
