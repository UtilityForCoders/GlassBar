//! Tecla Windows: pulsada sola abre el lanzador de GlassBar en lugar del Inicio
//! nativo. Los atajos que la barra de Windows atiende (Win+1…9, Win+T, Win+B,
//! Win+A, Win+N, Win+Alt+D) los atiende GlassBar: con la barra nativa oculta irían
//! a ella. Las demás combinaciones (Win+E, Win+D, Win+L…) siguen llegando a Windows.
//!
//! El filtro vive en un hilo propio que no hace nada más. Si Windows tiene que
//! esperar a un filtro de teclado ocupado, deja pasar la tecla (y se abre su
//! Inicio) y, si pasa varias veces, lo quita sin avisar.
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN,
    VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, PostMessageW, PostThreadMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
};

/// Tecla sin asignar que Windows ignora; intercalada antes de soltar Win, hace que
/// la pulsación cuente como combinación y el Inicio no se abra.
const MASK_KEY: u16 = 0xE8;

static TARGET: AtomicIsize = AtomicIsize::new(0);
static MESSAGE: AtomicU32 = AtomicU32::new(0);
static SHORTCUT_MESSAGE: AtomicU32 = AtomicU32::new(0);
static WIN_DOWN: AtomicBool = AtomicBool::new(false);
static COMBO: AtomicBool = AtomicBool::new(false);
/// Con Win pulsada se atendió algún atajo: Windows no vio esa tecla y, al soltar
/// Win, abriría su Inicio si no se intercala la tecla máscara.
static SWALLOWED: AtomicBool = AtomicBool::new(false);
/// Teclas de atajo atendidas que aún no se han soltado (una por bit, por su código
/// virtual): su repetición y su liberación tampoco llegan a Windows.
static HELD: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];

/// Cómo abrir la app de un atajo Win+número.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppMode {
    /// La trae al frente (o la abre); si ya está delante, pasa a su siguiente ventana.
    Switch,
    /// Win+Mayús+número: otra instancia.
    NewInstance,
    /// Win+Ctrl+Mayús+número: como administrador.
    Admin,
    /// Win+Alt+número: su menú (jump list).
    Menu,
}

/// Atajo de la barra que atiende GlassBar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    /// Win+1…9 y Win+0: la app en esa posición del dock (0 es la décima).
    App(usize, AppMode),
    /// Win+T: el dock con el teclado, desde el primer programa.
    FocusDock,
    /// Win+B: el dock con el teclado, en la flecha de los iconos ocultos.
    FocusTray,
    /// Win+A: el panel rápido.
    Quick,
    /// Win+N y Win+Alt+D: el calendario.
    Calendar,
}

const MODE_SHIFT: usize = 8;

impl Shortcut {
    /// El atajo de la tecla `vk` pulsada con Win y estos modificadores.
    pub fn from_key(vk: u32, shift: bool, ctrl: bool, alt: bool) -> Option<Self> {
        let digit = match vk {
            0x30 => Some(9),
            0x31..=0x39 => Some((vk - 0x31) as usize),
            _ => None,
        };
        if let Some(index) = digit {
            let mode = match (shift, ctrl, alt) {
                (false, _, false) => AppMode::Switch,
                (true, false, false) => AppMode::NewInstance,
                (true, true, false) => AppMode::Admin,
                (false, false, true) => AppMode::Menu,
                _ => return None,
            };
            return Some(Shortcut::App(index, mode));
        }
        if shift || ctrl {
            return None;
        }
        match (vk, alt) {
            (0x54, false) => Some(Shortcut::FocusDock),
            (0x42, false) => Some(Shortcut::FocusTray),
            (0x41, false) => Some(Shortcut::Quick),
            (0x4E, false) | (0x44, true) => Some(Shortcut::Calendar),
            _ => None,
        }
    }

