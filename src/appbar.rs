//! Barras de aplicación: Windows reserva su franja y las ventanas maximizadas no
//! la tapan. Las usan el dock en el modo "siempre visible" (abajo) y la barra
//! superior (arriba).
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
        registered.then_some(Self { hwnd })
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
            data.lParam = LPARAM(0);
            SHAppBarMessage(ABM_SETPOS, &mut data);
        }
        ensure_work_area(monitor, data.rc, top);
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
}
