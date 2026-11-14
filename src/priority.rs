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
    SWP_NOMOVE, SWP_NOSIZE, WINEVENT_OUTOFCONTEXT, WS_CAPTION,
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
        covers(rect, monitor, style & WS_CAPTION.0 == WS_CAPTION.0)
    }
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
}
