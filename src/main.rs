#![windows_subsystem = "windows"]

//! # GlassBar
//! Barra de tareas moderna ultra-transparente estilo Windows 11 Glass para Windows, ultraligera en Rust.
//! Comportamiento auto-hide nativo (sale suavemente desde abajo al acercar el mouse) y estética translúcida.

mod apps;
mod audio;
mod autohide;
mod autostart;
mod bar;
mod bluetooth;
mod catalog;
mod clock;
mod glass;
mod guard;
mod icons;
mod launcher;
mod monitors;
mod pins;
mod priority;
mod quick;
mod shell;
mod spring;
mod status;
mod system;
mod textfield;
mod tray;
mod usage;
mod wifi;
mod winkey;

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
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetMessageW, KillTimer,
    LoadCursorW, PostQuitMessage, RegisterClassExW, SetTimer, ShowWindow, TranslateMessage,
    UpdateLayeredWindow, IDC_ARROW, MA_NOACTIVATE, MSG, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA,
    WM_DESTROY, WM_DROPFILES, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::autohide::AutoHide;
use crate::bar::{BarState, HoverTarget, BOTTOM_MARGIN};
use crate::shell::TaskbarReplacer;
use crate::tray::{Tray, CMD_AUTOSTART, CMD_EXIT, CMD_REMOVE, WM_TRAY};

const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_SHOW_LAUNCHER: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 2;
/// Cambió la ventana activa: el dock comprueba si debe ponerse encima o apartarse.
const WM_FOREGROUND: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 3;
/// El hilo de sistema terminó de leer Wi-Fi, Bluetooth, brillo y red.
const WM_QUICK_UPDATE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 4;

const TIMER_ANIM: usize = 1;
const TIMER_TICK: usize = 2;
const TIMER_EDGE: usize = 3;
const TIMER_TIP: usize = 4;

/// ~60 FPS durante animaciones físicas (16 ms). En reposo el timer se apaga (0% CPU).
const FRAME_MS: u32 = 16;
/// Refresco del reloj y verificación de apps activas.
const TICK_MS: u32 = 1000;
/// Verificación de proximidad del cursor al borde inferior para auto-hide inmediato (60 ms).
const EDGE_MS: u32 = 40;
/// Tiempo con el cursor quieto sobre un icono antes de mostrar su nombre.
const TIP_MS: u32 = 600;

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

/// Globo con el nombre del programa bajo el cursor; no recibe clics ni foco.
struct Tooltip {
    hwnd: HWND,
    surface: Option<Surface>,
    visible: bool,
}
impl Tooltip {
    fn new(owner: HWND) -> Result<Self> {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_TRANSPARENT,
                w!("GlassBarTooltip"),
                w!("GlassBar — Nombre"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                Some(owner),
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?
        };
        Ok(Self {
            hwnd,
            surface: None,
            visible: false,
        })
    }
    /// Muestra `text` centrado en `cx` con su borde inferior en `bottom` (coordenadas de pantalla).
    fn show(&mut self, text: &str, cx: i32, bottom: i32, left: i32, right: i32, scale: f32) {
        let size = 12.0 * scale;
        let text = clock::fit_text(text, size, 320.0 * scale);
        let width = (clock::text_width(&text, size) + 22.0 * scale).ceil() as u32;
        let height = (28.0 * scale).ceil() as u32;
        let Ok(mut surface) = Surface::new(width, height) else {
            return;
        };
        let (w, h) = (width as f32, height as f32);
        surface.pixmap.fill(tiny_skia::Color::TRANSPARENT);
        apps::fill_rounded(
            &mut surface.pixmap,
            [32, 32, 32, 235],
            0.5,
            0.5,
            w - 1.0,
            h - 1.0,
            7.0 * scale,
        );
        apps::stroke_rounded(
            &mut surface.pixmap,
            [255, 255, 255, 40],
            0.5,
            0.5,
            w - 1.0,
            h - 1.0,
            7.0 * scale,
            1.0,
        );
        clock::draw_text(
            &mut surface.pixmap,
            &text,
            11.0 * scale,
            h / 2.0 - size * 0.38,
            size,
            1.0,
        );
        surface.copy_to_dib();
        let pos = POINT {
            x: (cx - width as i32 / 2).clamp(left + 4, (right - width as i32 - 4).max(left + 4)),
            y: bottom - height as i32,
        };
        let size = SIZE {
            cx: width as i32,
            cy: height as i32,
        };
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
                Some(&pos),
                Some(&size),
                Some(surface.dc),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        self.surface = Some(surface);
        self.visible = true;
    }
    fn hide(&mut self) {
        if self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible = false;
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
    transition: launcher::Transition,
}
impl LauncherWindow {
    fn new(
        owner: HWND,
        model: launcher::LauncherState,
        dock_left: i32,
        dock_top: i32,
    ) -> Result<Self> {
        let (screen, dpi) = monitors::get_info(monitors::primary());
        let scale = dpi
            .min((screen.right - screen.left - 24) as f32 / launcher::WIDTH)
            .min((dock_top - screen.top - 24) as f32 / launcher::HEIGHT)
            .max(0.3);
        let width = (launcher::WIDTH * scale).ceil() as u32;
        let height = (launcher::HEIGHT * scale).ceil() as u32;
        // Centrado sobre el dock, como el Inicio de Windows 11.
        let _ = dock_left;
        let pos = POINT {
            x: screen.left + (screen.right - screen.left - width as i32) / 2,
            y: dock_top - height as i32 - 12,
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
            model,
            visible: false,
            transition: launcher::Transition::hidden(),
        })
    }
    fn render(&mut self) {
        self.model.draw(&mut self.surface.pixmap, self.scale);
        self.surface.copy_to_dib();
        self.present();
    }
    /// Siguiente fotograma: solo se redibuja si el menú de energía está entrando;
    /// la apertura y el cierre del panel solo mueven y desvanecen la ventana.
    fn tick(&mut self) {
        if self.model.animating() {
            self.render();
        } else {
            self.present();
        }
    }
    fn present(&mut self) {
        let animating = self.visible && self.model.animating();
        present_popup(
            self.hwnd,
            &self.surface,
            self.pos,
            self.scale,
            &self.transition,
            self.visible,
            animating,
        );
    }
    fn show(&mut self) {
        self.visible = true;
        self.transition.open(Instant::now());
        self.render();
        take_focus(self.hwnd);
    }
    /// Empieza el cierre; la ventana se oculta cuando termina de desvanecerse.
    fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.transition.close(Instant::now());
        self.present();
    }
}
impl Drop for LauncherWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

