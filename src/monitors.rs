//! Detección y manejo de monitores para GlassBar con soporte para DPI scaling.

use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, HDC, HMONITOR, MONITORINFO,
    MONITORINFOEXW, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

#[allow(dead_code)]
const PRIMARY: u32 = 1;

#[allow(dead_code)]
pub struct Monitor {
    pub handle: HMONITOR,
    pub device: String,
    pub label: String,
}

#[allow(dead_code)]
pub fn list() -> Vec<Monitor> {
    unsafe extern "system" fn add(handle: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
        list.push(handle);
        true.into()
    }
    let mut handles: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(add),
            LPARAM(&mut handles as *mut _ as isize),
        );
    }
    handles
        .into_iter()
        .filter_map(|handle| {
            let mut info = MONITORINFOEXW {
                monitorInfo: MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32,
                    ..Default::default()
                },
                ..Default::default()
            };
            unsafe { GetMonitorInfoW(handle, &mut info.monitorInfo).ok().ok()? };
            let len = info.szDevice.iter().position(|&c| c == 0).unwrap_or(32);
            let device = String::from_utf16_lossy(&info.szDevice[..len]);
            let number = device.trim_start_matches(|c: char| !c.is_ascii_digit());
            let mut label = format!("Pantalla {number}");
            if info.monitorInfo.dwFlags & PRIMARY != 0 {
                label.push_str(" (principal)");
            }
            Some(Monitor {
                handle,
                device,
                label,
            })
        })
        .collect()
}

pub fn primary() -> HMONITOR {
    unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) }
}

pub fn get_info(monitor: HMONITOR) -> (RECT, f32) {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
    unsafe {
        let _ = GetMonitorInfoW(monitor, &mut info);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
    }
    let scale = dpi_x as f32 / 96.0;
    (info.rcMonitor, scale)
}
