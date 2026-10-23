//! Where the window opens: centred on the screen the user is working on (the
//! one under the mouse), sized to most of it, or maximized on a small one.
//! Best effort: when the platform can't tell (Wayland), the app falls back
//! to the screen egui reports (see `App::place_window`).

/// Window size for a screen area of `avail` (same unit as `scale`, which is
/// the size of one point): most of a small screen, but on a big one no more
/// than a little wider than the home column (820 points), so the layout
/// doesn't get sparse; never less than the window's minimum size.
pub fn fit(avail: (f64, f64), scale: f64) -> (f64, f64) {
    let (w, h) = avail;
    ((w * 0.8).clamp(860.0 * scale, 1080.0 * scale).min(w), (h * 0.85).clamp(560.0 * scale, 800.0 * scale).min(h))
}

pub enum Placed {
    #[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
    Done,
    /// Moved to another screen: size it next frame, once its DPI applies.
    #[cfg_attr(not(windows), allow(dead_code))]
    Again,
    /// The usable part of the screen under the mouse, in physical pixels
    /// from the top left of the desktop (x, y, width, height): the app
    /// places the window there itself.
    #[cfg_attr(any(target_os = "macos", windows), allow(dead_code))]
    Area(f64, f64, f64, f64),
    /// The platform can't say: use the fallback.
    Unknown,
}

#[cfg(target_os = "macos")]
pub fn on_active_screen(frame: &eframe::Frame) -> Placed {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSScreen, NSView};
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Some(mtm) = MainThreadMarker::new() else { return Placed::Unknown };
    let Ok(handle) = frame.window_handle() else { return Placed::Unknown };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return Placed::Unknown };
    // SAFETY: winit hands out a valid NSView for the lifetime of the window,
    // and eframe calls us on the main thread.
    let view: &NSView = unsafe { h.ns_view.cast::<NSView>().as_ref() };
    let Some(window) = view.window() else { return Placed::Unknown };
    // AppKit coordinates: points, origin at the bottom left of the main screen
    let mouse = NSEvent::mouseLocation();
    let under_mouse = NSScreen::screens(mtm).iter().find(|s| {
        let f = s.frame();
        mouse.x >= f.origin.x && mouse.x < f.origin.x + f.size.width && mouse.y >= f.origin.y && mouse.y < f.origin.y + f.size.height
    });
    let Some(screen) = under_mouse.or_else(|| NSScreen::mainScreen(mtm)) else { return Placed::Unknown };
    let area = screen.visibleFrame(); // without the menu bar and the Dock
    let (w, h) = fit((area.size.width, area.size.height), 1.0);
    let origin = NSPoint::new(area.origin.x + (area.size.width - w) / 2.0, area.origin.y + (area.size.height - h) / 2.0);
    window.setFrame_display(NSRect::new(origin, NSSize::new(w, h)), true);
    Placed::Done
}

#[cfg(windows)]
pub fn on_active_screen(frame: &eframe::Frame) -> Placed {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{HWND, POINT};
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL};
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER};
    let Ok(handle) = frame.window_handle() else { return Placed::Unknown };
    let RawWindowHandle::Win32(h) = handle.as_raw() else { return Placed::Unknown };
    let hwnd = h.hwnd.get() as HWND;
    // SAFETY: plain Win32 calls on our own window, with valid out-pointers.
    unsafe {
        let mut cursor = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut cursor) == 0 {
            return Placed::Unknown;
        }
        let target = MonitorFromPoint(cursor, MONITOR_DEFAULTTONULL);
        if target.is_null() {
            return Placed::Unknown;
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(target, &mut info) == 0 {
            return Placed::Unknown;
        }
        let work = info.rcWork; // physical pixels, without the taskbar
        if MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) != target {
            // Another screen, maybe with another scale: move there first; the
            // size is worked out next frame with that screen's DPI.
            SetWindowPos(hwnd, std::ptr::null_mut(), work.left + 40, work.top + 40, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            return Placed::Again;
        }
        let scale = match GetDpiForWindow(hwnd) {
            0 => 1.0,
            dpi => dpi as f64 / 96.0,
        };
        let (aw, ah) = ((work.right - work.left) as f64, (work.bottom - work.top) as f64);
        let (w, h) = fit((aw, ah), scale);
        let (x, y) = (work.left + ((aw - w) / 2.0) as i32, work.top + ((ah - h) / 2.0) as i32);
        if SetWindowPos(hwnd, std::ptr::null_mut(), x, y, w as i32, h as i32, SWP_NOZORDER | SWP_NOACTIVATE) == 0 {
            return Placed::Unknown;
        }
    }
    Placed::Done
}

/// X11: the monitor under the mouse (XRandR), less the panels and docks the
/// window manager reserves (`_NET_WORKAREA`). Wayland doesn't let apps place
/// windows at all, so there it is up to the compositor.
#[cfg(not(any(target_os = "macos", windows, target_os = "android")))]
pub fn on_active_screen(frame: &eframe::Frame) -> Placed {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let x11 = frame.window_handle().is_ok_and(|h| matches!(h.as_raw(), RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_)));
    if !x11 {
        return Placed::Unknown;
    }
    match x11_work_area() {
        Some((x, y, w, h)) => Placed::Area(x, y, w, h),
        None => Placed::Unknown,
    }
}

/// Android: the app always fills the screen.
#[cfg(target_os = "android")]
pub fn on_active_screen(_frame: &eframe::Frame) -> Placed {
    Placed::Done
}