/// Pone en pantalla un panel emergente (lanzador o panel rápido) ya dibujado, con la
/// opacidad y la posición de su transición. Mientras algo se mueve, un temporizador
/// pide el siguiente fotograma; al terminar de cerrarse, oculta la ventana.
fn present_popup(
    hwnd: HWND,
    surface: &Surface,
    pos: POINT,
    scale: f32,
    transition: &launcher::Transition,
    visible: bool,
    animating: bool,
) {
    let now = Instant::now();
    let shown = transition.shown(now);
    let running = transition.running(now);
    unsafe {
        if running || animating {
            let _ = SetTimer(Some(hwnd), TIMER_ANIM, FRAME_MS, None);
        } else {
            let _ = KillTimer(Some(hwnd), TIMER_ANIM);
        }
        if !visible && !running {
            let _ = ShowWindow(hwnd, SW_HIDE);
            return;
        }
    }
    let pos = POINT {
        x: pos.x,
        y: pos.y + (transition.offset(now) * scale).round() as i32,
    };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        SourceConstantAlpha: (shown * 255.0).round() as u8,
        AlphaFormat: AC_SRC_ALPHA as u8,
        ..Default::default()
    };
    unsafe {
        let _ = UpdateLayeredWindow(
            hwnd,
            None,
            Some(&pos),
            Some(&SIZE {
                cx: surface.width as i32,
                cy: surface.height as i32,
            }),
            Some(surface.dc),
            Some(&POINT { x: 0, y: 0 }),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
    }
}

/// Muestra un panel emergente y le da el foco, para que se cierre al hacer clic fuera.
fn take_focus(hwnd: HWND) {
    unsafe {
        use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow, SW_SHOW,
        };
        let _ = ShowWindow(hwnd, SW_SHOW);
        // Abierto con la tecla Windows no hubo clic en GlassBar y Windows puede
        // negar el foco; compartir la entrada con la ventana activa lo permite.
        let current = GetCurrentThreadId();
        let foreground = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let attached = foreground != 0
            && foreground != current
            && AttachThreadInput(current, foreground, true).as_bool();
        let _ = SetForegroundWindow(hwnd);
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(hwnd));
        if attached {
            let _ = AttachThreadInput(current, foreground, false);
        }
    }
}

