//! Ícono de GlassBar en la bandeja del sistema (System Tray) y menú contextual.

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, LoadIconW,
    SetForegroundWindow, TrackPopupMenuEx, HICON, HMENU, IDI_APPLICATION, MF_CHECKED, MF_DISABLED,
    MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
    TPM_RETURNCMD, WM_APP,
};

use crate::autostart;
use crate::style::{Appearance, Choice, State, Styles};

pub const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 101;

pub const CMD_AUTOSTART: u32 = 2;
pub const CMD_EXIT: u32 = 3;

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

    /// Despliega el menú contextual de GlassBar y devuelve la opción elegida.
    pub fn show_context_menu(&self, styles: &Styles) -> Option<u32> {
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

            // Un submenú por situación con su apariencia, como en TranslucentTB.
            for state in State::ALL {
                if let Ok(submenu) = style_menu(styles, state) {
                    let name = HSTRING::from(state.name());
                    let _ = AppendMenuW(menu, MF_POPUP, submenu.0 as usize, PCWSTR(name.as_ptr()));
                }
            }
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

/// Submenú de una situación: si cuenta (el escritorio siempre) y su apariencia.
unsafe fn style_menu(styles: &Styles, state: State) -> windows::core::Result<HMENU> {
    let menu = CreatePopupMenu()?;
    let look = styles.look(state);
    let checked = |on: bool| if on { MF_CHECKED } else { MF_UNCHECKED };
    if state != State::Desktop {
        let _ = AppendMenuW(
            menu,
            MF_STRING | checked(look.enabled),
            Choice::Toggle(state).command() as usize,
            w!("Activado"),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    }
    for appearance in Appearance::ALL {
        let name = HSTRING::from(appearance.name());
        // Desactivada, sin marca: elegir una apariencia la activa.
        let _ = AppendMenuW(
            menu,
            MF_STRING | checked(look.enabled && look.appearance == appearance),
            Choice::Set(state, appearance).command() as usize,
            PCWSTR(name.as_ptr()),
        );
    }
    Ok(menu)
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
