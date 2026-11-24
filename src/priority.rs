//! Prioridad del dock sobre las demás ventanas: se pone encima al mostrarse y, a la
//! vista, cuando cambia la ventana activa, salvo si esta también es "siempre
//! encima" (no se le pelea el sitio) o es una app a pantalla completa (juego, vídeo,
//! presentación); entonces se aparta, como la barra de Windows.
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId,
    PostMessageW, SetWindowPos, EVENT_SYSTEM_FOREGROUND, GWL_STYLE, HWND_TOPMOST, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WINEVENT_OUTOFCONTEXT, WS_CAPTION,
};

static TARGET: AtomicIsize = AtomicIsize::new(0);
static MESSAGE: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn on_foreground(
    _: HWINEVENTHOOK,
    _: u32,
    _: HWND,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    // Solo avisa: el dock decide en su propio mensaje, fuera de cualquier préstamo.
    let target = HWND(TARGET.load(Ordering::Relaxed) as *mut _);
    let _ = PostMessageW(
        Some(target),
        MESSAGE.load(Ordering::Relaxed),
        WPARAM(0),
        LPARAM(0),
    );
}

/// Publica `message` en `target` cada vez que cambia la ventana activa.
pub struct ForegroundWatch {
    hook: HWINEVENTHOOK,
}
impl ForegroundWatch {
    pub fn install(target: HWND, message: u32) -> Option<Self> {
        TARGET.store(target.0 as isize, Ordering::Relaxed);
        MESSAGE.store(message, Ordering::Relaxed);
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(on_foreground),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        (!hook.is_invalid()).then_some(Self { hook })
    }
}
impl Drop for ForegroundWatch {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWinEvent(self.hook);
        }
    }
}

/// Lleva el dock (y sus ventanas propias, como el lanzador) al frente de las
/// ventanas "siempre encima", sin quitarle el foco a nadie.
pub fn raise(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// Pone `hwnd` encima de las demás ventanas, salvo de las "siempre encima" de
/// otros programas que se solapan con `area` (una isla como Notch Isla, un vídeo
/// flotante): se queda justo debajo de la más baja de ellas, así siguen viéndose y
/// recibiendo sus clics donde dibujan.
pub fn raise_below_overlapping(hwnd: HWND, area: RECT) {
    use windows::Win32::Foundation::{LPARAM, RECT as R};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, IsWindowVisible, GWL_EXSTYLE, WS_EX_TOPMOST,
    };
    struct Search {
        own: HWND,
        area: R,
        lowest: Option<HWND>,
    }
    unsafe extern "system" fn visit(window: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let search = &mut *(lparam.0 as *mut Search);
        if window == search.own || !IsWindowVisible(window).as_bool() {
            return true.into();
        }
        if GetWindowLongW(window, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 == 0 {
            return true.into();
        }
        let mut process = 0;
        GetWindowThreadProcessId(window, Some(&mut process));
        if process == std::process::id() {
            return true.into();
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut _,
            std::mem::size_of::<u32>() as u32,
        );
        let mut rect = R::default();
        if cloaked != 0 || GetWindowRect(window, &mut rect).is_err() {
            return true.into();
        }
        if overlaps(rect, search.area) {
            // EnumWindows va de arriba abajo: la última que se encuentra es la más baja.
            search.lowest = Some(window);
        }
        true.into()
    }
    let mut search = Search {
        own: hwnd,
        area,
        lowest: None,
    };
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize));
        match search.lowest {
            Some(above) => {
                let _ = SetWindowPos(
                    hwnd,
                    Some(above),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
            None => raise(hwnd),
        }
    }
}