    /// Para mandarlo en un mensaje.
    pub fn encode(self) -> usize {
        match self {
            Shortcut::App(index, mode) => {
                let mode = match mode {
                    AppMode::Switch => 0,
                    AppMode::NewInstance => 1,
                    AppMode::Admin => 2,
                    AppMode::Menu => 3,
                };
                1 | (mode << 4) | (index << MODE_SHIFT)
            }
            Shortcut::FocusDock => 2,
            Shortcut::FocusTray => 3,
            Shortcut::Quick => 4,
            Shortcut::Calendar => 5,
        }
    }

    pub fn decode(value: usize) -> Option<Self> {
        Some(match value & 0xF {
            1 => {
                let mode = match (value >> 4) & 0xF {
                    0 => AppMode::Switch,
                    1 => AppMode::NewInstance,
                    2 => AppMode::Admin,
                    3 => AppMode::Menu,
                    _ => return None,
                };
                Shortcut::App(value >> MODE_SHIFT, mode)
            }
            2 => Shortcut::FocusDock,
            3 => Shortcut::FocusTray,
            4 => Shortcut::Quick,
            5 => Shortcut::Calendar,
            _ => return None,
        })
    }
}

pub struct WinKey {
    /// Hilo del filtro, para pedirle que termine.
    thread: u32,
    handle: Option<std::thread::JoinHandle<()>>,
}
impl WinKey {
    /// Publica `message` en `target` cada vez que se pulsa y suelta Win sola, y
    /// `shortcut` (con el atajo codificado en `wparam`) con cada atajo de la barra.
    pub fn install(target: HWND, message: u32, shortcut: u32) -> Option<Self> {
        TARGET.store(target.0 as isize, Ordering::Relaxed);
        MESSAGE.store(message, Ordering::Relaxed);
        SHORTCUT_MESSAGE.store(shortcut, Ordering::Relaxed);
        let (ready, installed) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || unsafe {
            let module = GetModuleHandleW(None).ok();
            let hook = module.and_then(|m| {
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), Some(m.into()), 0).ok()
            });
            let thread = windows::Win32::System::Threading::GetCurrentThreadId();
            let _ = ready.send(hook.map(|_| thread));
            let Some(hook) = hook else {
                return;
            };
            // Windows llama al filtro desde el bucle de mensajes de este hilo.
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
            let _ = UnhookWindowsHookEx(hook);
        });
        let thread = installed.recv().ok().flatten()?;
        Some(Self {
            thread,
            handle: Some(handle),
        })
    }
}
impl Drop for WinKey {
    fn drop(&mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn held(vk: u32) -> (&'static AtomicU32, u32) {
    let vk = vk & 0xFF;
    (&HELD[(vk / 32) as usize], 1 << (vk % 32))
}

fn pressed(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) < 0 }
}

fn post(message: &AtomicU32, wparam: usize) {
    let target = HWND(TARGET.load(Ordering::Relaxed) as *mut _);
    unsafe {
        let _ = PostMessageW(
            Some(target),
            message.load(Ordering::Relaxed),
            WPARAM(wparam),
            LPARAM(0),
        );
    }
}

unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let key = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        // Lo que inyectamos nosotros (y otros programas) pasa sin tocar.
        let injected = key.flags.0 & LLKHF_INJECTED.0 != 0;
        let is_win = key.vkCode == VK_LWIN.0 as u32 || key.vkCode == VK_RWIN.0 as u32;
        let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        if !injected {
            let (slot, bit) = held(key.vkCode);
            if !is_win && !down && slot.fetch_and(!bit, Ordering::Relaxed) & bit != 0 {
                // Se suelta una tecla de atajo ya atendida.
                return LRESULT(1);
            }
            if is_win && down {
                if !WIN_DOWN.swap(true, Ordering::Relaxed) {
                    COMBO.store(false, Ordering::Relaxed);
                    SWALLOWED.store(false, Ordering::Relaxed);
                }
            } else if down && WIN_DOWN.load(Ordering::Relaxed) {
                COMBO.store(true, Ordering::Relaxed);
                // Mantenida, la tecla se repite: el atajo solo cuenta una vez.
                if slot.load(Ordering::Relaxed) & bit != 0 {
                    return LRESULT(1);
                }
                let shortcut = Shortcut::from_key(
                    key.vkCode,
                    pressed(VK_SHIFT),
                    pressed(VK_CONTROL),
                    pressed(VK_MENU),
                );
                if let Some(shortcut) = shortcut {
                    slot.fetch_or(bit, Ordering::Relaxed);
                    SWALLOWED.store(true, Ordering::Relaxed);
                    post(&SHORTCUT_MESSAGE, shortcut.encode());
                    return LRESULT(1);
                }
            } else if is_win && !down && WIN_DOWN.swap(false, Ordering::Relaxed) {
                let alone = !COMBO.load(Ordering::Relaxed);
                if alone || SWALLOWED.swap(false, Ordering::Relaxed) {
                    // Se retiene esta liberación de Win y se reenvía tras la tecla
                    // máscara: así no se abre el Inicio nativo.
                    let vk = key.vkCode as u16;
                    let inputs = [
                        keyboard(MASK_KEY, false),
                        keyboard(MASK_KEY, true),
                        keyboard(vk, true),
                    ];
                    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
                    if alone {
                        post(&MESSAGE, 0);
                    }
                    return LRESULT(1);
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

fn keyboard(vk: u16, up: bool) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if vk == VK_LWIN.0 || vk == VK_RWIN.0 {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_pick_the_app_and_the_modifiers_how_to_open_it() {
        let key = |vk, shift, ctrl, alt| Shortcut::from_key(vk, shift, ctrl, alt);
        assert_eq!(
            key(0x31, false, false, false),
            Some(Shortcut::App(0, AppMode::Switch))
        );
        assert_eq!(
            key(0x30, false, false, false),
            Some(Shortcut::App(9, AppMode::Switch))
        );
        assert_eq!(
            key(0x33, false, true, false),
            Some(Shortcut::App(2, AppMode::Switch))
        );
        assert_eq!(
            key(0x32, true, false, false),
            Some(Shortcut::App(1, AppMode::NewInstance))
        );
        assert_eq!(
            key(0x32, true, true, false),
            Some(Shortcut::App(1, AppMode::Admin))
        );
        assert_eq!(
            key(0x35, false, false, true),
            Some(Shortcut::App(4, AppMode::Menu))
        );
        assert_eq!(key(0x35, true, false, true), None);
    }

    #[test]
    fn letters_open_the_dock_panels_and_leave_the_rest_to_windows() {
        let key = |vk, alt| Shortcut::from_key(vk, false, false, alt);
        assert_eq!(key(0x54, false), Some(Shortcut::FocusDock));
        assert_eq!(key(0x42, false), Some(Shortcut::FocusTray));
        assert_eq!(key(0x41, false), Some(Shortcut::Quick));
        assert_eq!(key(0x4E, false), Some(Shortcut::Calendar));
        assert_eq!(key(0x44, true), Some(Shortcut::Calendar));
        // Win+D (mostrar escritorio), Win+E, Win+L: de Windows.
        assert_eq!(key(0x44, false), None);
        assert_eq!(key(0x45, false), None);
        assert_eq!(key(0x4C, false), None);
        assert_eq!(Shortcut::from_key(0x41, true, false, false), None);
    }

    #[test]
    fn shortcuts_survive_the_message() {
        let all = [
            Shortcut::App(0, AppMode::Switch),
            Shortcut::App(9, AppMode::Menu),
            Shortcut::App(3, AppMode::Admin),
            Shortcut::App(7, AppMode::NewInstance),
            Shortcut::FocusDock,
            Shortcut::FocusTray,
            Shortcut::Quick,
            Shortcut::Calendar,
        ];
        for shortcut in all {
            assert_eq!(Shortcut::decode(shortcut.encode()), Some(shortcut));
        }
        assert_eq!(Shortcut::decode(0), None);
    }
}