/// Panel rápido (Wi-Fi, Bluetooth, brillo y volumen) sobre los iconos de estado.
struct QuickWindow {
    hwnd: HWND,
    surface: Surface,
    pos: POINT,
    scale: f32,
    model: quick::QuickState,
    visible: bool,
    transition: launcher::Transition,
    backend: system::Backend,
}
impl QuickWindow {
    fn new(owner: HWND) -> Result<Self> {
        let (_, dpi) = monitors::get_info(monitors::primary());
        let scale = dpi.max(0.3);
        let width = (quick::WIDTH * scale).ceil() as u32;
        let height = (quick::HEIGHT * scale).ceil() as u32;
        let surface = Surface::new(width, height)?;
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                w!("GlassBarQuick"),
                w!("GlassBar — Configuración rápida"),
                WS_POPUP,
                0,
                0,
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
            pos: POINT::default(),
            scale,
            model: quick::QuickState::default(),
            visible: false,
            transition: launcher::Transition::hidden(),
            backend: system::Backend::start(hwnd, WM_QUICK_UPDATE),
        })
    }
    /// Encima de los iconos de estado (`anchor_x`), sin salirse de la pantalla.
    fn place(&mut self, anchor_x: i32, dock_top: i32) {
        let (screen, _) = monitors::get_info(monitors::primary());
        let width = self.surface.width as i32;
        let margin = (12.0 * self.scale).round() as i32;
        self.pos = POINT {
            x: (anchor_x - width / 2).clamp(
                screen.left + margin,
                (screen.right - width - margin).max(screen.left + margin),
            ),
            y: dock_top - self.surface.height as i32 - margin,
        };
    }
    fn render(&mut self) {
        self.model.draw(&mut self.surface.pixmap, self.scale);
        self.surface.copy_to_dib();
        self.present();
    }
    fn present(&mut self) {
        present_popup(
            self.hwnd,
            &self.surface,
            self.pos,
            self.scale,
            &self.transition,
            self.visible,
            false,
        );
    }
    /// Cambia entre la vista principal y la de redes. La de redes es más alta: el
    /// panel crece hacia arriba y su borde inferior queda en el mismo sitio.
    fn set_view(&mut self, view: quick::View) {
        self.model.view = view;
        self.model.hovered = None;
        self.model.list_scroll = 0.0;
        let height = (self.model.height() * self.scale).ceil() as u32;
        if height == self.surface.height {
            return;
        }
        if let Ok(surface) = Surface::new(self.surface.width, height) {
            let bottom = self.pos.y + self.surface.height as i32;
            self.surface = surface;
            self.pos.y = bottom - height as i32;
        }
    }
    fn show(&mut self) {
        self.visible = true;
        // Siempre abre en la vista principal, como el panel de Windows.
        let anchor_bottom = self.pos.y + self.surface.height as i32;
        self.set_view(quick::View::Main);
        self.pos.y = anchor_bottom - self.surface.height as i32;
        self.model.join = None;
        // Lo último conocido mientras el hilo de sistema vuelve a leerlo.
        self.model.system = self.backend.snapshot();
        self.backend.refresh();
        self.transition.open(Instant::now());
        self.render();
        take_focus(self.hwnd);
    }
    fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.model.dragging = None;
        self.model.hovered = None;
        self.transition.close(Instant::now());
        self.present();
    }
    /// Lleva una barra al valor `value` (de 0 a 1) y lo aplica al sistema.
    fn set_slider(&mut self, slider: quick::Slider, value: f32) {
        match slider {
            quick::Slider::Volume => {
                let muted = self.model.volume.is_some_and(|v| v.muted) && value == 0.0;
                self.model.volume = Some(status::Volume {
                    level: value,
                    muted,
                });
                status::set_volume(value);
            }
            quick::Slider::Brightness => {
                let percent = (value * 100.0).round() as u8;
                self.model.system.brightness = Some(percent);
                self.backend.set_brightness(percent);
            }
        }
        self.render();
    }
}
impl Drop for QuickWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn quick_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        WA_INACTIVE, WM_ACTIVATE, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE, WM_KEYDOWN, WM_MOUSEWHEEL,
    };
    // Coordenadas lógicas del panel a partir de las del mensaje.
    let point = |scale: f32| {
        (
            (lparam.0 & 0xffff) as i16 as f32 / scale,
            ((lparam.0 >> 16) & 0xffff) as i16 as f32 / scale,
        )
    };
    match msg {
        WM_ACTIVATE if (wparam.0 & 0xffff) as u32 == WA_INACTIVE => {
            with_app(|app| app.quick.as_mut().map(QuickWindow::hide));
        }
        WM_CLOSE => {
            with_app(|app| app.quick.as_mut().map(QuickWindow::hide));
        }
        WM_TIMER if wparam.0 == TIMER_ANIM => {
            with_app(|app| app.quick.as_mut().map(QuickWindow::present));
        }
        WM_QUICK_UPDATE => {
            with_app(|app| {
                if let Some(panel) = &mut app.quick {
                    panel.model.system = panel.backend.snapshot();
                    panel.model.settle(Instant::now());
                    if panel.visible {
                        panel.render();
                    }
                }
            });
        }
        WM_CAPTURECHANGED => {
            with_app(|app| {
                if let Some(panel) = &mut app.quick {
                    panel.model.dragging = None;
                }
            });
        }
        WM_MOUSELEAVE => {
            with_app(|app| {
                if let Some(panel) = app.quick.as_mut().filter(|p| p.visible) {
                    if panel.model.hovered.take().is_some() {
                        panel.render();
                    }
                }
            });
        }
        WM_MOUSEMOVE => {
            with_app(|app| {
                let Some(panel) = app.quick.as_mut().filter(|p| p.visible) else {
                    return;
                };
                let mut track = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut track);
                let (x, y) = point(panel.scale);
                if let Some(slider) = panel.model.dragging {
                    panel.set_slider(slider, quick::QuickState::slider_value(x));
                    return;
                }
                let hit = panel.model.hit_test(x, y);
                if hit != panel.model.hovered {
                    panel.model.hovered = hit;
                    panel.render();
                }
            });
        }
        WM_LBUTTONDOWN => {
            with_app(|app| {
                let Some(panel) = app.quick.as_mut().filter(|p| p.visible) else {
                    return;
                };
                let (x, y) = point(panel.scale);
                if let Some(quick::Hit::Slider(slider)) = panel.model.hit_test(x, y) {
                    SetCapture(hwnd);
                    panel.model.dragging = Some(slider);
                    panel.set_slider(slider, quick::QuickState::slider_value(x));
                }
            });
        }
        WM_LBUTTONUP => {
            with_app(|app| {
                let Some(panel) = app.quick.as_mut().filter(|p| p.visible) else {
                    return;
                };
                if panel.model.dragging.take().is_some() {
                    let _ = ReleaseCapture();
                    panel.render();
                    return;
                }
                let (x, y) = point(panel.scale);
                match panel.model.hit_test(x, y) {
                    Some(quick::Hit::Tile(radio)) => {
                        // Se ve al instante; el hilo de sistema lo confirma al terminar.
                        let state = match radio {
                            system::Radio::Wifi => &mut panel.model.system.wifi,
                            system::Radio::Bluetooth => &mut panel.model.system.bluetooth,
                        };
                        if let Some(on) = state {
                            *on = !*on;
                            panel.backend.set_radio(radio, *on);
                        }
                        panel.render();
                    }
                    Some(quick::Hit::Mute) => {
                        if let Some(volume) = &mut panel.model.volume {
                            volume.muted = !volume.muted;
                            status::set_mute(volume.muted);
                        }
                        panel.render();
                    }
                    Some(quick::Hit::More) => {
                        panel.hide();
                        status::open_quick_settings();
                    }
                    Some(quick::Hit::List(radio)) => {
                        panel.set_view(quick::View::List(radio));
                        if radio == system::Radio::Wifi {
                            panel.backend.scan();
                        } else {
                            panel.backend.refresh();
                        }
                        panel.render();
                    }
                    Some(quick::Hit::AudioList) => {
                        panel.model.audio_app = None;
                        panel.set_view(quick::View::Audio);
                        panel.backend.refresh();
                        panel.render();
                    }
                    Some(quick::Hit::Back) => {
                        panel.model.join = None;
                        panel.model.audio_app = None;
                        panel.set_view(quick::View::Main);
                        panel.render();
                    }
                    Some(quick::Hit::Row(index)) => {
                        let open = |uri: &str| {
                            let _ = std::process::Command::new("explorer.exe").arg(uri).spawn();
                        };
                        let pending = |key: String, connect| quick::Pending {
                            key,
                            connect,
                            since: Instant::now(),
                        };
                        if panel.model.view == quick::View::Audio {
                            let row = panel.model.audio_rows().get(index).copied();
                            let mixer = &mut panel.model.system.audio;
                            // Se ve al instante; el hilo de sistema lo confirma al releer.
                            match row {
                                Some(quick::AudioRow::Output(output)) => {
                                    if let Some(output) = mixer.outputs.get(output) {
                                        mixer.default = Some(output.id.clone());
                                        panel.backend.set_default_output(output.id.clone());
                                    }
                                }
                                Some(quick::AudioRow::App(app)) => {
                                    panel.model.toggle_audio_app(app);
                                }
                                Some(quick::AudioRow::Choice(app, output)) => {
                                    let id = output
                                        .and_then(|o| mixer.outputs.get(o))
                                        .map(|o| o.id.clone());
                                    if let Some(app) = mixer.apps.get_mut(app) {
                                        app.output = id.clone();
                                        panel.backend.set_app_output(app.pids.clone(), id);
                                    }
                                    panel.model.audio_app = None;
                                    panel.model.scroll_list(0.0);
                                }
                                _ => {}
                            }
                            panel.render();
                            return;
                        }
                        if panel.model.view == quick::View::List(system::Radio::Bluetooth) {
                            let Some(device) = panel.model.devices().get(index).cloned() else {
                                return;
                            };
                            if device.audio {
                                let connect = !device.connected;
                                panel.model.pending =
                                    Some(pending(device.address.to_string(), connect));
                                panel.backend.set_bluetooth_audio(device.address, connect);
                            } else {
                                // Ratones, teclados… se conectan solos al encenderlos.
                                panel.hide();
                                open("ms-settings:bluetooth");
                                return;
                            }
                            panel.render();
                            return;
                        }
                        let Some(network) = panel.model.networks().get(index).cloned() else {
                            return;
                        };
                        if network.connected {
                            panel.model.pending = Some(pending(network.ssid.clone(), false));
                            panel.backend.disconnect();
                        } else if let Some(profile) = network.profile.clone() {
                            panel.model.pending = Some(pending(network.ssid.clone(), true));
                            panel.backend.connect(profile);
                        } else if network.security == Some(wifi::Security::Open) {
                            // Red abierta: se conecta directamente, sin contraseña.
                            panel.model.toggle_join(index);
                            if let Some((ssid, security, password)) =
                                panel.model.take_join(Instant::now())
                            {
                                panel.backend.join(ssid, security, password);
                            }
                        } else if network.security.is_some() {
                            panel.model.toggle_join(index);
                        } else {
                            // Redes de empresa (con usuario) o WEP: en la lista de Windows.
                            panel.hide();
                            open("ms-availablenetworks:");
                            return;
                        }
                        panel.render();
                    }
                    Some(quick::Hit::JoinConfirm) => {
                        if let Some((ssid, security, password)) =
                            panel.model.take_join(Instant::now())
                        {
                            panel.backend.join(ssid, security, password);
                        }
                        panel.render();
                    }
                    Some(quick::Hit::JoinCancel) => {
                        panel.model.join = None;
                        panel.model.scroll_list(0.0);
                        panel.render();
                    }
                    Some(quick::Hit::Settings) => {
                        let uri = match panel.model.view {
                            quick::View::List(system::Radio::Bluetooth) => "ms-settings:bluetooth",
                            // "Volumen y dispositivos de aplicaciones" de Windows.
                            quick::View::Audio => "ms-settings:apps-volume",
                            _ => "ms-settings:network-wifi",
                        };
                        panel.hide();
                        let _ = std::process::Command::new("explorer.exe").arg(uri).spawn();
                    }
                    _ => {}
                }
            });
        }
        // La rueda sobre una barra la mueve de 2 % en 2 %.
        WM_MOUSEWHEEL => {
            with_app(|app| {
                let Some(panel) = app.quick.as_mut().filter(|p| p.visible) else {
                    return;
                };
                let steps = ((wparam.0 >> 16) & 0xffff) as i16 as f32 / 120.0;
                // En las listas, la rueda las desplaza (algo más de una fila por muesca).
                if matches!(panel.model.view, quick::View::List(_)) {
                    panel.model.scroll_list(-steps * 60.0);
                    let (x, y) = {
                        let mut cursor = POINT::default();
                        let _ = GetCursorPos(&mut cursor);
                        (
                            (cursor.x - panel.pos.x) as f32 / panel.scale,
                            (cursor.y - panel.pos.y) as f32 / panel.scale,
                        )
                    };
                    panel.model.hovered = panel.model.hit_test(x, y);
                    panel.render();
                    return;
                }
                let slider = match panel.model.hovered {
                    Some(quick::Hit::Slider(slider)) => slider,
                    Some(quick::Hit::Mute) => quick::Slider::Volume,
                    _ => return,
                };
                if let Some(value) = panel.model.value(slider) {
                    let value = ((value * 50.0).round() + steps).clamp(0.0, 50.0) / 50.0;
                    panel.set_slider(slider, value);
                }
            });
        }
        // Con el formulario de una red nueva abierto, el teclado escribe la contraseña.
        WM_CHAR => {
            with_app(|app| {
                let Some(panel) = app.quick.as_mut().filter(|p| p.visible) else {
                    return;
                };
                let Some(join) = &mut panel.model.join else {
                    return;
                };
                if let Some(ch) = char::from_u32(wparam.0 as u32).filter(|c| !c.is_control()) {
                    join.field.insert(&ch.to_string());
                    join.error = None;
                    panel.render();
                }
            });
        }
        WM_KEYDOWN => {
            with_app(|app| {
                let Some(panel) = app.quick.as_mut().filter(|p| p.visible) else {
                    return;
                };
                let Some(join) = &mut panel.model.join else {
                    // Sin formulario, Escape cierra el panel.
                    if wparam.0 == 0x1B {
                        panel.hide();
                    }
                    return;
                };
                use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
                let ctrl = GetKeyState(0x11) < 0;
                let shift = GetKeyState(0x10) < 0;
                match wparam.0 {
                    // Escape cierra el formulario; Enter se conecta (o muestra por qué no).
                    0x1B => {
                        panel.model.join = None;
                        panel.model.scroll_list(0.0);
                    }
                    0x0D => {
                        if let Some((ssid, security, password)) =
                            panel.model.take_join(Instant::now())
                        {
                            panel.backend.join(ssid, security, password);
                        }
                    }
                    // Al editar la contraseña se quita el aviso anterior.
                    key => {
                        match key {
                            0x08 => join.field.backspace(ctrl),
                            0x2E => join.field.delete(ctrl),
                            0x25 => join.field.left(ctrl, shift),
                            0x27 => join.field.right(ctrl, shift),
                            0x24 => join.field.home(shift),
                            0x23 => join.field.end(shift),
                            0x41 if ctrl => join.field.select_all(),
                            // Pegar sí; copiar o cortar una contraseña oculta, no.
                            0x56 if ctrl => {
                                if let Some(text) = clipboard_text(hwnd) {
                                    join.field.insert(&text);
                                }
                            }
                            _ => return,
                        }
                        join.error = None;
                    }
                }
                panel.render();
            });
        }
        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
    }
    LRESULT(0)
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