/// La franja reservada de una barra solo aparta a las ventanas maximizadas: una
/// ventana normal que se abre donde la dejó el usuario sin la barra (o donde la
/// pone la app) puede quedar con su barra de título debajo de ella. Si la ventana
/// activa está así, se baja justo debajo de `strip` (arriba de `monitor`), sin
/// cambiar su tamaño.
pub fn keep_below(strip: RECT, monitor: RECT) {
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
    use windows::Win32::UI::WindowsAndMessaging::{
        IsIconic, IsWindowVisible, IsZoomed, GWL_EXSTYLE, WS_CHILD, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_THICKFRAME,
    };
    unsafe {
        let window = GetForegroundWindow();
        if window.is_invalid()
            || !IsWindowVisible(window).as_bool()
            || IsIconic(window).as_bool()
            || IsZoomed(window).as_bool()
            || is_desktop(window)
        {
            return;
        }
        let mut process = 0;
        GetWindowThreadProcessId(window, Some(&mut process));
        if process == std::process::id() {
            return;
        }
        let style = GetWindowLongW(window, GWL_STYLE) as u32;
        let ex_style = GetWindowLongW(window, GWL_EXSTYLE) as u32;
        // Solo ventanas de app normales: con título o borde para redimensionar
        // (también las de título propio, como Chrome o VS Code). Los menús,
        // avisos y ventanas "siempre encima" se dejan donde están.
        let framed = style & WS_CAPTION.0 == WS_CAPTION.0 || style & WS_THICKFRAME.0 != 0;
        if !framed
            || style & WS_CHILD.0 != 0
            || ex_style & (WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0) != 0
        {
            return;
        }
        let mut rect = RECT::default();
        if GetWindowRect(window, &mut rect).is_err() {
            return;
        }
        // El marco visible: `GetWindowRect` incluye los bordes invisibles.
        let mut frame = rect;
        let _ = DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut frame as *mut RECT as *mut _,
            std::mem::size_of::<RECT>() as u32,
        );
        if let Some(dy) = push_down(frame, strip, monitor) {
            let _ = SetWindowPos(
                window,
                None,
                rect.left,
                rect.top + dy,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

/// Cuánto hay que bajar una ventana con marco visible `frame` para que su parte
/// de arriba no quede debajo de `strip`, o `None` si no la tapa. Una que cubre
/// todo `monitor` (pantalla completa) no se toca.
fn push_down(frame: RECT, strip: RECT, monitor: RECT) -> Option<i32> {
    let fullscreen = frame.left <= monitor.left
        && frame.top <= monitor.top
        && frame.right >= monitor.right
        && frame.bottom >= monitor.bottom;
    (overlaps(frame, strip) && !fullscreen).then_some(strip.bottom - frame.top)
}

/// Los dos rectángulos comparten algún píxel (y ninguno está vacío).
fn overlaps(a: RECT, b: RECT) -> bool {
    a.right > a.left
        && a.bottom > a.top
        && a.left < b.right
        && b.left < a.right
        && a.top < b.bottom
        && b.top < a.bottom
}

/// La ventana activa es de otra app y está marcada como "siempre encima" (un
/// vídeo flotante, "Siempre visible" de PowerToys…): el dock no se le pone delante.
pub fn foreground_topmost() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GWL_EXSTYLE, WS_EX_TOPMOST};
    unsafe {
        let window = GetForegroundWindow();
        if window.is_invalid() {
            return false;
        }
        let mut process = 0;
        GetWindowThreadProcessId(window, Some(&mut process));
        process != std::process::id()
            && GetWindowLongW(window, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0
    }
}

/// Windows dice que hay un juego a pantalla completa exclusiva (Direct3D) o que
/// está en modo presentación: cuenta aunque esa app no tenga el foco.
pub fn system_fullscreen() -> bool {
    use windows::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
    };
    unsafe { SHQueryUserNotificationState() }
        .is_ok_and(|state| state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_PRESENTATION_MODE)
}

/// La ventana activa es de otra app y ocupa todo `monitor` sin barra de título.
pub fn fullscreen_app(monitor: RECT) -> bool {
    unsafe {
        let window = GetForegroundWindow();
        if window.is_invalid() {
            return false;
        }
        let mut process = 0;
        GetWindowThreadProcessId(window, Some(&mut process));
        if process == std::process::id() || is_desktop(window) {
            return false;
        }
        let mut rect = RECT::default();
        if GetWindowRect(window, &mut rect).is_err() {
            return false;
        }
        let style = GetWindowLongW(window, GWL_STYLE) as u32;
        let captioned = style & WS_CAPTION.0 == WS_CAPTION.0;
        if covers(rect, monitor, captioned) {
            return true;
        }
        let Some((screen, work)) = monitor_of(window) else {
            return false;
        };
        if !overlaps(screen, monitor) {
            return false;
        }
        // Juegos "sin bordes" que se desplazaron con la franja reservada de la barra:
        // tienen el tamaño del monitor pero empiezan más abajo. Se devuelven a la
        // esquina del monitor y cuentan como pantalla completa.
        if shifted_fullscreen(rect, screen, captioned) {
            let _ = SetWindowPos(
                window,
                None,
                screen.left,
                screen.top,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            return true;
        }
        // Un juego que Windows reconoce, sin título y llenando al menos el área de
        // trabajo, también: así no hace falta que tape el monitor entero.
        fills_work_area(rect, work, captioned) && crate::games::is_game(process)
    }
}

/// Rectángulo del monitor donde está casi toda `window` y su área de trabajo.
fn monitor_of(window: HWND) -> Option<(RECT, RECT)> {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONULL,
    };
    unsafe {
        let handle = MonitorFromWindow(window, MONITOR_DEFAULTTONULL);
        if handle.is_invalid() {
            return None;
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        GetMonitorInfoW(handle, &mut info)
            .as_bool()
            .then_some((info.rcMonitor, info.rcWork))
    }
}

/// Sin título y ocupando, como poco, toda el área de trabajo: un juego a pantalla
/// completa aunque la franja de una barra lo deje más bajo. Un lanzador o un juego
/// en ventana (más pequeños o con título) no cuentan.
fn fills_work_area(window: RECT, work: RECT, captioned: bool) -> bool {
    !captioned
        && window.left <= work.left
        && window.top <= work.top
        && window.right >= work.right
        && window.bottom >= work.bottom
}

/// Una ventana sin título con el tamaño exacto del monitor, movida un poco de su
/// esquina: un juego a pantalla completa sin bordes que Windows o el propio juego
/// corrió por la franja de una barra. Las apps maximizadas sin título (VS Code,
/// Spotify) ocupan el área de trabajo, más pequeña, y no cuentan.
fn shifted_fullscreen(window: RECT, monitor: RECT, captioned: bool) -> bool {
    let (width, height) = (monitor.right - monitor.left, monitor.bottom - monitor.top);
    let (dx, dy) = (window.left - monitor.left, window.top - monitor.top);
    !captioned
        && window.right - window.left == width
        && window.bottom - window.top == height
        && (dx, dy) != (0, 0)
        && dx.abs() < width / 4
        && dy.abs() < height / 4
}

/// El escritorio también ocupa todo el monitor sin título, pero no es pantalla completa.
fn is_desktop(window: HWND) -> bool {
    let mut name = [0u16; 32];
    let len = unsafe { GetClassNameW(window, &mut name) }.max(0) as usize;
    let class = String::from_utf16_lossy(&name[..len]);
    class == "Progman" || class == "WorkerW"
}

/// Una ventana maximizada normal conserva su barra de título; a pantalla completa no
/// la tiene y cubre el monitor entero.
fn covers(window: RECT, monitor: RECT, captioned: bool) -> bool {
    !captioned
        && window.left <= monitor.left
        && window.top <= monitor.top
        && window.right >= monitor.right
        && window.bottom >= monitor.bottom
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_needs_a_shared_pixel() {
        let rect = |left, top, right, bottom| RECT {
            left,
            top,
            right,
            bottom,
        };
        let bar = rect(0, 0, 1920, 28);
        assert!(overlaps(rect(655, 0, 1265, 16), bar));
        assert!(!overlaps(rect(0, 28, 1920, 1080), bar));
        assert!(!overlaps(rect(100, 0, 100, 16), bar));
    }

    #[test]
    fn only_borderless_windows_covering_the_monitor_count_as_fullscreen() {
        let rect = |left, top, right, bottom| RECT {
            left,
            top,
            right,
            bottom,
        };
        let monitor = rect(0, 0, 1920, 1080);
        // Juego o vídeo a pantalla completa.
        assert!(covers(monitor, monitor, false));
        assert!(covers(rect(-8, -8, 1928, 1088), monitor, false));
        // Ventana maximizada: tiene título aunque cubra el monitor.
        assert!(!covers(rect(-8, -8, 1928, 1088), monitor, true));
        // Sin título pero sin ocupar todo el monitor (una ventana flotante).
        assert!(!covers(rect(100, 100, 900, 700), monitor, false));
        assert!(!covers(rect(0, 0, 1920, 1040), monitor, false));
        // A pantalla completa en otro monitor.
        assert!(!covers(rect(1920, 0, 3840, 1080), monitor, false));
    }

    #[test]
    fn borderless_games_pushed_down_by_the_bar_count_as_fullscreen() {
        let rect = |left, top, right, bottom| RECT {
            left,
            top,
            right,
            bottom,
        };
        let monitor = rect(0, 0, 1920, 1080);
        // Rocket League sin bordes, 56 px más abajo por la barra superior.
        let game = rect(0, 56, 1920, 1136);
        assert!(!covers(game, monitor, false));
        assert!(shifted_fullscreen(game, monitor, false));
        // En su sitio ya lo cubre `covers`; no hay nada que mover.
        assert!(!shifted_fullscreen(monitor, monitor, false));
        // Maximizada sin título (VS Code): ocupa el área de trabajo, no el monitor.
        assert!(!shifted_fullscreen(rect(0, 28, 1920, 1080), monitor, false));
        // Con título no es un juego sin bordes.
        assert!(!shifted_fullscreen(game, monitor, true));
        // Del tamaño del monitor pero lejos de su esquina: no se toca.
        assert!(!shifted_fullscreen(
            rect(0, 600, 1920, 1680),
            monitor,
            false
        ));
        // En un segundo monitor a la izquierda.
        let left = rect(-1920, 0, 0, 1080);
        assert!(shifted_fullscreen(rect(-1920, 56, 0, 1136), left, false));
    }

    #[test]
    fn windows_under_the_top_bar_are_pushed_below_it() {
        let rect = |left, top, right, bottom| RECT {
            left,
            top,
            right,
            bottom,
        };
        let monitor = rect(0, 0, 1920, 1080);
        let strip = rect(0, 0, 1920, 28);
        // Abierta arriba del todo: baja lo justo para asomar bajo la barra.
        assert_eq!(push_down(rect(200, 0, 1000, 600), strip, monitor), Some(28));
        assert_eq!(
            push_down(rect(200, 10, 1000, 600), strip, monitor),
            Some(18)
        );
        // Ya debajo de la barra, o en otro monitor: no se mueve.
        assert_eq!(push_down(rect(200, 28, 1000, 600), strip, monitor), None);
        assert_eq!(push_down(rect(2000, 0, 2800, 600), strip, monitor), None);
        // A pantalla completa tapa la barra a propósito.
        assert_eq!(push_down(monitor, strip, monitor), None);
    }

    #[test]
    fn known_games_only_need_to_fill_the_work_area() {
        let rect = |left, top, right, bottom| RECT {
            left,
            top,
            right,
            bottom,
        };
        let work = rect(0, 28, 1920, 1080);
        // Juego sin bordes encajado bajo la barra superior.
        assert!(fills_work_area(work, work, false));
        assert!(fills_work_area(rect(0, 0, 1920, 1080), work, false));
        // Lanzador sin título (el cliente de League of Legends) o juego en ventana.
        assert!(!fills_work_area(rect(320, 180, 1600, 900), work, false));
        assert!(!fills_work_area(rect(-8, 20, 1928, 1088), work, true));
    }
}
