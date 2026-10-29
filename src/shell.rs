//! Sustitución temporal de la barra nativa, incluso cuando Explorer vuelve a mostrarla.
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::*;

static REPLACING: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn on_show(
    _: HWINEVENTHOOK,
    _: u32,
    hwnd: HWND,
    object: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    if !REPLACING.load(Ordering::Relaxed) || object != 0 || hwnd.is_invalid() {
        return;
    }
    let mut name = [0u16; 128];
    let len = GetClassNameW(hwnd, &mut name);
    let class = String::from_utf16_lossy(&name[..len.max(0) as usize]);
    if class == "Shell_TrayWnd" || class == "Shell_SecondaryTrayWnd" {
        let _ = ShowWindowAsync(hwnd, SW_HIDE);
    }
}

fn taskbars() -> Vec<HWND> {
    let mut result = Vec::new();
    unsafe {
        if let Ok(hwnd) = FindWindowW(w!("Shell_TrayWnd"), None) {
            result.push(hwnd);
        }
        let mut previous = None;
        while let Ok(hwnd) = FindWindowExW(None, previous, w!("Shell_SecondaryTrayWnd"), None) {
            result.push(hwnd);
            previous = Some(hwnd);
        }
    }
    result
}

pub struct TaskbarReplacer {
    hook: HWINEVENTHOOK,
    active: bool,
}
impl TaskbarReplacer {
    pub fn new() -> Self {
        Self {
            hook: HWINEVENTHOOK::default(),
            active: false,
        }
    }
    pub fn hide(&mut self) {
        REPLACING.store(true, Ordering::Relaxed);
        self.active = true;
        unsafe {
            if self.hook.is_invalid() {
                self.hook = SetWinEventHook(
                    EVENT_OBJECT_SHOW,
                    EVENT_OBJECT_SHOW,
                    None,
                    Some(on_show),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
            }
        }
        self.maintain();
    }
    pub fn maintain(&self) {
        if !self.active {
            return;
        }
        for hwnd in taskbars() {
            unsafe {
                if IsWindowVisible(hwnd).as_bool() {
                    let _ = ShowWindowAsync(hwnd, SW_HIDE);
                }
            }
        }
    }
    pub fn restore(&mut self) {
        if !self.active {
            return;
        }
        REPLACING.store(false, Ordering::Relaxed);
        unsafe {
            if !self.hook.is_invalid() {
                let _ = UnhookWinEvent(self.hook);
                self.hook = HWINEVENTHOOK::default();
            }
        }
        self.active = false;
        for hwnd in taskbars() {
            unsafe {
                let _ = ShowWindowAsync(hwnd, SW_SHOW);
            }
        }
    }
}
impl Drop for TaskbarReplacer {
    fn drop(&mut self) {
        self.restore();
    }
}
