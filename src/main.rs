#![windows_subsystem = "windows"]

//! # GlassBar
//! Barra de tareas moderna ultra-transparente estilo Windows 11 Glass para Windows, ultraligera en Rust.
//! Comportamiento auto-hide nativo (sale suavemente desde abajo al acercar el mouse) y estética translúcida.

mod apps;
mod autohide;
mod autostart;
mod bar;
mod catalog;
mod clock;
mod glass;
mod icons;
mod launcher;
mod monitors;
mod pins;
mod shell;
mod spring;
mod tray;

use std::cell::RefCell;
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;
use windows::core::{w, Result};
use windows::Win32::Foundation::{
    GetLastError, COLORREF, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, AC_SRC_ALPHA,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC,
    HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetMessageW, KillTimer,
    LoadCursorW, PostQuitMessage, RegisterClassExW, SetTimer, ShowWindow, TranslateMessage,
    UpdateLayeredWindow, IDC_ARROW, MA_NOACTIVATE, MSG, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_DESTROY,
    WM_DROPFILES, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_RBUTTONUP,
    WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

use crate::autohide::AutoHide;
use crate::bar::{BarState, HoverTarget, BOTTOM_MARGIN};
use crate::shell::TaskbarReplacer;
use crate::tray::{Tray, CMD_AUTOSTART, CMD_EXIT, CMD_REMOVE, WM_TRAY};

const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_SHOW_LAUNCHER: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 2;

const TIMER_ANIM: usize = 1;
const TIMER_TICK: usize = 2;
const TIMER_EDGE: usize = 3;

/// ~60 FPS durante animaciones físicas (16 ms). En reposo el timer se apaga (0% CPU).
const FRAME_MS: u32 = 16;
/// Refresco del reloj y verificación de apps activas.
const TICK_MS: u32 = 1000;
/// Verificación de proximidad del cursor al borde inferior para auto-hide inmediato (60 ms).
const EDGE_MS: u32 = 40;

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    width: u32,
    height: u32,
    pixmap: Pixmap,
}

impl Surface {
    fn new(width: u32, height: u32) -> Result<Self> {
        unsafe {
            let dc = CreateCompatibleDC(None);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    biHeight: -(height as i32), // Filas de arriba hacia abajo
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
            {
                Ok(b) => b,
                Err(e) => {
                    let _ = DeleteDC(dc);
                    return Err(e);
                }
            };
            let old = SelectObject(dc, bitmap.into());
            let pixmap = Pixmap::new(width, height).expect("tamaño de superficie inválido");
            Ok(Self {
                dc,
                bitmap,
                old,
                bits: bits.cast(),
                width,
                height,
                pixmap,
            })
        }
    }

