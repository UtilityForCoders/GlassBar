#![windows_subsystem = "windows"]

//! # GlassBar
//! Barra de tareas moderna ultra-transparente estilo Windows 11 Glass para Windows, ultraligera en Rust.
//! Comportamiento auto-hide nativo (sale suavemente desde abajo al acercar el mouse) y estética translúcida.

mod accent;
mod appbar;
mod appmenu;
mod apps;
mod attention;
mod audio;
mod autohide;
mod autostart;
mod backup;
mod bar;
mod bluetooth;
mod calendar;
mod catalog;
mod categories;
mod clock;
mod diag;
mod extras;
mod font;
mod frames;
mod games;
mod glass;
mod guard;
mod icons;
mod jumplist;
mod keyboard;
mod language;
mod launcher;
mod monitors;
mod notifcenter;
mod notifications;
mod pins;
mod preview;
mod priority;
mod quick;
mod senders;
mod settings;
mod shell;
mod spotlight;
mod spring;
mod status;
mod style;
mod system;
mod systray;
mod textfield;
mod toggles;
mod topbar;
mod tray;
mod traypanel;
mod update;
mod usage;
mod wifi;
mod winkey;
mod winx;

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

use crate::appmenu::{
    CMD_APP_CLOSE, CMD_APP_END_TASK, CMD_APP_OPEN, CMD_APP_PIN, CMD_APP_UNPIN, CMD_AUTOHIDE,
    CMD_AUTOSTART, CMD_CLOCK_SECONDS, CMD_EXIT, CMD_JUMP, CMD_NOTIFICATIONS, CMD_SETTINGS,
    CMD_TOP_BAR, CMD_UPDATE, CMD_UPDATE_CHECK,
};
use crate::autohide::AutoHide;
use crate::bar::{BarState, HoverTarget, BOTTOM_MARGIN};
use crate::shell::TaskbarReplacer;
use crate::tray::{Tray, WM_TRAY};

const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_SHOW_LAUNCHER: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 2;
/// Cambió la ventana activa: el dock comprueba si debe ponerse encima o apartarse.
const WM_FOREGROUND: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 3;
/// DWM avisa de que cambió el color de énfasis o de las ventanas.
const WM_DWMCOLORIZATIONCOLORCHANGED: u32 = 0x0320;
/// El hilo de sistema terminó de leer Wi-Fi, Bluetooth, brillo y red.
const WM_QUICK_UPDATE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 4;
/// Cambiaron los iconos de la bandeja de otros programas (ver `systray`).
const WM_TRAY_ICONS: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 6;
/// Un hilo de las actualizaciones terminó (ver `App::handle_update_events`).
const WM_UPDATE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 7;
/// Cambió algún ajuste del sistema: se mira si la barra nativa sigue con el ocultar
/// automático (fuera del aviso, que Explorer puede estar esperando).
const WM_WORK_AREA: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 8;
/// Atajo de la barra (Win+1…9, Win+T…), codificado en `wparam` (ver `winkey`).
const WM_SHORTCUT: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 9;
/// Aviso de Windows a la barra de aplicación del modo siempre visible.
const WM_APPBAR: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 10;
/// Abrir el menú del icono de GlassBar (ver `WM_TRAY`).
const WM_TRAY_MENU: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 12;
/// Aviso de Windows a la barra de aplicación de la barra superior.
const WM_TOP_APPBAR: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 11;
/// El hilo de iconos del lanzador cargó alguno nuevo.
const WM_LAUNCHER_ICONS: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 5;
/// El hilo de los avisos de Windows tiene algo nuevo (ver `notifications`).
/// Llegaron iconos al buscador rápido.
const WM_SPOTLIGHT_ICONS: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 14;
/// Identificador del atajo global Alt+Espacio (`RegisterHotKey`).
const HOTKEY_SPOTLIGHT: i32 = 1;
const WM_NOTIFICATIONS: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 13;

const TIMER_ANIM: usize = 1;
const TIMER_TICK: usize = 2;
const TIMER_EDGE: usize = 3;
const TIMER_TIP: usize = 4;
/// Una vez, poco después de arrancar: vuelve a pedir los iconos de la bandeja.
const TIMER_ANNOUNCE: usize = 5;
/// Buscar actualizaciones: una vez poco después de arrancar y luego cada 6 horas.
const TIMER_UPDATE_FIRST: usize = 6;
const TIMER_UPDATE: usize = 7;
/// Parpadeo del cursor del texto en el buscador del lanzador.
const TIMER_CARET: usize = 8;

/// Refresco del reloj y verificación de apps activas.
const TICK_MS: u32 = 1000;
/// Verificación de proximidad del cursor al borde inferior para auto-hide inmediato (60 ms).
const EDGE_MS: u32 = 40;
/// Sondeo del borde con el puntero quieto y nada abierto ni en movimiento: sin nadie moviendo
/// el ratón no hay borde que detectar, y 25 despertares por segundo impiden que el procesador
/// descanse (se nota en la batería). Se hace todo lo mismo, cinco veces por segundo, y al
/// primer movimiento vuelve el ritmo normal.
const EDGE_IDLE_MS: u32 = 200;
/// Cuánto debe llevar quieto el puntero para pasar al sondeo lento.
const EDGE_IDLE_AFTER: Duration = Duration::from_secs(2);
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

/// Imagen de una ventana por capas, de `width` × `height` píxeles. Sus búferes se
/// pueden soltar (`release`) mientras la ventana está oculta: se vuelven a crear
/// al dibujarla otra vez.
struct Surface {
    width: u32,
    height: u32,
    buffers: Option<Buffers>,
}

/// Lo que ocupa memoria de una superficie: el DIB que se envía a Windows y la
/// imagen de tiny-skia donde se dibuja.
struct Buffers {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    pixmap: Pixmap,
}

impl Surface {
    fn new(width: u32, height: u32) -> Result<Self> {
        Ok(Self {
            width,
            height,
            buffers: Some(Buffers::new(width, height)?),
        })
    }

    /// La imagen donde se dibuja; la crea si se había soltado. `None` solo si
    /// Windows no da memoria para el DIB.
    fn pixmap(&mut self) -> Option<&mut Pixmap> {
        if self.buffers.is_none() {
            self.buffers = Buffers::new(self.width, self.height).ok();
        }
        self.buffers.as_mut().map(|b| &mut b.pixmap)
    }

    /// El DC con el DIB para `UpdateLayeredWindow` (nulo si está soltado).
    fn dc(&self) -> HDC {
        self.buffers.as_ref().map(|b| b.dc).unwrap_or_default()
    }

    /// Suelta los búferes; la ventana tiene que estar oculta.
    fn release(&mut self) {
        self.buffers = None;
    }

    /// Convierte RGBA premultiplicado de tiny-skia a BGRA premultiplicado de Windows DIB.
    fn copy_to_dib(&mut self) {
        let len = (self.width * self.height * 4) as usize;
        let Some(buffers) = &mut self.buffers else {
            return;
        };
        let dst = unsafe { std::slice::from_raw_parts_mut(buffers.bits, len) };
        for (d, s) in dst
            .chunks_exact_mut(4)
            .zip(buffers.pixmap.data().chunks_exact(4))
        {
            d[0] = s[2]; // B
            d[1] = s[1]; // G
            d[2] = s[0]; // R
            d[3] = s[3]; // A
        }
    }
}

impl Buffers {
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
                pixmap,
            })
        }
    }
}

impl Drop for Buffers {
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
    /// Desenfoque de debajo, como el del dock.
    backdrop: Option<style::Backdrop>,
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
            backdrop: style::Backdrop::new(),
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
        let Some(pixmap) = surface.pixmap() else {
            return;
        };
        pixmap.fill(tiny_skia::Color::TRANSPARENT);
        apps::fill_rounded(
            pixmap,
            glass::panel(),
            0.5,
            0.5,
            w - 1.0,
            h - 1.0,
            7.0 * scale,
        );
        apps::stroke_rounded(
            pixmap,
            [255, 255, 255, 40],
            0.5,
            0.5,
            w - 1.0,
            h - 1.0,
            7.0 * scale,
            1.0,
        );
        clock::draw_text(
            pixmap,
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
                Some(surface.dc()),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            // Encima de todo, también de los paneles abiertos después (bandeja, menús).
            use windows::Win32::UI::WindowsAndMessaging::{
                SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            };
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        if let Some(backdrop) = &self.backdrop {
            let shape = style::Shape::inset(width, height, 0.5, 7.0 * scale);
            backdrop.under(self.hwnd, surface_rect(pos, &surface), &[shape], 1.0);
        }
        self.surface = Some(surface);
        self.visible = true;
    }
    fn hide(&mut self) {
        if self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            if let Some(backdrop) = &self.backdrop {
                backdrop.hide();
            }
            self.visible = false;
            // Windows guarda su propia copia de lo enviado: oculta no hace falta.
            self.surface = None;
        }
    }
}

struct LauncherWindow {
    hwnd: HWND,
    /// Desenfoque de debajo, como el del dock.
    backdrop: Option<style::Backdrop>,
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
        mut model: launcher::LauncherState,
        dock_left: i32,
        dock_top: i32,
    ) -> Result<Self> {
        let _ = dock_left;
        let (scale, width, height, pos) = Self::geometry(dock_top);
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
        model.load_icons_with(launcher::IconLoader::start(
            scale,
            hwnd.0 as isize,
            WM_LAUNCHER_ICONS,
        ));
        Ok(Self {
            hwnd,
            backdrop: style::Backdrop::new(),
            surface,
            pos,
            scale,
            model,
            visible: false,
            transition: launcher::Transition::hidden(),
        })
    }
    /// Escala, tamaño y posición: centrado en el monitor del dock y encima de él,
    /// como el Inicio de Windows 11.
    fn geometry(dock_top: i32) -> (f32, u32, u32, POINT) {
        let (screen, dpi) = monitors::get_info(monitors::dock());
        let scale = dpi
            .min((screen.right - screen.left - 24) as f32 / launcher::WIDTH)
            .min((dock_top - screen.top - 24) as f32 / launcher::HEIGHT)
            .max(0.3);
        let width = (launcher::WIDTH * scale).ceil() as u32;
        let height = (launcher::HEIGHT * scale).ceil() as u32;
        let pos = POINT {
            x: screen.left + (screen.right - screen.left - width as i32) / 2,
            y: dock_top - height as i32 - 12,
        };
        (scale, width, height, pos)
    }
    /// Lo lleva al monitor del dock, que pudo cambiar desde la última apertura.
    fn relocate(&mut self, dock_top: i32) {
        let (scale, width, height, pos) = Self::geometry(dock_top);
        if (width, height) != (self.surface.width, self.surface.height) {
            let Ok(surface) = Surface::new(width, height) else {
                return;
            };
            self.surface = surface;
        }
        if scale != self.scale {
            self.scale = scale;
            self.model.reload_icons_with(launcher::IconLoader::start(
                scale,
                self.hwnd.0 as isize,
                WM_LAUNCHER_ICONS,
            ));
        }
        self.pos = pos;
    }
    fn render(&mut self) {
        if let Some(pixmap) = self.surface.pixmap() {
            self.model.draw(pixmap, self.scale);
        }
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
        let shapes = [panel_shape(&self.surface, self.scale, 12.0)];
        let hidden = present_popup(
            self.hwnd,
            &mut self.surface,
            self.pos,
            self.scale,
            &self.transition,
            self.visible,
            animating,
            self.backdrop.as_ref(),
            &shapes,
        );
        if hidden {
            self.model.release_layers();
        }
    }
    fn show(&mut self) {
        self.visible = true;
        self.transition.open(Instant::now());
        self.render();
        take_focus(self.hwnd);
        restart_caret(self.hwnd);
    }
    /// Empieza el cierre; la ventana se oculta cuando termina de desvanecerse.
    fn hide(&mut self) {
        if !self.visible {
            return;
        }
        unsafe {
            let _ =
                windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(self.hwnd), TIMER_CARET);
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
/// pide el siguiente fotograma; al terminar de cerrarse, oculta la ventana y suelta
/// los búferes de `surface` (devuelve `true`).
#[allow(clippy::too_many_arguments)]
fn present_popup(
    hwnd: HWND,
    surface: &mut Surface,
    pos: POINT,
    scale: f32,
    transition: &launcher::Transition,
    visible: bool,
    animating: bool,
    backdrop: Option<&style::Backdrop>,
    shapes: &[style::Shape],
) -> bool {
    let now = Instant::now();
    let shown = transition.shown(now);
    let running = transition.running(now);
    unsafe {
        if running || animating {
            frames::start(hwnd);
        } else {
            frames::stop(hwnd);
        }
        if !visible && !running {
            let _ = ShowWindow(hwnd, SW_HIDE);
            if let Some(backdrop) = backdrop {
                backdrop.hide();
            }
            surface.release();
            return true;
        }
    }
    let pos = POINT {
        x: pos.x,
        y: pos.y + (transition.offset(now) * scale).round() as i32,
    };
    if let Some(backdrop) = backdrop {
        backdrop.under(hwnd, surface_rect(pos, surface), shapes, shown);
    }
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
            Some(surface.dc()),
            Some(&POINT { x: 0, y: 0 }),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
    }
    false
}

/// Rectángulo en pantalla de una superficie con su esquina en `pos`.
fn surface_rect(pos: POINT, surface: &Surface) -> windows::Win32::Foundation::RECT {
    windows::Win32::Foundation::RECT {
        left: pos.x,
        top: pos.y,
        right: pos.x + surface.width as i32,
        bottom: pos.y + surface.height as i32,
    }
}

/// El panel de una superficie, dibujado a `scale` de cada borde y con esquinas de
/// `radius` (las dos en unidades lógicas), para el desenfoque de debajo.
fn panel_shape(surface: &Surface, scale: f32, radius: f32) -> style::Shape {
    style::Shape::inset(surface.width, surface.height, scale, radius * scale)
}

/// Muestra un panel emergente y le da el foco, para que se cierre al hacer clic fuera.
fn take_focus(hwnd: HWND) {
    unsafe {
        use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId, IsHungAppWindow, SetForegroundWindow,
            SW_SHOW,
        };
        let _ = ShowWindow(hwnd, SW_SHOW);
        // Normalmente basta con pedirlo: tras un clic en GlassBar, Windows lo da.
        if SetForegroundWindow(hwnd).as_bool() && GetForegroundWindow() == hwnd {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(hwnd));
            return;
        }
        // Abierto con la tecla Windows no hubo clic en GlassBar y Windows puede
        // negar el foco; compartir la entrada con la ventana activa lo permite. Eso
        // hace esperar a su hilo: con una ventana colgada, GlassBar dejaría de
        // responder, así que entonces no se intenta.
        let active = GetForegroundWindow();
        if IsHungAppWindow(active).as_bool() {
            return;
        }
        let current = GetCurrentThreadId();
        let foreground = GetWindowThreadProcessId(active, None);
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

/// Borde desde el que se abre un panel: encima del dock o, desde la barra
/// superior, debajo de ella. `monitor` da la escala y los límites.
#[derive(Clone, Copy, Default)]
struct Edge {
    y: i32,
    below: bool,
    monitor: windows::Win32::Graphics::Gdi::HMONITOR,
}
impl Edge {
    /// Borde superior de un panel de `height` píxeles a `margin` del borde.
    fn panel_top(self, height: i32, margin: i32) -> i32 {
        if self.below {
            self.y + margin
        } else {
            self.y - height - margin
        }
    }
}

