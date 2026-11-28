//! Barras de aplicación: Windows reserva su franja y las ventanas maximizadas no
//! la tapan. Las usan el dock en el modo "siempre visible" (abajo) y la barra
//! superior (arriba).
use std::cell::Cell;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABE_BOTTOM, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, APPBARDATA,
};

/// Aviso de Windows de que cambió alguna barra (`ABN_POSCHANGED`): hay que volver a
/// pedir el sitio.
pub const ABN_POSCHANGED: usize = 1;
/// Aviso de Windows de que se abrió (`lParam` distinto de 0) o se cerró una app a
/// pantalla completa en el monitor de la barra.
pub const ABN_FULLSCREENAPP: usize = 2;

pub struct AppBar {
    hwnd: HWND,
    /// La última franja que se pidió con `ABM_SETPOS`.
    placed: Cell<Option<RECT>>,
}

fn data(hwnd: HWND) -> APPBARDATA {
    APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        ..Default::default()
    }
}

/// La franja que se reserva: `height` píxeles en el borde inferior de `monitor`,
/// o en el superior con `top`.
pub fn strip(monitor: RECT, height: i32, top: bool) -> RECT {
    if top {
        RECT {
            bottom: monitor.top + height,
            ..monitor
        }
    } else {
        RECT {
            top: monitor.bottom - height,
            ..monitor
        }
    }
}

impl AppBar {
    /// Registra `hwnd` como barra de aplicación; Windows le avisa con `callback`.
    pub fn register(hwnd: HWND, callback: u32) -> Option<Self> {
        let mut data = data(hwnd);
        data.uCallbackMessage = callback;
        let registered = unsafe { SHAppBarMessage(ABM_NEW, &mut data) } != 0;
        registered.then_some(Self {
            hwnd,
            placed: Cell::new(None),
        })
    }

    /// Reserva `height` píxeles en el borde inferior de `monitor`, o en el
    /// superior con `top`.
    pub fn place(&self, monitor: RECT, height: i32, top: bool) {
        let mut data = data(self.hwnd);
        data.uEdge = if top { ABE_TOP } else { ABE_BOTTOM };
        data.rc = strip(monitor, height, top);
        unsafe {
            SHAppBarMessage(ABM_QUERYPOS, &mut data);
            // Windows puede mover el borde si otra barra ya ocupa el sitio.
            if top {
                data.rc.bottom = data.rc.top + height;
            } else {
                data.rc.top = data.rc.bottom - height;
            }
            // Cada `ABM_SETPOS` hace que Windows avise con `ABN_POSCHANGED` a las demás
            // barras, y al recibirlo se vuelve a llamar aquí. Si la franja no cambió no
            // se vuelve a pedir: con el dock y la barra superior a la vez, o con la
            // barra nativa cambiando de estado al arrancar, los avisos se rebotaban
            // sin fin y las barras se recolocaban (parpadeaban) una y otra vez.
            if !needs_setpos(self.placed.get(), data.rc) {
                ensure_work_area(monitor, data.rc, top);
                return;
            }
            data.lParam = LPARAM(0);
            SHAppBarMessage(ABM_SETPOS, &mut data);
        }
        self.placed.set(Some(data.rc));
        ensure_work_area(monitor, data.rc, top);
        refit_maximized(monitor);
    }
}

/// Si hay que volver a pedir la franja `wanted`: solo si es otra que la última pedida.
fn needs_setpos(placed: Option<RECT>, wanted: RECT) -> bool {
    placed != Some(wanted)
}

/// Dónde debería estar una ventana maximizada en el área de trabajo `work`, si está
/// en otro sitio. Windows la maximiza un poco más grande que el área, por el grosor
/// de su borde, que se mide en el lado izquierdo (las barras solo cambian el de
/// arriba o el de abajo). Solo se mueve si ocupa el ancho del área (está
/// maximizada en este monitor) y no coincide arriba o abajo.
pub fn maximized_rect(window: RECT, work: RECT) -> Option<RECT> {
    let border = work.left - window.left;
    if !(0..=32).contains(&border) {
        return None;
    }
    let expected = RECT {
        left: work.left - border,
        top: work.top - border,
        right: work.right + border,
        bottom: work.bottom + border,
    };
    let near = |a: i32, b: i32| (a - b).abs() <= 1;
    if !near(window.right, expected.right) {
        return None;
    }
    let fits = near(window.top, expected.top) && near(window.bottom, expected.bottom);
    (!fits).then_some(expected)
}

