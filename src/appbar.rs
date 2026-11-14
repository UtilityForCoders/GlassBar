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
}