/// Panel rápido (Wi-Fi, Bluetooth, brillo y volumen) sobre los iconos de estado.
struct QuickWindow {
    hwnd: HWND,
    /// Desenfoque de debajo, como el del dock.
    backdrop: Option<style::Backdrop>,
    surface: Surface,
    pos: POINT,
    scale: f32,
    model: quick::QuickState,
    visible: bool,
    transition: launcher::Transition,
    backend: system::Backend,
    /// Abierto bajo la barra superior: al cambiar de vista crece hacia abajo.
    below: bool,
}
impl QuickWindow {
    fn new(owner: HWND) -> Result<Self> {
        let (_, dpi) = monitors::get_info(monitors::dock());
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
            backdrop: style::Backdrop::new(),
            surface,
            pos: POINT::default(),
            scale,
            model: quick::QuickState::default(),
            visible: false,
            transition: launcher::Transition::hidden(),
            below: false,
            backend: system::Backend::start(hwnd, WM_QUICK_UPDATE),
        })
    }
    /// Encima de los iconos de estado (`anchor_x`), sin salirse de la pantalla.
    fn place(&mut self, anchor_x: i32, edge: Edge) {
        self.below = edge.below;
        let (screen, dpi) = monitors::get_info(edge.monitor);
        // El dock pudo pasar a un monitor con otra escala desde la última apertura.
        let scale = dpi.max(0.3);
        if scale != self.scale {
            let width = (quick::WIDTH * scale).ceil() as u32;
            let height = (self.model.height() * scale).ceil() as u32;
            if let Ok(surface) = Surface::new(width, height) {
                self.surface = surface;
                self.scale = scale;
            }
        }
        let width = self.surface.width as i32;
        let margin = (12.0 * self.scale).round() as i32;
        self.pos = POINT {
            x: (anchor_x - width / 2).clamp(
                screen.left + margin,
                (screen.right - width - margin).max(screen.left + margin),
            ),
            y: edge.panel_top(self.surface.height as i32, margin),
        };
    }
    fn render(&mut self) {
        if let Some(pixmap) = self.surface.pixmap() {
            self.model.draw(pixmap, self.scale);
        }
        self.surface.copy_to_dib();
        self.present();
    }
    fn present(&mut self) {
        let shapes = [panel_shape(&self.surface, self.scale, 12.0)];
        present_popup(
            self.hwnd,
            &mut self.surface,
            self.pos,
            self.scale,
            &self.transition,
            self.visible,
            false,
            self.backdrop.as_ref(),
            &shapes,
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
            if !self.below {
                self.pos.y = bottom - height as i32;
            }
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
    /// Lleva el volumen de una aplicación del mezclador a `value` (de 0 a 1).
    fn set_app_volume(&mut self, app: usize, value: f32) {
        if let Some(app) = self.model.system.audio.apps.get_mut(app) {
            // Se ve al instante; el hilo de sistema lo aplica.
            app.volume = value;
            self.backend.set_app_volume(app.pids.clone(), value);
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

/// Panel vecino de `open` entre los de la bandeja, el idioma (si se muestra), el
/// estado y la hora, en el orden de la barra y dando la vuelta.
fn neighbor_menu(open: HoverTarget, step: isize, language: bool) -> HoverTarget {
    let order: Vec<_> = [
        HoverTarget::Overflow,
        HoverTarget::Language,
        HoverTarget::Status(0),
        HoverTarget::Clock,
    ]
    .into_iter()
    .filter(|t| language || *t != HoverTarget::Language)
    .collect();
    let index = order.iter().position(|t| *t == open).unwrap_or(0) as isize;
    order[(index + step).rem_euclid(order.len() as isize) as usize]
}

/// Un clic (o Enter) sobre `hit` en el panel rápido.
fn press_quick(app: &mut App, hit: Option<quick::Hit>) {
    let Some(panel) = app.quick.as_mut().filter(|p| p.visible) else {
        return;
    };
    match hit {
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
        Some(quick::Hit::Toggle(toggle)) => {
            // Como los de Wi-Fi y Bluetooth: se ve al instante y el hilo
            // de sistema lo confirma al releerlo.
            if let Some(on) = panel.model.system.toggle_mut(toggle) {
                *on = !*on;
                let on = *on;
                panel.backend.set_toggle(toggle, on);
                // El modo avión apaga el Wi-Fi y el Bluetooth: se ve ya, sin
                // esperar a que el hilo de sistema relea las radios. Al
                // quitarlo, Windows decide cuáles vuelven.
                if toggle == toggles::Toggle::Airplane && on {
                    for radio in [
                        &mut panel.model.system.wifi,
                        &mut panel.model.system.bluetooth,
                    ] {
                        if radio.is_some() {
                            *radio = Some(false);
                        }
                    }
                }
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
        Some(quick::Hit::Language) => {
            // Desde el mismo sitio: el panel de idioma sustituye a este.
            panel.hide();
            app.toggle_language();
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
                    panel.model.pending = Some(pending(device.address.to_string(), connect));
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
                if let Some((ssid, security, password)) = panel.model.take_join(Instant::now()) {
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
            if let Some((ssid, security, password)) = panel.model.take_join(Instant::now()) {
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
                    panel.model.app_drag = None;
                    panel.model.scroll_drag = None;
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
                if let Some(app) = panel.model.app_drag {
                    panel.set_app_volume(app, quick::QuickState::app_slider_value(x));
                    return;
                }
                if panel.model.scroll_drag.is_some() {
                    panel.model.drag_scrollbar(y);
                    panel.render();
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
                match panel.model.hit_test(x, y) {
                    Some(quick::Hit::Slider(slider)) => {
                        SetCapture(hwnd);
                        panel.model.dragging = Some(slider);
                        panel.set_slider(slider, quick::QuickState::slider_value(x));
                    }
                    Some(quick::Hit::AppVolume(app)) => {
                        SetCapture(hwnd);
                        panel.model.app_drag = Some(app);
                        panel.set_app_volume(app, quick::QuickState::app_slider_value(x));
                    }
                    Some(quick::Hit::Scrollbar) => {
                        SetCapture(hwnd);
                        panel.model.grab_scrollbar(y);
                        panel.render();
                    }
                    _ => {}
                }
            });
        }
        WM_LBUTTONUP => {
            with_app(|app| {
                let Some(panel) = app.quick.as_mut().filter(|p| p.visible) else {
                    return;
                };
                if panel.model.dragging.take().is_some()
                    || panel.model.app_drag.take().is_some()
                    || panel.model.scroll_drag.take().is_some()
                {
                    let _ = ReleaseCapture();
                    panel.render();
                    return;
                }
                let (x, y) = point(panel.scale);
                let hit = panel.model.hit_test(x, y);
                press_quick(app, hit);
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
                // Sobre la barra del volumen de una aplicación, la mueve.
                if let Some(quick::Hit::AppVolume(app)) = panel.model.hovered {
                    if let Some(value) = panel.model.app_volume(app) {
                        let value = ((value * 50.0).round() + steps).clamp(0.0, 50.0) / 50.0;
                        panel.set_app_volume(app, value);
                    }
                    return;
                }
                if panel.model.view != quick::View::Main {
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
                    // Sin formulario: Escape cierra el panel, arriba y abajo recorren lo
                    // que se puede elegir, Enter o espacio lo pulsan, e izquierda y
                    // derecha pasan al panel vecino (o mueven la barra elegida).
                    match wparam.0 {
                        0x1B => panel.hide(),
                        0x26 | 0x28 => {
                            panel
                                .model
                                .move_focus(if wparam.0 == 0x26 { -1 } else { 1 });
                            panel.render();
                        }
                        0x25 | 0x27 => {
                            let step = if wparam.0 == 0x25 { -1 } else { 1 };
                            if let Some(quick::Hit::AppVolume(app)) = panel.model.hovered {
                                if let Some(value) = panel.model.app_volume(app) {
                                    let value = ((value * 50.0).round() + step as f32)
                                        .clamp(0.0, 50.0)
                                        / 50.0;
                                    panel.set_app_volume(app, value);
                                }
                                return;
                            }
                            let slider = match panel.model.hovered {
                                Some(quick::Hit::Slider(slider)) => Some(slider),
                                _ => None,
                            };
                            match slider.and_then(|s| Some((s, panel.model.value(s)?))) {
                                Some((slider, value)) => {
                                    let value = ((value * 50.0).round() + step as f32)
                                        .clamp(0.0, 50.0)
                                        / 50.0;
                                    panel.set_slider(slider, value);
                                }
                                None => app.step_menu(step),
                            }
                        }
                        0x0D | 0x20 => {
                            let hit = panel.model.hovered;
                            press_quick(app, hit);
                        }
                        _ => {}
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

/// (Re)empieza el parpadeo del cursor del buscador con el ritmo que tenga Windows,
/// para que no se apague justo al escribir. Sin parpadeo en Windows, queda fijo.
fn restart_caret(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{GetCaretBlinkTime, KillTimer, SetTimer};
    unsafe {
        let period = GetCaretBlinkTime();
        if period == 0 || period == u32::MAX {
            let _ = KillTimer(Some(hwnd), TIMER_CARET);
        } else {
            SetTimer(Some(hwnd), TIMER_CARET, period, None);
        }
    }
}

/// Lee el catálogo y los iconos de la vista inicial en otro hilo.
fn prepare_launcher(
    pinned: &[apps::AppEntry],
) -> std::sync::mpsc::Receiver<launcher::LauncherState> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let pinned = pinned.to_vec();
    // La escala del monitor del dock: hasta abrir el lanzador por primera vez, los
    // iconos esperan ya reducidos (si al final se dibujan más pequeños, se vuelven a
    // reducir al dibujarlos).
    let (_, dpi) = monitors::get_info(monitors::dock());
    let scale = dpi.max(0.3);
    std::thread::spawn(move || {
        let mut model = launcher::LauncherState::new(&pinned);
        model.preload_icons(scale);
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
        WA_INACTIVE, WM_ACTIVATE, WM_CHAR, WM_CLOSE, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEWHEEL,
        WM_RBUTTONUP, WM_SETCURSOR, WM_SYSKEYDOWN,
    };
    let mut action = None;
    // Al escribir o pulsar, el cursor del texto se ve fijo y el parpadeo empieza de nuevo.
    if matches!(msg, WM_CHAR | WM_KEYDOWN | WM_LBUTTONDOWN) {
        restart_caret(hwnd);
    }
    match msg {
        WM_TIMER if wparam.0 == TIMER_CARET => {
            with_app(|app| {
                if let Some(menu) = app.launcher.as_mut().filter(|m| m.visible) {
                    if menu.model.blink() {
                        menu.render();
                    }
                }
            });
        }
        // Sobre el buscador, el cursor de texto (I), como en cualquier cuadro de texto.
        WM_SETCURSOR if (lparam.0 & 0xffff) as u32 == 1 => {
            use windows::Win32::Graphics::Gdi::ScreenToClient;
            use windows::Win32::UI::WindowsAndMessaging::{SetCursor, IDC_IBEAM};
            let over = with_app(|app| {
                let menu = app.launcher.as_ref().filter(|m| m.visible)?;
                let mut point = POINT::default();
                let _ = GetCursorPos(&mut point);
                let _ = ScreenToClient(hwnd, &mut point);
                Some(
                    menu.model
                        .over_text(point.x as f32 / menu.scale, point.y as f32 / menu.scale),
                )
            })
            .flatten()
            .unwrap_or(false);
            let cursor = if over { IDC_IBEAM } else { IDC_ARROW };
            if let Ok(cursor) = LoadCursorW(None, cursor) {
                SetCursor(Some(cursor));
            }
            return LRESULT(1);
        }
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
        WM_LAUNCHER_ICONS => {
            with_app(|app| {
                if let Some(menu) = app.launcher.as_mut().filter(|m| m.visible) {
                    menu.render();
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
                            let double = Duration::from_millis(
                                windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime()
                                    as u64,
                            );
                            menu.model.press_search(x, shift, Instant::now(), double);
                            menu.model.selection = None;
                        }
                        if let Some(hit) = hit {
                            action = menu.model.activate(hit);
                        }
                    }
                    menu.model.set_hovered(hit);
                    menu.render();
                }
            });
        }
        // Clic derecho en una app: su menú contextual, como en el Inicio de Windows.
        // Con un menú ya abierto, lo cierra.
        WM_RBUTTONUP => {
            with_app(|app| {
                if let Some(menu) = app.launcher.as_mut().filter(|menu| menu.visible) {
                    let x = (lparam.0 & 0xffff) as i16 as f32 / menu.scale;
                    let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32 / menu.scale;
                    if !menu.model.close_overlay() {
                        if let Some(hit) = menu.model.hit_test(x, y) {
                            menu.model.open_context(hit, x, y);
                        }
                    }
                    menu.render();
                }
            });
        }
        // Mayús+F10 abre el menú contextual de la app elegida, como en Windows.
        WM_SYSKEYDOWN if wparam.0 == 0x79 => {
            with_app(|app| {
                if let Some(menu) = app.launcher.as_mut().filter(|menu| menu.visible) {
                    use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
                    if GetKeyState(0x10) < 0 && menu.model.open_context_from_keyboard() {
                        menu.render();
                    }
                }
            });
        }
        WM_MOUSEWHEEL => {
            with_app(|app| {
                if let Some(menu) = app.launcher.as_mut().filter(|menu| menu.visible) {
                    if !menu.model.overlay_open() {
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
                    let menu_open = menu.model.overlay_open();
                    match wparam.0 {
                        0x1B => {
                            if menu.model.back() {
                                menu.render();
                            } else {
                                menu.hide();
                            }
                        }
                        // Enter abre lo elegido; con un menú abierto, su opción resaltada.
                        0x0D => {
                            if let Some(hit) = menu.model.first_match() {
                                action = menu.model.activate(hit);
                                menu.render();
                            }
                        }
                        // Tecla de menú: el menú contextual de la app elegida.
                        0x5D => {
                            if menu.model.open_context_from_keyboard() {
                                menu.render();
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
                        0x28 => {
                            menu.model.move_selection(0, 1);
                            menu.render();
                        }
                        0x26 => {
                            menu.model.move_selection(0, -1);
                            menu.render();
                        }
                        0x22 if !menu_open => {
                            menu.model.scroll_page(true);
                            menu.render();
                        }
                        0x21 if !menu_open => {
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
        if action.closes_launcher() {
            with_app(|app| {
                if let Some(menu) = &mut app.launcher {
                    menu.hide();
                }
            });
        }
        match action {
            launcher::Action::Launch(entry) => {
                if apps::launch_entry(&entry) {
                    let _ = catalog::record(&entry);
                }
            }
            launcher::Action::RunAsAdmin(entry) => {
                if apps::launch_entry_as_admin(&entry) {
                    let _ = catalog::record(&entry);
                }
            }
            launcher::Action::OpenFile(entry) => {
                apps::launch_entry(&entry);
            }
            launcher::Action::Copy(text) => set_clipboard_text(hwnd, &text),
            launcher::Action::OpenLocation(entry) => {
                apps::open_entry_location(&entry);
            }
            launcher::Action::Uninstall => {
                apps::open_uninstall();
            }
            launcher::Action::Jump(entry, item) => {
                jumplist::open(&item, &entry.command);
            }
            launcher::Action::Pin(entry) => {
                with_app(|app| app.pin_from_launcher(entry, true));
            }
            launcher::Action::Unpin(entry) => {
                with_app(|app| app.pin_from_launcher(entry, false));
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

/// Buscador rápido (Alt+Espacio), centrado en la parte alta del monitor del ratón.
struct SpotlightWindow {
    hwnd: HWND,
    backdrop: Option<style::Backdrop>,
    surface: Surface,
    /// Esquina del panel y su escala en el monitor donde se abrió.
    pos: POINT,
    scale: f32,
    model: spotlight::SpotlightState,
    visible: bool,
    transition: launcher::Transition,
    /// Última posición del ratón vista: al crecer la lista bajo un ratón quieto,
    /// Windows avisa de un movimiento que no ha habido y no debe cambiar lo elegido.
    cursor: POINT,
}
impl SpotlightWindow {
    fn new(owner: HWND, model: spotlight::SpotlightState) -> Result<Self> {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                w!("GlassBarSpotlight"),
                w!("GlassBar — Buscar"),
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
            backdrop: style::Backdrop::new(),
            surface: Surface::new(1, 1)?,
            pos: POINT::default(),
            scale: 0.0,
            model,
            visible: false,
            transition: launcher::Transition::hidden(),
            cursor: POINT::default(),
        })
    }
    /// Lo coloca en el monitor del ratón, a un quinto de su alto, como Spotlight.
    fn place(&mut self) {
        let mut cursor = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut cursor);
        }
        self.cursor = cursor;
        let (screen, dpi) = monitors::get_info(monitors::at(cursor));
        let scale = dpi
            .min((screen.right - screen.left - 24) as f32 / spotlight::WIDTH)
            .max(0.3);
        if scale != self.scale {
            self.scale = scale;
            self.model.load_icons_with(launcher::IconLoader::start(
                scale,
                self.hwnd.0 as isize,
                WM_SPOTLIGHT_ICONS,
            ));
        }
        let width = (spotlight::WIDTH * scale).ceil() as i32;
        self.pos = POINT {
            x: screen.left + (screen.right - screen.left - width) / 2,
            y: screen.top + (screen.bottom - screen.top) / 5,
        };
    }
    /// Dibuja el panel; la ventana crece o encoge con la lista de resultados.
    fn render(&mut self) {
        let width = (spotlight::WIDTH * self.scale).ceil() as u32;
        let height = (self.model.height() * self.scale).ceil() as u32;
        if (width, height) != (self.surface.width, self.surface.height) {
            self.surface.release();
            self.surface.width = width;
            self.surface.height = height;
        }
        if let Some(pixmap) = self.surface.pixmap() {
            self.model.draw(pixmap, self.scale);
        }
        self.surface.copy_to_dib();
        self.present();
    }
    fn present(&mut self) {
        let shapes = [panel_shape(&self.surface, self.scale, 14.0)];
        present_popup(
            self.hwnd,
            &mut self.surface,
            self.pos,
            self.scale,
            &self.transition,
            self.visible,
            false,
            self.backdrop.as_ref(),
            &shapes,
        );
    }
    fn show(&mut self) {
        self.place();
        self.visible = true;
        self.transition.open(Instant::now());
        self.render();
        take_focus(self.hwnd);
        restart_caret(self.hwnd);
    }
    fn hide(&mut self) {
        if !self.visible {
            return;
        }
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_CARET);
        }
        self.visible = false;
        self.transition.close(Instant::now());
        self.present();
    }
}
impl Drop for SpotlightWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn spotlight_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
    use windows::Win32::UI::WindowsAndMessaging::{
        WA_INACTIVE, WM_ACTIVATE, WM_CHAR, WM_CLOSE, WM_KEYDOWN, WM_SETCURSOR,
    };
    let mut action = None;
    if matches!(msg, WM_CHAR | WM_KEYDOWN) {
        restart_caret(hwnd);
    }
    match msg {
        WM_TIMER if wparam.0 == TIMER_CARET => {
            with_app(|app| {
                if let Some(panel) = app.spotlight.as_mut().filter(|p| p.visible) {
                    if panel.model.blink() {
                        panel.render();
                    }
                }
            });
        }
        WM_TIMER if wparam.0 == TIMER_ANIM => {
            with_app(|app| {
                if let Some(panel) = &mut app.spotlight {
                    panel.present();
                }
            });
        }
        WM_SPOTLIGHT_ICONS => {
            with_app(|app| {
                if let Some(panel) = app.spotlight.as_mut().filter(|p| p.visible) {
                    panel.render();
                }
            });
        }
        WM_SETCURSOR if (lparam.0 & 0xffff) as u32 == 1 => {
            use windows::Win32::Graphics::Gdi::ScreenToClient;
            use windows::Win32::UI::WindowsAndMessaging::{SetCursor, IDC_IBEAM};
            let over = with_app(|app| {
                let panel = app.spotlight.as_ref().filter(|p| p.visible)?;
                let mut point = POINT::default();
                let _ = GetCursorPos(&mut point);
                let _ = ScreenToClient(hwnd, &mut point);
                Some(
                    panel
                        .model
                        .over_text(point.x as f32 / panel.scale, point.y as f32 / panel.scale),
                )
            })
            .flatten()
            .unwrap_or(false);
            if let Ok(cursor) = LoadCursorW(None, if over { IDC_IBEAM } else { IDC_ARROW }) {
                SetCursor(Some(cursor));
            }
            return LRESULT(1);
        }
        // Se cierra al pasar a otra ventana, como Spotlight.
        WM_ACTIVATE if (wparam.0 & 0xffff) as u32 == WA_INACTIVE => {
            with_app(|app| {
                if let Some(panel) = &mut app.spotlight {
                    panel.hide();
                }
            });
        }
        WM_CLOSE => {
            with_app(|app| {
                if let Some(panel) = &mut app.spotlight {
                    panel.hide();
                }
            });
        }
        // Pasar el ratón elige el resultado; el clic lo abre (con Ctrl, su carpeta).
        WM_MOUSEMOVE | WM_LBUTTONUP => {
            with_app(|app| {
                if let Some(panel) = app.spotlight.as_mut().filter(|p| p.visible) {
                    let x = (lparam.0 & 0xffff) as i16 as f32 / panel.scale;
                    let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32 / panel.scale;
                    let Some(index) = panel.model.hit_test(x, y) else {
                        return;
                    };
                    if msg == WM_LBUTTONUP {
                        action = panel.model.activate(index, GetKeyState(0x11) < 0);
                        return;
                    }
                    let mut cursor = POINT::default();
                    let _ = GetCursorPos(&mut cursor);
                    if (cursor.x, cursor.y) == (panel.cursor.x, panel.cursor.y) {
                        return;
                    }
                    panel.cursor = cursor;
                    if panel.model.select(index) {
                        panel.render();
                    }
                }
            });
        }
        WM_CHAR => {
            with_app(|app| {
                if let Some(panel) = app.spotlight.as_mut().filter(|p| p.visible) {
                    // Retroceso (y Ctrl+Retroceso, que llega como 0x7F) se atiende en WM_KEYDOWN.
                    if let Some(ch) = char::from_u32(wparam.0 as u32).filter(|c| !c.is_control()) {
                        panel.model.insert_text(&ch.to_string());
                        panel.render();
                    }
                }
            });
        }
        WM_KEYDOWN => {
            with_app(|app| {
                let Some(panel) = app.spotlight.as_mut().filter(|p| p.visible) else {
                    return;
                };
                let ctrl = GetKeyState(0x11) < 0;
                let shift = GetKeyState(0x10) < 0;
                match wparam.0 {
                    // Esc borra lo escrito y, con el campo vacío, cierra.
                    0x1B => {
                        if panel.model.query.is_empty() {
                            panel.hide();
                            return;
                        }
                        panel.model.edit(|field| field.clear());
                    }
                    // Enter abre el resultado elegido; Ctrl+Enter lo muestra en su carpeta.
                    0x0D => {
                        action = panel.model.activate(panel.model.selected(), ctrl);
                        return;
                    }
                    0x28 => panel.model.move_selection(1),
                    0x26 => panel.model.move_selection(-1),
                    0x41 if ctrl => panel.model.edit(|field| field.select_all()),
                    0x43 if ctrl => {
                        if let Some(text) = panel.model.query.selected_text() {
                            set_clipboard_text(hwnd, &text);
                        }
                        return;
                    }
                    0x58 if ctrl => {
                        if let Some(text) = panel.model.edit(|field| field.cut()) {
                            set_clipboard_text(hwnd, &text);
                        }
                    }
                    0x56 if ctrl => {
                        if let Some(text) = clipboard_text(hwnd) {
                            panel
                                .model
                                .insert_text(text.lines().next().unwrap_or_default());
                        }
                    }
                    0x08 => panel.model.edit(|field| field.backspace(ctrl)),
                    0x2E => panel.model.edit(|field| field.delete(ctrl)),
                    0x25 => panel.model.edit(|field| field.left(ctrl, shift)),
                    0x27 => panel.model.edit(|field| field.right(ctrl, shift)),
                    0x24 => panel.model.edit(|field| field.home(shift)),
                    0x23 => panel.model.edit(|field| field.end(shift)),
                    _ => return,
                }
                panel.render();
            });
        }
        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
    }
    // Fuera del préstamo de APP: abrir algo puede reenviar mensajes a GlassBar.
    if let Some(action) = action {
        with_app(|app| {
            if let Some(panel) = &mut app.spotlight {
                panel.hide();
            }
        });
        match action {
            spotlight::Action::Launch(entry) => {
                if apps::launch_entry(&entry) {
                    let _ = catalog::record(&entry);
                }
            }
            spotlight::Action::Open(entry) => {
                apps::launch_entry(&entry);
            }
            spotlight::Action::Copy(text) => set_clipboard_text(hwnd, &text),
            spotlight::Action::Reveal(entry) => {
                apps::open_entry_location(&entry);
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
    /// Bandeja propia que recibe los iconos de los demás programas; `None` si no se
    /// pudo crear (entonces la flecha abre el panel nativo).
    systray: Option<systray::Host>,
    /// Panel de las apps en segundo plano; se crea la primera vez que se abre.
    tray_panel: Option<TrayWindow>,
    /// Menú de una app del dock (clic derecho); se crea la primera vez que se abre.
    app_menu: Option<AppMenuWindow>,
    /// Cuándo se pulsó un icono de ese panel: el dock no se oculta mientras su
    /// programa abre la ventana o el menú.
    tray_clicked: Option<Instant>,
    launcher: Option<LauncherWindow>,
    /// Buscador rápido (Alt+Espacio); se crea la primera vez que se abre.
    spotlight: Option<SpotlightWindow>,
    /// Panel rápido; se crea la primera vez que se abre.
    quick: Option<QuickWindow>,
    /// Panel de idioma del teclado; también se crea al abrirlo.
    language: Option<LanguageWindow>,
    /// Calendario que abre la hora; también se crea al abrirlo.
    calendar: Option<CalendarWindow>,
    /// Lanzador preparado en segundo plano al iniciar (catálogo e iconos), para que
    /// la primera apertura no tenga que esperar a leerlos.
    prepared: Option<std::sync::mpsc::Receiver<launcher::LauncherState>>,
    tooltip: Option<Tooltip>,
    /// Vista previa de ventanas; `preview_away` es desde cuándo el cursor no está ni
    /// en ella ni en su programa (se cierra tras un momento, para poder llegar a ella).
    preview: Option<PreviewWindow>,
    preview_away: Option<Instant>,
    animating: bool,
    last_tick: Instant,
    mouse_tracked: bool,
    /// Algún botón del ratón estaba pulsado en el último sondeo (ver
    /// `close_on_outside_click`).
    mouse_down: bool,
    /// Lee red, volumen y batería cada segundo.
    status: status::Reader,
    /// Apariencia de cada situación (escritorio, ventana maximizada…).
    styles: style::Styles,
    /// Cuándo se miró la pantalla por última vez para elegir la apariencia.
    scene_read: Instant,
    /// Última vez que se miró si la bandeja propia sigue delante de la de Explorer.
    tray_checked: Instant,
    /// Última posición del puntero vista por `TIMER_EDGE` y cuándo cambió por última vez.
    edge_cursor: POINT,
    edge_moved: Instant,
    /// `TIMER_EDGE` va al ritmo lento (`EDGE_IDLE_MS`).
    edge_slow: bool,
    /// Ventana con el desenfoque de Windows que va debajo del dock con las
    /// apariencias Desenfoque y Acrílico.
    backdrop: Option<style::Backdrop>,
    /// Dónde y con qué apariencia se puso el desenfoque la última vez.
    backdrop_at: std::cell::Cell<Option<(windows::Win32::Foundation::RECT, style::Appearance)>>,
    /// Actualizaciones (ver `update`): versión nueva encontrada y los hilos que
    /// buscan o descargan, que responden por `update_events`.
    new_release: Option<update::Release>,
    /// Versión de la que ya se avisó, para no repetirlo cada 6 horas.
    notified: Option<update::Version>,
    checking: bool,
    /// La búsqueda en curso la pidió el usuario desde el menú: se le dice qué salió.
    manual_check: bool,
    updating: bool,
    update_sender: std::sync::mpsc::Sender<UpdateEvent>,
    update_events: std::sync::mpsc::Receiver<UpdateEvent>,
    /// Ventanas que piden atención (ver `attention`).
    attention: attention::Attention,
    /// El dock se recorre con el teclado (Win+T, Win+B): la ventana que estaba
    /// activa antes, para devolverle el foco con Escape.
    keyboard_nav: Option<isize>,
    /// Sitio reservado en el modo siempre visible (ver `appbar`).
    appbar: Option<appbar::AppBar>,
    /// Windows avisó a esa barra de una app a pantalla completa en su monitor.
    appbar_fullscreen: bool,
    /// Barra superior con los controles, si está activada (ver `topbar`).
    top: Option<TopBarWindow>,
    /// El panel abierto salió de la barra superior: el dock no tiene que asomarse.
    panels_from_top: bool,
    /// Ventana de configuración (ver `settings`).
    settings: Option<SettingsWindow>,
    /// Avisos de Windows como tarjetas arriba a la derecha, si está activado.
    notifications: Option<Notifications>,
}

/// Cómo abrir una app del dock.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Launch {
    /// La trae al frente, o la abre si no está abierta.
    Switch,
    /// Otra instancia (Mayús+clic, clic central).
    NewInstance,
    /// Como administrador (Ctrl+Mayús+clic).
    Admin,
}

/// Respuesta de los hilos de las actualizaciones.
enum UpdateEvent {
    Checked(std::result::Result<Option<update::Release>, String>),
    Downloaded(std::result::Result<std::path::PathBuf, String>),
}

/// Avisa a la ventana del dock (`hwnd`) de que hay una respuesta de actualizaciones.
fn notify_update(hwnd: isize) {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
            Some(HWND(hwnd as *mut _)),
            WM_UPDATE,
            WPARAM(0),
            LPARAM(0),
        );
    }
}

impl App {
    fn new(hwnd: HWND) -> Result<Self> {
        let monitor = monitors::primary();
        monitors::set_dock(monitor);
        let (rc, scale) = monitors::get_info(monitor);

        let mut bar = BarState::new();
        apps::update_running_apps(&mut bar.primary_apps, &mut bar.open_apps);
        let mut status_reader = status::Reader::new();
        bar.status = status_reader.read();
        bar.keyboard = keyboard::current();
        let styles = style::Styles::load();
        bar.appearance = styles.look(style::State::Desktop).appearance;
        glass::set_menu_opacity(styles.menu_opacity());
        clock::set_seconds(styles.clock_seconds());
        let (update_sender, update_events) = std::sync::mpsc::channel();
        let scale = (scale * styles.dock_factor())
            .min((rc.right - rc.left) as f32 / (bar.dock_width() + 40.0));
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

        // Antes que el icono propio, para que también llegue a la bandeja propia.
        let systray = systray::Host::install(hwnd, WM_TRAY_ICONS);
        // Lo que tarda en leerse para las jump lists, listo antes del primer clic derecho.
        jumplist::warm_up();
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
            systray,
            tray_panel: None,
            app_menu: None,
            tray_clicked: None,
            launcher: None,
            spotlight: None,
            quick: None,
            language: None,
            calendar: None,
            prepared: Some(prepare_launcher(&pinned)),
            tooltip: Tooltip::new(hwnd).ok(),
            preview: PreviewWindow::new(hwnd).ok(),
            preview_away: None,
            animating: false,
            last_tick: Instant::now(),
            mouse_tracked: false,
            mouse_down: false,
            status: status_reader,
            styles,
            scene_read: Instant::now(),
            tray_checked: Instant::now(),
            edge_cursor: POINT::default(),
            edge_moved: Instant::now(),
            edge_slow: false,
            backdrop: style::Backdrop::new(),
            backdrop_at: std::cell::Cell::new(None),
            new_release: None,
            notified: None,
            checking: false,
            manual_check: false,
            updating: false,
            update_sender,
            update_events,
            attention: attention::Attention::install(hwnd),
            keyboard_nav: None,
            appbar: None,
            appbar_fullscreen: false,
            top: None,
            panels_from_top: false,
            settings: None,
            notifications: None,
        })
    }

    /// Cierra todos los paneles y menús: solo puede haber uno abierto, que si no se
    /// tapan. El dock nunca se lleva el foco, así que un clic en él no cierra el que
    /// esté abierto: lo cierra quien abre otro.
    fn close_popups(&mut self) {
        if let Some(menu) = &mut self.launcher {
            menu.hide();
        }
        if let Some(panel) = &mut self.spotlight {
            panel.hide();
        }
        if let Some(panel) = &mut self.quick {
            panel.hide();
        }
        if let Some(panel) = &mut self.language {
            panel.hide();
        }
        if let Some(panel) = &mut self.calendar {
            panel.hide();
        }
        if let Some(panel) = &mut self.tray_panel {
            panel.hide();
        }
        if let Some(menu) = &mut self.app_menu {
            menu.hide();
        }
    }

    /// Cierra lo abierto si se acaba de pulsar fuera del dock y de los paneles.
    /// Normalmente se cierran al perder el foco, pero si Windows no se lo dio al
    /// abrirse nunca lo pierden; esto se mira cada 40 ms y no depende de él.
    fn close_on_outside_click(&mut self, cursor: POINT) {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
        };
        let down = [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
            .iter()
            .any(|&key| unsafe { GetAsyncKeyState(key.0 as i32) } < 0);
        // Solo al pulsar: arrastrar un deslizador hasta fuera del panel no lo cierra.
        let pressed = down && !self.mouse_down;
        self.mouse_down = down;
        if !pressed {
            return;
        }
        let popups = [
            self.launcher.as_ref().filter(|p| p.visible).map(|p| p.hwnd),
            self.spotlight
                .as_ref()
                .filter(|p| p.visible)
                .map(|p| p.hwnd),
            self.quick.as_ref().filter(|p| p.visible).map(|p| p.hwnd),
            self.language.as_ref().filter(|p| p.visible).map(|p| p.hwnd),
            self.calendar.as_ref().filter(|p| p.visible).map(|p| p.hwnd),
            self.tray_panel
                .as_ref()
                .filter(|p| p.visible)
                .map(|p| p.hwnd),
            self.app_menu.as_ref().filter(|p| p.visible).map(|p| p.hwnd),
        ];
        if popups.iter().all(Option::is_none) {
            return;
        }
        // El menú de un icono de la bandeja es de su programa: pulsar en él no es fuera.
        if self.tray_panel.as_ref().is_some_and(|p| p.menu.is_some()) || menu_open() {
            return;
        }
        let inside = |hwnd: HWND| unsafe {
            let mut rect = windows::Win32::Foundation::RECT::default();
            windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect).is_ok()
                && cursor.x >= rect.left
                && cursor.x < rect.right
                && cursor.y >= rect.top
                && cursor.y < rect.bottom
        };
        let top_bar = self.top.as_ref().is_some_and(|top| inside(top.hwnd));
        if inside(self.hwnd) || top_bar || popups.into_iter().flatten().any(inside) {
            return;
        }
        self.close_popups();
    }

    /// Abre en el cursor el menú del icono de GlassBar (configuración y salir).
    fn open_tray_menu(&mut self) {
        if self.app_menu.is_none() {
            match AppMenuWindow::new(self.hwnd) {
                Ok(menu) => self.app_menu = Some(menu),
                Err(_) => return,
            }
        }
        let mut cursor = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut cursor);
        }
        let (screen, _) = monitors::get_info(monitors::at(cursor));
        if let Some(menu) = &mut self.app_menu {
            menu.open_at_cursor(
                appmenu::tray(),
                MenuPurpose::GlassBar,
                cursor,
                (screen.left, screen.right),
            );
        }
    }

    /// Abre en el cursor el menú del botón de inicio, cerrando los paneles abiertos.
    fn open_windows_menu(&mut self) {
        self.close_popups();
        if self.app_menu.is_none() {
            match AppMenuWindow::new(self.hwnd) {
                Ok(menu) => self.app_menu = Some(menu),
                Err(_) => return,
            }
        }
        let mut cursor = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut cursor);
        }
        let bounds = (self.screen_left, self.screen_right);
        if let Some(menu) = &mut self.app_menu {
            menu.open_windows(cursor, bounds);
        }
    }

    /// Busca en un hilo aparte si hay una versión nueva; responde con `WM_UPDATE`.
    /// `manual` si lo pidió el usuario: entonces se avisa también si no hay nada.
    fn check_updates(&mut self, manual: bool) {
        if self.updating {
            return;
        }
        self.manual_check |= manual;
        if self.checking {
            return;
        }
        self.checking = true;
        let (sender, hwnd) = (self.update_sender.clone(), self.hwnd.0 as isize);
        std::thread::spawn(move || {
            let _ = sender.send(UpdateEvent::Checked(update::newer()));
            notify_update(hwnd);
        });
    }

    /// Descarga la versión nueva en un hilo aparte; al terminar se instala.
    fn start_update(&mut self) {
        let Some(release) = self.new_release.clone().filter(|_| !self.updating) else {
            return;
        };
        self.updating = true;
        self.tray.notify(
            "Actualizando GlassBar",
            &format!("Descargando la v{}…", release.version),
        );
        let (sender, hwnd) = (self.update_sender.clone(), self.hwnd.0 as isize);
        std::thread::spawn(move || {
            let _ = sender.send(UpdateEvent::Downloaded(update::download(&release)));
            notify_update(hwnd);
        });
    }

    /// Atiende lo que respondieron los hilos de las actualizaciones.
    fn handle_update_events(&mut self) {
        while let Ok(event) = self.update_events.try_recv() {
            match event {
                UpdateEvent::Checked(result) => {
                    self.checking = false;
                    let manual = std::mem::take(&mut self.manual_check);
                    // Sin conexión o sin respuesta, se vuelve a mirar en 6 horas.
                    let release = match result {
                        Ok(release) => release,
                        Err(error) => {
                            if manual {
                                self.tray.notify(
                                    "No se pudo buscar actualizaciones",
                                    &format!("Motivo: {error}."),
                                );
                            }
                            continue;
                        }
                    };
                    if release.is_none() && manual {
                        self.tray.notify(
                            "GlassBar está al día",
                            &format!("La v{} es la última versión.", update::Version::current()),
                        );
                    }
                    // Sola, se avisa una vez por versión, no cada 6 horas.
                    if let Some(release) = &release {
                        if manual || self.notified != Some(release.version) {
                            self.notified = Some(release.version);
                            self.tray.notify(
                                "Hay una versión nueva de GlassBar",
                                &format!(
                                    "La v{} está lista. Haz clic aquí para actualizar.",
                                    release.version
                                ),
                            );
                        }
                    }
                    self.new_release = release;
                }
                UpdateEvent::Downloaded(Ok(installer)) => match update::install(&installer) {
                    // El instalador cierra GlassBar si sigue abierto y lo vuelve a abrir.
                    Ok(()) => {
                        self.release_shell();
                        unsafe {
                            PostQuitMessage(0);
                        }
                    }
                    Err(error) => self.update_failed(&error),
                },
                UpdateEvent::Downloaded(Err(error)) => self.update_failed(&error),
            }
        }
    }

    fn update_failed(&mut self, error: &str) {
        self.refresh_settings();
        self.updating = false;
        self.tray.notify(
            "No se pudo actualizar GlassBar",
            &format!("Motivo: {error}."),
        );
    }

    /// Mira lo que pasa en pantalla y, si cambia la apariencia del dock, lo redibuja.
    fn update_style(&mut self) {
        self.scene_read = Instant::now();
        let appearance = if self.styles.watches_screen() {
            let launcher = self.launcher.as_ref().filter(|menu| menu.visible);
            let monitor = windows::Win32::Foundation::RECT {
                left: self.screen_left,
                top: self.screen_top,
                right: self.screen_right,
                bottom: self.screen_bottom,
            };
            let scene = style::Scene::read(
                monitor,
                launcher.is_some(),
                launcher.is_some_and(|menu| !menu.model.query.is_empty()),
            );
            self.styles.resolve(&scene)
        } else {
            self.styles.look(style::State::Desktop).appearance
        };
        if appearance != self.bar.appearance {
            self.bar.appearance = appearance;
            self.render();
        }
    }

    /// Pone el desenfoque de Windows detrás del dock si su apariencia lo pide, con
    /// la forma del dock; solo cuando cambia la apariencia o el tamaño.
    fn sync_backdrop(&self) {
        let Some(backdrop) = &self.backdrop else {
            return;
        };
        let rect = self.backdrop_rect();
        backdrop.update(
            self.hwnd,
            self.bar.appearance,
            rect,
            (glass::DOCK_RADIUS * self.scale).round() as i32,
        );
        self.backdrop_at.set(Some((rect, self.bar.appearance)));
    }

    /// Como `sync_backdrop`, pero solo si el dock se movió, cambió de tamaño o de
    /// apariencia: al redibujarlo en su sitio (pasar el cursor por los iconos), mover
    /// el desenfoque en cada fotograma costaba 0,06-0,3 ms.
    fn sync_backdrop_if_changed(&self) {
        if self.backdrop_at.get() != Some((self.backdrop_rect(), self.bar.appearance)) {
            self.sync_backdrop();
        }
    }

    /// Rectángulo del dock en pantalla, sin el margen de la sombra.
    fn backdrop_rect(&self) -> windows::Win32::Foundation::RECT {
        let px = |v: f32| (v * self.scale).round() as i32;
        windows::Win32::Foundation::RECT {
            left: self.pos.x + px(bar::SHADOW_PAD_X),
            top: self.pos.y + px(bar::SHADOW_PAD_Y),
            right: self.pos.x + self.surface.width as i32 - px(bar::SHADOW_PAD_X),
            bottom: self.pos.y + self.surface.height as i32 - px(bar::SHADOW_PAD_Y),
        }
    }

    /// Abre o cierra el panel rápido sobre los iconos de red, volumen y batería.
    fn toggle_quick(&mut self) {
        if let Some(panel) = self.quick.as_mut().filter(|p| p.visible) {
            panel.hide();
            return;
        }
        self.close_popups();
        if self.quick.is_none() {
            match QuickWindow::new(self.hwnd) {
                Ok(panel) => self.quick = Some(panel),
                Err(_) => return,
            }
        }
        let (anchor, edge, _) = self.panel_edge(HoverTarget::Status(0));
        if let Some(panel) = &mut self.quick {
            panel.model.volume = self.bar.status.volume;
            panel.model.battery = self.bar.status.battery;
            panel.model.language = self.bar.keyboard.as_ref().map(|k| k.language.clone());
            panel.place(anchor, edge);
            panel.show();
        }
        self.opened_from(edge);
    }

    /// Alt+Espacio: abre el buscador rápido o, si ya está abierto, lo cierra.
    fn toggle_spotlight(&mut self) {
        if self.spotlight.as_ref().is_some_and(|p| p.visible) {
            if let Some(panel) = &mut self.spotlight {
                panel.hide();
            }
            return;
        }
        self.close_popups();
        let pinned: Vec<_> = self
            .bar
            .primary_apps
            .iter()
            .map(|a| a.entry.clone())
            .collect();
        if self.spotlight.is_none() {
            let model = spotlight::SpotlightState::new(&pinned);
            match SpotlightWindow::new(self.hwnd, model) {
                Ok(panel) => self.spotlight = Some(panel),
                Err(_) => return,
            }
        }
        if let Some(panel) = &mut self.spotlight {
            panel.model.refresh(&pinned);
            panel.show();
        }
    }

    fn toggle_launcher(&mut self) {
        if self.launcher.as_ref().is_some_and(|menu| menu.visible) {
            if let Some(menu) = &mut self.launcher {
                menu.hide();
            }
            return;
        }
        self.close_popups();
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
            menu.relocate(self.y_shown);
        }
        if let Some(menu) = &mut self.launcher {
            menu.show();
        }
        self.expand();
    }

    /// Con una app a pantalla completa el dock se aparta; si no, al cambiar de ventana
    /// (`raise`) vuelve a ponerse por encima de las demás, incluida la barra nativa.
    /// Pone al día el indicador de idioma con el teclado de la ventana activa; si
    /// aparece o desaparece (al instalar o quitar teclados), el dock se reajusta.
    fn update_keyboard(&mut self) {
        let keyboard = keyboard::current();
        if keyboard == self.bar.keyboard {
            return;
        }
        let resized = keyboard.is_some() != self.bar.keyboard.is_some();
        self.bar.keyboard = keyboard;
        if resized {
            self.relayout();
        } else {
            self.render();
        }
    }

    /// Abre o cierra el panel de las apps en segundo plano sobre la flecha.
    fn toggle_tray_panel(&mut self) {
        if let Some(panel) = self.tray_panel.as_mut().filter(|p| p.visible) {
            panel.hide();
            return;
        }
        self.close_popups();
        if self.tray_panel.is_none() {
            match TrayWindow::new(self.hwnd) {
                Ok(panel) => self.tray_panel = Some(panel),
                Err(_) => return,
            }
        }
        let (anchor, edge, bounds) = self.panel_edge(HoverTarget::Overflow);
        if let Some(panel) = &mut self.tray_panel {
            panel.show(anchor, edge, bounds);
        }
        self.opened_from(edge);
    }

    /// Izquierda o derecha con un panel de la barra abierto: pasa al panel del botón
    /// vecino, como en una barra de menús.
    fn step_menu(&mut self, step: isize) {
        let open = if self.tray_panel.as_ref().is_some_and(|p| p.visible) {
            HoverTarget::Overflow
        } else if self.language.as_ref().is_some_and(|p| p.visible) {
            HoverTarget::Language
        } else if self.quick.as_ref().is_some_and(|p| p.visible) {
            HoverTarget::Status(0)
        } else if self.calendar.as_ref().is_some_and(|p| p.visible) {
            HoverTarget::Clock
        } else {
            return;
        };
        let language = self.bar.keyboard.is_some() && self.top.is_none();
        match neighbor_menu(open, step, language) {
            HoverTarget::Overflow => self.toggle_tray_panel(),
            HoverTarget::Language => self.toggle_language(),
            HoverTarget::Status(_) => self.toggle_quick(),
            _ => self.toggle_calendar(),
        }
    }

    /// Abre o cierra el panel de idioma sobre el indicador. El teclado elegido se
    /// aplica a la ventana que estaba activa al abrirlo.
    fn toggle_language(&mut self) {
        if let Some(panel) = self.language.as_mut().filter(|p| p.visible) {
            panel.hide();
            return;
        }
        self.close_popups();
        if self.language.is_none() {
            match LanguageWindow::new(self.hwnd) {
                Ok(panel) => self.language = Some(panel),
                Err(_) => return,
            }
        }
        let target = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        let (anchor, edge, bounds) = self.panel_edge(HoverTarget::Language);
        let current = self.bar.keyboard.as_ref().map(|k| k.id);
        if let Some(panel) = &mut self.language {
            panel.show(target, current, anchor, edge, bounds);
        }
        self.opened_from(edge);
    }

    /// Hay algo abierto desde el dock (un panel, un menú, un arrastre) que se
    /// quedaría en el otro monitor si el dock cambiara de monitor.
    fn busy(&self) -> bool {
        self.bar.drag.is_some()
            || self.launcher.as_ref().is_some_and(|p| p.visible)
            || self.quick.as_ref().is_some_and(|p| p.visible)
            || self.language.as_ref().is_some_and(|p| p.visible)
            || self.calendar.as_ref().is_some_and(|p| p.visible)
            || self
                .tray_panel
                .as_ref()
                .is_some_and(|p| p.visible || p.menu.is_some())
            || self.app_menu.as_ref().is_some_and(|p| p.visible)
            || self
                .tray_clicked
                .is_some_and(|at| at.elapsed() < Duration::from_millis(1500))
            || menu_open()
    }

    /// Lleva el dock a `monitor`: sube desde su borde inferior, con su escala.
    fn move_to_monitor(&mut self, monitor: windows::Win32::Graphics::Gdi::HMONITOR) {
        monitors::set_dock(monitor);
        self.close_popups();
        if let Some(preview) = &mut self.preview {
            preview.hide();
        }
        if let Some(tip) = &mut self.tooltip {
            tip.hide();
        }
        // Sale del borde del monitor nuevo en vez de cruzar la pantalla.
        self.visibility.motion.value = 0.0;
        self.visibility.motion.velocity = 0.0;
        self.fit_monitor();
    }

    /// Vuelve a leer el monitor del dock (tamaño, posición y escala) y lo recoloca;
    /// si se desconectó, pasa al principal.
    fn fit_monitor(&mut self) {
        let monitor = monitors::dock();
        monitors::set_dock(monitor);
        let (rc, _) = monitors::get_info(monitor);
        self.screen_top = rc.top;
        self.screen_bottom = rc.bottom;
        self.screen_left = rc.left;
        self.screen_right = rc.right;
        self.y_hidden = rc.bottom;
        self.relayout();
        self.update_style();
        self.sync_appbar();
        if let Some(top) = &mut self.top {
            top.place();
        }
        self.check_priority(false);
    }

    /// Marca las apps con alguna ventana que pide atención; `true` si cambió alguna.
    fn mark_attention(&mut self) -> bool {
        let mut changed = false;
        for app in self
            .bar
            .primary_apps
            .iter_mut()
            .chain(&mut self.bar.open_apps)
        {
            let attention = self.attention.first_of(&app.windows).is_some();
            changed |= attention != app.attention;
            app.attention = attention;
        }
        changed
    }

    /// Atiende un aviso del Shell sobre una ventana (ver `attention`).
    fn shell_event(&mut self, code: usize, window: isize) {
        let change = self.attention.handle(code, window);
        if change == attention::Change::None {
            return;
        }
        // La ventana puede ser tan nueva que aún no esté en el dock.
        if change == attention::Change::Started
            && apps::update_running_apps(&mut self.bar.primary_apps, &mut self.bar.open_apps)
        {
            self.mark_attention();
            self.relayout();
        } else if self.mark_attention() {
            self.render();
        }
        let shown = self
            .bar
            .primary_apps
            .iter()
            .chain(&self.bar.open_apps)
            .any(|app| app.windows.contains(&window));
        // Como la barra de Windows escondida, el dock se asoma un momento para avisar.
        if change == attention::Change::Started && shown && !self.fullscreen {
            self.reveal();
            self.visibility
                .defer_hide(Instant::now(), Duration::from_millis(3000));
            self.start_animating();
        }
    }

    /// Arranca o para los avisos propios según el ajuste. Al pararlos, los de
    /// Windows vuelven a verse.
    fn sync_notifications(&mut self) {
        if !self.styles.notifications() {
            if self.notifications.take().is_some() {
                self.sync_center();
            }
            return;
        }
        if self.notifications.is_none() {
            self.notifications = Notifications::start(self.hwnd);
            if let Some(n) = &mut self.notifications {
                n.dnd = self.styles.do_not_disturb();
            }
        }
    }

    fn handle_notifications(&mut self) {
        // Bajo la barra superior, si está en el monitor de los avisos.
        let below = self
            .top
            .as_ref()
            .filter(|top| top.monitor == monitors::primary())
            .map(|top| top.rect.top + top.height());
        let mut changed = false;
        if let Some(notifications) = &mut self.notifications {
            notifications.window.below = below;
            changed = notifications.handle_events(&self.styles);
        }
        if changed {
            self.sync_center();
        }
    }

    /// Algo cambió en el centro de notificaciones: se ponen al día el panel de la
    /// hora, si está abierto (y entonces no queda nada sin leer), y el número de
    /// avisos sin leer sobre la hora.
    fn sync_center(&mut self) {
        let open = self.calendar.as_ref().is_some_and(|p| p.visible);
        if let Some(n) = &mut self.notifications {
            if open {
                n.center.unread = 0;
            }
        }
        if let Some(panel) = &mut self.calendar {
            panel.refresh(self.notifications.as_mut());
        }
        let unread = self.notifications.as_ref().map_or(0, |n| n.center.unread);
        if self.bar.unread != unread {
            self.bar.unread = unread;
            self.render();
        }
        if let Some(top) = &mut self.top {
            if top.model.unread != unread {
                top.model.unread = unread;
                top.render();
            }
        }
    }

    /// Clic en el centro de notificaciones.
    fn center_click(&mut self, hit: notifcenter::Hit) {
        use notifcenter::{Hit, Part};
        let Some(n) = &mut self.notifications else {
            return;
        };
        match hit {
            Hit::ClearAll => {
                let live = n.center.clear();
                n.dismiss(live);
            }
            Hit::Group(sender, true) => {
                let live = n.center.remove_sender(&sender);
                n.dismiss(live);
            }
            Hit::Entry(id, Part::Close) => {
                let live = n.center.remove(id);
                n.dismiss(live);
            }
            Hit::Entry(id, Part::Body) => {
                n.open_entry(id);
                if let Some(panel) = &mut self.calendar {
                    panel.hide();
                }
            }
            Hit::DoNotDisturb => {
                let on = !self.styles.do_not_disturb();
                self.styles.set_do_not_disturb(on);
                let _ = self.styles.save();
                n.show_cards(&self.styles);
            }
            Hit::Group(_, false) | Hit::Panel => return,
        }
        self.sync_center();
    }

    /// Abre o cierra el calendario sobre la hora.
    fn toggle_calendar(&mut self) {
        if let Some(panel) = self.calendar.as_mut().filter(|p| p.visible) {
            panel.hide();
            return;
        }
        self.close_popups();
        if self.calendar.is_none() {
            match CalendarWindow::new(self.hwnd) {
                Ok(panel) => self.calendar = Some(panel),
                Err(_) => return,
            }
        }
        let (anchor, edge, bounds) = self.panel_edge(HoverTarget::Clock);
        if let Some(panel) = &mut self.calendar {
            panel.show(anchor, edge, bounds, self.notifications.as_mut());
        }
        self.opened_from(edge);
        // Abierto, ya no queda nada sin leer.
        self.sync_center();
    }

    /// Lo que hace un clic (o Enter con el teclado) sobre `target`; `launch` dice
    /// cómo abrir una app.
    fn click(&mut self, target: HoverTarget, launch: Launch) {
        match target {
            HoverTarget::PrimaryApp(_) | HoverTarget::OpenApp(_) if launch != Launch::Switch => {
                if let Some(menu) = &mut self.launcher {
                    menu.hide();
                }
                if let Some(a) = self.bar.item_mut(target) {
                    a.press_spring.set_target(1.0);
                    let entry = a.pin_entry();
                    let opened = if launch == Launch::Admin {
                        apps::launch_entry_as_admin(&entry)
                    } else {
                        apps::launch_entry(&entry)
                    };
                    if opened {
                        let _ = catalog::record(&entry);
                    }
                }
            }
            HoverTarget::Launcher => {
                self.bar.launcher_press.set_target(1.0);
                self.toggle_launcher();
            }
            HoverTarget::Language => {
                if let Some(tip) = &mut self.tooltip {
                    tip.hide();
                }
                self.toggle_language();
            }
            HoverTarget::PrimaryApp(_) => {
                if let Some(menu) = &mut self.launcher {
                    menu.hide();
                }
                // La ventana que pide atención, si la hay, como en Windows.
                let attention = &self.attention;
                if let Some(a) = self.bar.item_mut(target) {
                    a.press_spring.set_target(1.0);
                    if let Some(window) = attention.first_of(&a.windows) {
                        apps::focus_window(window);
                    } else if a.activate() {
                        let _ = catalog::record(&a.entry);
                    }
                }
            }
            HoverTarget::OpenApp(_) => {
                if let Some(menu) = &mut self.launcher {
                    menu.hide();
                }
                let attention = &self.attention;
                if let Some(a) = self.bar.item_mut(target) {
                    a.press_spring.set_target(1.0);
                    if let Some(hwnd) = attention.first_of(&a.windows).or(a.window) {
                        apps::focus_window(hwnd);
                    }
                }
            }
            HoverTarget::Overflow => {
                if let Some(menu) = &mut self.launcher {
                    menu.hide();
                }
                if let Some(tip) = &mut self.tooltip {
                    tip.hide();
                }
                // Siempre el panel propio: si la bandeja propia no pudo ponerse
                // delante de la de Explorer, sale vacío y lo dice.
                self.toggle_tray_panel();
            }
            HoverTarget::Status(_) => {
                if let Some(tip) = &mut self.tooltip {
                    tip.hide();
                }
                self.toggle_quick();
            }
            HoverTarget::ShowDesktop => {
                if let Some(tip) = &mut self.tooltip {
                    tip.hide();
                }
                self.close_popups();
                shell::toggle_desktop();
            }
            HoverTarget::Clock => {
                if let Some(tip) = &mut self.tooltip {
                    tip.hide();
                }
                self.toggle_calendar();
            }
        }
    }

    /// Abre sobre su icono el menú de la app `target` (su jump list y sus opciones).
    fn open_app_menu(&mut self, target: HoverTarget) {
        let Some(item) = self.bar.item_mut(target) else {
            return;
        };
        let (name, windows) = (item.entry.name.clone(), item.windows.len());
        let icon = item.icon.clone();
        let launch = item.pin_entry().command;
        // Su jump list, como en la barra de Windows.
        let jump = jumplist::app_id(&launch, &item.windows)
            .map(|id| jumplist::sections(&id))
            .unwrap_or_default();
        // "Finalizar tarea" si está activado en Windows y hay qué terminar
        // (el Explorador no, que es el escritorio).
        let end_task = apps::end_task_enabled() && !item.task_processes().is_empty();
        let chosen = MenuApp {
            command: item.entry.command.clone(),
            pinned: matches!(target, HoverTarget::PrimaryApp(_)),
            launch,
            jump,
        };
        // Se abre sobre el icono de la app, como en Windows.
        let anchor = self.pos.x
            + self
                .bar
                .item_label(target, self.scale)
                .map(|(_, cx)| cx.round() as i32)
                .unwrap_or_default();
        self.close_popups();
        if self.app_menu.is_none() {
            match AppMenuWindow::new(self.hwnd) {
                Ok(menu) => self.app_menu = Some(menu),
                Err(_) => return,
            }
        }
        let (top, bounds) = (self.y_shown, (self.screen_left, self.screen_right));
        if let Some(menu) = &mut self.app_menu {
            menu.open(
                &name,
                icon.as_ref(),
                windows,
                end_task,
                chosen,
                anchor,
                top,
                bounds,
            );
        }
        self.expand();
    }

    /// La app en la posición `index` del dock (anclados y luego abiertos).
    fn app_at(&self, index: usize) -> Option<HoverTarget> {
        let pinned = self.bar.primary_apps.len();
        if index < pinned {
            Some(HoverTarget::PrimaryApp(index))
        } else if index - pinned < self.bar.open_apps.len() {
            Some(HoverTarget::OpenApp(index - pinned))
        } else {
            None
        }
    }

    /// Win+número: la ventana que pide atención, si la hay; si la app ya está
    /// delante con varias ventanas, la que lleva más tiempo sin usarse (así se
    /// recorren todas); si no, como un clic.
    fn switch_to(&mut self, target: HoverTarget) {
        let attention = &self.attention;
        let Some(app) = self.bar.item_mut(target) else {
            return;
        };
        if let Some(window) = attention.first_of(&app.windows) {
            apps::focus_window(window);
            return;
        }
        let foreground =
            unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }.0 as isize;
        if app.windows.len() > 1 && app.windows.contains(&foreground) {
            if let Some(&oldest) = app.windows.last() {
                apps::focus_window(oldest);
                return;
            }
        }
        if app.activate() {
            let _ = catalog::record(&app.entry);
        }
    }

    /// Atiende un atajo de la barra (ver `winkey`).
    fn run_shortcut(&mut self, shortcut: winkey::Shortcut) {
        use winkey::{AppMode, Shortcut};
        match shortcut {
            Shortcut::App(index, mode) => {
                let Some(target) = self.app_at(index) else {
                    return;
                };
                if let Some(menu) = &mut self.launcher {
                    menu.hide();
                }
                match mode {
                    AppMode::Switch => self.switch_to(target),
                    AppMode::NewInstance => self.click(target, Launch::NewInstance),
                    AppMode::Admin => self.click(target, Launch::Admin),
                    AppMode::Menu => {
                        self.expand();
                        self.open_app_menu(target);
                    }
                }
            }
            Shortcut::FocusDock => {
                let first = self.app_at(0).unwrap_or(HoverTarget::Launcher);
                self.enter_keyboard_nav(first);
            }
            // Con la barra superior, la flecha no está en el dock: se abre su panel.
            Shortcut::FocusTray if self.top.is_some() => self.toggle_tray_panel(),
            Shortcut::FocusTray => self.enter_keyboard_nav(HoverTarget::Overflow),
            Shortcut::Quick => self.toggle_quick(),
            Shortcut::Calendar => self.toggle_calendar(),
        }
    }

    /// Empieza a recorrer el dock con el teclado, resaltando `target`.
    fn enter_keyboard_nav(&mut self, target: HoverTarget) {
        self.close_popups();
        if self.keyboard_nav.is_none() {
            let previous =
                unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
            self.keyboard_nav = Some(previous.0 as isize);
        }
        self.expand();
        take_focus(self.hwnd);
        self.bar.update_hover(Some(target));
        self.restart_tooltip();
        self.start_animating();
        self.render();
    }

    /// Deja de recorrer el dock con el teclado; con `back`, devuelve el foco a la
    /// ventana que lo tenía.
    fn leave_keyboard_nav(&mut self, back: bool) {
        let Some(previous) = self.keyboard_nav.take() else {
            return;
        };
        if back {
            let previous = HWND(previous as *mut _);
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::{IsWindow, SetForegroundWindow};
                if IsWindow(Some(previous)).as_bool() {
                    let _ = SetForegroundWindow(previous);
                }
            }
        }
        self.bar.update_hover(None);
        self.restart_tooltip();
        self.start_animating();
        self.render();
    }

    /// Una tecla con el dock recorrido con el teclado; `true` si la atendió.
    fn dock_key(&mut self, key: u32) -> bool {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_SHIFT};
        if self.keyboard_nav.is_none() {
            return false;
        }
        let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
        let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
        let current = self.bar.hovered;
        // Izquierda, derecha, Tab (Mayús+Tab hacia atrás), Inicio y Fin.
        let step = match key {
            0x25 => Some(-1),
            0x27 => Some(1),
            0x09 => Some(if shift { -1 } else { 1 }),
            0x24 => Some(-1000),
            0x23 => Some(1000),
            _ => None,
        };
        if let Some(step) = step {
            let next = self.bar.focus_step(current, step);
            self.bar.update_hover(Some(next));
            self.restart_tooltip();
            self.start_animating();
            self.render();
            return true;
        }
        match key {
            // Enter o espacio: como un clic (Mayús, otra instancia; Ctrl+Mayús, administrador).
            0x0D | 0x20 => {
                let Some(target) = current else {
                    return true;
                };
                let launch = match (shift, ctrl) {
                    (true, true) => Launch::Admin,
                    (true, false) => Launch::NewInstance,
                    _ => Launch::Switch,
                };
                self.leave_keyboard_nav(false);
                self.click(target, launch);
                self.start_animating();
                self.render();
                true
            }
            // Tecla de menú o Mayús+F10: el menú de la app.
            0x5D | 0x79 if key == 0x5D || shift => {
                if let Some(target @ (HoverTarget::PrimaryApp(_) | HoverTarget::OpenApp(_))) =
                    current
                {
                    self.leave_keyboard_nav(false);
                    self.open_app_menu(target);
                }
                true
            }
            0x1B => {
                self.leave_keyboard_nav(true);
                true
            }
            _ => false,
        }
    }

    /// Altura que reserva el modo siempre visible: del borde superior del dock al
    /// borde inferior del monitor.
    fn reserved_height(&self) -> i32 {
        self.screen_bottom - self.y_shown
    }

    /// Pone o quita el sitio reservado según el modo, en el monitor del dock.
    fn sync_appbar(&mut self) {
        if !self.styles.always_visible() {
            self.appbar = None;
            self.appbar_fullscreen = false;
            return;
        }
        if self.appbar.is_none() {
            self.appbar = appbar::AppBar::register(self.hwnd, WM_APPBAR);
        }
        if let Some(bar) = &self.appbar {
            let monitor = windows::Win32::Foundation::RECT {
                left: self.screen_left,
                top: self.screen_top,
                right: self.screen_right,
                bottom: self.screen_bottom,
            };
            bar.place(monitor, self.reserved_height(), false);
        }
    }

    /// Al salir: devuelve la barra nativa y el sitio reservado.
    fn release_shell(&mut self) {
        self.appbar = None;
        self.top = None;
        self.shell.restore();
    }

    /// Dónde abrir el panel de `target`: bajo su sitio en la barra superior, si
    /// está, o sobre su sitio en el dock. Centro, borde y límites de la pantalla.
    fn panel_edge(&self, target: HoverTarget) -> (i32, Edge, (i32, i32)) {
        if let Some(top) = &self.top {
            if let Some(center) = top.model.center(target) {
                let edge = Edge {
                    y: top.rect.top + top.height(),
                    below: true,
                    monitor: top.monitor,
                };
                let x = top.rect.left + (center * top.scale).round() as i32;
                return (x, edge, (top.rect.left, top.rect.right));
            }
        }
        let center = match target {
            HoverTarget::Status(_) => self.bar.status_group_center(self.scale),
            HoverTarget::Language => self.bar.language_center(self.scale),
            HoverTarget::Overflow => self.bar.overflow_center(self.scale),
            HoverTarget::Clock => self.bar.clock_center(self.scale),
            _ => self
                .bar
                .item_label(target, self.scale)
                .map(|(_, cx)| cx)
                .unwrap_or_default(),
        };
        let edge = Edge {
            y: self.y_shown,
            below: false,
            monitor: monitors::dock(),
        };
        (
            self.pos.x + center.round() as i32,
            edge,
            (self.screen_left, self.screen_right),
        )
    }

    /// Tras abrir un panel: si salió del dock, el dock se muestra; si salió de la
    /// barra superior, no hace falta.
    fn opened_from(&mut self, edge: Edge) {
        self.panels_from_top = edge.below;
        if !edge.below {
            self.expand();
        }
    }

    /// Pone o quita la barra superior según el ajuste; con ella, el dock solo
    /// lleva las apps.
    fn sync_top_bar(&mut self) {
        let on = self.styles.top_bar();
        if on && self.top.is_none() {
            match TopBarWindow::new() {
                Ok(mut top) => {
                    top.place();
                    self.top = Some(top);
                    // Se muestra ya, salvo que haya una app a pantalla completa.
                    self.check_priority(true);
                }
                Err(_) => return,
            }
        } else if !on {
            self.top = None;
        }
        if self.bar.compact != on {
            self.close_popups();
            self.bar.compact = on;
            self.relayout();
        }
        self.update_active_app();
        self.refresh_top_bar();
    }

    /// Pasa a la barra superior el estado, el teclado, la hora y la apariencia, y
    /// la redibuja si algo cambió.
    fn refresh_top_bar(&mut self) {
        let Some(top) = &mut self.top else {
            return;
        };
        top.model.status = self.bar.status;
        top.model.keyboard = self.bar.keyboard.clone();
        top.model.appearance = self.bar.appearance;
        top.model.clock = clock::day_and_time();
        top.render();
    }

    /// El nombre de la app activa para la barra superior. Las ventanas de GlassBar
    /// no cuentan: abrir un panel no cambia el nombre.
    fn update_active_app(&mut self) {
        let Some(top) = &mut self.top else {
            return;
        };
        if let Some(name) = active_app_name() {
            top.model.app = name;
        }
    }

    /// El cursor se movió por la barra superior (`x`, `y` en píxeles de la ventana).
    fn top_hover(&mut self, x: i32, y: i32) {
        let Some(top) = &mut self.top else {
            return;
        };
        let target = top
            .model
            .hit_test(x as f32 / top.scale, y as f32 / top.scale);
        if target != top.model.hovered {
            top.model.hovered = target;
            top.render();
        }
    }

    /// Clic derecho en la barra superior: en el logo de Inicio, el menú de Windows.
    fn top_right_click(&mut self, x: i32, y: i32) {
        let Some(top) = &self.top else {
            return;
        };
        let target = top
            .model
            .hit_test(x as f32 / top.scale, y as f32 / top.scale);
        if target == Some(HoverTarget::Launcher) {
            self.open_windows_menu();
        }
    }

    /// Clic en la barra superior: lo mismo que en el dock.
    fn top_click(&mut self, x: i32, y: i32) {
        let Some(top) = &self.top else {
            return;
        };
        let target = top
            .model
            .hit_test(x as f32 / top.scale, y as f32 / top.scale);
        match target {
            Some(HoverTarget::Launcher) => self.toggle_launcher(),
            Some(target) => self.click(target, Launch::Switch),
            None => {}
        }
    }

    /// Estado de las actualizaciones para la configuración.
    fn settings_updates(&self) -> settings::Updates {
        settings::Updates {
            checking: self.checking,
            available: self.new_release.as_ref().map(|r| r.version.to_string()),
            busy: self.updating,
        }
    }

    /// Abre la ventana de configuración (con el icono de GlassBar en la bandeja).
    fn open_settings(&mut self) {
        self.close_popups();
        let model = settings::SettingsState::new(
            self.styles.clone(),
            autostart::is_enabled(),
            self.settings_updates(),
        );
        if self.settings.is_none() {
            match SettingsWindow::new(model.clone()) {
                Ok(window) => self.settings = Some(window),
                Err(_) => return,
            }
        }
        if let Some(window) = &mut self.settings {
            if window.visible {
                window.restore();
                take_focus(window.hwnd);
                return;
            }
            window.model = model;
            window.show();
        }
    }

    /// Vuelve a leer los ajustes en la configuración abierta, tras cambiar alguno.
    fn refresh_settings(&mut self) {
        let updates = self.settings_updates();
        let styles = self.styles.clone();
        let Some(window) = self.settings.as_mut().filter(|w| w.visible) else {
            return;
        };
        window.model.styles = styles;
        window.model.autostart = autostart::is_enabled();
        window.model.updates = updates;
        window.render();
    }

    fn check_priority(&mut self, raise: bool) {
        let monitor = windows::Win32::Foundation::RECT {
            left: self.screen_left,
            top: self.screen_top,
            right: self.screen_right,
            bottom: self.screen_bottom,
        };
        self.fullscreen = priority::fullscreen_app(monitor)
            || priority::system_fullscreen()
            || (self.appbar.is_some() && self.appbar_fullscreen);
        // Escondido no hace falta: se pone encima al mostrarse (ver `reveal`). Y a
        // una ventana "siempre encima" que se acaba de activar no se le quita el sitio.
        let other_topmost = priority::foreground_topmost();
        if raise && !self.fullscreen && self.visibility.expanded && !other_topmost {
            self.raise();
        }
        if let Some(top) = &mut self.top {
            let fullscreen = priority::fullscreen_app(top.rect)
                || priority::system_fullscreen()
                || top.appbar_fullscreen;
            top.set_hidden(fullscreen);
            if raise && !fullscreen && !other_topmost {
                priority::raise_below_overlapping(top.hwnd, top.rect);
            }
        }
    }

    /// Pone el dock por encima de las demás ventanas "siempre encima", con su
    /// desenfoque justo debajo.
    fn raise(&self) {
        priority::raise(self.hwnd);
        self.sync_backdrop();
    }

    /// Ancla o desancla en el dock una app desde el lanzador, que sigue abierto
    /// con su sección "Anclado" al día.
    fn pin_from_launcher(&mut self, entry: apps::AppEntry, pin: bool) {
        let same = |a: &apps::AppItem| a.entry.command.eq_ignore_ascii_case(&entry.command);
        let position = self.bar.primary_apps.iter().position(same);
        match (pin, position) {
            (true, None) => self.bar.primary_apps.push(apps::AppItem::new(entry)),
            (false, Some(index)) => {
                self.bar.primary_apps.remove(index);
            }
            _ => return,
        }
        // Sus ventanas pasan al anclado (o vuelven a los abiertos).
        apps::update_running_apps(&mut self.bar.primary_apps, &mut self.bar.open_apps);
        self.pins_changed();
        let pinned: Vec<_> = self
            .bar
            .primary_apps
            .iter()
            .map(|a| a.entry.clone())
            .collect();
        if let Some(menu) = self.launcher.as_mut().filter(|m| m.visible) {
            menu.model.set_pinned(&pinned);
            menu.render();
        }
    }

    fn pins_changed(&mut self) {
        self.save_pins();
        self.relayout();
        self.reveal();
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
        let (rc, dpi) = monitors::get_info(monitors::dock());
        self.scale = (dpi * self.styles.dock_factor())
            .min((rc.right - rc.left) as f32 / (self.bar.dock_width() + 40.0));
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
        // Con una vista previa abierta, pasar a otro programa con ventana la cambia al
        // momento, como en Windows; si no la tiene, se cierra.
        let previewing = self.preview.as_ref().is_some_and(|p| p.visible);
        if previewing && self.bar.pressed.is_none() {
            if let Some(target) = self.bar.hovered {
                if !self.show_preview(target) {
                    if let Some(preview) = &mut self.preview {
                        preview.hide();
                    }
                } else {
                    return;
                }
            }
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
                        | HoverTarget::Language
                        | HoverTarget::Clock
                        | HoverTarget::ShowDesktop
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
        if target == HoverTarget::Overflow && self.tray_panel.as_ref().is_some_and(|p| p.visible) {
            return;
        }
        if matches!(target, HoverTarget::Status(_))
            && self.quick.as_ref().is_some_and(|q| q.visible)
        {
            return;
        }
        if target == HoverTarget::Language && self.language.as_ref().is_some_and(|l| l.visible) {
            return;
        }
        if target == HoverTarget::Clock && self.calendar.as_ref().is_some_and(|c| c.visible) {
            return;
        }
        // Programa con ventana abierta: vista previa en lugar del nombre.
        if self.show_preview(target) {
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

    /// Muestra la vista previa de las ventanas del programa `target`, si tiene alguna.
    fn show_preview(&mut self, target: HoverTarget) -> bool {
        if !matches!(target, HoverTarget::PrimaryApp(_) | HoverTarget::OpenApp(_)) {
            return false;
        }
        let Some((name, cx)) = self.bar.item_label(target, self.scale) else {
            return false;
        };
        let windows = self
            .bar
            .item_mut(target)
            .map(|a| a.windows.clone())
            .unwrap_or_default();
        if windows.is_empty() {
            return false;
        }
        // Cada tarjeta con el título de su ventana; sin título, el del programa.
        let sources: Vec<(isize, String)> = windows
            .into_iter()
            .map(|w| {
                let title = window_title(w);
                let title = if title.trim().is_empty() {
                    name.clone()
                } else {
                    title
                };
                (w, title)
            })
            .collect();
        let (anchor, top, scale) = (self.pos.x + cx.round() as i32, self.y_shown, self.scale);
        let bounds = (self.screen_left, self.screen_right);
        let Some(preview) = &mut self.preview else {
            return false;
        };
        let shown = preview.show(&sources, target, anchor, top, bounds, scale);
        if shown {
            self.preview_away = None;
            if let Some(tip) = &mut self.tooltip {
                tip.hide();
            }
        }
        shown
    }

    /// Cierra la vista previa si el cursor ya no está ni en ella ni en su programa
    /// desde hace un momento (el justo para pasar del icono al panel).
    fn check_preview(&mut self, cursor: POINT, now: Instant) {
        let Some(preview) = self.preview.as_mut().filter(|p| p.visible) else {
            return;
        };
        let over = preview.contains(cursor) || self.bar.hovered == preview.target;
        if over || self.bar.pressed.is_some() && self.bar.pressed == preview.target {
            self.preview_away = None;
            return;
        }
        let since = *self.preview_away.get_or_insert(now);
        if now.duration_since(since) > Duration::from_millis(250) {
            preview.hide();
            self.preview_away = None;
        }
    }

    fn expand(&mut self) {
        self.reveal();
        self.start_animating();
    }

    /// Hace que el dock se muestre. Escondido no se mira la pantalla (ver
    /// `TIMER_TICK`): se mira al salir, para que aparezca ya con su apariencia.
    fn reveal(&mut self) {
        let hidden = !self.visibility.expanded;
        self.visibility.reveal();
        if hidden {
            self.update_style();
            // Escondido pudo quedar debajo de otra ventana "siempre encima".
            if !self.fullscreen {
                self.raise();
            }
        }
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
                Some(self.surface.dc()),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
        // El desenfoque acompaña al dock al aparecer, ocultarse o cambiar de ancho.
        self.sync_backdrop_if_changed();
    }

    /// Mueve el dock a `pos` sin volver a enviar su imagen: al mostrarse o esconderse
    /// solo cambia la posición, y subir la imagen entera costaba ~0,6 ms por fotograma.
    fn move_window(&self) {
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
                None,
                None,
                None,
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
        self.sync_backdrop();
    }

    /// Redibuja el dock en la superficie y lo envía al compositor
    fn render(&mut self) {
        if let Some(pixmap) = self.surface.pixmap() {
            self.bar.draw(pixmap, self.scale);
        }
        self.surface.copy_to_dib();
        self.update_position();
    }

    /// Empieza a recibir un fotograma por refresco de la pantalla (ver `frames`).
    fn start_animating(&mut self) {
        if !self.animating {
            self.animating = true;
            self.last_tick = Instant::now();
            frames::start(self.hwnd);
        }
    }

    /// Deja de recibir fotogramas cuando los resortes se asientan (0% CPU).
    fn stop_animating(&mut self) {
        if self.animating {
            self.animating = false;
            frames::stop(self.hwnd);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // La barra se volvió a crear (Explorer reiniciado, o la bandeja propia pidiendo
    // los iconos): se registra otra vez el icono de GlassBar.
    if msg != 0 && msg == taskbar_created() {
        with_app(|app| app.tray.register());
        return LRESULT(0);
    }
    // Una ventana pide atención, o se activó o cerró una que la pedía.
    if msg != 0 && msg == attention::hook_message() {
        with_app(|app| app.shell_event(wparam.0, lparam.0));
        return LRESULT(0);
    }
    match msg {
        // Cambió el color de énfasis (Windows avisa con estos dos mensajes, también
        // por otros ajustes): se vuelve a leer y, si es otro, se redibuja lo abierto.
        windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE
        | WM_DWMCOLORIZATIONCOLORCHANGED => {
            // Explorer avisa con WM_SETTINGCHANGE al cambiar el ocultar automático
            // de su barra y recalcular el área de trabajo.
            if msg == windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(hwnd),
                    WM_WORK_AREA,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
            with_app(|app| {
                if accent::refresh() {
                    app.render();
                    if let Some(panel) = app.quick.as_mut().filter(|p| p.visible) {
                        panel.render();
                    }
                    if let Some(menu) = app.launcher.as_mut().filter(|m| m.visible) {
                        menu.render();
                    }
                }
            });
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_TRAY_MENU => {
            with_app(|app| app.open_tray_menu());
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_HOTKEY
            if wparam.0 == HOTKEY_SPOTLIGHT as usize =>
        {
            with_app(|app| app.toggle_spotlight());
            LRESULT(0)
        }
        WM_SHORTCUT => {
            if let Some(shortcut) = winkey::Shortcut::decode(wparam.0) {
                with_app(|app| app.run_shortcut(shortcut));
            }
            LRESULT(0)
        }
        WM_APPBAR => {
            match wparam.0 {
                appbar::ABN_POSCHANGED => {
                    with_app(|app| app.sync_appbar());
                }
                appbar::ABN_FULLSCREENAPP => {
                    with_app(|app| {
                        app.appbar_fullscreen = lparam.0 != 0;
                        app.check_priority(false);
                    });
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_WORK_AREA => {
            with_app(|app| app.shell.keep_work_area());
            LRESULT(0)
        }
        WM_UPDATE => {
            with_app(|app| app.handle_update_events());
            LRESULT(0)
        }
        WM_NOTIFICATIONS => {
            with_app(|app| app.handle_notifications());
            LRESULT(0)
        }
        // Se conectó o desconectó un monitor, o cambió su resolución.
        windows::Win32::UI::WindowsAndMessaging::WM_DISPLAYCHANGE => {
            with_app(|app| app.fit_monitor());
            LRESULT(0)
        }
        WM_TRAY_ICONS => {
            with_app(|app| {
                if let Some(panel) = app.tray_panel.as_mut().filter(|p| p.visible) {
                    panel.refresh();
                }
            });
            LRESULT(0)
        }
        WM_FOREGROUND => {
            with_app(|app| {
                // Se abrió el Inicio nativo pese a todo: se abre el lanzador, que al
                // llevarse el foco lo cierra (como hacer clic fuera de él).
                if app.shell.is_active() && shell::native_start_open() {
                    if !app.launcher.as_ref().is_some_and(|menu| menu.visible) {
                        app.toggle_launcher();
                    }
                    return;
                }
                // Con el menú de un icono de la bandeja abierto, el dock y su panel no
                // se ponen delante: el menú quedaría detrás.
                let tray_menu = app.tray_panel.as_ref().is_some_and(|p| p.menu.is_some())
                    || app
                        .tray_clicked
                        .is_some_and(|at| at.elapsed() < Duration::from_millis(1500));
                app.check_priority(!tray_menu);
                // Una ventana que se abre o se activa con la parte de arriba debajo
                // de la barra superior se baja para que se vea su título.
                if let Some(top) = app.top.as_ref().filter(|top| !top.hidden) {
                    let strip = appbar::strip(top.rect, top.height(), true);
                    priority::keep_below(strip, top.rect);
                }
                app.update_keyboard();
                app.update_style();
                app.update_active_app();
                app.refresh_top_bar();
            });
            LRESULT(0)
        }
        WM_SHOW_LAUNCHER => {
            with_app(|app| {
                // Con la tecla Windows, en el monitor del cursor: donde se está trabajando.
                if !app.launcher.as_ref().is_some_and(|menu| menu.visible) {
                    let mut cursor = POINT::default();
                    let _ = GetCursorPos(&mut cursor);
                    let monitor = monitors::at(cursor);
                    if monitor != monitors::dock() && !app.styles.always_visible() {
                        app.move_to_monitor(monitor);
                    }
                }
                app.toggle_launcher();
            });
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
                // Un clic en el dock cierra el menú abierto, como en Windows; los
                // paneles se cierran al abrir otro o al llevarse el foco otra app.
                if let Some(menu) = &mut app.app_menu {
                    menu.hide();
                }
                if let Some(target) = app.bar.hovered {
                    app.bar.pressed = Some(target);
                    if let Some(preview) = &mut app.preview {
                        preview.hide();
                    }
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
                    // Como en Windows: Mayús abre otra instancia y Ctrl+Mayús, como administrador.
                    let (shift, ctrl) = (wparam.0 & 0x4 != 0, wparam.0 & 0x8 != 0);
                    let launch = match (shift, ctrl) {
                        (true, true) => Launch::Admin,
                        (true, false) => Launch::NewInstance,
                        _ => Launch::Switch,
                    };
                    app.click(target, launch);
                    app.start_animating();
                    app.render();
                }
            });
            LRESULT(0)
        }

        // Clic central sobre una app: otra instancia, como en la barra de Windows.
        windows::Win32::UI::WindowsAndMessaging::WM_MBUTTONUP => {
            with_app(|app| {
                let x = (lparam.0 & 0xffff) as i16 as f32;
                let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32;
                let target = app.bar.hit_test(x, y, app.scale);
                if let Some(target @ (HoverTarget::PrimaryApp(_) | HoverTarget::OpenApp(_))) =
                    target
                {
                    app.click(target, Launch::NewInstance);
                    app.start_animating();
                    app.render();
                }
            });
            LRESULT(0)
        }

        // Con el dock recorrido con el teclado (Win+T, Win+B).
        windows::Win32::UI::WindowsAndMessaging::WM_KEYDOWN
        | windows::Win32::UI::WindowsAndMessaging::WM_SYSKEYDOWN => {
            if with_app(|app| app.dock_key(wparam.0 as u32)) == Some(true) {
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_KILLFOCUS => {
            with_app(|app| app.leave_keyboard_nav(false));
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
                    app.bar.status = app.status.read();
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
                let target = app.bar.hit_test(x, y, app.scale);
                app.bar.hovered = target;
                // El nombre o la vista previa taparían el menú.
                if let Some(tip) = &mut app.tooltip {
                    tip.hide();
                }
                if let Some(preview) = &mut app.preview {
                    preview.hide();
                }
                match target {
                    // Sobre una app: su propio menú, como en la barra de Windows.
                    Some(target @ (HoverTarget::PrimaryApp(_) | HoverTarget::OpenApp(_))) => {
                        app.open_app_menu(target);
                    }
                    // Sobre el botón de inicio, el de Windows (Win+X).
                    Some(HoverTarget::Launcher) => app.open_windows_menu(),
                    // En el resto del dock, nada: la configuración se abre con el
                    // icono de GlassBar en la bandeja.
                    _ => {}
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

                    let moved = current_y != app.pos.y;
                    app.pos.y = current_y;
                    if bar_moving {
                        app.render();
                    } else if moved {
                        app.move_window();
                    }

                    if !bar_moving && !slide_moving && !app.visibility.waiting() {
                        app.stop_animating();
                    }
                });
            } else if id == TIMER_TICK {
                with_app(|app| {
                    app.shell.maintain();
                    app.refresh_top_bar();
                    // Pasar a pantalla completa (F11) no cambia la ventana activa.
                    app.check_priority(false);
                    // Red, volumen y batería; si aparece o desaparece la batería cambia el ancho.
                    let status = app.status.read();
                    // Ahorro de batería encendido desde el panel: al conectar el
                    // cargador el umbral vuelve a ser el del usuario, para que no se
                    // encienda solo al desconectarlo (como con el botón de Windows).
                    if status.battery.is_some_and(|b| b.plugged) && toggles::saver_changed() {
                        toggles::restore_saver();
                    }
                    // El idioma también cambia con atajos (Win+Espacio, Alt+Mayús).
                    app.update_keyboard();
                    // Maximizar o restaurar no cambia la ventana activa. Con el dock
                    // escondido no se ve: se mira al empezar a mostrarse.
                    if app.visibility.expanded {
                        app.update_style();
                    }
                    // El porcentaje de la batería también cambia el ancho.
                    let resized = status.items().len() != app.bar.status.items().len()
                        || status.battery_text() != app.bar.status.battery_text();
                    let status_changed = status != app.bar.status;
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
                    let running = |app: &App| -> Vec<bool> {
                        app.bar.primary_apps.iter().map(|a| a.is_running).collect()
                    };
                    let was_running = running(app);
                    let opened_or_closed = apps::update_running_apps(
                        &mut app.bar.primary_apps,
                        &mut app.bar.open_apps,
                    );
                    // Por si se perdió el aviso de que una ventana dejó de pedir atención.
                    app.attention.prune();
                    let attention_changed = app.mark_attention();
                    if opened_or_closed || resized {
                        app.relayout();
                    } else if status_changed
                        || attention_changed
                        || running(app) != was_running
                        || app.bar.clock_outdated()
                    {
                        // Solo si algo de lo que se ve cambió: redibujar el dock entero
                        // cada segundo era la mayor parte de su gasto en reposo.
                        app.render();
                    }
                });
            } else if id == TIMER_UPDATE_FIRST || id == TIMER_UPDATE {
                with_app(|app| {
                    if id == TIMER_UPDATE_FIRST {
                        let _ = KillTimer(Some(app.hwnd), TIMER_UPDATE_FIRST);
                    }
                    app.check_updates(false);
                });
            } else if id == TIMER_ANNOUNCE {
                with_app(|app| {
                    let _ = KillTimer(Some(app.hwnd), TIMER_ANNOUNCE);
                    if let Some(host) = &app.systray {
                        host.announce_again();
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
                    // El dock pasa al monitor de cuyo borde inferior se acerca el cursor,
                    // como si cada pantalla tuviera el suyo.
                    let monitor = monitors::at(pt);
                    // Siempre visible, el dock no cambia de monitor: con él se movería
                    // el sitio reservado y las ventanas de las dos pantallas.
                    if monitor != monitors::dock() && !app.busy() && !app.styles.always_visible() {
                        let (rc, dpi) = monitors::get_info(monitor);
                        if cursor_at_edge(pt, rc.left, rc.right, rc.bottom, dpi) {
                            app.move_to_monitor(monitor);
                        }
                    }
                    let at_edge = cursor_at_edge(
                        pt,
                        app.screen_left,
                        app.screen_right,
                        app.screen_bottom,
                        app.scale,
                    );
                    app.check_preview(pt, Instant::now());
                    app.close_on_outside_click(pt);
                    // Con el dock a la vista, la apariencia sigue a la pantalla más de
                    // cerca (escribir en el lanzador, maximizar una ventana).
                    if app.visibility.expanded
                        && app.scene_read.elapsed() >= Duration::from_millis(250)
                    {
                        app.update_style();
                    }
                    let own_menu = app.app_menu.as_ref().is_some_and(|m| m.visible);
                    if let Some(panel) = app.tray_panel.as_mut().filter(|p| p.visible) {
                        panel.watch_menu(own_menu, app.hwnd);
                    }
                    // Que los iconos de los demás programas sigan llegando a la bandeja propia.
                    // Cinco veces por segundo basta: mirarlo cada 40 ms era la mitad del
                    // gasto de este temporizador.
                    if app.tray_checked.elapsed() >= Duration::from_millis(200) {
                        app.tray_checked = Instant::now();
                        if let Some(host) = &app.systray {
                            host.keep_in_front();
                        }
                    }
                    let was_expanded = app.visibility.expanded;
                    let inside = app.visibility.expanded
                        && pt.x >= app.pos.x
                        && pt.x < app.pos.x + app.surface.width as i32
                        && pt.y >= app.y_shown
                        && pt.y < app.screen_bottom;
                    app.visibility.sample(
                        (at_edge && !app.fullscreen)
                            || (app.styles.always_visible() && !app.fullscreen)
                            || app.keyboard_nav.is_some()
                            || inside
                            || app.bar.drag.is_some()
                            || app.launcher.as_ref().is_some_and(|l| l.visible)
                            // Los paneles abiertos desde la barra superior no lo asoman.
                            || (!app.panels_from_top
                                && (app.quick.as_ref().is_some_and(|q| q.visible)
                                    || app.language.as_ref().is_some_and(|l| l.visible)
                                    || app.calendar.as_ref().is_some_and(|c| c.visible)
                                    || app.tray_panel.as_ref().is_some_and(|p| p.visible)
                                    || app.tray_panel.as_ref().is_some_and(|p| p.menu.is_some())))
                            || app.app_menu.as_ref().is_some_and(|m| m.visible)
                            || app.preview.as_ref().is_some_and(|p| p.visible)
                            // El menú de un icono de la bandeja abierto, o un icono recién
                            // pulsado, que puede tardar en abrir el suyo. Los menús de
                            // otros programas no cuentan.
                            || app
                                .tray_clicked
                                .is_some_and(|at| at.elapsed() < Duration::from_millis(1500)),
                        Instant::now(),
                    );
                    // Empieza a mostrarse: la apariencia, al día desde el primer fotograma.
                    if app.visibility.expanded && !was_expanded {
                        app.update_style();
                    }
                    if app.visibility.active() {
                        app.start_animating();
                    }
                    if pt.x != app.edge_cursor.x || pt.y != app.edge_cursor.y {
                        app.edge_cursor = pt;
                        app.edge_moved = Instant::now();
                    }
                    let slow = edge_can_idle(
                        app.edge_moved.elapsed(),
                        app.animating
                            || app.visibility.active()
                            || app.busy()
                            || app.keyboard_nav.is_some()
                            || app.bar.pressed.is_some()
                            || app.preview.as_ref().is_some_and(|p| p.visible),
                    );
                    if slow != app.edge_slow {
                        app.edge_slow = slow;
                        let _ = SetTimer(
                            Some(app.hwnd),
                            TIMER_EDGE,
                            if slow { EDGE_IDLE_MS } else { EDGE_MS },
                            None,
                        );
                    }
                });
            }
            LRESULT(0)
        }

        WM_TRAY => {
            let event = (lparam.0 & 0xffff) as u32;
            // Clic derecho en el icono de GlassBar: su menú, con Configuración y Salir.
            // Se abre después, en su propio mensaje: desde el panel de apps en segundo
            // plano el clic llega mientras GlassBar aún atiende ese panel, y entonces
            // no podría abrirse.
            if event == WM_RBUTTONUP {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(hwnd),
                    WM_TRAY_MENU,
                    WPARAM(0),
                    LPARAM(0),
                );
            } else if event == tray::NIN_BALLOONUSERCLICK {
                // Clic en el aviso de una versión nueva: se actualiza.
                with_app(|app| app.start_update());
            }
            LRESULT(0)
        }

        windows::Win32::UI::WindowsAndMessaging::WM_ENDSESSION => {
            if wparam.0 != 0 {
                with_app(|app| app.release_shell());
                toggles::restore_saver();
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        // Alt+F4 con el dock activo (tras recorrerlo con el teclado, o porque Windows
        // le pasó la activación al cerrarse un panel suyo) cerraría GlassBar. Se
        // ignora: se sale desde la configuración o el menú. El `WM_CLOSE` que manda
        // `--replace` no pasa por aquí y sigue cerrándolo.
        windows::Win32::UI::WindowsAndMessaging::WM_SYSCOMMAND if is_sc_close(wparam) => LRESULT(0),
        WM_DESTROY => {
            with_app(|app| {
                app.release_shell();
            });
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Qué hacer con una pulsación de Alt+F4 en una ventana de GlassBar.
#[derive(Debug, PartialEq)]
enum AltF4 {
    /// Lo de Windows: no es Alt+F4, o es la configuración (que se esconde) o una
    /// ventana ajena (un diálogo de archivos).
    Default,
    /// En el dock: no se hace nada, en vez de cerrar GlassBar.
    Ignore,
    /// En un panel: como Esc, que lo cierra. Windows lo destruiría y ya no se
    /// podría volver a abrir.
    Escape,
}

fn alt_f4(msg: &MSG) -> AltF4 {
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, WM_SYSKEYDOWN};
    if msg.message != WM_SYSKEYDOWN || msg.wParam.0 != 0x73 {
        return AltF4::Default;
    }
    let mut name = [0u16; 64];
    let len = unsafe { GetClassNameW(msg.hwnd, &mut name) }.max(0) as usize;
    alt_f4_for(&String::from_utf16_lossy(&name[..len]))
}

/// `alt_f4` según la clase de la ventana.
fn alt_f4_for(class: &str) -> AltF4 {
    match class {
        "GlassBarWindow" => AltF4::Ignore,
        "GlassBarSettings" => AltF4::Default,
        class if class.starts_with("GlassBar") => AltF4::Escape,
        _ => AltF4::Default,
    }
}

/// `WM_SYSCOMMAND` con `SC_CLOSE`: Alt+F4 o "Cerrar" del menú de la ventana. Los
/// cuatro bits bajos los usa Windows por su cuenta.
fn is_sc_close(wparam: WPARAM) -> bool {
    wparam.0 & 0xFFF0 == windows::Win32::UI::WindowsAndMessaging::SC_CLOSE as usize
}

/// Panel de idioma del teclado sobre el indicador del dock.
struct LanguageWindow {
    hwnd: HWND,
    /// Desenfoque de debajo, como el del dock.
    backdrop: Option<style::Backdrop>,
    surface: Option<Surface>,
    pos: POINT,
    scale: f32,
    model: language::LanguageState,
    visible: bool,
    transition: launcher::Transition,
    /// Ventana que estaba activa al abrirlo: a ella se aplica el teclado elegido.
    target: HWND,
}
impl LanguageWindow {
    fn new(owner: HWND) -> Result<Self> {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                w!("GlassBarLanguage"),
                w!("GlassBar — Idioma del teclado"),
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
            backdrop: style::Backdrop::new(),
            surface: None,
            pos: POINT::default(),
            scale: 1.0,
            model: language::LanguageState::default(),
            visible: false,
            transition: launcher::Transition::hidden(),
            target: HWND::default(),
        })
    }
    /// Lo abre sobre `anchor_x`, encima del dock, con los teclados instalados.
    fn show(
        &mut self,
        target: HWND,
        current: Option<isize>,
        anchor_x: i32,
        edge: Edge,
        (left, right): (i32, i32),
    ) {
        self.target = target;
        self.model = language::LanguageState {
            layouts: keyboard::installed()
                .into_iter()
                .map(keyboard::describe)
                .collect(),
            current,
            hovered: None,
        };
        let (_, dpi) = monitors::get_info(edge.monitor);
        self.scale = dpi.max(0.3);
        let width = (language::WIDTH * self.scale).ceil() as u32;
        let height = (self.model.height() * self.scale).ceil() as u32;
        let Ok(surface) = Surface::new(width, height) else {
            return;
        };
        self.surface = Some(surface);
        let margin = (12.0 * self.scale).round() as i32;
        self.pos = POINT {
            x: (anchor_x - width as i32 / 2).clamp(
                left + margin,
                (right - width as i32 - margin).max(left + margin),
            ),
            y: edge.panel_top(height as i32, margin),
        };
        self.visible = true;
        self.transition.open(Instant::now());
        self.render();
        take_focus(self.hwnd);
    }
    fn render(&mut self) {
        if let Some(surface) = &mut self.surface {
            if let Some(pixmap) = surface.pixmap() {
                self.model.draw(pixmap, self.scale);
            }
            surface.copy_to_dib();
        }
        self.present();
    }
    fn present(&mut self) {
        if let Some(surface) = &mut self.surface {
            let shapes = [panel_shape(surface, self.scale, 12.0)];
            present_popup(
                self.hwnd,
                surface,
                self.pos,
                self.scale,
                &self.transition,
                self.visible,
                false,
                self.backdrop.as_ref(),
                &shapes,
            );
        }
    }
    fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.model.hovered = None;
        self.transition.close(Instant::now());
        self.present();
    }
}
impl Drop for LanguageWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

/// Aplica el teclado elegido en el panel de idioma a la ventana que estaba activa y
/// le devuelve el foco.
fn choose_language(app: &mut App, index: usize) {
    let Some(panel) = &mut app.language else {
        return;
    };
    let Some(layout) = panel.model.layouts.get(index).map(|l| l.id) else {
        return;
    };
    let target = panel.target;
    panel.hide();
    unsafe {
        if !target.is_invalid() {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(target);
            keyboard::switch(target, layout);
        }
    }
    app.update_keyboard();
}

unsafe extern "system" fn language_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{WA_INACTIVE, WM_ACTIVATE, WM_KEYDOWN};
    let point = |scale: f32| {
        (
            (lparam.0 & 0xffff) as i16 as f32 / scale,
            ((lparam.0 >> 16) & 0xffff) as i16 as f32 / scale,
        )
    };
    match msg {
        WM_ACTIVATE if (wparam.0 & 0xffff) as u32 == WA_INACTIVE => {
            with_app(|app| app.language.as_mut().map(LanguageWindow::hide));
        }
        WM_TIMER if wparam.0 == TIMER_ANIM => {
            with_app(|app| app.language.as_mut().map(LanguageWindow::present));
        }
        WM_MOUSEMOVE => {
            with_app(|app| {
                let Some(panel) = app.language.as_mut().filter(|p| p.visible) else {
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
                let hit = panel.model.hit_test(x, y);
                if hit != panel.model.hovered {
                    panel.model.hovered = hit;
                    panel.render();
                }
            });
        }
        WM_MOUSELEAVE => {
            with_app(|app| {
                if let Some(panel) = app.language.as_mut().filter(|p| p.visible) {
                    if panel.model.hovered.take().is_some() {
                        panel.render();
                    }
                }
            });
        }
        WM_LBUTTONUP => {
            with_app(|app| {
                let Some(panel) = app.language.as_mut().filter(|p| p.visible) else {
                    return;
                };
                let (x, y) = point(panel.scale);
                match panel.model.hit_test(x, y) {
                    Some(language::Hit::Layout(index)) => choose_language(app, index),
                    Some(language::Hit::Settings) => {
                        panel.hide();
                        let _ = std::process::Command::new("explorer.exe")
                            .arg("ms-settings:keyboard")
                            .spawn();
                    }
                    None => {}
                }
            });
        }
        // Escape cierra; las flechas recorren los teclados y Enter elige.
        WM_KEYDOWN => {
            with_app(|app| {
                let Some(panel) = app.language.as_mut().filter(|p| p.visible) else {
                    return;
                };
                match wparam.0 {
                    0x1B => panel.hide(),
                    0x26 | 0x28 => {
                        panel
                            .model
                            .move_selection(if wparam.0 == 0x26 { -1 } else { 1 });
                        panel.render();
                    }
                    0x25 | 0x27 => app.step_menu(if wparam.0 == 0x25 { -1 } else { 1 }),
                    0x0D => {
                        if let Some(language::Hit::Layout(index)) = panel.model.hovered {
                            choose_language(app, index);
                        }
                    }
                    _ => {}
                }
            });
        }
        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
    }
    LRESULT(0)
}

/// Avisos de Windows mostrados como tarjetas propias (ver `notifications`): el hilo
/// que los lee y la ventana que los dibuja.
struct Notifications {
    watcher: notifications::Watcher,
    window: NotificationWindow,
    /// Historial de los avisos (ver `notifcenter`), en el panel de la hora.
    center: notifcenter::Center,
    /// Los últimos avisos en pantalla, ya sin los de apps silenciadas: al quitar No
    /// molestar vuelven a salir como tarjetas.
    last: Vec<notifications::Toast>,
    /// No molestar está puesto (copia del ajuste, para dibujar su botón).
    dnd: bool,
}
impl Notifications {
    fn start(owner: HWND) -> Option<Self> {
        let window = NotificationWindow::new(owner).ok()?;
        let watcher = notifications::Watcher::start(owner, WM_NOTIFICATIONS)?;
        notifications::warm_icons();
        Some(Self {
            watcher,
            window,
            center: notifcenter::Center::default(),
            last: Vec::new(),
            dnd: false,
        })
    }

    /// Lee lo que mandó el hilo de los avisos. Los de las apps silenciadas no salen;
    /// con No molestar, solo van al centro de notificaciones. Devuelve si cambió
    /// algo del centro (un aviso nuevo o un icono).
    fn handle_events(&mut self, styles: &style::Styles) -> bool {
        let mut latest = None;
        let mut changed = false;
        for event in self.watcher.events() {
            match event {
                notifications::Event::Toasts(toasts) => latest = Some(toasts),
                notifications::Event::Icon(sender, icon) => {
                    self.window.stack.set_icon(sender, icon);
                    changed = true;
                }
            }
        }
        let mut center = changed;
        if let Some(mut toasts) = latest {
            toasts.retain(|t| !styles.is_muted(&t.sender));
            for sender in self.center.record(&toasts) {
                // Con No molestar no hay tarjeta que pida el icono.
                if !self.window.stack.has_icon(&sender) {
                    self.window.stack.mark_icon_pending(&sender);
                    self.watcher.find_icon(sender);
                }
                center = true;
            }
            self.last = toasts;
            self.show_cards(styles);
            changed = true;
        }
        if changed {
            self.window.refresh();
        }
        center
    }

    /// Pone las tarjetas al día con los últimos avisos (ninguna con No molestar).
    fn show_cards(&mut self, styles: &style::Styles) {
        self.dnd = styles.do_not_disturb();
        let toasts = if self.dnd {
            Vec::new()
        } else {
            self.last.clone()
        };
        for sender in self.window.stack.update(toasts) {
            self.window.stack.mark_icon_pending(&sender);
            self.watcher.find_icon(sender);
        }
        self.window.refresh();
    }

    /// Cierra en Windows (y en las tarjetas) los avisos de `keys`, que se quitaron
    /// del centro de notificaciones.
    fn dismiss(&mut self, keys: Vec<notifications::Key>) {
        if keys.is_empty() {
            return;
        }
        for key in keys {
            self.window.stack.take(&key);
            self.watcher.send(notifications::Command::Dismiss(key));
        }
        self.window.refresh();
    }

    /// Se pulsó un aviso del centro: si Windows aún lo enseña, se pulsa allí;
    /// si no, se abre su app. En los dos casos sale del historial.
    fn open_entry(&mut self, id: u64) {
        let Some(entry) = self.center.entry(id) else {
            return;
        };
        let (key, sender) = (entry.key.clone(), entry.toast.sender.clone());
        let live = self.center.is_live(&key);
        self.center.remove(id);
        if live {
            self.window.stack.take(&key);
            self.watcher.send(notifications::Command::Open(key));
            self.window.refresh();
        } else {
            notifications::open_app(&sender);
        }
    }

    /// Clic en una tarjeta: se pulsa lo mismo en el aviso de Windows. Con la
    /// campana se cierran todos los avisos de esa app y se devuelve su nombre, para
    /// silenciarla, y si también se apagó en Windows.
    fn click(&mut self, x: f32, y: f32) -> Option<(String, bool)> {
        let (key, part) = self.window.stack.hit_test(x, y)?;
        let mut muted = None;
        if part == notifications::Part::Mute {
            if let Some(toast) = self.window.stack.toast(&key) {
                let sender = toast.sender.clone();
                // Lo que llega del móvil lo manda Enlace Móvil: apagarlo en Windows
                // callaría todo el móvil.
                let in_windows =
                    !toast.is_from_phone() && notifications::disable_in_windows(&sender);
                for key in self.window.stack.take_sender(&sender) {
                    self.watcher.send(notifications::Command::Dismiss(key));
                }
                muted = Some((sender, in_windows));
            }
        } else if self.window.stack.take(&key) {
            // Pulsado o con un botón, ya está atendido: sale del historial.
            if matches!(
                part,
                notifications::Part::Body | notifications::Part::Action(_)
            ) {
                self.center.forget(&key);
            }
            self.watcher.send(match part {
                notifications::Part::Body => notifications::Command::Open(key),
                notifications::Part::Close | notifications::Part::Mute => {
                    notifications::Command::Dismiss(key)
                }
                notifications::Part::Action(n) => notifications::Command::Action(key, n),
            });
        }
        self.window.stack.hovered = None;
        self.window.refresh();
        muted
    }
}

/// Ventana transparente arriba a la derecha donde se dibujan las tarjetas. No toma
/// el foco: aparece sin quitárselo a la ventana en la que se está escribiendo.
struct NotificationWindow {
    hwnd: HWND,
    backdrop: Option<style::Backdrop>,
    surface: Option<Surface>,
    pos: POINT,
    scale: f32,
    stack: notifications::Stack,
    last_frame: Instant,
    shown: bool,
    /// Borde inferior de la barra superior, si está: las tarjetas salen debajo.
    below: Option<i32>,
}
impl NotificationWindow {
    fn new(owner: HWND) -> Result<Self> {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("GlassBarNotifications"),
                w!("GlassBar — Notificaciones"),
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
            backdrop: style::Backdrop::new(),
            surface: None,
            pos: POINT::default(),
            scale: 1.0,
            stack: notifications::Stack::default(),
            last_frame: Instant::now(),
            shown: false,
            below: None,
        })
    }

    /// En la esquina de arriba a la derecha del monitor principal, bajo la barra
    /// superior si está.
    fn place(&mut self) {
        let monitor = monitors::primary();
        let (_, dpi) = monitors::get_info(monitor);
        let work = monitors::work_area(monitor);
        self.scale = dpi.max(0.3);
        let width = (notifications::WIDTH * self.scale).ceil() as u32;
        let height = (notifications::max_height() * self.scale).ceil() as u32;
        if !self
            .surface
            .as_ref()
            .is_some_and(|s| s.width == width && s.height == height)
        {
            self.surface = Surface::new(width, height).ok();
        }
        self.pos = POINT {
            x: work.right - width as i32,
            y: work.top.max(self.below.unwrap_or(work.top)),
        };
    }

    /// Algo cambió: se redibuja y se anima hasta que todo quede quieto.
    fn refresh(&mut self) {
        if !self.shown {
            self.place();
        }
        self.last_frame = Instant::now();
        frames::start(self.hwnd);
        self.frame();
    }

    fn frame(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;
        if !self.stack.step(dt) {
            frames::stop(self.hwnd);
        }
        if self.stack.is_empty() {
            self.hide();
            return;
        }
        self.render();
    }

    fn render(&mut self) {
        let Some(surface) = &mut self.surface else {
            return;
        };
        if let Some(pixmap) = surface.pixmap() {
            self.stack.draw(pixmap, self.scale);
        }
        surface.copy_to_dib();
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
                    cx: surface.width as i32,
                    cy: surface.height as i32,
                }),
                Some(surface.dc()),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            if !self.shown {
                use windows::Win32::UI::WindowsAndMessaging::{
                    SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
                };
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                );
                self.shown = true;
            }
        }
        if let Some(backdrop) = &self.backdrop {
            let shapes = self.stack.shapes(self.scale);
            backdrop.under(self.hwnd, surface_rect(self.pos, surface), &shapes, 1.0);
        }
    }

    fn hide(&mut self) {
        frames::stop(self.hwnd);
        if !self.shown {
            return;
        }
        self.shown = false;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        if let Some(backdrop) = &self.backdrop {
            backdrop.hide();
        }
        if let Some(surface) = &mut self.surface {
            surface.release();
        }
    }
}
impl Drop for NotificationWindow {
    fn drop(&mut self) {
        frames::stop(self.hwnd);
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn notifications_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let point = |scale: f32| {
        (
            (lparam.0 & 0xffff) as i16 as f32 / scale,
            ((lparam.0 >> 16) & 0xffff) as i16 as f32 / scale,
        )
    };
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_TIMER if wparam.0 == TIMER_ANIM => {
            with_app(|app| {
                if let Some(n) = &mut app.notifications {
                    n.window.frame();
                }
            });
        }
        WM_MOUSEMOVE => {
            with_app(|app| {
                let Some(n) = &mut app.notifications else {
                    return;
                };
                let mut track = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut track);
                let (x, y) = point(n.window.scale);
                let hit = n.window.stack.hit_test(x, y);
                if hit != n.window.stack.hovered {
                    n.window.stack.hovered = hit;
                    n.window.refresh();
                }
            });
        }
        WM_MOUSELEAVE => {
            with_app(|app| {
                if let Some(n) = &mut app.notifications {
                    if n.window.stack.mouse_left() {
                        n.window.refresh();
                    }
                }
            });
        }
        WM_LBUTTONUP => {
            with_app(|app| {
                let Some(n) = &mut app.notifications else {
                    return;
                };
                let (x, y) = point(n.window.scale);
                if let Some((sender, in_windows)) = n.click(x, y) {
                    app.styles.mute_app(&sender, in_windows);
                    let _ = app.styles.save();
                    app.refresh_settings();
                }
                // Una tarjeta pulsada sale también del historial.
                app.sync_center();
            });
        }
        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
    }
    LRESULT(0)
}

/// Panel de la hora: el calendario y, con las notificaciones de GlassBar, el
/// centro de notificaciones al otro lado (encima con el dock, debajo con la barra
/// superior), como dos paneles de cristal en la misma ventana.
struct CalendarWindow {
    hwnd: HWND,
    backdrop: Option<style::Backdrop>,
    surface: Option<Surface>,
    pos: POINT,
    scale: f32,
    model: calendar::CalendarState,
    visible: bool,
    transition: launcher::Transition,
    /// Sobre qué se abrió: la hora, el borde de la barra y los límites a los lados.
    anchor: Option<(i32, Edge, (i32, i32))>,
    /// Alto del centro de notificaciones (0 sin él) y borde superior de cada
    /// panel, en unidades lógicas.
    center_h: f32,
    center_top: f32,
    calendar_top: f32,
}

/// Separación entre el centro de notificaciones y el calendario.
const CENTER_GAP: f32 = 12.0;

/// Parte del panel de la hora bajo el cursor.
#[derive(Clone, Debug, PartialEq)]
enum ClockPanelHit {
    Calendar(calendar::Hit),
    Center(notifcenter::Hit),
}

impl CalendarWindow {
    fn new(owner: HWND) -> Result<Self> {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                w!("GlassBarCalendar"),
                w!("GlassBar — Calendario"),
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
            backdrop: style::Backdrop::new(),
            surface: None,
            pos: POINT::default(),
            scale: 1.0,
            model: calendar::CalendarState::new(calendar::today(), calendar::Names::default()),
            visible: false,
            transition: launcher::Transition::hidden(),
            anchor: None,
            center_h: 0.0,
            center_top: 0.0,
            calendar_top: 0.0,
        })
    }
    /// Lo abre sobre `anchor_x`, junto a la barra, en el mes de hoy; con `n`, con
    /// el centro de notificaciones.
    fn show(
        &mut self,
        anchor_x: i32,
        edge: Edge,
        bounds: (i32, i32),
        mut n: Option<&mut Notifications>,
    ) {
        let today = calendar::today();
        self.model = calendar::CalendarState::new(today, calendar::names(today));
        self.anchor = Some((anchor_x, edge, bounds));
        if !self.layout(n.as_deref_mut()) {
            return;
        }
        self.visible = true;
        self.transition.open(Instant::now());
        self.render(n.as_deref());
        take_focus(self.hwnd);
    }
    /// Calcula el tamaño y el sitio de la ventana (el centro de notificaciones
    /// crece o mengua con sus avisos) y prepara la superficie. Devuelve si la hay.
    fn layout(&mut self, n: Option<&mut Notifications>) -> bool {
        let Some((anchor_x, edge, (left, right))) = self.anchor else {
            return false;
        };
        let (_, dpi) = monitors::get_info(edge.monitor);
        self.scale = dpi.max(0.3);
        let margin = (12.0 * self.scale).round() as i32;
        self.center_h = match n {
            Some(n) => {
                // El sitio que queda entre la barra y el otro borde de la pantalla.
                let work = monitors::work_area(edge.monitor);
                let free = if edge.below {
                    work.bottom - edge.y
                } else {
                    edge.y - work.top
                };
                let room = (free - margin * 2) as f32 / self.scale - calendar::HEIGHT - CENTER_GAP;
                n.center.fit(room)
            }
            None => 0.0,
        };
        let extra = if self.center_h > 0.0 {
            self.center_h + CENTER_GAP
        } else {
            0.0
        };
        // El calendario, junto a la barra; el centro, al otro lado.
        (self.calendar_top, self.center_top) = if edge.below {
            (0.0, calendar::HEIGHT + CENTER_GAP)
        } else {
            (extra, 0.0)
        };
        let width = (calendar::WIDTH * self.scale).ceil() as u32;
        let height = ((calendar::HEIGHT + extra) * self.scale).ceil() as u32;
        if !self
            .surface
            .as_ref()
            .is_some_and(|s| s.width == width && s.height == height)
        {
            match Surface::new(width, height) {
                Ok(surface) => self.surface = Some(surface),
                Err(_) => return false,
            }
        }
        self.pos = POINT {
            x: (anchor_x - width as i32 / 2).clamp(
                left + margin,
                (right - width as i32 - margin).max(left + margin),
            ),
            y: edge.panel_top(height as i32, margin),
        };
        true
    }
    /// Lo vuelve a colocar y a dibujar tras un cambio en el centro de notificaciones.
    fn refresh(&mut self, mut n: Option<&mut Notifications>) {
        if self.visible && self.layout(n.as_deref_mut()) {
            self.render(n.as_deref());
        }
    }
    fn render(&mut self, n: Option<&Notifications>) {
        let scale = self.scale;
        let Some(surface) = &mut self.surface else {
            return;
        };
        if let Some(pixmap) = surface.pixmap() {
            pixmap.fill(tiny_skia::Color::TRANSPARENT);
            let width = (calendar::WIDTH * scale).ceil() as u32;
            let mut parts = Vec::new();
            if let Some(mut part) = Pixmap::new(width, (calendar::HEIGHT * scale).ceil() as u32) {
                self.model.draw(&mut part, scale);
                parts.push((part, self.calendar_top));
            }
            if let Some(n) = n.filter(|_| self.center_h > 0.0) {
                if let Some(mut part) = Pixmap::new(width, (self.center_h * scale).ceil() as u32) {
                    n.center.draw(&mut part, scale, &n.window.stack, n.dnd);
                    parts.push((part, self.center_top));
                }
            }
            for (part, top) in parts {
                pixmap.draw_pixmap(
                    0,
                    (top * scale).round() as i32,
                    part.as_ref(),
                    &tiny_skia::PixmapPaint::default(),
                    tiny_skia::Transform::identity(),
                    None,
                );
            }
        }
        surface.copy_to_dib();
        self.present();
    }
    /// Qué hay en `(x, y)` (unidades lógicas de la ventana).
    fn hit_test(&self, x: f32, y: f32, n: Option<&Notifications>) -> Option<ClockPanelHit> {
        if let Some(n) = n.filter(|_| self.over_center(y)) {
            return n
                .center
                .hit_test(x, y - self.center_top)
                .map(ClockPanelHit::Center);
        }
        self.model
            .hit_test(x, y - self.calendar_top)
            .map(ClockPanelHit::Calendar)
    }
    /// `y` (unidades lógicas de la ventana) cae en el centro de notificaciones.
    fn over_center(&self, y: f32) -> bool {
        self.center_h > 0.0 && (self.center_top..self.center_top + self.center_h).contains(&y)
    }
    fn present(&mut self) {
        if let Some(surface) = &mut self.surface {
            let s = self.scale;
            let shape = |top: f32, height: f32| style::Shape {
                x: s,
                y: (top + 1.0) * s,
                width: (calendar::WIDTH - 2.0) * s,
                height: (height - 2.0) * s,
                radius: 12.0 * s,
            };
            let mut shapes = vec![shape(self.calendar_top, calendar::HEIGHT)];
            if self.center_h > 0.0 {
                shapes.push(shape(self.center_top, self.center_h));
            }
            present_popup(
                self.hwnd,
                surface,
                self.pos,
                self.scale,
                &self.transition,
                self.visible,
                false,
                self.backdrop.as_ref(),
                &shapes,
            );
        }
    }
    fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.model.hovered = None;
        self.transition.close(Instant::now());
        self.present();
    }
    /// Muestra otro mes.
    fn shift(&mut self, delta: i32, n: Option<&Notifications>) {
        self.model.shift(delta);
        self.render(n);
    }
}
impl Drop for CalendarWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn calendar_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        WA_INACTIVE, WM_ACTIVATE, WM_KEYDOWN, WM_MOUSEWHEEL,
    };
    let point = |scale: f32| {
        (
            (lparam.0 & 0xffff) as i16 as f32 / scale,
            ((lparam.0 >> 16) & 0xffff) as i16 as f32 / scale,
        )
    };
    match msg {
        WM_ACTIVATE if (wparam.0 & 0xffff) as u32 == WA_INACTIVE => {
            with_app(|app| {
                if let Some(panel) = &mut app.calendar {
                    panel.hide();
                }
                if let Some(n) = &mut app.notifications {
                    n.center.hovered = None;
                }
            });
        }
        WM_TIMER if wparam.0 == TIMER_ANIM => {
            with_app(|app| app.calendar.as_mut().map(CalendarWindow::present));
        }
        WM_MOUSEMOVE => {
            with_app(|app| {
                let Some(panel) = app.calendar.as_mut().filter(|p| p.visible) else {
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
                let (on_calendar, on_center) =
                    match panel.hit_test(x, y, app.notifications.as_ref()) {
                        Some(ClockPanelHit::Calendar(hit)) => (Some(hit), None),
                        Some(ClockPanelHit::Center(hit)) => (None, Some(hit)),
                        None => (None, None),
                    };
                let center_changed = app
                    .notifications
                    .as_ref()
                    .is_some_and(|n| n.center.hovered != on_center);
                if on_calendar != panel.model.hovered || center_changed {
                    panel.model.hovered = on_calendar;
                    if let Some(n) = &mut app.notifications {
                        n.center.hovered = on_center;
                    }
                    panel.render(app.notifications.as_ref());
                }
            });
        }
        WM_MOUSELEAVE => {
            with_app(|app| {
                if let Some(panel) = app.calendar.as_mut().filter(|p| p.visible) {
                    let center = app
                        .notifications
                        .as_mut()
                        .is_some_and(|n| n.center.hovered.take().is_some());
                    if panel.model.hovered.take().is_some() || center {
                        panel.render(app.notifications.as_ref());
                    }
                }
            });
        }
        // La rueda desplaza la lista de avisos sobre el centro de notificaciones y,
        // sobre el calendario, recorre los meses, como en el de Windows.
        WM_MOUSEWHEEL => {
            with_app(|app| {
                if let Some(panel) = app.calendar.as_mut().filter(|p| p.visible) {
                    let delta = ((wparam.0 >> 16) & 0xffff) as i16;
                    // La posición llega en coordenadas de pantalla.
                    let screen_y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
                    let y = (screen_y - panel.pos.y) as f32 / panel.scale;
                    if panel.over_center(y) {
                        if let Some(n) = &mut app.notifications {
                            let step = notifcenter::WHEEL_STEP * f32::from(delta).abs() / 120.0;
                            if n.center.scroll_by(if delta > 0 { -step } else { step }) {
                                panel.render(Some(n));
                            }
                        }
                    } else {
                        panel.shift(if delta > 0 { -1 } else { 1 }, app.notifications.as_ref());
                    }
                }
            });
        }
        WM_LBUTTONUP => {
            with_app(|app| {
                let Some(panel) = app.calendar.as_mut().filter(|p| p.visible) else {
                    return;
                };
                let (x, y) = point(panel.scale);
                let n = app.notifications.as_ref();
                match panel.hit_test(x, y, n) {
                    Some(ClockPanelHit::Calendar(hit)) => match hit {
                        calendar::Hit::Previous => panel.shift(-1, n),
                        calendar::Hit::Next => panel.shift(1, n),
                        calendar::Hit::Today => {
                            if panel.model.go_today() {
                                panel.render(n);
                            }
                        }
                        calendar::Hit::Day(index) => {
                            if panel.model.choose_day(index) {
                                panel.render(n);
                            }
                        }
                        calendar::Hit::Settings => {
                            panel.hide();
                            let _ = std::process::Command::new("explorer.exe")
                                .arg("ms-settings:dateandtime")
                                .spawn();
                        }
                    },
                    Some(ClockPanelHit::Center(hit)) => app.center_click(hit),
                    None => {}
                }
            });
        }
        // Escape cierra; flechas y RePág/AvPág cambian de mes; Inicio vuelve a hoy.
        WM_KEYDOWN => {
            with_app(|app| {
                let Some(panel) = app.calendar.as_mut().filter(|p| p.visible) else {
                    return;
                };
                let n = app.notifications.as_ref();
                match wparam.0 {
                    0x1B => panel.hide(),
                    // Arriba y abajo cambian de mes; izquierda y derecha, de panel.
                    0x21 | 0x26 => panel.shift(-1, n),
                    0x22 | 0x28 => panel.shift(1, n),
                    0x25 | 0x27 => app.step_menu(if wparam.0 == 0x25 { -1 } else { 1 }),
                    0x24 if panel.model.go_today() => {
                        panel.render(n);
                    }
                    _ => {}
                }
            });
        }
        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
    }
    LRESULT(0)
}

/// Mensaje "TaskbarCreated" que se anuncia a todos al crearse la barra.
fn taskbar_created() -> u32 {
    static MESSAGE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe {
        windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW(w!("TaskbarCreated"))
    })
}

/// Hay un menú emergente abierto (clase `#32768`), de cualquier programa.
/// Distancia máxima (en píxeles lógicos) entre el punto del clic derecho en un icono
/// y el menú que abre su programa.
const MENU_REACH: f32 = 48.0;

/// Hay un menú de Windows (#32768) a la vista. Windows guarda también ventanas de
/// menú ocultas: se miran todas, no solo la primera que aparece.
fn menu_open() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, IsWindowVisible};
    let mut after = None;
    // Pocas: una por menú o submenú abierto, más las que Windows tiene guardadas.
    for _ in 0..32 {
        let Ok(menu) = (unsafe { FindWindowExW(None, after, w!("#32768"), None) }) else {
            return false;
        };
        if unsafe { IsWindowVisible(menu) }.as_bool() {
            return true;
        }
        after = Some(menu);
    }
    false
}

/// El menú de un icono del panel, abierto con clic derecho.
struct TrayMenu {
    /// Cuándo se pulsó, y si ya se vio el menú abierto.
    since: Instant,
    seen: bool,
    /// Proceso del programa del icono, y punto de la pantalla donde se pulsó: su
    /// menú es una ventana suya (o de un proceso que abrió) junto a ese punto.
    pid: Option<u32>,
    at: POINT,
    /// Procesos ajenos ya vistos junto al punto, y si son del programa.
    known: std::collections::HashMap<u32, bool>,
}

/// Panel de las apps en segundo plano (ver `traypanel` y `systray`).
struct TrayWindow {
    hwnd: HWND,
    /// Desenfoque de debajo, como el del dock.
    backdrop: Option<style::Backdrop>,
    surface: Option<Surface>,
    pos: POINT,
    scale: f32,
    model: traypanel::TrayPanel,
    visible: bool,
    transition: launcher::Transition,
    /// Dónde abrirlo: centro de la flecha, borde superior del dock y bordes de la pantalla.
    anchor: (i32, Edge, (i32, i32)),
    /// Clic derecho en un icono, mientras su menú sigue abierto (o por abrirse): el
    /// panel sigue abierto aunque el menú se lleve el foco.
    menu: Option<TrayMenu>,
    /// Tras un clic izquierdo el panel espera el tiempo de doble clic de Windows
    /// antes de cerrarse: si llega el segundo clic, se le manda al programa como
    /// doble clic (algunos solo abren su ventana así).
    close_after: Option<Instant>,
    /// Icono al que se avisó de que el cursor se quedó encima (`NIN_POPUPOPEN`):
    /// hay que avisarle también cuando se va.
    popup: Option<systray::Key>,
}
impl TrayWindow {
    fn new(owner: HWND) -> Result<Self> {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                w!("GlassBarTray"),
                w!("GlassBar — Apps en segundo plano"),
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
            backdrop: style::Backdrop::new(),
            surface: None,
            pos: POINT::default(),
            scale: 1.0,
            model: traypanel::TrayPanel::default(),
            visible: false,
            transition: launcher::Transition::hidden(),
            anchor: (0, Edge::default(), (0, 0)),
            menu: None,
            close_after: None,
            popup: None,
        })
    }
    /// Avisa al icono con el cursor encima de que se fue (si se le avisó al llegar).
    fn end_popup(&mut self) {
        if let Some(key) = self.popup.take() {
            systray::popup_close(key, cursor_pos());
        }
    }
    fn show(&mut self, anchor_x: i32, edge: Edge, bounds: (i32, i32)) {
        self.anchor = (anchor_x, edge, bounds);
        let (_, dpi) = monitors::get_info(edge.monitor);
        self.scale = dpi.max(0.3);
        self.visible = true;
        self.transition.open(Instant::now());
        self.refresh();
        take_focus(self.hwnd);
    }
    /// Vuelve a leer los iconos (al abrirlo, o si un programa cambió el suyo) y
    /// ajusta el tamaño a cuántos hay.
    fn refresh(&mut self) {
        // Mientras se arrastra un icono la lista no cambia; al soltarlo se relee.
        if self.model.dragging() {
            return;
        }
        self.model.drag = None;
        self.model.icons = systray::shown()
            .into_iter()
            .map(|icon| traypanel::TrayIcon {
                key: icon.key,
                id: icon.id,
                image: icon.image,
                tip: icon.tip,
            })
            .collect();
        traypanel::sort_icons(&mut self.model.icons, &traypanel::load_order());
        if self
            .model
            .hovered
            .is_some_and(|i| i >= self.model.icons.len())
        {
            self.model.hovered = None;
        }
        let (w, h) = self.model.size();
        let (width, height) = (
            (w * self.scale).ceil() as u32,
            (h * self.scale).ceil() as u32,
        );
        if self
            .surface
            .as_ref()
            .is_none_or(|s| s.width != width || s.height != height)
        {
            match Surface::new(width, height) {
                Ok(surface) => self.surface = Some(surface),
                Err(_) => return,
            }
        }
        let (anchor_x, edge, (left, right)) = self.anchor;
        let margin = (12.0 * self.scale).round() as i32;
        self.pos = POINT {
            x: (anchor_x - width as i32 / 2).clamp(
                left + margin,
                (right - width as i32 - margin).max(left + margin),
            ),
            y: edge.panel_top(height as i32, margin),
        };
        self.render();
    }
    fn render(&mut self) {
        if let Some(surface) = &mut self.surface {
            if let Some(pixmap) = surface.pixmap() {
                self.model.draw(pixmap, self.scale);
            }
            surface.copy_to_dib();
        }
        self.present();
    }
    fn present(&mut self) {
        if let Some(surface) = &mut self.surface {
            let shapes = [panel_shape(surface, self.scale, 10.0)];
            present_popup(
                self.hwnd,
                surface,
                self.pos,
                self.scale,
                &self.transition,
                self.visible,
                false,
                self.backdrop.as_ref(),
                &shapes,
            );
        }
    }
    fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.menu = None;
        self.close_after = None;
        self.end_popup();
        self.model.hovered = None;
        self.model.drag = None;
        self.transition.close(Instant::now());
        self.present();
    }
    /// Se mira cada 40 ms con el panel abierto. Cierra el panel cuando pasó el
    /// tiempo de doble clic tras un clic izquierdo, y sigue el menú abierto con clic
    /// derecho: al cerrarse, si el programa abrió una ventana o se pulsó fuera, el
    /// panel se cierra; si no (Escape, o de vuelta al panel), recupera el foco para
    /// cerrarse como siempre.
    ///
    /// `own_menu` si está abierto el menú propio de GlassBar: es el de su icono, que
    /// también está en el panel. `dock` es la ventana del dock.
    fn watch_menu(&mut self, own_menu: bool, dock: HWND) {
        if self.close_after.is_some_and(|at| Instant::now() >= at) {
            self.hide();
            return;
        }
        let reach = (MENU_REACH * self.scale).round() as i32;
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        // El menú de Windows (#32768), el de GlassBar (no se le puede quitar el foco:
        // se cerraría a medias), o uno propio del programa (Qt, Java, Steam, XAML...),
        // que no es de esa clase. Si no se reconoce pero el programa sigue con el
        // foco en una ventana oculta o flotante (la de su menú), también cuenta:
        // quitárselo cerraría el menú.
        let open = own_menu
            || menu_open()
            || menu.pid.is_some_and(|pid| {
                systray::menu_of(pid, menu.at, reach, &mut menu.known)
                    || systray::menu_has_focus(pid, &mut menu.known)
            });
        if open {
            menu.seen = true;
            return;
        }
        // Algunos programas tardan en abrirlo; si no llega a abrirse, se deja de esperar.
        if !menu.seen && menu.since.elapsed() < Duration::from_millis(1500) {
            return;
        }
        self.menu = None;
        // Otra ventana delante, también una de GlassBar que abrió su menú (la
        // configuración): no se le quita el foco.
        let elsewhere = unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindowVisible};
            let front = GetForegroundWindow();
            front != self.hwnd && front != dock && IsWindowVisible(front).as_bool()
        };
        if elsewhere {
            self.hide();
        } else {
            // Sin compartir la entrada con la ventana activa (como `take_focus`):
            // suele ser del programa, que puede seguir ocupado con su menú y dejaría
            // a GlassBar sin responder. Si Windows niega el foco, el panel se cierra
            // igual al pulsar fuera.
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(self.hwnd);
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(self.hwnd));
            }
        }
    }
}
/// La app sobre la que se abrió el menú: quién es (se vuelve a buscar al elegir,
/// porque el dock puede cambiar mientras el menú está abierto), cómo se abre y su
/// jump list.
struct MenuApp {
    command: String,
    pinned: bool,
    launch: String,
    jump: Vec<jumplist::Section>,
}

