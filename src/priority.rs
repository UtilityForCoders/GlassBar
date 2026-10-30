//! Prioridad del dock sobre las demás ventanas: vuelve a ponerse encima cuando cambia
//! la ventana activa, salvo si es una app a pantalla completa (juego, vídeo,
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

/// La ventana activa es de otra app y ocupa todo `monitor` sin barra de título.
pub fn fullscreen_app(monitor: RECT) -> bool {
    let _p = crate::profile::scope("priority.fullscreen_app");
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