    /// Convierte RGBA premultiplicado de tiny-skia a BGRA premultiplicado de Windows DIB.
    fn copy_to_dib(&mut self) {
        let len = (self.width * self.height * 4) as usize;
        let dst = unsafe { std::slice::from_raw_parts_mut(self.bits, len) };
        for (d, s) in dst
            .chunks_exact_mut(4)
            .zip(self.pixmap.data().chunks_exact(4))
        {
            d[0] = s[2]; // B
            d[1] = s[1]; // G
            d[2] = s[0]; // R
            d[3] = s[3]; // A
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

struct LauncherWindow {
    hwnd: HWND,
    surface: Surface,
    pos: POINT,
    scale: f32,
    model: launcher::LauncherState,
    visible: bool,
}
impl LauncherWindow {
    fn new(owner: HWND, pinned: &[apps::AppEntry], dock_left: i32, dock_top: i32) -> Result<Self> {
        let (screen, dpi) = monitors::get_info(monitors::primary());
        let scale = dpi
            .min((screen.right - screen.left - 24) as f32 / launcher::WIDTH)
            .min((dock_top - screen.top - 24) as f32 / launcher::HEIGHT)
            .max(0.3);
        let width = (launcher::WIDTH * scale).ceil() as u32;
        let height = (launcher::HEIGHT * scale).ceil() as u32;
        let pos = POINT {
            x: dock_left.clamp(screen.left + 12, screen.right - width as i32 - 12),
            y: dock_top - height as i32 - 8,
        };
        let surface = Surface::new(width, height)?;
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                w!("GlassBarLauncher"),
                w!("GlassBar — Aplicaciones"),
                WS_POPUP,
                pos.x,
                pos.y,
                width as i32,
                height as i32,
                Some(owner),
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?
        };
        Ok(Self {
            hwnd,
            surface,
            pos,
            scale,
            model: launcher::LauncherState::new(pinned),
            visible: false,
        })
    }
    fn render(&mut self) {
        self.model.draw(&mut self.surface.pixmap, self.scale);
        self.surface.copy_to_dib();
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
            ..Default::default()
        };
        unsafe {
            let _ = UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&self.pos),
                Some(&SIZE {
                    cx: self.surface.width as i32,
                    cy: self.surface.height as i32,
                }),
                Some(self.surface.dc),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }
    fn show(&mut self) {
        self.visible = true;
        self.render();
        unsafe {
            let _ = ShowWindow(self.hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOW);
            let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(self.hwnd);
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(self.hwnd));
        }
    }
    fn hide(&mut self) {
        self.visible = false;
        unsafe {
            let _ = ShowWindow(self.hwnd, windows::Win32::UI::WindowsAndMessaging::SW_HIDE);
        }
    }
}
impl Drop for LauncherWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

unsafe fn clipboard_text(hwnd: HWND) -> Option<String> {
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::{DataExchange::*, Memory::*};
    OpenClipboard(Some(hwnd)).ok()?;
    let text = (|| {
        let handle = GetClipboardData(13).ok()?;
        let global = HGLOBAL(handle.0);
        let len = (GlobalSize(global) / 2).min(16384);
        let pointer = GlobalLock(global).cast::<u16>();
        if pointer.is_null() {
            return None;
        }
        let data = std::slice::from_raw_parts(pointer, len);
        let end = data.iter().position(|&c| c == 0).unwrap_or(len);
        let text = String::from_utf16_lossy(&data[..end]);
        let _ = GlobalUnlock(global);
        Some(text)
    })();
    let _ = CloseClipboard();
    text
}