/// Copia `text` al portapapeles como texto Unicode.
unsafe fn set_clipboard_text(hwnd: HWND, text: &str) {
    use windows::Win32::Foundation::{GlobalFree, HANDLE};
    use windows::Win32::System::{DataExchange::*, Memory::*};
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    if OpenClipboard(Some(hwnd)).is_err() {
        return;
    }
    let _ = EmptyClipboard();
    if let Ok(global) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
        let pointer = GlobalLock(global).cast::<u16>();
        if pointer.is_null() {
            let _ = GlobalFree(Some(global));
        } else {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), pointer, wide.len());
            let _ = GlobalUnlock(global);
            // 13 = CF_UNICODETEXT. Si Windows lo acepta, la memoria pasa a ser suya.
            if SetClipboardData(13, Some(HANDLE(global.0))).is_err() {
                let _ = GlobalFree(Some(global));
            }
        }
    }
    let _ = CloseClipboard();
}

/// Bloquea, suspende, apaga o reinicia. Apagar y reiniciar piden a Windows el
/// cierre normal, sin forzar el de las aplicaciones abiertas.
fn run_power(option: launcher::Power) -> bool {
    use launcher::Power;
    let shutdown = |flag: &str| {
        std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .map(|root| {
                std::process::Command::new(root.join("System32").join("shutdown.exe"))
                    .args([flag, "/t", "0"])
                    .spawn()
                    .is_ok()
            })
            .unwrap_or(false)
    };
    match option {
        Power::Lock => unsafe { windows::Win32::System::Shutdown::LockWorkStation().is_ok() },
        Power::Sleep => unsafe {
            windows::Win32::System::Power::SetSuspendState(false, false, false)
        },
        Power::Shutdown => shutdown("/s"),
        Power::Restart => shutdown("/r"),
    }
}