/// De qué es el menú abierto.
enum MenuPurpose {
    /// El de una app del dock.
    App(MenuApp),
    /// El del icono de GlassBar en la bandeja: configuración y salir.
    GlassBar,
    /// El del botón de inicio, con las opciones del de Windows (Win+X).
    Windows,
}

/// Menú propio (ver `appmenu`), con el estilo de Windows 11: el de cada app del
/// dock y el de GlassBar.
struct AppMenuWindow {
    hwnd: HWND,
    /// Desenfoque de debajo, como el del dock.
    backdrop: Option<style::Backdrop>,
    surface: Option<Surface>,
    pos: POINT,
    scale: f32,
    model: appmenu::Cascade,
    visible: bool,
    transition: launcher::Transition,
    purpose: Option<MenuPurpose>,
    /// Barra que se está arrastrando; el ratón queda capturado hasta soltarla.
    sliding: Option<appmenu::Hit>,
    /// Iconos ya reducidos al tamaño del menú, por archivo o tipo de archivo: así
    /// las siguientes aperturas no los vuelven a leer.
    icons: std::collections::HashMap<String, Option<Pixmap>>,
    icons_scale: f32,
}
impl AppMenuWindow {
    fn new(owner: HWND) -> Result<Self> {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                w!("GlassBarAppMenu"),
                w!("GlassBar — Menú de la app"),
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
            backdrop: style::Backdrop::new(),
            surface: None,
            pos: POINT::default(),
            scale: 1.0,
            model: appmenu::Cascade::default(),
            visible: false,
            transition: launcher::Transition::hidden(),
            purpose: None,
            sliding: None,
            icons: std::collections::HashMap::new(),
            icons_scale: 0.0,
        })
    }

    /// Icono del tamaño del menú, leído una sola vez por `key`.
    fn icon(&mut self, key: String, load: impl FnOnce() -> Option<Pixmap>) -> Option<Pixmap> {
        let side = (appmenu::ICON * self.scale).round().max(1.0) as u32;
        self.icons
            .entry(key)
            .or_insert_with(|| load().map(|icon| apps::downscale(&icon, side)))
            .clone()
    }

    /// Icono de un elemento de la jump list: el suyo propio; si no, el de su
    /// archivo. Los archivos comparten el de su tipo; las carpetas, los programas y
    /// los accesos directos tienen el suyo.
    fn jump_icon(&mut self, item: &jumplist::JumpItem) -> Option<Pixmap> {
        let (path, index) = item.icon_source();
        let key = match (&item.icon, &item.action) {
            (None, jumplist::Action::Open(file)) => {
                let file = std::path::Path::new(file);
                match file.extension().map(|e| e.to_string_lossy().to_lowercase()) {
                    Some(ext)
                        if !file.is_dir()
                            && !matches!(ext.as_str(), "exe" | "lnk" | "ico" | "url") =>
                    {
                        format!("*.{ext}")
                    }
                    _ => path.to_lowercase(),
                }
            }
            _ => format!("{},{index}", path.to_lowercase()),
        };
        self.icon(key, || icons::load_resource(&path, index))
    }

    /// Abre el menú de la app sobre su icono (`anchor_x`), encima del dock.
    #[allow(clippy::too_many_arguments)]
    fn open(
        &mut self,
        name: &str,
        app_icon: Option<&Pixmap>,
        windows: usize,
        end_task: bool,
        app: MenuApp,
        anchor_x: i32,
        dock_top: i32,
        (left, right): (i32, i32),
    ) {
        use appmenu::{Entry, Icon};
        let (_, dpi) = monitors::get_info(monitors::dock());
        self.scale = dpi.max(0.3);
        if self.icons_scale != self.scale {
            self.icons.clear();
            self.icons_scale = self.scale;
        }
        let mut entries = Vec::new();
        let mut next = CMD_JUMP;
        let jump = app.jump.clone();
        for section in &jump {
            entries.push(Entry::Header(section.title.clone()));
            for item in &section.items {
                entries.push(Entry::Item {
                    icon: Icon::Image(self.jump_icon(item)),
                    label: item.name.clone(),
                    command: next,
                });
                next += 1;
            }
        }
        if !entries.is_empty() {
            entries.push(Entry::Separator);
        }
        let side = (appmenu::ICON * self.scale).round().max(1.0) as u32;
        entries.push(Entry::Item {
            icon: Icon::Image(app_icon.map(|icon| apps::downscale(icon, side))),
            label: name.to_string(),
            command: CMD_APP_OPEN,
        });
        entries.push(if app.pinned {
            Entry::Item {
                icon: Icon::Glyph(appmenu::GLYPH_UNPIN),
                label: "Desanclar de la barra de tareas".into(),
                command: CMD_APP_UNPIN,
            }
        } else {
            Entry::Item {
                icon: Icon::Glyph(appmenu::GLYPH_PIN),
                label: "Anclar a la barra de tareas".into(),
                command: CMD_APP_PIN,
            }
        });
        if windows > 0 {
            // Como en Windows 11, solo si está activado en sus ajustes.
            if end_task {
                entries.push(Entry::Item {
                    icon: Icon::Glyph(appmenu::GLYPH_END_TASK),
                    label: "Finalizar tarea".into(),
                    command: CMD_APP_END_TASK,
                });
            }
            entries.push(Entry::Item {
                icon: Icon::Glyph(appmenu::GLYPH_CLOSE),
                label: if windows == 1 {
                    "Cerrar ventana".into()
                } else {
                    "Cerrar todas las ventanas".into()
                },
                command: CMD_APP_CLOSE,
            });
        }
        self.model = appmenu::Cascade::new(appmenu::AppMenu::new(entries), false);
        self.purpose = Some(MenuPurpose::App(app));
        let width = (self.model.size().0 * self.scale).ceil() as i32;
        let margin = (12.0 * self.scale).round() as i32;
        self.show(anchor_x - width / 2, dock_top - margin, (left, right));
    }

    /// Abre el menú de GlassBar con su esquina inferior izquierda en el cursor,
    /// como el de un clic derecho en Windows.
    /// Abre el menú del botón de inicio en el cursor, como el de Windows (Win+X).
    fn open_windows(&mut self, cursor: POINT, bounds: (i32, i32)) {
        self.open_at_cursor(winx::menu(), MenuPurpose::Windows, cursor, bounds);
    }

    /// Abre `root` con su esquina inferior izquierda en el cursor.
    fn open_at_cursor(
        &mut self,
        root: appmenu::AppMenu,
        purpose: MenuPurpose,
        cursor: POINT,
        (left, right): (i32, i32),
    ) {
        let (_, dpi) = monitors::get_info(monitors::dock());
        self.scale = dpi.max(0.3);
        // Los submenús, a la derecha si caben; si no, a la izquierda.
        let wide = appmenu::Cascade::new(root, false);
        let room = cursor.x + (wide.size().0 * self.scale).ceil() as i32 <= right;
        self.model = if room {
            wide
        } else {
            appmenu::Cascade::new(wide.root, true)
        };
        self.purpose = Some(purpose);
        // Con los submenús a la izquierda, el menú principal sigue en el cursor.
        let root_x = (self.model.root_origin().0 * self.scale).round() as i32;
        self.show(cursor.x - root_x, cursor.y, (left, right));
    }

    /// Muestra el menú ya preparado con su esquina superior izquierda en `x` y su
    /// borde inferior en `bottom`, sin salirse de la pantalla.
    fn show(&mut self, x: i32, bottom: i32, (left, right): (i32, i32)) {
        let (w, h) = self.model.size();
        let (width, height) = (
            (w * self.scale).ceil() as u32,
            (h * self.scale).ceil() as u32,
        );
        if self
            .surface
            .as_ref()
            .is_none_or(|s| s.width != width || s.height != height)
        {
            match Surface::new(width, height) {
                Ok(surface) => self.surface = Some(surface),
                Err(_) => return,
            }
        }
        let margin = (12.0 * self.scale).round() as i32;
        self.pos = POINT {
            x: x.clamp(
                left + margin,
                (right - width as i32 - margin).max(left + margin),
            ),
            y: bottom - height as i32,
        };
        // Pulsado en la barra superior no cabe encima: se abre hacia abajo.
        let (screen, _) = monitors::get_info(monitors::at(POINT { x, y: bottom }));
        if self.pos.y < screen.top {
            self.pos.y = bottom;
        }
        self.visible = true;
        self.transition.open(Instant::now());
        self.render();
        take_focus(self.hwnd);
    }
    fn render(&mut self) {
        if let Some(surface) = &mut self.surface {
            if let Some(pixmap) = surface.pixmap() {
                self.model.draw(pixmap, self.scale);
            }
            surface.copy_to_dib();
        }
        self.present();
    }
    fn present(&mut self) {
        if let Some(surface) = &mut self.surface {
            present_popup(
                self.hwnd,
                surface,
                self.pos,
                self.scale,
                &self.transition,
                self.visible,
                false,
                self.backdrop.as_ref(),
                &self.model.shapes(self.scale),
            );
        }
    }
    fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.model.leave();
        self.transition.close(Instant::now());
        self.present();
    }
    /// Cierra el menú y devuelve la orden elegida y de qué menú era.
    fn choose(&mut self, command: u32) -> Option<(u32, MenuPurpose)> {
        self.hide();
        Some((command, self.purpose.take()?))
    }
}
impl Drop for AppMenuWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

