//! Apps que piden atención, como el botón naranja de la barra de Windows: un
//! programa hace parpadear su ventana (`FlashWindowEx`) al recibir un mensaje o
//! terminar algo en segundo plano, y Windows lo anuncia a las ventanas registradas
//! con `RegisterShellHookWindow`. El aviso dura hasta que la ventana se activa o se
//! cierra, igual que en Windows aunque ya haya dejado de parpadear.
//!
//! Los iconos superpuestos y el progreso (`ITaskbarList3`) no llegan aquí: Windows
//! se los manda solo a la barra de Explorer y no deja que otro programa ocupe su
//! lugar mientras Explorer está abierto.
use std::collections::HashSet;
use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    DeregisterShellHookWindow, GetForegroundWindow, IsWindow, RegisterShellHookWindow,
    RegisterWindowMessageW,
};

const HSHELL_WINDOWDESTROYED: usize = 2;
const HSHELL_WINDOWACTIVATED: usize = 4;
const HSHELL_RUDEAPPACTIVATED: usize = 0x8004;
const HSHELL_FLASH: usize = 0x8006;

/// Mensaje con el que llegan los avisos del Shell; 0 si no se pudo registrar.
static MESSAGE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Mensaje con el que llegan los avisos del Shell; 0 si no se pudo registrar.
pub fn hook_message() -> u32 {
    MESSAGE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Qué cambió con un aviso del Shell.
#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    None,
    /// Una ventana empezó a pedir atención.
    Started,
    /// Una ventana dejó de pedirla.
    Ended,
}

pub struct Attention {
    hwnd: HWND,
    message: u32,
    flashing: HashSet<isize>,
}

impl Attention {
    /// Registra `hwnd` para recibir los avisos del Shell. Si Windows no lo permite,
    /// el dock funciona igual, sin avisos.
    pub fn install(hwnd: HWND) -> Self {
        let message = unsafe {
            if RegisterShellHookWindow(hwnd).as_bool() {
                RegisterWindowMessageW(w!("SHELLHOOK"))
            } else {
                0
            }
        };
        MESSAGE.store(message, std::sync::atomic::Ordering::Relaxed);
        Self {
            hwnd,
            message,
            flashing: HashSet::new(),
        }
    }

    /// Atiende un aviso del Shell: `code` es su `wParam` y `window` su `lParam`.
    pub fn handle(&mut self, code: usize, window: isize) -> Change {
        match code {
            HSHELL_FLASH => {
                // La ventana activa no avisa: ya se está mirando.
                let foreground = unsafe { GetForegroundWindow() }.0 as isize;
                if window != foreground && self.flashing.insert(window) {
                    Change::Started
                } else {
                    Change::None
                }
            }
            HSHELL_WINDOWACTIVATED | HSHELL_RUDEAPPACTIVATED | HSHELL_WINDOWDESTROYED => {
                if self.flashing.remove(&window) {
                    Change::Ended
                } else {
                    Change::None
                }
            }
            _ => Change::None,
        }
    }

    /// Quita las ventanas que ya no existen o que están delante (por si el aviso de
    /// activación no llegó). Devuelve `true` si quitó alguna.
    pub fn prune(&mut self) -> bool {
        if self.flashing.is_empty() {
            return false;
        }
        let foreground = unsafe { GetForegroundWindow() }.0 as isize;
        let before = self.flashing.len();
        self.flashing.retain(|&window| {
            window != foreground && unsafe { IsWindow(Some(HWND(window as *mut _))) }.as_bool()
        });
        self.flashing.len() != before
    }

    pub fn contains(&self, window: isize) -> bool {
        self.flashing.contains(&window)
    }

    /// Primera de `windows` que pide atención.
    pub fn first_of(&self, windows: &[isize]) -> Option<isize> {
        windows.iter().copied().find(|w| self.contains(*w))
    }
}

impl Drop for Attention {
    fn drop(&mut self) {
        if self.message != 0 {
            unsafe {
                let _ = DeregisterShellHookWindow(self.hwnd);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attention() -> Attention {
        Attention {
            hwnd: HWND::default(),
            message: 0,
            flashing: HashSet::new(),
        }
    }

    #[test]
    fn a_flash_lasts_until_the_window_is_activated_or_closed() {
        let mut state = attention();
        // Ventanas que no existen: nunca son la activa.
        assert_eq!(state.handle(HSHELL_FLASH, 0x7ffe_1234), Change::Started);
        // Parpadea varias veces, pero solo empieza una vez.
        assert_eq!(state.handle(HSHELL_FLASH, 0x7ffe_1234), Change::None);
        assert_eq!(
            state.first_of(&[0x7ffe_9999, 0x7ffe_1234]),
            Some(0x7ffe_1234)
        );
        assert_eq!(
            state.handle(HSHELL_WINDOWACTIVATED, 0x7ffe_1234),
            Change::Ended
        );
        assert!(!state.contains(0x7ffe_1234));

        state.handle(HSHELL_FLASH, 0x7ffe_5678);
        assert_eq!(
            state.handle(HSHELL_WINDOWDESTROYED, 0x7ffe_5678),
            Change::Ended
        );
        assert_eq!(
            state.handle(HSHELL_WINDOWACTIVATED, 0x7ffe_5678),
            Change::None
        );
    }

    #[test]
    fn windows_that_no_longer_exist_stop_asking() {
        let mut state = attention();
        state.handle(HSHELL_FLASH, 0x7ffe_1234);
        assert!(state.prune());
        assert!(!state.contains(0x7ffe_1234));
        assert!(!state.prune());
    }
}