unsafe extern "system" fn launcher_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        WA_INACTIVE, WM_ACTIVATE, WM_CHAR, WM_CLOSE, WM_KEYDOWN, WM_MOUSEWHEEL,
    };
    let mut action = None;
    match msg {
        WM_ACTIVATE if (wparam.0 & 0xffff) as u32 == WA_INACTIVE => {
            with_app(|app| {
                if let Some(menu) = &mut app.launcher {
                    menu.hide();
                }
            });
        }
        WM_CLOSE => {
            with_app(|app| {
                if let Some(menu) = &mut app.launcher {
                    menu.hide();
                }
            });
        }
        WM_MOUSEMOVE | WM_LBUTTONUP => {
            with_app(|app| {
                if let Some(menu) = &mut app.launcher {
                    let x = (lparam.0 & 0xffff) as i16 as f32 / menu.scale;
                    let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32 / menu.scale;
                    let hit = menu.model.hit_test(x, y);
                    if msg == WM_LBUTTONUP {
                        if (24.0..68.0).contains(&y) && (24.0..462.0).contains(&x) {
                            menu.model.caret = menu.model.query.chars().count();
                            menu.model.select_all = false;
                            menu.model.selection = None;
                        }
                        if let Some(hit) = hit {
                            action = menu.model.activate(hit);
                        }
                    }
                    menu.model.hovered = hit;
                    menu.render();
                }
            });
        }
        WM_MOUSEWHEEL => {
            with_app(|app| {
                if let Some(menu) = &mut app.launcher {
                    if !menu.model.confirming_shutdown {
                        menu.model.change_page(((wparam.0 >> 16) as i16) < 0);
                        menu.render();
                    }
                }
            });
        }
        WM_CHAR => {
            with_app(|app| {
                if let Some(menu) = &mut app.launcher {
                    if let Some(ch) = char::from_u32(wparam.0 as u32) {
                        menu.model.type_char(ch);
                        menu.render();
                    }
                }
            });
        }
        WM_KEYDOWN => {
            with_app(|app| {
                if let Some(menu) = &mut app.launcher {
                    let ctrl = windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x11) < 0;
                    match wparam.0 {
                        0x1B => {
                            if menu.model.confirming_shutdown {
                                menu.model.confirming_shutdown = false;
                                menu.render();
                            } else {
                                menu.hide();
                            }
                        }
                        0x0D if !menu.model.confirming_shutdown => {
                            if let Some(hit) = menu.model.first_match() {
                                action = menu.model.activate(hit);
                            }
                        }
                        0x41 if ctrl && !menu.model.confirming_shutdown => {
                            menu.model.select_all = true;
                            menu.render();
                        }
                        0x56 if ctrl && !menu.model.confirming_shutdown => {
                            if let Some(text) = clipboard_text(hwnd) {
                                menu.model.insert_text(&text);
                                menu.render();
                            }
                        }
                        0x25 if !menu.model.confirming_shutdown => {
                            if menu.model.selection.is_some() {
                                menu.model.move_selection(-1);
                            } else {
                                menu.model.move_caret(-1);
                            }
                            menu.render();
                        }
                        0x27 if !menu.model.confirming_shutdown => {
                            if menu.model.selection.is_some() {
                                menu.model.move_selection(1);
                            } else {
                                menu.model.move_caret(1);
                            }
                            menu.render();
                        }
                        0x24 if !menu.model.confirming_shutdown => {
                            menu.model.caret = 0;
                            menu.model.select_all = false;
                            menu.render();
                        }
                        0x23 if !menu.model.confirming_shutdown => {
                            menu.model.caret = menu.model.query.chars().count();
                            menu.model.select_all = false;
                            menu.render();
                        }
                        0x2E if !menu.model.confirming_shutdown => {
                            menu.model.delete_forward();
                            menu.render();
                        }
                        0x28 if !menu.model.confirming_shutdown => {
                            menu.model.move_selection(4);
                            menu.render();
                        }
                        0x26 if !menu.model.confirming_shutdown => {
                            menu.model.move_selection(-4);
                            menu.render();
                        }
                        0x22 if !menu.model.confirming_shutdown => {
                            menu.model.change_page(true);
                            menu.render();
                        }
                        0x21 if !menu.model.confirming_shutdown => {
                            menu.model.change_page(false);
                            menu.render();
                        }
                        _ => {}
                    }
                }
            });
        }
        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
    }
    // Ejecutar después de liberar el préstamo de APP: Shell puede reenviar eventos.
    if let Some(action) = action {
        with_app(|app| {
            if let Some(menu) = &mut app.launcher {
                menu.hide();
            }
        });
        match action {
            launcher::Action::Launch(entry) => {
                if apps::launch_entry(&entry) {
                    let _ = catalog::record(&entry);
                }
            }
            launcher::Action::Shutdown => {
                let result = std::env::var_os("WINDIR")
                    .map(std::path::PathBuf::from)
                    .map(|root| {
                        std::process::Command::new(root.join("System32").join("shutdown.exe"))
                            .args(["/s", "/t", "0"])
                            .spawn()
                    });
                if !matches!(result, Some(Ok(_))) {
                    windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
                        Some(hwnd),
                        w!("No se pudo solicitar el apagado."),
                        w!("GlassBar"),
                        windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
                    );
                }
            }
        }
    }
    LRESULT(0)
}

struct App {
    hwnd: HWND,
    surface: Surface,
    pos: POINT,
    scale: f32,
    y_shown: i32,
    y_hidden: i32,
    screen_bottom: i32,
    screen_left: i32,
    screen_right: i32,
    visibility: AutoHide,
    bar: BarState,
    shell: TaskbarReplacer,
    tray: Tray,
    launcher: Option<LauncherWindow>,
    animating: bool,
    last_tick: Instant,
    mouse_tracked: bool,
}