/// Lleva a cabo la opción elegida en el menú de una app.
fn run_menu_choice(app: &mut App, command: u32, chosen: MenuApp) {
    if let Some(index) = command.checked_sub(CMD_JUMP) {
        if let Some(entry) = chosen
            .jump
            .iter()
            .flat_map(|s| &s.items)
            .nth(index as usize)
        {
            jumplist::open(entry, &chosen.launch);
        }
        return;
    }
    // La app se busca otra vez: si el dock cambió (se abrió o cerró otra), su
    // posición ya no es la misma.
    let same = |a: &apps::AppItem| a.entry.command.eq_ignore_ascii_case(&chosen.command);
    let target = if chosen.pinned {
        app.bar
            .primary_apps
            .iter()
            .position(same)
            .map(HoverTarget::PrimaryApp)
    } else {
        app.bar
            .open_apps
            .iter()
            .position(same)
            .map(HoverTarget::OpenApp)
    };
    if let Some(target) = target {
        handle_app_command(app, target, command);
    }
}

unsafe extern "system" fn app_menu_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        WA_INACTIVE, WM_ACTIVATE, WM_CAPTURECHANGED, WM_KEYDOWN, WM_LBUTTONDOWN,
    };
    let point = |scale: f32| {
        (
            (lparam.0 & 0xffff) as i16 as f32 / scale,
            ((lparam.0 >> 16) & 0xffff) as i16 as f32 / scale,
        )
    };
    // Primero se cierra el menú y se suelta; luego se actúa, que puede tocar el dock.
    let choose = |command: Option<u32>| {
        let Some(command) = command else {
            return;
        };
        let choice = with_app(|app| {
            app.app_menu
                .as_mut()
                .filter(|m| m.visible)
                .and_then(|m| m.choose(command))
        })
        .flatten();
        match choice {
            Some((command, MenuPurpose::App(chosen))) => {
                with_app(|app| run_menu_choice(app, command, chosen));
            }
            Some((command, MenuPurpose::GlassBar)) => {
                with_app(|app| handle_menu_command(app, command));
            }
            Some((command, MenuPurpose::Windows)) => {
                with_app(|app| match winx::Action::from_command(command) {
                    // Buscar abre el lanzador, que tiene su propio buscador.
                    Some(winx::Action::Search) => app.toggle_launcher(),
                    Some(action) => {
                        action.run();
                    }
                    None => {}
                });
            }
            None => {}
        }
    };
    match msg {
        WM_ACTIVATE if (wparam.0 & 0xffff) as u32 == WA_INACTIVE => {
            with_app(|app| app.app_menu.as_mut().map(AppMenuWindow::hide));
        }
        WM_TIMER if wparam.0 == TIMER_ANIM => {
            with_app(|app| app.app_menu.as_mut().map(AppMenuWindow::present));
        }
        WM_MOUSEMOVE => {
            with_app(|app| {
                let Some(menu) = app.app_menu.as_mut().filter(|m| m.visible) else {
                    return;
                };
                let mut track = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut track);
                let (x, y) = point(menu.scale);
                // Arrastrando una barra, la sigue aunque el cursor salga de su fila.
                if let Some(hit) = menu.sliding {
                    if let Some((command, value)) = menu.model.slide(hit, x) {
                        apply_menu_slider(&mut app.styles, menu, hit, command, value);
                    }
                    return;
                }
                if menu.model.point(x, y) {
                    menu.render();
                }
            });
        }
        WM_MOUSELEAVE => {
            with_app(|app| {
                if let Some(menu) = app.app_menu.as_mut().filter(|m| m.visible) {
                    if menu.sliding.is_none() && menu.model.leave() {
                        menu.render();
                    }
                }
            });
        }
        WM_LBUTTONDOWN => {
            let grabbed = with_app(|app| {
                let menu = app.app_menu.as_mut().filter(|m| m.visible)?;
                let (x, y) = point(menu.scale);
                let (hit, _) = menu.model.slider_at(x, y)?;
                menu.sliding = Some(hit);
                let (command, value) = menu.model.slide(hit, x)?;
                apply_menu_slider(&mut app.styles, menu, hit, command, value);
                Some(())
            })
            .flatten();
            // Fuera de `with_app`: capturar avisa a esta misma ventana.
            if grabbed.is_some() {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd);
            }
        }
        // Al soltar la barra, o si Windows le quita el ratón, se guarda el valor.
        WM_CAPTURECHANGED => {
            with_app(|app| {
                let menu = app.app_menu.as_mut()?;
                menu.sliding.take()?;
                let _ = app.styles.save();
                Some(())
            });
        }
        WM_LBUTTONUP
            if with_app(|app| app.app_menu.as_ref().is_some_and(|m| m.sliding.is_some()))
                == Some(true) =>
        {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
        }
        WM_LBUTTONUP => {
            let command = with_app(|app| {
                let menu = app.app_menu.as_mut().filter(|m| m.visible)?;
                let (x, y) = point(menu.scale);
                let command = menu.model.click(x, y);
                // Un clic en una opción con submenú lo abre.
                menu.render();
                command
            })
            .flatten();
            choose(command);
        }
        WM_KEYDOWN => {
            let key = with_app(|app| {
                let menu = app.app_menu.as_mut().filter(|m| m.visible)?;
                let key = menu.model.key(wparam.0);
                match key {
                    appmenu::Key::Close => menu.hide(),
                    appmenu::Key::Moved => menu.render(),
                    appmenu::Key::Choose(_) => {}
                    appmenu::Key::Slide(command, value) => {
                        let hit = if menu.model.in_sub {
                            menu.model.sub_hovered().map(appmenu::Hit::Sub)
                        } else {
                            menu.model.root.hovered.map(appmenu::Hit::Root)
                        };
                        if let Some(hit) = hit {
                            apply_menu_slider(&mut app.styles, menu, hit, command, value);
                            let _ = app.styles.save();
                        }
                    }
                }
                Some(key)
            })
            .flatten();
            if let Some(appmenu::Key::Choose(command)) = key {
                choose(Some(command));
            }
        }
        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
    }
    LRESULT(0)
}

