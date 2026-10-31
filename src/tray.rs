//! Ícono de GlassBar en la bandeja del sistema (System Tray) y menú contextual.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, LoadIconW,
    SetForegroundWindow, TrackPopupMenuEx, HICON, HMENU, IDI_APPLICATION, MF_DISABLED, MF_GRAYED,
    MF_SEPARATOR, MF_STRING, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD, WM_APP,
};

use crate::autostart;

pub const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 101;

pub const CMD_AUTOSTART: u32 = 2;
pub const CMD_EXIT: u32 = 3;
/// Opciones del menú de una app del dock.
pub const CMD_APP_OPEN: u32 = 10;
pub const CMD_APP_PIN: u32 = 11;
pub const CMD_APP_UNPIN: u32 = 12;
pub const CMD_APP_CLOSE: u32 = 13;

/// Texto de una opción de menú, terminado en cero como lo pide Windows.
fn wide(text: &str) -> Vec<u16> {
    // "&" marca la tecla de acceso en los menús; se dobla para que se vea tal cual.
    text.replace('&', "&&")
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
}

impl Tray {
    pub fn new(hwnd: HWND) -> Self {
        let icon = unsafe {
            let instance = GetModuleHandleW(None).unwrap_or_default();
            // Carga el ícono incrustado en el recurso 1 (ver app.rc)
            LoadIconW(Some(instance.into()), PCWSTR(1 as *const u16))
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
        let tip_str = "GlassBar - Liquid Glass Taskbar";
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

    /// Menú de una app del dock, como el de la barra de Windows: su nombre (lo abre,
    /// o abre otra ventana si ya está abierto), anclar o desanclar y, si tiene
    /// ventanas, cerrarlas. Devuelve la opción elegida.
    pub fn show_app_menu(&self, name: &str, pinned: bool, windows: usize) -> Option<u32> {
        unsafe {
            let menu: HMENU = CreatePopupMenu().ok()?;
            let name = wide(name);
            let _ = AppendMenuW(
                menu,
                MF_STRING,
                CMD_APP_OPEN as usize,
                PCWSTR(name.as_ptr()),
            );
            let _ = if pinned {
                AppendMenuW(
                    menu,
                    MF_STRING,
                    CMD_APP_UNPIN as usize,
                    w!("Desanclar de la barra"),
                )
            } else {
                AppendMenuW(
                    menu,
                    MF_STRING,
                    CMD_APP_PIN as usize,
                    w!("Anclar a la barra"),
                )
            };
            if windows > 0 {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
                let close = if windows == 1 {
                    w!("Cerrar ventana")
                } else {
                    w!("Cerrar todas las ventanas")
                };
                let _ = AppendMenuW(menu, MF_STRING, CMD_APP_CLOSE as usize, close);
            }
            self.track(menu)
        }
    }

    /// Despliega el menú contextual de GlassBar y devuelve la opción elegida.
    pub fn show_context_menu(&self) -> Option<u32> {
        unsafe {
            let menu: HMENU = CreatePopupMenu().ok()?;

            // Cabecera deshabilitada
            let _ = AppendMenuW(
                menu,
                MF_STRING | MF_GRAYED | MF_DISABLED,
                0,
                w!("GlassBar v0.1.0 — Liquid Glass"),
            );
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);

            // Iniciar con Windows
            let autostart_text = if autostart::is_enabled() {
                w!("✓ Iniciar con Windows")
            } else {
                w!("Iniciar con Windows")
            };
            let _ = AppendMenuW(menu, MF_STRING, CMD_AUTOSTART as usize, autostart_text);

            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT as usize, w!("Salir de GlassBar"));
            self.track(menu)
        }
    }

    /// Muestra `menu` en el cursor, lo destruye y devuelve la opción elegida.
    unsafe fn track(&self, menu: HMENU) -> Option<u32> {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);

        // Obligatorio para que el menú se cierre al hacer clic fuera
        let _ = SetForegroundWindow(self.hwnd);

        let cmd = TrackPopupMenuEx(
            menu,
            TPM_RETURNCMD.0 | TPM_LEFTALIGN.0 | TPM_BOTTOMALIGN.0,
            pt.x,
            pt.y,
            self.hwnd,
            None,
        );

        let _ = DestroyMenu(menu);

        (cmd.0 != 0).then_some(cmd.0 as u32)
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