/// Windows no siempre recoloca las ventanas que ya estaban maximizadas cuando una
/// barra reserva su franja (pasa al abrir GlassBar con ventanas abiertas): se
/// quedarían debajo de la barra hasta volver a pulsarlas. Se ajustan a mano al área
/// de trabajo del monitor de `monitor`.
fn refit_maximized(monitor: RECT) {
    use windows::core::BOOL;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromRect, MonitorFromWindow, HMONITOR, MONITORINFO,
        MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, IsZoomed,
        SetWindowPos, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER,
    };
    struct Target {
        monitor: HMONITOR,
        work: RECT,
    }
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let target = &*(lparam.0 as *const Target);
        if !IsWindowVisible(hwnd).as_bool() || !IsZoomed(hwnd).as_bool() {
            return true.into();
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == std::process::id()
            || MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) != target.monitor
        {
            return true.into();
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return true.into();
        }
        if let Some(fit) = maximized_rect(rect, target.work) {
            let _ = SetWindowPos(
                hwnd,
                None,
                fit.left,
                fit.top,
                fit.right - fit.left,
                fit.bottom - fit.top,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
        true.into()
    }
    unsafe {
        let handle = MonitorFromRect(&monitor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(handle, &mut info).as_bool() {
            return;
        }
        let target = Target {
            monitor: handle,
            work: info.rcWork,
        };
        let _ = EnumWindows(Some(visit), LPARAM(&target as *const Target as isize));
    }
}

/// El área de trabajo que hace falta para que `strip` quede fuera, o `None` si
/// `work` ya la deja fuera.
pub fn missing_reservation(work: RECT, strip: RECT, top: bool) -> Option<RECT> {
    if top {
        (work.top < strip.bottom).then_some(RECT {
            top: strip.bottom,
            ..work
        })
    } else {
        (work.bottom > strip.top).then_some(RECT {
            bottom: strip.top,
            ..work
        })
    }
}

/// Explorer acepta la franja pero a veces no recalcula el área de trabajo hasta
/// que cambia otra barra (pasa tras reiniciar GlassBar con la barra nativa oculta):
/// las ventanas se maximizarían por debajo de la barra. Si no la recalculó, se pone
/// a mano; Windows recoloca entonces las ventanas maximizadas.
fn ensure_work_area(monitor: RECT, strip: RECT, top: bool) {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPIF_SENDCHANGE, SPI_SETWORKAREA,
    };
    unsafe {
        let handle = MonitorFromRect(&monitor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(handle, &mut info).as_bool() {
            return;
        }
        if let Some(mut work) = missing_reservation(info.rcWork, strip, top) {
            let _ = SystemParametersInfoW(
                SPI_SETWORKAREA,
                0,
                Some(&mut work as *mut RECT as *mut _),
                SPIF_SENDCHANGE,
            );
        }
    }
}

impl Drop for AppBar {
    fn drop(&mut self) {
        let mut data = data(self.hwnd);
        unsafe {
            SHAppBarMessage(ABM_REMOVE, &mut data);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn the_same_strip_is_not_asked_for_twice() {
        let strip = rect(0, 1010, 1920, 1080);
        assert!(needs_setpos(None, strip));
        assert!(!needs_setpos(Some(strip), strip));
        assert!(needs_setpos(Some(rect(0, 1020, 1920, 1080)), strip));
    }

    #[test]
    fn the_strip_is_the_bottom_of_the_monitor() {
        let monitor = RECT {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1080,
        };
        let rc = strip(monitor, 70, false);
        assert_eq!(
            (rc.left, rc.top, rc.right, rc.bottom),
            (-1920, 1010, 0, 1080)
        );
        let rc = strip(monitor, 28, true);
        assert_eq!((rc.left, rc.top, rc.right, rc.bottom), (-1920, 0, 0, 28));
    }

    #[test]
    fn the_work_area_leaves_the_strip_out() {
        let rect = |left, top, right, bottom| RECT {
            left,
            top,
            right,
            bottom,
        };
        let monitor = rect(0, 0, 1920, 1080);
        let top = strip(monitor, 28, true);
        let fixed = missing_reservation(monitor, top, true).unwrap();
        assert_eq!((fixed.top, fixed.bottom), (28, 1080));
        assert!(missing_reservation(fixed, top, true).is_none());
        let bottom = strip(monitor, 70, false);
        let fixed = missing_reservation(rect(0, 28, 1920, 1080), bottom, false).unwrap();
        assert_eq!((fixed.top, fixed.bottom), (28, 1010));
    }

    #[test]
    fn maximized_windows_under_the_bar_move_into_the_work_area() {
        // Barra superior de 28 px: el área de trabajo empieza en 28.
        let work = rect(0, 28, 1920, 1080);
        // Maximizada antes de la barra, con su borde de 8 px.
        let under = rect(-8, -8, 1928, 1088);
        assert_eq!(maximized_rect(under, work), Some(rect(-8, 20, 1928, 1088)));
        // Ya en su sitio, o sin borde y en su sitio: nada que hacer.
        assert_eq!(maximized_rect(rect(-8, 20, 1928, 1088), work), None);
        assert_eq!(maximized_rect(rect(0, 28, 1920, 1080), work), None);
        // Sin la barra (el área vuelve a ser toda la pantalla), crece de nuevo.
        let full = rect(0, 0, 1920, 1080);
        assert_eq!(
            maximized_rect(rect(-8, 20, 1928, 1088), full),
            Some(rect(-8, -8, 1928, 1088))
        );
        // No ocupa el ancho del área: no se toca.
        assert_eq!(maximized_rect(rect(-8, -8, 1000, 1088), work), None);
    }
}