/// Aplica una barra del menú de GlassBar mientras se mueve. La opacidad de los
/// menús cambia ya, también la del propio menú; se guarda al soltar la barra.
fn apply_menu_slider(
    styles: &mut style::Styles,
    menu: &mut AppMenuWindow,
    hit: appmenu::Hit,
    command: u32,
    value: f32,
) {
    if command == appmenu::CMD_MENU_OPACITY {
        styles.set_menu_opacity((value * 100.0).round() as u8);
        let percent = styles.menu_opacity();
        glass::set_menu_opacity(percent);
        // Sin bajar del mínimo, aunque el cursor vaya más allá.
        menu.model.set_slider(hit, percent as f32 / 100.0);
    }
    menu.render();
}

impl Drop for TrayWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

/// Muestra el nombre del icono del panel de la bandeja bajo el cursor, encima de él
/// como en Windows.
/// Punto de la pantalla donde está el cursor.
fn cursor_pos() -> POINT {
    let mut at = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut at);
    }
    at
}

fn show_tray_tip(app: &mut App) {
    let Some(panel) = app.tray_panel.as_mut().filter(|p| p.visible) else {
        return;
    };
    // Como en la bandeja de Windows, el cursor quieto sobre un icono también avisa
    // a su programa, que puede abrir su propia ventanita.
    let hovered = panel
        .model
        .hovered
        .map(|index| panel.model.icons[index].key);
    if panel.popup != hovered {
        panel.end_popup();
        if let Some(key) = hovered {
            systray::popup_open(key, cursor_pos());
            panel.popup = Some(key);
        }
    }
    let scale = panel.scale;
    let label = panel.model.hovered.map(|index| {
        let (cx, cy) = panel.model.cell(index);
        (
            panel.model.icons[index].tip.clone(),
            panel.pos.x + ((cx + 20.0) * scale).round() as i32,
            panel.pos.y + ((cy - 4.0) * scale).round() as i32,
        )
    });
    let (left, right) = (app.screen_left, app.screen_right);
    if let Some(tip) = &mut app.tooltip {
        match label.filter(|(text, _, _)| !text.trim().is_empty()) {
            Some((text, cx, bottom)) => tip.show(&text, cx, bottom, left, right, scale),
            None => tip.hide(),
        }
    }
}