impl App {
    fn new(hwnd: HWND) -> Result<Self> {
        let monitor = monitors::primary();
        let (rc, scale) = monitors::get_info(monitor);

        let bar = BarState::new();
        let scale = scale.min((rc.right - rc.left) as f32 / (bar.dock_width() + 40.0));
        let (width, height) = bar.window_size(scale);
        let surface = Surface::new(width, height)?;

        // Posición X centrada
        let x = rc.left + ((rc.right - rc.left) - width as i32) / 2;

        // Posición Y mostrada (flotando sobre el borde inferior)
        let y_shown = rc.bottom - height as i32 - (BOTTOM_MARGIN * scale).round() as i32;

        // Completamente fuera de pantalla; el sondeo del borde permite desplegarla.
        let y_hidden = rc.bottom;

        // Inicia expandida en el primer cuadro para mostrarse al arrancar
        let pos = POINT { x, y: y_shown };

        // Oculta la barra nativa de Windows
        let mut shell = TaskbarReplacer::new();
        shell.hide();

        let tray = Tray::new(hwnd);

        Ok(Self {
            hwnd,
            surface,
            pos,
            scale,
            y_shown,
            y_hidden,
            screen_bottom: rc.bottom,
            screen_left: rc.left,
            screen_right: rc.right,
            visibility: AutoHide::new(Instant::now()),
            bar,
            shell,
            tray,
            launcher: None,
            animating: false,
            last_tick: Instant::now(),
            mouse_tracked: false,
        })
    }

    fn toggle_launcher(&mut self) {
        if self.launcher.as_ref().is_some_and(|menu| menu.visible) {
            if let Some(menu) = &mut self.launcher {
                menu.hide();
            }
            return;
        }
        let pinned: Vec<_> = self
            .bar
            .primary_apps
            .iter()
            .map(|a| a.entry.clone())
            .collect();
        if self.launcher.is_none() {
            match LauncherWindow::new(self.hwnd, &pinned, self.pos.x, self.y_shown) {
                Ok(menu) => self.launcher = Some(menu),
                Err(_) => return,
            }
        } else if let Some(menu) = &mut self.launcher {
            menu.model = launcher::LauncherState::new(&pinned);
        }
        if let Some(menu) = &mut self.launcher {
            menu.show();
        }
        self.expand();
    }

