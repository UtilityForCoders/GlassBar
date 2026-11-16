//! Ícono de GlassBar en la bandeja del sistema (System Tray). Su menú es el propio
//! de GlassBar (ver `appmenu`).

use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
pub use windows::Win32::UI::Shell::NIN_BALLOONUSERCLICK;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, LoadIconW, HICON, IDI_APPLICATION, WM_APP,
};

pub const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 101;
/// Número del ícono de GlassBar entre sus recursos (`1 ICON` en `app.rc`).
const ICON_RESOURCE: usize = 1;

/// Un recurso por su número, como `MAKEINTRESOURCE`: el número va en el lugar del
/// puntero al nombre. No es un puntero colgante: `std::ptr::dangling::<u16>()` vale 2
/// (la alineación de `u16`) y pediría el recurso 2, que no existe.
fn resource(id: usize) -> PCWSTR {
    PCWSTR(id as *const u16)
}

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
}

impl Tray {
    pub fn new(hwnd: HWND) -> Self {
        let icon = unsafe {
            let instance = GetModuleHandleW(None).unwrap_or_default();
            LoadIconW(Some(instance.into()), resource(ICON_RESOURCE))
                .or_else(|_| LoadIconW(None, IDI_APPLICATION))
                .unwrap_or_default()
        };

        let tray = Self { hwnd, icon };
        tray.register();
        tray
    }

    /// Registra el icono en la bandeja. También cuando la barra se vuelve a crear
    /// (Explorer reiniciado, o la bandeja propia pidiendo los iconos).
    pub fn register(&self) {
        let mut tip = [0u16; 128];
        let tip_str = "GlassBar";
        for (i, c) in tip_str.encode_utf16().enumerate().take(127) {
            tip[i] = c;
        }

        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ID,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: self.icon,
            szTip: tip,
            ..Default::default()
        };

        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        }
    }
}

impl Tray {
    /// Muestra una notificación de Windows desde el icono de GlassBar. Al pulsarla,
    /// la ventana recibe `WM_TRAY` con `NIN_BALLOONUSERCLICK`.
    pub fn notify(&self, title: &str, text: &str) {
        fn copy<const N: usize>(text: &str) -> [u16; N] {
            let mut out = [0u16; N];
            for (slot, c) in out.iter_mut().zip(text.encode_utf16().take(N - 1)) {
                *slot = c;
            }
            out
        }
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ID,
            uFlags: NIF_INFO,
            szInfoTitle: copy(title),
            szInfo: copy(text),
            dwInfoFlags: NIIF_INFO,
            ..Default::default()
        };
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ID,
            ..Default::default()
        };
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            if !self.icon.is_invalid() {
                let _ = DestroyIcon(self.icon);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_resource_is_the_one_declared_in_app_rc() {
        assert_eq!(resource(ICON_RESOURCE).0 as usize, 1);
        let declared = include_str!("../app.rc")
            .lines()
            .find_map(|line| line.trim().strip_suffix(" ICON \"assets/icon.ico\""))
            .and_then(|id| id.parse::<usize>().ok());
        assert_eq!(declared, Some(ICON_RESOURCE));
    }
}
