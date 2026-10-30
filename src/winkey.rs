//! Tecla Windows: pulsada sola abre el lanzador de GlassBar en lugar del Inicio
//! nativo; las combinaciones (Win+E, Win+D, Win+L…) siguen llegando a Windows.
//!
//! El filtro vive en un hilo propio que no hace nada más. Si Windows tiene que
//! esperar a un filtro de teclado ocupado, deja pasar la tecla (y se abre su
//! Inicio) y, si pasa varias veces, lo quita sin avisar.
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_LWIN, VK_RWIN,
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
static WIN_DOWN: AtomicBool = AtomicBool::new(false);
static COMBO: AtomicBool = AtomicBool::new(false);

pub struct WinKey {
    /// Hilo del filtro, para pedirle que termine.
    thread: u32,
    handle: Option<std::thread::JoinHandle<()>>,
}
impl WinKey {
    /// Publica `message` en `target` cada vez que se pulsa y suelta Win sola.
    pub fn install(target: HWND, message: u32) -> Option<Self> {
        TARGET.store(target.0 as isize, Ordering::Relaxed);
        MESSAGE.store(message, Ordering::Relaxed);
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

unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let key = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        // Lo que inyectamos nosotros (y otros programas) pasa sin tocar.
        let injected = key.flags.0 & LLKHF_INJECTED.0 != 0;
        let is_win = key.vkCode == VK_LWIN.0 as u32 || key.vkCode == VK_RWIN.0 as u32;
        let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        if !injected {
            if is_win && down {
                if !WIN_DOWN.swap(true, Ordering::Relaxed) {
                    COMBO.store(false, Ordering::Relaxed);
                }
            } else if down && WIN_DOWN.load(Ordering::Relaxed) {
                COMBO.store(true, Ordering::Relaxed);
            } else if is_win
                && !down
                && WIN_DOWN.swap(false, Ordering::Relaxed)
                && !COMBO.load(Ordering::Relaxed)
            {
                // Se retiene esta liberación de Win y se reenvía tras la tecla máscara.
                let vk = key.vkCode as u16;
                let inputs = [
                    keyboard(MASK_KEY, false),
                    keyboard(MASK_KEY, true),
                    keyboard(vk, true),
                ];
                SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
                let target = HWND(TARGET.load(Ordering::Relaxed) as *mut _);
                let _ = PostMessageW(
                    Some(target),
                    MESSAGE.load(Ordering::Relaxed),
                    WPARAM(0),
                    LPARAM(0),
                );
                return LRESULT(1);
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