    fn pins_changed(&mut self) {
        if pins::save(&self.bar.primary_apps).is_err() {
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::MessageBoxW(Some(self.hwnd),
                        w!("No se pudo guardar la lista de programas. El cambio solo durará esta sesión."),
                        w!("GlassBar"), windows::Win32::UI::WindowsAndMessaging::MB_ICONWARNING);
            }
        }
        let (rc, dpi) = monitors::get_info(monitors::primary());
        self.scale = dpi.min((rc.right - rc.left) as f32 / (self.bar.dock_width() + 40.0));
        let (width, height) = self.bar.window_size(self.scale);
        if let Ok(surface) = Surface::new(width, height) {
            self.surface = surface;
            self.pos.x = rc.left + ((rc.right - rc.left) - width as i32) / 2;
            self.y_shown = rc.bottom - height as i32 - (BOTTOM_MARGIN * self.scale).round() as i32;
            self.pos.y = (self.y_hidden as f32
                + (self.y_shown - self.y_hidden) as f32 * self.visibility.motion.value)
                .round() as i32;
            self.bar.pressed = None;
            self.bar.update_hover(None);
            self.expand();
            self.render();
        }
        self.visibility.reveal();
        self.visibility
            .defer_hide(Instant::now(), Duration::from_millis(1200));
        self.start_animating();
    }

    fn expand(&mut self) {
        self.visibility.reveal();
        self.start_animating();
    }

    /// Actualiza únicamente la posición física de la ventana en pantalla sin redibujar
    fn update_position(&self) {
        let size = SIZE {
            cx: self.surface.width as i32,
            cy: self.surface.height as i32,
        };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };

        unsafe {
            let _ = UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&self.pos),
                Some(&size),
                Some(self.surface.dc),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }

    /// Redibuja el dock en la superficie y lo envía al compositor
    fn render(&mut self) {
        self.bar.draw(&mut self.surface.pixmap, self.scale);
        self.surface.copy_to_dib();
        self.update_position();
    }

    /// Enciende el temporizador de 60 fps
    fn start_animating(&mut self) {
        if !self.animating {
            self.animating = true;
            self.last_tick = Instant::now();
            unsafe {
                let _ = SetTimer(Some(self.hwnd), TIMER_ANIM, FRAME_MS, None);
            }
        }
    }

    /// Apaga el temporizador de 60 fps cuando los resortes se asientan (0% CPU)
    fn stop_animating(&mut self) {
        if self.animating {
            self.animating = false;
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_ANIM);
            }
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_SHOW_LAUNCHER => {
            with_app(|app| app.toggle_launcher());
            LRESULT(0)
        }
        WM_DROPFILES => {
            use windows::Win32::UI::Shell::{DragFinish, DragQueryFileW, HDROP};
            let drop = HDROP(wparam.0 as *mut _);
            let count = DragQueryFileW(drop, u32::MAX, None);
            let mut paths = Vec::new();
            for i in 0..count {
                let len = DragQueryFileW(drop, i, None);
                let mut wide = vec![0u16; len as usize + 1];
                DragQueryFileW(drop, i, Some(&mut wide));
                paths.push(String::from_utf16_lossy(&wide[..len as usize]));
            }
            DragFinish(drop);
            with_app(|app| {
                let mut changed = false;
                for path in paths {
                    if app
                        .bar
                        .primary_apps
                        .iter()
                        .any(|a| a.entry.command.eq_ignore_ascii_case(&path))
                    {
                        continue;
                    }
                    if let Some(item) = pins::from_drop(path) {
                        app.bar.primary_apps.push(item);
                        changed = true;
                    }
                }
                if changed {
                    app.pins_changed();
                }
            });
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let x = (lparam.0 & 0xffff) as i16 as f32;
            let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32;

            with_app(|app| {
                // Al tocar la barra, expande hacia arriba de inmediato
                app.expand();

                if !app.mouse_tracked {
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut tme);
                    app.mouse_tracked = true;
                }

                let target = app.bar.hit_test(x, y, app.scale);
                if target != app.bar.hovered {
                    app.bar.update_hover(target);
                    app.start_animating();
                    app.render();
                }
            });
            LRESULT(0)
        }

        WM_MOUSELEAVE => {
            with_app(|app| {
                app.mouse_tracked = false;

                if app.bar.hovered.is_some() {
                    app.bar.update_hover(None);
                    app.render();
                }

                app.start_animating();
            });
            LRESULT(0)
        }

        WM_LBUTTONDOWN => {
            with_app(|app| {
                if let Some(target) = app.bar.hovered {
                    app.bar.pressed = Some(target);
                    match target {
                        HoverTarget::Launcher => app.bar.launcher_press.set_target(0.90),
                        HoverTarget::PrimaryApp(i) => {
                            if let Some(a) = app.bar.primary_apps.get_mut(i) {
                                a.press_spring.set_target(0.90);
                            }
                        }
                        _ => {}
                    }
                    app.start_animating();
                    app.render();
                }
            });
            LRESULT(0)
        }

        WM_LBUTTONUP => {
            with_app(|app| {
                if let Some(target) = app.bar.pressed.take() {
                    match target {
                        HoverTarget::Launcher => {
                            app.bar.launcher_press.set_target(1.0);
                            app.toggle_launcher();
                        }
                        HoverTarget::PrimaryApp(i) => {
                            if let Some(menu) = &mut app.launcher {
                                menu.hide();
                            }
                            if let Some(a) = app.bar.primary_apps.get_mut(i) {
                                a.press_spring.set_target(1.0);
                                if a.launch() {
                                    let _ = catalog::record(&a.entry);
                                }
                            }
                        }
                        HoverTarget::Clock => {
                            let _ = std::process::Command::new("explorer.exe")
                                .arg("ms-settings:dateandtime")
                                .spawn();
                        }
                    }
                    app.start_animating();
                    app.render();
                }
            });
            LRESULT(0)
        }

        WM_RBUTTONUP => {
            with_app(|app| {
                let x = (lparam.0 & 0xffff) as i16 as f32;
                let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32;
                app.bar.hovered = app.bar.hit_test(x, y, app.scale);
                if let Some(cmd) = app
                    .tray
                    .show_context_menu(matches!(app.bar.hovered, Some(HoverTarget::PrimaryApp(_))))
                {
                    handle_menu_command(app, cmd);
                }
            });
            LRESULT(0)
        }

        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),

        WM_TIMER => {
            let id = wparam.0;
            if id == TIMER_ANIM {
                with_app(|app| {
                    let now = Instant::now();
                    let dt = now.duration_since(app.last_tick).as_secs_f32();
                    app.last_tick = now;

                    let slide_moving = app.visibility.step(now, dt);
                    let bar_moving = app.bar.step_animations(dt);

                    // Interpolación física del desplazamiento vertical
                    let current_y = (app.y_hidden as f32
                        + (app.y_shown - app.y_hidden) as f32 * app.visibility.motion.value)
                        .round() as i32;

                    if current_y != app.pos.y {
                        app.pos.y = current_y;
                        app.update_position();
                    }

                    if bar_moving {
                        app.render();
                    }

                    if !bar_moving && !slide_moving && !app.visibility.waiting() {
                        app.stop_animating();
                    }
                });
            } else if id == TIMER_TICK {
                with_app(|app| {
                    app.shell.maintain();
                    apps::update_running_apps(&mut app.bar.primary_apps);
                    app.render();
                });
            } else if id == TIMER_EDGE {
                with_app(|app| {
                    // Detección en el borde inferior de la pantalla para auto-hide
                    let mut pt = POINT::default();
                    unsafe {
                        let _ = GetCursorPos(&mut pt);
                    }
                    let at_edge = cursor_at_edge(
                        pt,
                        app.screen_left,
                        app.screen_right,
                        app.screen_bottom,
                        app.scale,
                    );
                    let inside = app.visibility.expanded
                        && pt.x >= app.pos.x
                        && pt.x < app.pos.x + app.surface.width as i32
                        && pt.y >= app.y_shown
                        && pt.y < app.screen_bottom;
                    app.visibility.sample(
                        at_edge || inside || app.launcher.as_ref().is_some_and(|l| l.visible),
                        Instant::now(),
                    );
                    if app.visibility.active() {
                        app.start_animating();
                    }
                });
            }
            LRESULT(0)
        }

        WM_TRAY => {
            let event = (lparam.0 & 0xffff) as u32;
            if event == WM_RBUTTONUP {
                with_app(|app| {
                    if let Some(cmd) = app.tray.show_context_menu(false) {
                        handle_menu_command(app, cmd);
                    }
                });
            }
            LRESULT(0)
        }

        windows::Win32::UI::WindowsAndMessaging::WM_ENDSESSION => {
            if wparam.0 != 0 {
                with_app(|app| app.shell.restore());
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DESTROY => {
            with_app(|app| {
                app.shell.restore();
            });
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn handle_menu_command(app: &mut App, cmd: u32) {
    match cmd {
        CMD_REMOVE => {
            if let Some(HoverTarget::PrimaryApp(i)) = app.bar.hovered {
                if i < app.bar.primary_apps.len() {
                    app.bar.primary_apps.remove(i);
                    app.pins_changed();
                }
            }
        }
        CMD_AUTOSTART => {
            let active = autostart::is_enabled();
            autostart::set(!active);
        }
        CMD_EXIT => {
            app.shell.restore();
            unsafe {
                PostQuitMessage(0);
            }
        }
        _ => {}
    }
}

fn cursor_at_edge(cursor: POINT, left: i32, right: i32, bottom: i32, scale: f32) -> bool {
    cursor.x >= left
        && cursor.x < right
        && cursor.y >= bottom - (6.0 * scale).ceil() as i32
        && cursor.y < bottom
}

#[cfg(test)]
mod autohide_tests {
    use super::*;
    #[test]
    fn edge_activation_stays_on_its_monitor_and_scales_with_dpi() {
        assert!(cursor_at_edge(
            POINT { x: 100, y: 1078 },
            0,
            1920,
            1080,
            1.0
        ));
        assert!(!cursor_at_edge(
            POINT { x: 100, y: 1000 },
            0,
            1920,
            1080,
            1.0
        ));
        assert!(!cursor_at_edge(
            POINT { x: 2000, y: 1078 },
            0,
            1920,
            1080,
            1.0
        ));
        assert!(!cursor_at_edge(
            POINT { x: 100, y: 1085 },
            0,
            1920,
            1080,
            1.0
        ));
        assert!(cursor_at_edge(
            POINT { x: -100, y: 1070 },
            -1920,
            0,
            1080,
            2.0
        ));
    }
}

fn main() -> Result<()> {
    unsafe {
        if std::env::args().any(|arg| arg == "--replace") {
            if let Ok(old) =
                windows::Win32::UI::WindowsAndMessaging::FindWindowW(w!("GlassBarWindow"), None)
            {
                let mut process_id = 0;
                windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
                    old,
                    Some(&mut process_id),
                );
                let previous = windows::Win32::System::Threading::OpenProcess(
                    windows::Win32::System::Threading::PROCESS_SYNCHRONIZE,
                    false,
                    process_id,
                )
                .ok();
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(old),
                    windows::Win32::UI::WindowsAndMessaging::WM_CLOSE,
                    WPARAM(0),
                    LPARAM(0),
                );
                if let Some(previous) = previous {
                    let _ = windows::Win32::System::Threading::WaitForSingleObject(previous, 3000);
                    let _ = windows::Win32::Foundation::CloseHandle(previous);
                }
                for _ in 0..60 {
                    if !windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(old)).as_bool() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
        // Garantiza una sola instancia de GlassBar en ejecución
        let _instance = CreateMutexW(None, false, w!("Local\\GlassBar.Instance"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(existing) =
                windows::Win32::UI::WindowsAndMessaging::FindWindowW(w!("GlassBarWindow"), None)
            {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(existing),
                    WM_SHOW_LAUNCHER,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
            return Ok(());
        }

        // Modo Per-Monitor DPI Aware v2
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let instance = GetModuleHandleW(None)?;
        let class_name = w!("GlassBarWindow");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err(windows::core::Error::from_thread());
        }

        let launcher_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(launcher_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("GlassBarLauncher"),
            ..Default::default()
        };
        if RegisterClassExW(&launcher_class) == 0 {
            return Err(windows::core::Error::from_thread());
        }

        // Ventana flotante transparente por píxel, siempre encima y sin robar foco
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            w!("GlassBar"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance.into()),
            None,
        )?;

        windows::Win32::UI::Shell::DragAcceptFiles(hwnd, true);
        let mut app = App::new(hwnd)?;
        app.render();

        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

        // Inicia el temporizador de animaciones (para el primer cuadro de entrada)
        app.start_animating();

        // Inicia el temporizador de reloj y verificación de apps (cada 1s)
        let _ = SetTimer(Some(hwnd), TIMER_TICK, TICK_MS, None);

        // Inicia el temporizador de proximidad al borde de pantalla (cada 60ms)
        let _ = SetTimer(Some(hwnd), TIMER_EDGE, EDGE_MS, None);

        APP.with(|cell| *cell.borrow_mut() = Some(app));

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        APP.with(|cell| cell.borrow_mut().take());
    }
    Ok(())
}