#[cfg(not(any(target_os = "macos", windows, target_os = "android")))]
fn x11_work_area() -> Option<(f64, f64, f64, f64)> {
    use std::os::raw::{c_int, c_uchar, c_uint, c_ulong};
    use x11_dl::{xlib, xrandr};
    // loaded at run time, like winit does: no link-time X11 dependency
    let xl = xlib::Xlib::open().ok()?;
    let xr = xrandr::Xrandr::open().ok()?;
    // SAFETY: our own connection to the display, closed before returning;
    // every out-pointer is valid and every returned buffer is freed.
    unsafe {
        let dpy = (xl.XOpenDisplay)(std::ptr::null());
        if dpy.is_null() {
            return None;
        }
        let root = (xl.XDefaultRootWindow)(dpy);
        let (mut px, mut py) = (0 as c_int, 0 as c_int);
        let (mut r, mut c, mut wx, mut wy, mut mask) = (0 as c_ulong, 0 as c_ulong, 0 as c_int, 0 as c_int, 0 as c_uint);
        let pointer = (xl.XQueryPointer)(dpy, root, &mut r, &mut c, &mut px, &mut py, &mut wx, &mut wy, &mut mask) != 0;
        let mut n: c_int = 0;
        let list = (xr.XRRGetMonitors)(dpy, root, xlib::True, &mut n);
        let mut monitor = None;
        if !list.is_null() {
            let monitors: Vec<(i32, i32, i32, i32, bool)> =
                std::slice::from_raw_parts(list, n.max(0) as usize).iter().map(|m| (m.x, m.y, m.width, m.height, m.primary != 0)).collect();
            (xr.XRRFreeMonitors)(list);
            let under = |&&(x, y, w, h, _): &&(i32, i32, i32, i32, bool)| pointer && px >= x && px < x + w && py >= y && py < y + h;
            monitor = monitors.iter().find(under).or_else(|| monitors.iter().find(|m| m.4)).or(monitors.first()).copied();
        }
        // _NET_WORKAREA: x, y, width, height for each desktop; the first one
        let mut work = None;
        let atom = (xl.XInternAtom)(dpy, c"_NET_WORKAREA".as_ptr(), xlib::True);
        if atom != 0 {
            let (mut kind, mut format, mut items, mut after) = (0 as c_ulong, 0 as c_int, 0 as c_ulong, 0 as c_ulong);
            let mut data: *mut c_uchar = std::ptr::null_mut();
            let ok = (xl.XGetWindowProperty)(dpy, root, atom, 0, 4, xlib::False, xlib::XA_CARDINAL, &mut kind, &mut format, &mut items, &mut after, &mut data) == 0;
            if ok && !data.is_null() {
                // format-32 properties come back as C longs
                if format == 32 && items >= 4 {
                    let v = std::slice::from_raw_parts(data as *const std::os::raw::c_long, 4);
                    work = Some((v[0] as i32, v[1] as i32, v[2] as i32, v[3] as i32));
                }
                (xl.XFree)(data.cast());
            }
        }
        (xl.XCloseDisplay)(dpy);
        let (mx, my, mw, mh, _) = monitor?;
        Some(intersect((mx, my, mw, mh), work))
    }
}

/// The monitor's part of the work area. With several monitors the work area
/// spans all of them, so it only trims what a panel takes off this one.
#[cfg_attr(any(target_os = "macos", windows), allow(dead_code))]
fn intersect(monitor: (i32, i32, i32, i32), work: Option<(i32, i32, i32, i32)>) -> (f64, f64, f64, f64) {
    let (mx, my, mw, mh) = monitor;
    if let Some((wx, wy, ww, wh)) = work {
        let (x0, y0) = (mx.max(wx), my.max(wy));
        let (x1, y1) = ((mx + mw).min(wx + ww), (my + mh).min(wy + wh));
        if x1 - x0 >= mw / 2 && y1 - y0 >= mh / 2 {
            return (x0 as f64, y0 as f64, (x1 - x0) as f64, (y1 - y0) as f64);
        }
    }
    (mx as f64, my as f64, mw as f64, mh as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_area_is_cut_to_the_monitor() {
        // two monitors side by side, a 32-pixel panel along the top
        let work = Some((0, 32, 4480, 1408));
        assert_eq!(intersect((1920, 0, 2560, 1440), work), (1920.0, 32.0, 2560.0, 1408.0));
        assert_eq!(intersect((0, 360, 1920, 1080), work), (0.0, 360.0, 1920.0, 1080.0));
        // no work area, or one that makes no sense: the whole monitor
        assert_eq!(intersect((0, 0, 800, 480), None), (0.0, 0.0, 800.0, 480.0));
        assert_eq!(intersect((0, 0, 800, 480), Some((5000, 0, 10, 10))), (0.0, 0.0, 800.0, 480.0));
        // a Raspberry Pi panel with the taskbar at the top
        assert_eq!(intersect((0, 0, 800, 480), Some((0, 36, 800, 444))), (0.0, 36.0, 800.0, 444.0));
    }

    #[test]
    fn window_fits_the_screen() {
        // a big screen: a little wider than the home column
        assert_eq!(fit((2560.0, 1408.0), 1.0), (1080.0, 800.0));
        // small screens are filled
        assert_eq!(fit((800.0, 444.0), 1.0), (800.0, 444.0));
        assert_eq!(fit((640.0, 480.0), 1.0), (640.0, 480.0));
    }
}