/// Lee el catálogo y los iconos de la vista inicial en otro hilo.
fn prepare_launcher(
    pinned: &[apps::AppEntry],
) -> std::sync::mpsc::Receiver<launcher::LauncherState> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let pinned = pinned.to_vec();
    std::thread::spawn(move || {
        let mut model = launcher::LauncherState::new(&pinned);
        model.preload_icons();
        let _ = sender.send(model);
    });
    receiver
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
        WM_TIMER if wparam.0 == TIMER_ANIM => {
            with_app(|app| {
                if let Some(menu) = &mut app.launcher {
                    menu.tick();
                }
            });
        }
        // Mientras se cierra, el panel ya no responde al ratón ni al teclado.
        WM_MOUSEMOVE | WM_LBUTTONUP => {
            with_app(|app| {
                if let Some(menu) = app.launcher.as_mut().filter(|menu| menu.visible) {
                    let x = (lparam.0 & 0xffff) as i16 as f32 / menu.scale;
                    let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32 / menu.scale;
                    let hit = menu.model.hit_test(x, y);
                    // Mover el ratón dentro del mismo elemento no cambia nada visible.
                    if msg == WM_MOUSEMOVE && hit == menu.model.hovered {
                        return;
                    }
                    if msg == WM_LBUTTONUP {
                        if menu.model.in_search(x, y) {
                            let shift =
                                windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x10) < 0;
                            menu.model.click_search(x, shift);
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
                if let Some(menu) = app.launcher.as_mut().filter(|menu| menu.visible) {
                    if !menu.model.power_menu {
                        // 120 unidades por muesca de la rueda: algo más de una fila.
                        let delta = ((wparam.0 >> 16) & 0xffff) as i16 as f32;
                        menu.model.scroll_by(-delta / 120.0 * 96.0);
                        menu.render();
                    }
                }
            });
        }
        WM_CHAR => {
            with_app(|app| {
                if let Some(menu) = app.launcher.as_mut().filter(|menu| menu.visible) {
                    // Retroceso (y Ctrl+Retroceso, que llega como 0x7F) se atiende en WM_KEYDOWN.
                    if let Some(ch) = char::from_u32(wparam.0 as u32).filter(|c| !c.is_control()) {
                        menu.model.insert_text(&ch.to_string());
                        menu.render();
                    }
                }
            });
        }
        WM_KEYDOWN => {
            with_app(|app| {
                if let Some(menu) = app.launcher.as_mut().filter(|menu| menu.visible) {
                    use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
                    let ctrl = GetKeyState(0x11) < 0;
                    let shift = GetKeyState(0x10) < 0;
                    let menu_open = menu.model.power_menu;
                    match wparam.0 {
                        0x1B => {
                            if menu_open {
                                menu.model.power_menu = false;
                                menu.render();
                            } else {
                                menu.hide();
                            }
                        }
                        0x0D if !menu_open => {
                            if let Some(hit) = menu.model.first_match() {
                                action = menu.model.activate(hit);
                            }
                        }
                        // Ctrl+A, Ctrl+C, Ctrl+X y Ctrl+V, como en cualquier cuadro de texto.
                        0x41 if ctrl && !menu_open => {
                            menu.model.edit(|field| field.select_all());
                            menu.render();
                        }
                        0x43 if ctrl && !menu_open => {
                            if let Some(text) = menu.model.query.selected_text() {
                                set_clipboard_text(hwnd, &text);
                            }
                        }
                        0x58 if ctrl && !menu_open => {
                            if let Some(Some(text)) = menu.model.edit(|field| field.cut()) {
                                set_clipboard_text(hwnd, &text);
                                menu.render();
                            }
                        }
                        0x56 if ctrl && !menu_open => {
                            if let Some(text) = clipboard_text(hwnd) {
                                menu.model.insert_text(&text);
                                menu.render();
                            }
                        }
                        // Retroceso y Supr; con Ctrl, palabra a palabra.
                        0x08 if !menu_open => {
                            menu.model.edit(|field| field.backspace(ctrl));
                            menu.render();
                        }
                        0x2E if !menu_open => {
                            menu.model.edit(|field| field.delete(ctrl));
                            menu.render();
                        }
                        // Las flechas recorren los resultados si hay uno elegido; con Ctrl
                        // o Mayús siempre mueven el cursor del texto.
                        0x25 | 0x27 if !menu_open => {
                            let forward = wparam.0 == 0x27;
                            if menu.model.selection.is_some() && !ctrl && !shift {
                                menu.model.move_selection(if forward { 1 } else { -1 }, 0);
                            } else {
                                menu.model.selection = None;
                                menu.model.edit(|field| {
                                    if forward {
                                        field.right(ctrl, shift)
                                    } else {
                                        field.left(ctrl, shift)
                                    }
                                });
                            }
                            menu.render();
                        }
                        0x24 if !menu_open => {
                            menu.model.edit(|field| field.home(shift));
                            menu.render();
                        }
                        0x23 if !menu_open => {
                            menu.model.edit(|field| field.end(shift));
                            menu.render();
                        }
                        0x28 if !menu.model.power_menu => {
                            menu.model.move_selection(0, 1);
                            menu.render();
                        }
                        0x26 if !menu.model.power_menu => {
                            menu.model.move_selection(0, -1);
                            menu.render();
                        }
                        0x22 if !menu.model.power_menu => {
                            menu.model.scroll_page(true);
                            menu.render();
                        }
                        0x21 if !menu.model.power_menu => {
                            menu.model.scroll_page(false);
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
            launcher::Action::Power(option) => {
                if !run_power(option) {
                    windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
                        Some(hwnd),
                        w!("Windows no aceptó la acción de energía."),
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
    screen_top: i32,
    screen_bottom: i32,
    screen_left: i32,
    screen_right: i32,
    /// Hay una app a pantalla completa en el monitor del dock: no aparece al llegar al borde.
    fullscreen: bool,
    visibility: AutoHide,
    bar: BarState,
    shell: TaskbarReplacer,
    tray: Tray,
    launcher: Option<LauncherWindow>,
    /// Panel rápido; se crea la primera vez que se abre.
    quick: Option<QuickWindow>,
    /// Lanzador preparado en segundo plano al iniciar (catálogo e iconos), para que
    /// la primera apertura no tenga que esperar a leerlos.
    prepared: Option<std::sync::mpsc::Receiver<launcher::LauncherState>>,
    tooltip: Option<Tooltip>,
    animating: bool,
    last_tick: Instant,
    mouse_tracked: bool,
}

impl App {
    fn new(hwnd: HWND) -> Result<Self> {
        let monitor = monitors::primary();
        let (rc, scale) = monitors::get_info(monitor);

        let mut bar = BarState::new();
        apps::update_running_apps(&mut bar.primary_apps, &mut bar.open_apps);
        bar.status = status::read();
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
        let pinned: Vec<_> = bar.primary_apps.iter().map(|a| a.entry.clone()).collect();

        Ok(Self {
            hwnd,
            surface,
            pos,
            scale,
            y_shown,
            y_hidden,
            screen_top: rc.top,
            screen_bottom: rc.bottom,
            screen_left: rc.left,
            screen_right: rc.right,
            fullscreen: false,
            visibility: AutoHide::new(Instant::now()),
            bar,
            shell,
            tray,
            launcher: None,
            quick: None,
            prepared: Some(prepare_launcher(&pinned)),
            tooltip: Tooltip::new(hwnd).ok(),
            animating: false,
            last_tick: Instant::now(),
            mouse_tracked: false,
        })
    }

    /// Abre o cierra el panel rápido sobre los iconos de red, volumen y batería.
    fn toggle_quick(&mut self) {
        if let Some(panel) = self.quick.as_mut().filter(|p| p.visible) {
            panel.hide();
            return;
        }
        if let Some(menu) = &mut self.launcher {
            menu.hide();
        }
        if self.quick.is_none() {
            match QuickWindow::new(self.hwnd) {
                Ok(panel) => self.quick = Some(panel),
                Err(_) => return,
            }
        }
        let anchor = self.pos.x + self.bar.status_group_center(self.scale).round() as i32;
        if let Some(panel) = &mut self.quick {
            panel.model.volume = self.bar.status.volume;
            panel.model.battery = self.bar.status.battery;
            panel.place(anchor, self.y_shown);
            panel.show();
        }
        self.expand();
    }

    fn toggle_launcher(&mut self) {
        if self.launcher.as_ref().is_some_and(|menu| menu.visible) {
            if let Some(menu) = &mut self.launcher {
                menu.hide();
            }
            return;
        }
        if let Some(panel) = &mut self.quick {
            panel.hide();
        }
        let pinned: Vec<_> = self
            .bar
            .primary_apps
            .iter()
            .map(|a| a.entry.clone())
            .collect();
        if self.launcher.is_none() {
            // Normalmente ya está listo; si no, se espera a ese mismo trabajo.
            let model = match self.prepared.take().and_then(|ready| ready.recv().ok()) {
                Some(mut model) => {
                    model.refresh(&pinned);
                    model
                }
                None => launcher::LauncherState::new(&pinned),
            };
            match LauncherWindow::new(self.hwnd, model, self.pos.x, self.y_shown) {
                Ok(menu) => self.launcher = Some(menu),
                Err(_) => return,
            }
        } else if let Some(menu) = &mut self.launcher {
            // Conserva el catálogo y los iconos ya cargados; solo pone al día el uso.
            menu.model.refresh(&pinned);
        }
        if let Some(menu) = &mut self.launcher {
            menu.show();
        }
        self.expand();
    }

    /// Con una app a pantalla completa el dock se aparta; si no, al cambiar de ventana
    /// (`raise`) vuelve a ponerse por encima de las demás, incluida la barra nativa.
    fn check_priority(&mut self, raise: bool) {
        let monitor = windows::Win32::Foundation::RECT {
            left: self.screen_left,
            top: self.screen_top,
            right: self.screen_right,
            bottom: self.screen_bottom,
        };
        self.fullscreen = priority::fullscreen_app(monitor);
        if raise && !self.fullscreen {
            priority::raise(self.hwnd);
        }
    }

    fn pins_changed(&mut self) {
        self.save_pins();
        self.relayout();
        self.visibility.reveal();
        self.visibility
            .defer_hide(Instant::now(), Duration::from_millis(1200));
        self.start_animating();
    }

    fn save_pins(&self) {
        if pins::save(&self.bar.primary_apps).is_err() {
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::MessageBoxW(Some(self.hwnd),
                        w!("No se pudo guardar la lista de programas. El cambio solo durará esta sesión."),
                        w!("GlassBar"), windows::Win32::UI::WindowsAndMessaging::MB_ICONWARNING);
            }
        }
    }

    /// Ajusta tamaño y posición del dock al número de iconos.
    fn relayout(&mut self) {
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
            self.bar.drag = None;
            self.bar.update_hover(None);
            self.restart_tooltip();
            self.start_animating();
            self.render();
        }
    }

    /// Oculta el nombre y, si el cursor quedó sobre un programa, lo vuelve a programar.
    fn restart_tooltip(&mut self) {
        if let Some(tip) = &mut self.tooltip {
            tip.hide();
        }
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_TIP);
            if matches!(
                self.bar.hovered,
                Some(
                    HoverTarget::PrimaryApp(_)
                        | HoverTarget::OpenApp(_)
                        | HoverTarget::Overflow
                        | HoverTarget::Status(_)
                )
            ) && self.bar.pressed.is_none()
            {
                let _ = SetTimer(Some(self.hwnd), TIMER_TIP, TIP_MS, None);
            }
        }
    }

    fn show_tooltip(&mut self) {
        let Some(target) = self.bar.hovered else {
            return;
        };
        // El nombre de la flecha o del estado taparía el panel que ellos mismos abren.
        if target == HoverTarget::Overflow && shell::overflow_window().is_some() {
            return;
        }
        if matches!(target, HoverTarget::Status(_))
            && self.quick.as_ref().is_some_and(|q| q.visible)
        {
            return;
        }
        let Some((name, cx)) = self.bar.item_label(target, self.scale) else {
            return;
        };
        if let Some(tip) = &mut self.tooltip {
            tip.show(
                &name,
                self.pos.x + cx.round() as i32,
                self.pos.y - (6.0 * self.scale).round() as i32,
                self.screen_left,
                self.screen_right,
                self.scale,
            );
        }
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
        WM_FOREGROUND => {
            with_app(|app| app.check_priority(true));
            LRESULT(0)
        }
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

                if app.bar.drag_to(x, app.scale) {
                    app.restart_tooltip();
                    app.start_animating();
                    app.render();
                    return;
                }

                let target = app.bar.hit_test(x, y, app.scale);
                if target != app.bar.hovered {
                    app.bar.update_hover(target);
                    app.restart_tooltip();
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
                app.restart_tooltip();

                app.start_animating();
            });
            LRESULT(0)
        }

        WM_LBUTTONDOWN => {
            with_app(|app| {
                if let Some(target) = app.bar.hovered {
                    app.bar.pressed = Some(target);
                    app.restart_tooltip();
                    let x = (lparam.0 & 0xffff) as i16 as f32;
                    app.bar.begin_drag(target, x, app.scale);
                    if app.bar.drag.is_some() {
                        let _ = SetCapture(hwnd);
                    }
                    match target {
                        HoverTarget::Launcher => app.bar.launcher_press.set_target(0.90),
                        HoverTarget::PrimaryApp(_) | HoverTarget::OpenApp(_) => {
                            if let Some(a) = app.bar.item_mut(target) {
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
            let _ = ReleaseCapture();
            with_app(|app| {
                if let Some(pinned_moved) = app.bar.end_drag() {
                    // Soltar tras arrastrar solo coloca el icono; no abre el programa.
                    app.bar.pressed = None;
                    for a in app
                        .bar
                        .primary_apps
                        .iter_mut()
                        .chain(&mut app.bar.open_apps)
                    {
                        a.press_spring.set_target(1.0);
                    }
                    if pinned_moved {
                        app.save_pins();
                    }
                    app.bar.update_hover(None);
                    app.start_animating();
                    app.render();
                    return;
                }
                if let Some(target) = app.bar.pressed.take() {
                    match target {
                        HoverTarget::Launcher => {
                            app.bar.launcher_press.set_target(1.0);
                            app.toggle_launcher();
                        }
                        HoverTarget::PrimaryApp(_) => {
                            if let Some(menu) = &mut app.launcher {
                                menu.hide();
                            }
                            if let Some(a) = app.bar.item_mut(target) {
                                a.press_spring.set_target(1.0);
                                if a.activate() {
                                    let _ = catalog::record(&a.entry);
                                }
                            }
                        }
                        HoverTarget::OpenApp(_) => {
                            if let Some(menu) = &mut app.launcher {
                                menu.hide();
                            }
                            if let Some(a) = app.bar.item_mut(target) {
                                a.press_spring.set_target(1.0);
                                if let Some(hwnd) = a.window {
                                    apps::focus_window(hwnd);
                                }
                            }
                        }
                        HoverTarget::Overflow => {
                            if let Some(menu) = &mut app.launcher {
                                menu.hide();
                            }
                            if let Some(tip) = &mut app.tooltip {
                                tip.hide();
                            }
                            shell::toggle_overflow(
                                app.pos.x + app.bar.overflow_center(app.scale).round() as i32,
                                app.y_shown,
                                app.screen_left,
                                app.screen_right,
                            );
                        }
                        HoverTarget::Status(_) => {
                            if let Some(tip) = &mut app.tooltip {
                                tip.hide();
                            }
                            app.toggle_quick();
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

        // La rueda sobre el volumen lo sube o lo baja, como en la barra de Windows.
        windows::Win32::UI::WindowsAndMessaging::WM_MOUSEWHEEL => {
            with_app(|app| {
                let volume = matches!(
                    app.bar.hovered,
                    Some(HoverTarget::Status(i)) if app.bar.status_item(i) == Some(status::Item::Volume)
                );
                if volume {
                    let delta = ((wparam.0 >> 16) & 0xffff) as i16 as f32 / 120.0;
                    status::change_volume(delta);
                    app.bar.status = status::read();
                    app.render();
                    app.restart_tooltip();
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
                    // Pasar a pantalla completa (F11) no cambia la ventana activa.
                    app.check_priority(false);
                    // Red, volumen y batería; si aparece o desaparece la batería cambia el ancho.
                    let status = status::read();
                    let resized = status.items().len() != app.bar.status.items().len();
                    app.bar.status = status;
                    // El volumen puede cambiar con las teclas o desde otra app.
                    if let Some(panel) = app.quick.as_mut().filter(|p| p.visible) {
                        let changed = panel.model.volume != status.volume
                            || panel.model.battery != status.battery;
                        if changed && panel.model.dragging.is_none() {
                            panel.model.volume = status.volume;
                            panel.model.battery = status.battery;
                            panel.render();
                        }
                    }
                    if apps::update_running_apps(&mut app.bar.primary_apps, &mut app.bar.open_apps)
                        || resized
                    {
                        app.relayout();
                    } else {
                        app.render();
                    }
                });
            } else if id == TIMER_TIP {
                with_app(|app| {
                    let _ = KillTimer(Some(app.hwnd), TIMER_TIP);
                    if app.bar.pressed.is_none() && app.visibility.expanded {
                        app.show_tooltip();
                    }
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
                        (at_edge && !app.fullscreen)
                            || inside
                            || app.bar.drag.is_some()
                            || app.launcher.as_ref().is_some_and(|l| l.visible)
                            || app.quick.as_ref().is_some_and(|q| q.visible)
                            // El panel de iconos ocultos abierto sobre la flecha; se mira
                            // al final porque recorre las ventanas, y solo con el dock fuera.
                            || (app.visibility.expanded && shell::overflow_window().is_some()),
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

unsafe extern "system" fn tooltip_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
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
    if guard::run(&std::env::args().collect::<Vec<_>>()) {
        return Ok(());
    }
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

        // Antes de ocultar la barra nativa: si este proceso muere, el vigilante la devuelve.
        guard::spawn();

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

        let quick_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(quick_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("GlassBarQuick"),
            ..Default::default()
        };
        if RegisterClassExW(&quick_class) == 0 {
            return Err(windows::core::Error::from_thread());
        }

        let tooltip_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(tooltip_wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("GlassBarTooltip"),
            ..Default::default()
        };
        if RegisterClassExW(&tooltip_class) == 0 {
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

        // La tecla Windows abre el lanzador propio; se quita al salir del bucle.
        let _winkey = winkey::WinKey::install(hwnd, WM_SHOW_LAUNCHER);
        let _foreground = priority::ForegroundWatch::install(hwnd, WM_FOREGROUND);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        APP.with(|cell| cell.borrow_mut().take());
    }
    Ok(())
}