unsafe extern "system" fn tray_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        WA_INACTIVE, WM_ACTIVATE, WM_CAPTURECHANGED, WM_KEYDOWN, WM_LBUTTONDBLCLK,
    };
    let point = |scale: f32| {
        (
            (lparam.0 & 0xffff) as i16 as f32 / scale,
            ((lparam.0 >> 16) & 0xffff) as i16 as f32 / scale,
        )
    };
    let cursor = || {
        let mut at = POINT::default();
        let _ = GetCursorPos(&mut at);
        at
    };
    // Pulsación sobre un icono: se la manda a su programa. Con `chosen`, el icono
    // elegido con el teclado en lugar del que está bajo el cursor.
    let press = |click: systray::Click, chosen: Option<usize>| {
        with_app(|app| {
            let Some(panel) = app.tray_panel.as_mut().filter(|p| p.visible) else {
                return;
            };
            let (x, y) = point(panel.scale);
            let index = chosen.or_else(|| panel.model.hit_test(x, y));
            if let Some(index) = index {
                let _ = unsafe { KillTimer(Some(hwnd), TIMER_TIP) };
                if let Some(tip) = &mut app.tooltip {
                    tip.hide();
                }
                app.tray_clicked = Some(Instant::now());
                let key = panel.model.icons[index].key;
                // Con el teclado, el menú del icono sale sobre él y no bajo el cursor.
                let at = match chosen {
                    Some(index) => {
                        let (cx, cy) = panel.model.center(index);
                        POINT {
                            x: panel.pos.x + (cx * panel.scale).round() as i32,
                            y: panel.pos.y + (cy * panel.scale).round() as i32,
                        }
                    }
                    None => cursor(),
                };
                match click {
                    // Sigue abierto mientras está el menú del icono (el proceso se
                    // pone al mandarle el clic, justo después).
                    systray::Click::Right => {
                        panel.menu = Some(TrayMenu {
                            since: Instant::now(),
                            seen: false,
                            pid: None,
                            at,
                            known: Default::default(),
                        })
                    }
                    // Espera un posible segundo clic; si el programa abre su ventana,
                    // el panel pierde el foco y se cierra antes.
                    systray::Click::Left => {
                        let double = unsafe {
                            windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime()
                        };
                        panel.close_after =
                            Some(Instant::now() + Duration::from_millis(double as u64));
                    }
                    systray::Click::Double => panel.hide(),
                }
                let pid = systray::click(key, click, at);
                if let Some(menu) = panel.menu.as_mut() {
                    menu.pid = pid;
                }
            }
        });
    };
    match msg {
        WM_ACTIVATE if (wparam.0 & 0xffff) as u32 == WA_INACTIVE => {
            with_app(|app| {
                if let Some(tip) = &mut app.tooltip {
                    tip.hide();
                }
                // El menú de un icono se lleva el foco: el panel sigue abierto.
                app.tray_panel
                    .as_mut()
                    .filter(|p| p.menu.is_none())
                    .map(TrayWindow::hide)
            });
        }
        WM_TIMER if wparam.0 == TIMER_ANIM => {
            with_app(|app| app.tray_panel.as_mut().map(TrayWindow::present));
        }
        WM_LBUTTONDOWN => {
            let pressed = with_app(|app| {
                let Some(panel) = app.tray_panel.as_mut().filter(|p| p.visible) else {
                    return false;
                };
                let (x, y) = point(panel.scale);
                panel.model.press(x, y);
                panel.model.drag.is_some()
            });
            // Captura el ratón para seguir el arrastre aunque salga del panel.
            if pressed == Some(true) {
                SetCapture(hwnd);
            }
        }
        WM_MOUSEMOVE => {
            with_app(|app| {
                let Some(panel) = app.tray_panel.as_mut().filter(|p| p.visible) else {
                    return;
                };
                // Con el botón pulsado sobre un icono: arrastre, sin nombres ni avisos.
                if panel.model.drag.is_some() {
                    let (x, y) = point(panel.scale);
                    if panel.model.drag_to(x, y) {
                        let _ = KillTimer(Some(hwnd), TIMER_TIP);
                        panel.render();
                        if let Some(tip) = &mut app.tooltip {
                            tip.hide();
                        }
                    }
                    return;
                }
                let mut track = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut track);
                let (x, y) = point(panel.scale);
                let hit = panel.model.hit_test(x, y);
                if hit == panel.model.hovered {
                    return;
                }
                panel.end_popup();
                panel.model.hovered = hit;
                panel.render();
                if let Some(index) = hit {
                    systray::hover(panel.model.icons[index].key, cursor());
                }
                // Con un nombre ya a la vista pasa al del icono vecino al momento;
                // si no, espera a que el cursor se quede quieto, como en el dock.
                let showing = app.tooltip.as_ref().is_some_and(|t| t.visible);
                if showing && hit.is_some() {
                    show_tray_tip(app);
                } else {
                    if let Some(tip) = &mut app.tooltip {
                        tip.hide();
                    }
                    if hit.is_some() {
                        let _ = SetTimer(Some(hwnd), TIMER_TIP, TIP_MS, None);
                    }
                }
            });
        }
        WM_TIMER if wparam.0 == TIMER_TIP => {
            let _ = KillTimer(Some(hwnd), TIMER_TIP);
            with_app(show_tray_tip);
        }
        WM_MOUSELEAVE => {
            let _ = KillTimer(Some(hwnd), TIMER_TIP);
            with_app(|app| {
                if let Some(tip) = &mut app.tooltip {
                    tip.hide();
                }
                if let Some(panel) = app.tray_panel.as_mut().filter(|p| p.visible) {
                    panel.end_popup();
                    if panel.model.hovered.take().is_some() {
                        panel.render();
                    }
                }
            });
        }
        WM_LBUTTONUP => {
            let dragged = with_app(|app| {
                let panel = app.tray_panel.as_mut().filter(|p| p.visible)?;
                let dragged = panel.model.release();
                if dragged {
                    let _ = traypanel::save_order(&panel.model.order());
                    panel.refresh();
                }
                Some(dragged)
            })
            .flatten();
            let _ = ReleaseCapture();
            // Soltar tras arrastrar no es un clic en el icono.
            if dragged != Some(true) {
                press(systray::Click::Left, None);
            }
        }
        WM_CAPTURECHANGED => {
            // Otra ventana se llevó el ratón: el icono se queda donde estaba al soltarlo.
            with_app(|app| {
                if let Some(panel) = app.tray_panel.as_mut() {
                    if panel.model.release() {
                        let _ = traypanel::save_order(&panel.model.order());
                        panel.refresh();
                    }
                }
            });
        }
        WM_LBUTTONDBLCLK => press(systray::Click::Double, None),
        WM_RBUTTONUP => press(systray::Click::Right, None),
        // Escape lo cierra; arriba y abajo recorren los iconos; Enter o espacio los
        // pulsan y la tecla de menú (o Mayús+F10) abre su menú; izquierda y derecha
        // pasan al panel vecino.
        WM_KEYDOWN => {
            let key = wparam.0;
            let shift = windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x10) < 0;
            let chosen = with_app(|app| {
                let panel = app.tray_panel.as_mut().filter(|p| p.visible)?;
                let click = match key {
                    0x1B => {
                        panel.hide();
                        return None;
                    }
                    0x26 | 0x28 => {
                        panel.model.move_focus(if key == 0x26 { -1 } else { 1 });
                        panel.render();
                        return None;
                    }
                    0x25 | 0x27 => {
                        app.step_menu(if key == 0x25 { -1 } else { 1 });
                        return None;
                    }
                    0x0D | 0x20 => systray::Click::Left,
                    0x5D => systray::Click::Right,
                    0x79 if shift => systray::Click::Right,
                    _ => return None,
                };
                panel.model.hovered.map(|index| (click, index))
            })
            .flatten();
            if let Some((click, index)) = chosen {
                press(click, Some(index));
            }
        }
        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
    }
    LRESULT(0)
}

