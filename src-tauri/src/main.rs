#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
mod scan;
mod window_style;

use serde::Serialize;
use std::process::Command;
use std::sync::Mutex;
use sysinfo::{DiskExt, System, SystemExt};
use tauri::api::process::CommandChild;
use tauri::Manager;

#[cfg(target_os = "macos")]
use window_vibrancy::NSVisualEffectMaterial;

#[cfg(target_os = "windows")]
use regex::Regex;

#[cfg(target_os = "linux")]
use {std::fs::metadata, std::path::PathBuf};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SquirrelDisk<'a> {
    name: &'a str,
    s_mount_point: String,
    total_space: u64,
    available_space: u64,
    is_removable: bool,
}

fn main() {
    tauri::Builder::default()
        .manage(MyState(Default::default()))
        .setup(|app| {
            // `window` is referenced from Windows/macOS-only cfg blocks
            // below, so it is `unused` on Linux builds.
            #[allow(unused_variables)]
            let window = app
                .get_window("main")
                .ok_or("missing main window in tauri.conf.json")?;

            #[cfg(target_os = "macos")]
            window_vibrancy::apply_vibrancy(&window, NSVisualEffectMaterial::HudWindow, None, None)
                .expect("Error applying blurred bg");

            #[cfg(target_os = "windows")]
            window_vibrancy::apply_blur(&window, Some((18, 18, 18, 125)))
                .expect("Error applying blurred bg");

            #[cfg(any(windows, target_os = "macos"))]
            if let Err(e) = window_style::set_window_styles(&window) {
                eprintln!("set_window_styles failed: {e:?}");
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_disks,
            start_scanning,
            stop_scanning,
            show_in_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
fn show_in_folder(path: String) {
    #[cfg(target_os = "windows")]
    {
        let re = match Regex::new(r"/") {
            Ok(re) => re,
            Err(e) => {
                eprintln!("show_in_folder regex compile failed: {e}");
                return;
            }
        };
        let result = re.replace_all(&path, "\\");
        // The trailing comma after `select` is required, do not remove.
        if let Err(e) = Command::new("explorer")
            .args(["/select,", result.as_ref()])
            .spawn()
        {
            eprintln!("show_in_folder explorer spawn failed: {e}");
        }
    }

    #[cfg(target_os = "linux")]
    {
        // see https://gitlab.freedesktop.org/dbus/dbus/-/issues/76
        let new_path = match metadata(&path) {
            Ok(md) if md.is_dir() => path,
            Ok(_) => {
                let mut path2 = PathBuf::from(&path);
                path2.pop();
                match path2.into_os_string().into_string() {
                    Ok(s) => s,
                    Err(_) => {
                        eprintln!("show_in_folder: path is not valid UTF-8: {path}");
                        return;
                    }
                }
            }
            Err(e) => {
                eprintln!("show_in_folder metadata failed for {path}: {e}");
                return;
            }
        };
        if let Err(e) = Command::new("xdg-open").arg(&new_path).spawn() {
            eprintln!(
                "show_in_folder xdg-open failed (is xdg-utils installed?): {e}"
            );
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Err(e) = Command::new("open").args(["-R", &path]).spawn() {
            eprintln!("show_in_folder open -R failed: {e}");
        }
    }
}
// Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
#[tauri::command]
fn get_disks() -> String {
    let mut sys = System::new_all();
    sys.refresh_all();

    let mut vec: Vec<SquirrelDisk> = Vec::new();

    for disk in sys.disks() {
        vec.push(SquirrelDisk {
            name: disk.name().to_str().unwrap_or("<non-utf8>"),
            s_mount_point: disk.mount_point().display().to_string(),
            total_space: disk.total_space(),
            available_space: disk.available_space(),
            is_removable: disk.is_removable(),
        });
    }
    serde_json::to_string(&vec).unwrap_or_else(|_| "[]".to_string())
}

pub struct MyState(Mutex<Option<CommandChild>>);

#[tauri::command]
fn start_scanning(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, MyState>,
    path: String,
    ratio: String,
) -> Result<(), ()> {
    scan::start(app_handle, state, path, ratio)
}

#[tauri::command]
fn stop_scanning(
    _app_handle: tauri::AppHandle,
    state: tauri::State<'_, MyState>,
    _path: String,
) -> Result<(), ()> {
    scan::stop(state);
    Ok(())
}