/// Vista previa de las ventanas de un programa del dock: una tarjeta por ventana.
/// El marco y los títulos se dibujan como el resto de GlassBar; las miniaturas en
/// vivo las pone Windows (DWM) encima, en el hueco de cada tarjeta.
struct PreviewWindow {
    hwnd: HWND,
    /// Desenfoque de debajo, como el del dock.
    backdrop: Option<style::Backdrop>,
    surface: Option<Surface>,
    pos: POINT,
    scale: f32,
    visible: bool,
    /// Ventanas que se muestran, con su miniatura registrada en DWM y su título.
    sources: Vec<isize>,
    thumbnails: Vec<isize>,
    titles: Vec<String>,
    /// Programa del dock al que pertenecen.
    target: Option<HoverTarget>,
    layout: preview::Layout,
    hovered: Option<usize>,
    close_hovered: Option<usize>,
}
impl PreviewWindow {
    fn new(owner: HWND) -> Result<Self> {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("GlassBarPreview"),
                w!("GlassBar — Vista previa"),
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
            backdrop: style::Backdrop::new(),
            surface: None,
            pos: POINT::default(),
            scale: 1.0,
            visible: false,
            sources: Vec::new(),
            thumbnails: Vec::new(),
            titles: Vec::new(),
            target: None,
            layout: preview::Layout::default(),
            hovered: None,
            close_hovered: None,
        })
    }

    fn unregister(&mut self) {
        for thumbnail in self.thumbnails.drain(..) {
            unsafe {
                let _ = windows::Win32::Graphics::Dwm::DwmUnregisterThumbnail(thumbnail);
            }
        }
    }

    /// Muestra las ventanas `sources` (con sus títulos) sobre `anchor_x`, encima del
    /// dock. Devuelve `false` si Windows no da ninguna miniatura (entonces se usa el
    /// globo con el nombre).
    #[allow(clippy::too_many_arguments)]
    fn show(
        &mut self,
        sources: &[(isize, String)],
        target: HoverTarget,
        anchor_x: i32,
        dock_top: i32,
        (left, right): (i32, i32),
        scale: f32,
    ) -> bool {
        use windows::Win32::Graphics::Dwm::{DwmQueryThumbnailSourceSize, DwmRegisterThumbnail};
        let ids: Vec<isize> = sources.iter().map(|(w, _)| *w).collect();
        if self.visible && self.sources == ids {
            self.target = Some(target);
            return true;
        }
        self.unregister();
        self.sources.clear();
        self.titles.clear();
        let mut sizes = Vec::new();
        for (window, title) in sources {
            unsafe {
                let Ok(thumbnail) = DwmRegisterThumbnail(self.hwnd, HWND(*window as *mut _)) else {
                    continue;
                };
                let Ok(size) = DwmQueryThumbnailSourceSize(thumbnail) else {
                    let _ = windows::Win32::Graphics::Dwm::DwmUnregisterThumbnail(thumbnail);
                    continue;
                };
                self.thumbnails.push(thumbnail);
                self.sources.push(*window);
                self.titles.push(title.clone());
                sizes.push((size.cx as f32, size.cy as f32));
            }
        }
        if sizes.is_empty() {
            self.hide();
            return false;
        }
        let margin = (8.0 * scale).round() as i32;
        let room = (right - left - margin * 2) as f32 / scale;
        self.layout = preview::layout(&sizes, scale, room);
        self.scale = scale;
        self.target = Some(target);
        self.hovered = None;
        self.close_hovered = None;
        let width = (self.layout.width * scale).ceil() as u32;
        let height = (self.layout.height * scale).ceil() as u32;
        let Ok(surface) = Surface::new(width, height) else {
            self.hide();
            return false;
        };
        self.surface = Some(surface);
        self.pos = POINT {
            x: (anchor_x - width as i32 / 2).clamp(
                left + margin,
                (right - width as i32 - margin).max(left + margin),
            ),
            y: dock_top - height as i32 - margin,
        };
        self.render();
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        self.visible = true;
        self.place_thumbnails();
        true
    }

    /// Le dice a Windows dónde pintar cada miniatura dentro de su tarjeta.
    fn place_thumbnails(&self) {
        use windows::Win32::Graphics::Dwm::{
            DwmUpdateThumbnailProperties, DWM_THUMBNAIL_PROPERTIES,
        };
        // DWM_TNP_RECTDESTINATION | DWM_TNP_OPACITY | DWM_TNP_VISIBLE.
        const FLAGS: u32 = 0x1 | 0x4 | 0x8;
        let s = self.scale;
        for (thumbnail, card) in self.thumbnails.iter().zip(&self.layout.cards) {
            let (x, y, w, h) = card.thumb;
            let properties = DWM_THUMBNAIL_PROPERTIES {
                dwFlags: FLAGS,
                rcDestination: windows::Win32::Foundation::RECT {
                    left: (x * s).round() as i32,
                    top: (y * s).round() as i32,
                    right: ((x + w) * s).round() as i32,
                    bottom: ((y + h) * s).round() as i32,
                },
                opacity: 255,
                fVisible: true.into(),
                ..Default::default()
            };
            unsafe {
                let _ = DwmUpdateThumbnailProperties(*thumbnail, &properties);
            }
        }
    }

    fn render(&mut self) {
        let Some(surface) = &mut self.surface else {
            return;
        };
        let Some(pixmap) = surface.pixmap() else {
            return;
        };
        preview::draw(
            pixmap,
            &self.layout,
            &self.titles,
            self.hovered,
            self.close_hovered,
            self.scale,
        );
        surface.copy_to_dib();
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
                    cx: surface.width as i32,
                    cy: surface.height as i32,
                }),
                Some(surface.dc()),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
        if let Some(backdrop) = &self.backdrop {
            // Las tarjetas, como las pinta `preview::draw`.
            let s = self.scale;
            let shapes: Vec<style::Shape> = self
                .layout
                .cards
                .iter()
                .map(|card| style::Shape {
                    x: (card.x + 1.0) * s,
                    y: (card.y + 1.0) * s,
                    width: (card.width - 2.0) * s,
                    height: (card.height - 2.0) * s,
                    radius: 8.0 * s,
                })
                .collect();
            backdrop.under(self.hwnd, surface_rect(self.pos, surface), &shapes, 1.0);
        }
    }

    fn hide(&mut self) {
        if !self.visible && self.thumbnails.is_empty() {
            return;
        }
        self.unregister();
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        if let Some(backdrop) = &self.backdrop {
            backdrop.hide();
        }
        self.visible = false;
        self.target = None;
        self.sources.clear();
        self.titles.clear();
        self.surface = None;
    }

    /// El cursor (en pantalla) está sobre el panel.
    fn contains(&self, point: POINT) -> bool {
        let Some(surface) = &self.surface else {
            return false;
        };
        self.visible
            && point.x >= self.pos.x
            && point.x < self.pos.x + surface.width as i32
            && point.y >= self.pos.y
            && point.y < self.pos.y + surface.height as i32
    }
}
impl Drop for PreviewWindow {
    fn drop(&mut self) {
        self.hide();
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

/// Título de una ventana, para la vista previa.
fn window_title(hwnd: isize) -> String {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
    let mut buffer = [0u16; 256];
    let len = unsafe { GetWindowTextW(HWND(hwnd as *mut _), &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

unsafe extern "system" fn preview_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let point = |scale: f32| {
        (
            (lparam.0 & 0xffff) as i16 as f32 / scale,
            ((lparam.0 >> 16) & 0xffff) as i16 as f32 / scale,
        )
    };
    match msg {
        // Pulsarla no quita el foco a la ventana activa.
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            with_app(|app| {
                let Some(panel) = app.preview.as_mut().filter(|p| p.visible) else {
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
                let hovered = panel.layout.card_at(x, y);
                let close = panel.layout.close_at(x, y);
                if (hovered, close) != (panel.hovered, panel.close_hovered) {
                    panel.hovered = hovered;
                    panel.close_hovered = close;
                    panel.render();
                }
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with_app(|app| {
                if let Some(panel) = app.preview.as_mut().filter(|p| p.visible) {
                    panel.hovered = None;
                    panel.close_hovered = None;
                    panel.render();
                }
            });
            LRESULT(0)
        }
        // Clic en una tarjeta: trae su ventana al frente; en su ✕, la cierra (como en
        // Windows, le pide que se cierre, así que puede preguntar si guardar).
        WM_LBUTTONUP => {
            with_app(|app| {
                let Some(panel) = app.preview.as_mut().filter(|p| p.visible) else {
                    return;
                };
                let (x, y) = point(panel.scale);
                let close = panel.layout.close_at(x, y);
                let Some(index) = close.or_else(|| panel.layout.card_at(x, y)) else {
                    return;
                };
                let source = panel.sources[index];
                panel.hide();
                if close.is_some() {
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                        Some(HWND(source as *mut _)),
                        windows::Win32::UI::WindowsAndMessaging::WM_CLOSE,
                        WPARAM(0),
                        LPARAM(0),
                    );
                } else {
                    apps::focus_window(source);
                }
            });
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

/// Opción elegida en el menú de la app `target` del dock.
fn handle_app_command(app: &mut App, target: HoverTarget, cmd: u32) {
    match (cmd, target) {
        (CMD_APP_OPEN, _) => {
            if let Some(item) = app.bar.item_mut(target) {
                let entry = item.pin_entry();
                if apps::launch_entry(&entry) {
                    let _ = catalog::record(&entry);
                }
            }
        }
        (CMD_APP_PIN, HoverTarget::OpenApp(i)) => {
            if let Some(item) = app.bar.open_apps.get(i) {
                let entry = item.pin_entry();
                app.bar.primary_apps.push(apps::AppItem::new(entry));
                // Sus ventanas pasan al anclado y deja de salir entre los abiertos.
                apps::update_running_apps(&mut app.bar.primary_apps, &mut app.bar.open_apps);
                app.pins_changed();
            }
        }
        (CMD_APP_UNPIN, HoverTarget::PrimaryApp(i)) if i < app.bar.primary_apps.len() => {
            app.bar.primary_apps.remove(i);
            // Si sigue abierto, vuelve a salir entre los abiertos.
            apps::update_running_apps(&mut app.bar.primary_apps, &mut app.bar.open_apps);
            app.pins_changed();
        }
        (CMD_APP_END_TASK, _) => {
            if let Some(item) = app.bar.item_mut(target) {
                item.end_task();
            }
        }
        (CMD_APP_CLOSE, _) => {
            if let Some(item) = app.bar.item_mut(target) {
                item.close_windows();
            }
        }
        _ => {}
    }
}

/// Nombre de la app activa: la descripción de su ejecutable ("Google Chrome"), su
/// título en las apps de Store (que comparten `ApplicationFrameHost.exe`) o
/// "Escritorio". `None` si es una ventana de GlassBar.
fn active_app_name() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId,
    };
    unsafe {
        let window = GetForegroundWindow();
        if window.is_invalid() {
            return Some("Escritorio".into());
        }
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        if pid == std::process::id() {
            return None;
        }
        let mut class = [0u16; 64];
        let len = GetClassNameW(window, &mut class).max(0) as usize;
        let class = String::from_utf16_lossy(&class[..len]);
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd") {
            return Some("Escritorio".into());
        }
        let (exe, _) = apps::process_info(pid)?;
        let file = exe.rsplit('\\').next().unwrap_or_default();
        if file.eq_ignore_ascii_case("ApplicationFrameHost.exe") {
            let title = window_title(window.0 as isize);
            if !title.is_empty() {
                return Some(title);
            }
        }
        apps::file_description(&exe)
            .filter(|name| !name.trim().is_empty())
            .or_else(|| {
                std::path::Path::new(&exe)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
            })
    }
}

/// La barra superior: una franja fina en lo alto del monitor principal, con su
/// sitio reservado (las ventanas maximizadas empiezan debajo).
struct TopBarWindow {
    hwnd: HWND,
    surface: Option<Surface>,
    scale: f32,
    monitor: windows::Win32::Graphics::Gdi::HMONITOR,
    /// Rectángulo del monitor.
    rect: windows::Win32::Foundation::RECT,
    model: topbar::TopBar,
    /// Lo último dibujado, para no repetirlo si nada cambió.
    drawn: Option<topbar::TopBar>,
    appbar: Option<appbar::AppBar>,
    /// Windows avisó de una app a pantalla completa en su monitor.
    appbar_fullscreen: bool,
    hidden: bool,
}

impl TopBarWindow {
    fn new() -> Result<Self> {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("GlassBarTop"),
                w!("GlassBar — Barra superior"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                // Sin dueña: una ventana con dueña siempre queda encima de ella, y el
                // dock se pone encima de todo al mostrarse; la barra lo seguiría y
                // taparía a Notch Isla.
                None,
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?
        };
        Ok(Self {
            hwnd,
            surface: None,
            scale: 1.0,
            monitor: monitors::primary(),
            rect: Default::default(),
            model: topbar::TopBar::new(),
            drawn: None,
            appbar: None,
            appbar_fullscreen: false,
            hidden: true,
        })
    }

    /// Alto en píxeles.
    fn height(&self) -> i32 {
        (topbar::HEIGHT * self.scale).ceil() as i32
    }

    /// Se coloca arriba del monitor principal y reserva su sitio. No se muestra:
    /// eso lo decide `check_priority`, que la deja apartada si hay una app a
    /// pantalla completa (Windows avisa de `ABN_POSCHANGED` también durante un juego).
    fn place(&mut self) {
        self.monitor = monitors::primary();
        let (rect, dpi) = monitors::get_info(self.monitor);
        self.rect = rect;
        self.scale = dpi.max(0.3);
        let width = (rect.right - rect.left).max(1) as u32;
        let height = self.height() as u32;
        if self
            .surface
            .as_ref()
            .is_none_or(|s| s.width != width || s.height != height)
        {
            self.surface = Surface::new(width, height).ok();
        }
        self.model.width = width as f32 / self.scale;
        if self.appbar.is_none() {
            self.appbar = appbar::AppBar::register(self.hwnd, WM_TOP_APPBAR);
        }
        if let Some(bar) = &self.appbar {
            bar.place(rect, height as i32, true);
        }
        self.drawn = None;
        self.render();
        // Encima de todo, salvo de una isla como Notch Isla, que tiene que seguir
        // encima de ella.
        if !self.hidden {
            priority::raise_below_overlapping(self.hwnd, self.rect);
        }
    }

    fn render(&mut self) {
        if self.drawn.as_ref() == Some(&self.model) {
            return;
        }
        let Some(surface) = &mut self.surface else {
            return;
        };
        if let Some(pixmap) = surface.pixmap() {
            self.model.draw(pixmap, self.scale);
        }
        surface.copy_to_dib();
        let size = SIZE {
            cx: surface.width as i32,
            cy: surface.height as i32,
        };
        let pos = POINT {
            x: self.rect.left,
            y: self.rect.top,
        };
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
                Some(&pos),
                Some(&size),
                Some(surface.dc()),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
        self.drawn = Some(self.model.clone());
    }

    /// Se aparta con una app a pantalla completa, como la barra de Windows.
    fn set_hidden(&mut self, hidden: bool) {
        if hidden == self.hidden {
            return;
        }
        self.hidden = hidden;
        unsafe {
            let _ = ShowWindow(self.hwnd, if hidden { SW_HIDE } else { SW_SHOWNOACTIVATE });
        }
    }
}

impl Drop for TopBarWindow {
    fn drop(&mut self) {
        // Primero se devuelve su sitio, con la ventana aún viva.
        self.appbar = None;
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn topbar_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let x = (lparam.0 & 0xffff) as i16 as i32;
    let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
    match msg {
        WM_MOUSEMOVE => {
            let mut track = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            let _ = TrackMouseEvent(&mut track);
            with_app(|app| app.top_hover(x, y));
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with_app(|app| app.top_hover(-1, -1));
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            with_app(|app| app.top_click(x, y));
            LRESULT(0)
        }
        // Clic derecho en el logo de Inicio: el menú de Windows (Win+X), como en el
        // dock. En el resto de la barra, nada.
        WM_RBUTTONUP => {
            with_app(|app| app.top_right_click(x, y));
            LRESULT(0)
        }
        // La rueda sobre el estado sube o baja el volumen, como en el dock.
        windows::Win32::UI::WindowsAndMessaging::WM_MOUSEWHEEL => {
            with_app(|app| {
                let over_status = app
                    .top
                    .as_ref()
                    .is_some_and(|top| matches!(top.model.hovered, Some(HoverTarget::Status(_))));
                if over_status {
                    let delta = ((wparam.0 >> 16) & 0xffff) as i16 as f32 / 120.0;
                    status::change_volume(delta);
                    app.bar.status = app.status.read();
                    app.refresh_top_bar();
                }
            });
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_TOP_APPBAR => {
            match wparam.0 {
                appbar::ABN_POSCHANGED => {
                    with_app(|app| {
                        if let Some(top) = &mut app.top {
                            top.place();
                        }
                        app.check_priority(false);
                    });
                }
                appbar::ABN_FULLSCREENAPP => {
                    with_app(|app| {
                        if let Some(top) = &mut app.top {
                            top.appbar_fullscreen = lparam.0 != 0;
                        }
                        app.check_priority(false);
                    });
                }
                _ => {}
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Ventana de configuración (ver `settings`), centrada en el monitor del dock. A
/// diferencia de los paneles, es una ventana de aplicación: sale en el dock, se
/// minimiza y sigue abierta al pasar a otra ventana.
struct SettingsWindow {
    hwnd: HWND,
    backdrop: Option<style::Backdrop>,
    surface: Option<Surface>,
    pos: POINT,
    scale: f32,
    model: settings::SettingsState,
    visible: bool,
    transition: launcher::Transition,
    /// Arrastrando la ventana por su cabecera: distancia del cursor a su esquina.
    grab: Option<POINT>,
    /// Donde la dejó el usuario; al volver a abrirla sale ahí en vez de centrada.
    placed: Option<POINT>,
}
impl SettingsWindow {
    fn new(model: settings::SettingsState) -> Result<Self> {
        use windows::Win32::UI::WindowsAndMessaging::{
            WS_EX_APPWINDOW, WS_MINIMIZEBOX, WS_SYSMENU,
        };
        // Sin dueño: una ventana con dueño no sale en la barra ni se minimiza sola.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_APPWINDOW,
                w!("GlassBarSettings"),
                w!("GlassBar — Configuración"),
                WS_POPUP | WS_MINIMIZEBOX | WS_SYSMENU,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?
        };
        Ok(Self {
            hwnd,
            backdrop: style::Backdrop::new(),
            surface: None,
            pos: POINT::default(),
            scale: 1.0,
            model,
            visible: false,
            transition: launcher::Transition::hidden(),
            grab: None,
            placed: None,
        })
    }
    fn show(&mut self) {
        let (screen, dpi) = monitors::get_info(monitors::dock());
        self.scale = dpi.max(0.3);
        let width = (settings::WIDTH * self.scale).ceil() as u32;
        let height = (settings::height() * self.scale).ceil() as u32;
        let Ok(surface) = Surface::new(width, height) else {
            return;
        };
        self.surface = Some(surface);
        let centered = POINT {
            x: screen.left + ((screen.right - screen.left) - width as i32) / 2,
            y: screen.top + ((screen.bottom - screen.top) - height as i32) / 2,
        };
        // Donde la dejó el usuario, si ese sitio sigue estando en una pantalla.
        self.pos = self
            .placed
            .filter(|p| {
                let area = monitors::work_area(monitors::at(*p));
                p.x >= area.left
                    && p.y >= area.top
                    && p.x + width as i32 <= area.right
                    && p.y + height as i32 <= area.bottom
            })
            .unwrap_or(centered);
        self.visible = true;
        self.restore();
        self.transition.open(Instant::now());
        self.render();
        take_focus(self.hwnd);
    }
    fn minimized(&self) -> bool {
        unsafe { windows::Win32::UI::WindowsAndMessaging::IsIconic(self.hwnd).as_bool() }
    }
    fn minimize(&mut self) {
        use windows::Win32::UI::WindowsAndMessaging::SW_MINIMIZE;
        self.model.hovered = None;
        self.grab = None;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_MINIMIZE);
        }
    }
    /// La vuelve a mostrar si estaba minimizada.
    fn restore(&mut self) {
        use windows::Win32::UI::WindowsAndMessaging::SW_RESTORE;
        if self.minimized() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_RESTORE);
            }
        }
    }
    fn render(&mut self) {
        if let Some(surface) = &mut self.surface {
            if let Some(pixmap) = surface.pixmap() {
                self.model.draw(pixmap, self.scale);
            }
            surface.copy_to_dib();
        }
        self.present();
    }
    fn present(&mut self) {
        // Minimizada no se pinta ni se mueve; su desenfoque, que es otra ventana, se
        // quita. Si se cerró estando así, se esconde sin animación.
        if self.minimized() {
            frames::stop(self.hwnd);
            if let Some(backdrop) = &self.backdrop {
                backdrop.hide();
            }
            if !self.visible {
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                }
            }
            return;
        }
        if let Some(surface) = &mut self.surface {
            let shapes = [panel_shape(surface, self.scale, 12.0)];
            present_popup(
                self.hwnd,
                surface,
                self.pos,
                self.scale,
                &self.transition,
                self.visible,
                false,
                self.backdrop.as_ref(),
                &shapes,
            );
        }
    }
    fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.model.hovered = None;
        self.model.dragging = false;
        self.grab = None;
        self.transition.close(Instant::now());
        self.present();
    }
}
impl Drop for SettingsWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

/// Pone al día la configuración (y su desenfoque) tras activarse, minimizarse o
/// restaurarse.
const WM_SETTINGS_SYNC: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x40;

/// Cambia la opacidad de los paneles desde la barra de la configuración.
fn set_settings_opacity(app: &mut App, x: f32) {
    let value = settings::SettingsState::opacity_at(x);
    app.styles.set_menu_opacity((value * 100.0).round() as u8);
    glass::set_menu_opacity(app.styles.menu_opacity());
    app.refresh_settings();
}

unsafe extern "system" fn settings_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        SIZE_MINIMIZED, SIZE_RESTORED, WA_INACTIVE, WM_ACTIVATE, WM_CLOSE, WM_KEYDOWN, WM_SIZE,
    };
    let point = |scale: f32| {
        (
            (lparam.0 & 0xffff) as i16 as f32 / scale,
            ((lparam.0 >> 16) & 0xffff) as i16 as f32 / scale,
        )
    };
    match msg {
        // Al activarse pasa delante de las demás, y al minimizarla o restaurarla
        // aparece o desaparece: su desenfoque, que es otra ventana, se pone o se quita.
        // Se hace después, en cola: estos avisos llegan también mientras GlassBar la
        // minimiza o la restaura (desde el dock), y entonces no se podría atender.
        WM_ACTIVATE | WM_SIZE
            if (msg == WM_ACTIVATE && (wparam.0 & 0xffff) as u32 != WA_INACTIVE)
                || (msg == WM_SIZE
                    && matches!(wparam.0 as u32, SIZE_MINIMIZED | SIZE_RESTORED)) =>
        {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                Some(hwnd),
                WM_SETTINGS_SYNC,
                WPARAM(0),
                LPARAM(0),
            );
        }
        WM_SETTINGS_SYNC => {
            with_app(|app| {
                if let Some(panel) = app.settings.as_mut().filter(|p| p.visible) {
                    panel.render();
                }
            });
        }
        // Alt+F4, o "Cerrar ventana" desde el dock: se esconde, como con la ✕. Sin
        // esto Windows la destruiría y la configuración ya no se podría abrir.
        WM_CLOSE => {
            with_app(|app| app.settings.as_mut().map(SettingsWindow::hide));
        }
        WM_TIMER if wparam.0 == TIMER_ANIM => {
            with_app(|app| app.settings.as_mut().map(SettingsWindow::present));
        }
        WM_MOUSEMOVE => {
            with_app(|app| {
                let Some(panel) = app.settings.as_mut().filter(|p| p.visible) else {
                    return;
                };
                if let Some(grab) = panel.grab {
                    let mut cursor = POINT::default();
                    let _ = GetCursorPos(&mut cursor);
                    // Sin salirse del área de trabajo del monitor del cursor (así no
                    // tapa la barra superior).
                    let screen = monitors::work_area(monitors::at(cursor));
                    let (width, height) = panel
                        .surface
                        .as_ref()
                        .map_or((0, 0), |s| (s.width as i32, s.height as i32));
                    panel.pos = POINT {
                        x: (cursor.x - grab.x)
                            .clamp(screen.left, (screen.right - width).max(screen.left)),
                        y: (cursor.y - grab.y)
                            .clamp(screen.top, (screen.bottom - height).max(screen.top)),
                    };
                    panel.placed = Some(panel.pos);
                    panel.present();
                    return;
                }
                let (x, y) = point(panel.scale);
                if panel.model.dragging {
                    set_settings_opacity(app, x);
                    return;
                }
                let mut track = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut track);
                let hit = panel.model.hit_test(x, y);
                if hit != panel.model.hovered {
                    panel.model.hovered = hit;
                    panel.render();
                }
            });
        }
        WM_MOUSELEAVE => {
            with_app(|app| {
                if let Some(panel) = app.settings.as_mut().filter(|p| p.visible) {
                    if !panel.model.dragging && panel.model.hovered.take().is_some() {
                        panel.render();
                    }
                }
            });
        }
        WM_LBUTTONDOWN => {
            with_app(|app| {
                let Some(panel) = app.settings.as_mut().filter(|p| p.visible) else {
                    return;
                };
                let (x, y) = point(panel.scale);
                if settings::SettingsState::in_title(x, y) {
                    let mut cursor = POINT::default();
                    let _ = GetCursorPos(&mut cursor);
                    panel.grab = Some(POINT {
                        x: cursor.x - panel.pos.x,
                        y: cursor.y - panel.pos.y,
                    });
                    let _ = SetCapture(hwnd);
                    return;
                }
                if panel.model.hit_test(x, y) == Some(settings::Hit::Opacity) {
                    panel.model.dragging = true;
                    let _ = SetCapture(hwnd);
                    set_settings_opacity(app, x);
                }
            });
        }
        WM_LBUTTONUP => {
            let mut backup = None;
            with_app(|app| {
                let Some(panel) = app.settings.as_mut().filter(|p| p.visible) else {
                    return;
                };
                if panel.grab.take().is_some() {
                    let _ = ReleaseCapture();
                    return;
                }
                if panel.model.dragging {
                    panel.model.dragging = false;
                    let _ = ReleaseCapture();
                    let _ = app.styles.save();
                    app.refresh_settings();
                    return;
                }
                let (x, y) = point(panel.scale);
                let Some(hit) = panel.model.hit_test(x, y) else {
                    return;
                };
                if hit == settings::Hit::Close {
                    panel.hide();
                    return;
                }
                if hit == settings::Hit::Minimize {
                    panel.minimize();
                    return;
                }
                if let settings::Hit::Page(page) = hit {
                    if panel.model.page != page {
                        panel.model.page = page;
                        panel.render();
                    }
                    return;
                }
                if let Some(command) = panel.model.command(hit) {
                    if matches!(command, appmenu::CMD_EXPORT | appmenu::CMD_IMPORT) {
                        backup = Some((app.hwnd, command == appmenu::CMD_EXPORT));
                        return;
                    }
                    handle_menu_command(app, command);
                    app.refresh_settings();
                }
            });
            // El diálogo de Windows atiende mensajes mientras está abierto: se abre con
            // la app ya libre, para que el dock siga respondiendo.
            if let Some((owner, export)) = backup {
                copy_settings(owner, export);
            }
        }
        WM_KEYDOWN if wparam.0 == 0x1B => {
            with_app(|app| app.settings.as_mut().map(SettingsWindow::hide));
        }
        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
    }
    LRESULT(0)
}

/// Exporta la configuración a un archivo que se elige, o la sustituye por la de
/// uno. Tras importar, GlassBar se reinicia para leerla.
fn copy_settings(owner: HWND, export: bool) {
    let Some(path) = backup::pick(owner, export) else {
        return;
    };
    let result = if export {
        backup::export(&path)
    } else {
        backup::import(&path)
    };
    let (title, text) = match (&result, export) {
        (Ok(()), true) => (
            "Configuración exportada",
            format!("Guardada en {}.", path.display()),
        ),
        (Ok(()), false) => {
            // La instancia nueva cierra esta y arranca con la configuración importada.
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).arg("--replace").spawn();
            }
            return;
        }
        (Err(error), true) => (
            "No se pudo exportar la configuración",
            format!("Motivo: {error}."),
        ),
        (Err(error), false) => (
            "No se pudo importar la configuración",
            format!("Motivo: {error}."),
        ),
    };
    with_app(|app| app.tray.notify(title, &text));
}

fn handle_menu_command(app: &mut App, cmd: u32) {
    match cmd {
        CMD_AUTOSTART => {
            let active = autostart::is_enabled();
            autostart::set(!active);
        }
        CMD_AUTOHIDE => {
            let always = !app.styles.always_visible();
            app.styles.set_always_visible(always);
            let _ = app.styles.save();
            app.sync_appbar();
            app.expand();
        }
        CMD_SETTINGS => app.open_settings(),
        appmenu::CMD_DOCK_SMALLER | appmenu::CMD_DOCK_BIGGER => {
            app.styles.step_dock_size(cmd == appmenu::CMD_DOCK_BIGGER);
            let _ = app.styles.save();
            app.relayout();
            // En el modo siempre visible cambia el sitio que Windows le reserva.
            app.sync_appbar();
            app.expand();
        }
        CMD_TOP_BAR => {
            let on = !app.styles.top_bar();
            app.styles.set_top_bar(on);
            let _ = app.styles.save();
            app.sync_top_bar();
        }
        CMD_NOTIFICATIONS => {
            let on = !app.styles.notifications();
            app.styles.set_notifications(on);
            let _ = app.styles.save();
            app.sync_notifications();
        }
        appmenu::CMD_UNMUTE_APPS => {
            let in_windows = app.styles.unmute_apps();
            let _ = app.styles.save();
            // Windows no vuelve a encender una app desde el registro: se enciende en
            // su Configuración.
            if in_windows {
                apps::open_notification_settings();
            }
        }
        CMD_CLOCK_SECONDS => {
            let on = !app.styles.clock_seconds();
            app.styles.set_clock_seconds(on);
            let _ = app.styles.save();
            clock::set_seconds(on);
            app.render();
            app.refresh_top_bar();
        }
        CMD_EXIT => {
            app.release_shell();
            unsafe {
                PostQuitMessage(0);
            }
        }
        CMD_UPDATE => app.start_update(),
        CMD_UPDATE_CHECK => app.check_updates(true),
        _ => {
            if let Some(choice) = style::Choice::from_command(cmd) {
                choice.apply(&mut app.styles);
                let _ = app.styles.save();
                // Que el cambio se vea ya.
                app.update_style();
            }
        }
    }
}

/// El sondeo del borde puede ir lento: el puntero lleva un rato quieto y no hay nada abierto,
/// pulsado ni en movimiento que dependa de él.
fn edge_can_idle(still_for: Duration, in_use: bool) -> bool {
    !in_use && still_for >= EDGE_IDLE_AFTER
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
    fn edge_polling_slows_only_when_the_pointer_rests_and_nothing_is_open() {
        assert!(edge_can_idle(Duration::from_secs(3), false));
        assert!(!edge_can_idle(Duration::from_millis(500), false));
        assert!(!edge_can_idle(Duration::from_secs(3), true));
    }
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
    // Si GlassBar falla, deja el motivo en %TEMP%\glassbar-crash.log.
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write;
        let path = std::env::temp_dir().join("glassbar-crash.log");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(
                file,
                "{info}\n{}\n",
                std::backtrace::Backtrace::force_capture()
            );
        }
    }));
    if guard::run(&std::env::args().collect::<Vec<_>>()) {
        return Ok(());
    }
    diag::start();
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

        // Fotogramas de las animaciones al ritmo de la pantalla.
        frames::init(TIMER_ANIM);

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

        let spotlight_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(spotlight_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("GlassBarSpotlight"),
            ..Default::default()
        };
        if RegisterClassExW(&spotlight_class) == 0 {
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

        let app_menu_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(app_menu_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("GlassBarAppMenu"),
            ..Default::default()
        };
        if RegisterClassExW(&app_menu_class) == 0 {
            return Err(windows::core::Error::from_thread());
        }

        let tray_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            // Doble clic: algunos programas abren su ventana así desde la bandeja.
            style: windows::Win32::UI::WindowsAndMessaging::CS_DBLCLKS,
            lpfnWndProc: Some(tray_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("GlassBarTray"),
            ..Default::default()
        };
        if RegisterClassExW(&tray_class) == 0 {
            return Err(windows::core::Error::from_thread());
        }

        let language_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(language_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("GlassBarLanguage"),
            ..Default::default()
        };
        if RegisterClassExW(&language_class) == 0 {
            return Err(windows::core::Error::from_thread());
        }

        let calendar_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(calendar_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("GlassBarCalendar"),
            ..Default::default()
        };
        if RegisterClassExW(&calendar_class) == 0 {
            return Err(windows::core::Error::from_thread());
        }

        let notifications_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(notifications_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("GlassBarNotifications"),
            ..Default::default()
        };
        if RegisterClassExW(&notifications_class) == 0 {
            return Err(windows::core::Error::from_thread());
        }

        let preview_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(preview_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("GlassBarPreview"),
            ..Default::default()
        };
        if RegisterClassExW(&preview_class) == 0 {
            return Err(windows::core::Error::from_thread());
        }

        let top_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(topbar_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("GlassBarTop"),
            ..Default::default()
        };
        if RegisterClassExW(&top_class) == 0 {
            return Err(windows::core::Error::from_thread());
        }

        let settings_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(settings_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            // Es una ventana de aplicación: sale en el dock y en Alt+Tab con su icono.
            hIcon: tray::app_icon(),
            lpszClassName: w!("GlassBarSettings"),
            ..Default::default()
        };
        if RegisterClassExW(&settings_class) == 0 {
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
        app.sync_appbar();
        app.sync_top_bar();
        app.sync_notifications();
        app.render();

        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

        // Inicia el temporizador de animaciones (para el primer cuadro de entrada)
        app.start_animating();

        // Inicia el temporizador de reloj y verificación de apps (cada 1s)
        let _ = SetTimer(Some(hwnd), TIMER_TICK, TICK_MS, None);

        // Inicia el temporizador de proximidad al borde de pantalla (cada 60ms)
        let _ = SetTimer(Some(hwnd), TIMER_EDGE, EDGE_MS, None);

        // Al arrancar, los programas pueden registrar sus iconos antes de que todo
        // esté listo; se les vuelven a pedir con el bucle de mensajes ya en marcha.
        let _ = SetTimer(Some(hwnd), TIMER_ANNOUNCE, 2000, None);

        // Actualizaciones: poco después de arrancar, sin estorbar al inicio, y luego
        // cada 6 horas.
        let _ = SetTimer(Some(hwnd), TIMER_UPDATE_FIRST, 15_000, None);
        let _ = SetTimer(Some(hwnd), TIMER_UPDATE, update::CHECK_EVERY_MS, None);

        APP.with(|cell| *cell.borrow_mut() = Some(app));

        // La tecla Windows abre el lanzador propio; se quita al salir del bucle.
        let _winkey = winkey::WinKey::install(hwnd, WM_SHOW_LAUNCHER, WM_SHORTCUT);
        let _foreground = priority::ForegroundWatch::install(hwnd, WM_FOREGROUND);
        // Alt+Espacio abre el buscador rápido. Si otro programa ya tiene el atajo
        // (p. ej. PowerToys Run), Windows lo niega y se queda con ese programa.
        {
            use windows::Win32::UI::Input::KeyboardAndMouse::{
                RegisterHotKey, MOD_ALT, MOD_NOREPEAT, VK_SPACE,
            };
            let _ = RegisterHotKey(
                Some(hwnd),
                HOTKEY_SPOTLIGHT,
                MOD_ALT | MOD_NOREPEAT,
                VK_SPACE.0 as u32,
            );
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            frames::delivered(msg.hwnd, msg.message, msg.wParam.0);
            match alt_f4(&msg) {
                AltF4::Default => {}
                AltF4::Ignore => continue,
                AltF4::Escape => {
                    msg.message = windows::Win32::UI::WindowsAndMessaging::WM_KEYDOWN;
                    msg.wParam = WPARAM(0x1B);
                    // Sin `TranslateMessage`: no hace falta el carácter de Esc.
                    DispatchMessageW(&msg);
                    continue;
                }
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        APP.with(|cell| cell.borrow_mut().take());
        // Sin GlassBar no hay quien devuelva el umbral del ahorro de batería.
        toggles::restore_saver();
    }
    Ok(())
}

#[cfg(test)]
mod menu_tests {
    use super::*;
    #[test]
    fn alt_f4_never_closes_glassbar_or_destroys_its_panels() {
        assert_eq!(alt_f4_for("GlassBarWindow"), AltF4::Ignore);
        assert_eq!(alt_f4_for("GlassBarSettings"), AltF4::Default);
        assert_eq!(alt_f4_for("GlassBarCalendar"), AltF4::Escape);
        assert_eq!(alt_f4_for("GlassBarLauncher"), AltF4::Escape);
        assert_eq!(alt_f4_for("#32770"), AltF4::Default);
        let sc_close = windows::Win32::UI::WindowsAndMessaging::SC_CLOSE as usize;
        assert!(is_sc_close(WPARAM(sc_close)));
        assert!(is_sc_close(WPARAM(sc_close | 0x2)));
        assert!(!is_sc_close(WPARAM(
            windows::Win32::UI::WindowsAndMessaging::SC_MINIMIZE as usize
        )));
    }
    #[test]
    fn left_and_right_follow_the_bar_and_wrap() {
        let next = |open, step, language| neighbor_menu(open, step, language);
        assert_eq!(next(HoverTarget::Overflow, 1, true), HoverTarget::Language);
        assert_eq!(
            next(HoverTarget::Overflow, 1, false),
            HoverTarget::Status(0)
        );
        assert_eq!(next(HoverTarget::Clock, 1, true), HoverTarget::Overflow);
        assert_eq!(next(HoverTarget::Overflow, -1, true), HoverTarget::Clock);
        assert_eq!(
            next(HoverTarget::Status(0), -1, true),
            HoverTarget::Language
        );
    }
}
