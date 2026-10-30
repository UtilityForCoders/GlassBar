//! Sustitución temporal de la barra nativa, incluso cuando Explorer vuelve a mostrarla.
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use windows::core::{w, BOOL};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, RECT};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::*;

static REPLACING: AtomicBool = AtomicBool::new(false);
/// Estilo extendido original de la barra nativa mientras está invisible para abrir
/// el panel; 0 si no se ha tocado.
static SAVED_STYLE: AtomicIsize = AtomicIsize::new(0);

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
    let class = class_of(hwnd);
    if class == "Shell_TrayWnd" || class == "Shell_SecondaryTrayWnd" {
        let _ = ShowWindowAsync(hwnd, SW_HIDE);
    }
}

/// La barra principal de Explorer. Se salta la `Shell_TrayWnd` propia de GlassBar
/// (ver `systray`), que se pone delante para recibir los iconos de la bandeja.
pub fn explorer_tray() -> Option<HWND> {
    let own = std::process::id();
    let mut previous = None;
    unsafe {
        while let Ok(hwnd) = FindWindowExW(None, previous, w!("Shell_TrayWnd"), None) {
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid != own {
                return Some(hwnd);
            }
            previous = Some(hwnd);
        }
    }
    None
}

fn taskbars() -> Vec<HWND> {
    let mut result = Vec::new();
    unsafe {
        if let Some(hwnd) = explorer_tray() {
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
    /// GlassBar está sustituyendo la barra nativa.
    pub fn is_active(&self) -> bool {
        self.active
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
                if Some(hwnd) == explorer_tray() {
                    restore_opacity(hwnd);
                }
                let _ = ShowWindowAsync(hwnd, SW_SHOW);
            }
        }
    }
}
/// Vuelve a mostrar las barras nativas desde otro proceso (el vigilante), cuando
/// GlassBar ya no está. Si murió a mitad de abrir el panel de iconos ocultos, también
/// quita la transparencia que dejó en la barra principal.
pub fn restore_native() {
    for hwnd in taskbars() {
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let transparent = WS_EX_TRANSPARENT.0 as isize;
            if style & transparent != 0 {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style & !transparent);
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
            }
            let _ = ShowWindowAsync(hwnd, SW_SHOW);
        }
    }
}

/// Abre (o cierra) el panel nativo de iconos ocultos y lo coloca sobre `anchor_x`,
/// con su borde inferior en `anchor_top`. Se ejecuta en otro hilo para no bloquear el dock.
pub fn toggle_overflow(anchor_x: i32, anchor_top: i32, screen_left: i32, screen_right: i32) {
    std::thread::spawn(move || unsafe {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        open_overflow(anchor_x, anchor_top, screen_left, screen_right);
        if initialized {
            CoUninitialize();
        }
    });
}

unsafe fn open_overflow(anchor_x: i32, anchor_top: i32, screen_left: i32, screen_right: i32) {
    let Some(tray) = explorer_tray() else {
        return;
    };
    // Explorer no responde a la flecha de la barra oculta; se muestra un instante,
    // totalmente transparente (y sin recibir clics), para pulsarla sin que se vea.
    let replacing = REPLACING.swap(false, Ordering::Relaxed);
    let was_visible = IsWindowVisible(tray).as_bool();
    if !was_visible {
        let style = GetWindowLongPtrW(tray, GWL_EXSTYLE);
        if SAVED_STYLE
            .compare_exchange(0, style, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            // Opacidad 0 no basta para que los clics la atraviesen: hace falta WS_EX_TRANSPARENT.
            let invisible = (WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0) as isize;
            SetWindowLongPtrW(tray, GWL_EXSTYLE, style | invisible);
            let _ = SetLayeredWindowAttributes(tray, COLORREF(0), 0, LWA_ALPHA);
        }
        let _ = ShowWindow(tray, SW_SHOWNA);
    }
    // Si ya está abierto, la misma pulsación lo cierra, como en la barra nativa.
    let was_open = overflow_window().is_some();
    let invoked = invoke_chevron(tray);
    let mut flyout = None;
    if invoked && !was_open {
        for _ in 0..100 {
            flyout = overflow_window();
            if flyout.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
    }
    if !was_visible {
        if replacing {
            let _ = ShowWindow(tray, SW_HIDE);
        }
        // Ya oculta, recupera su estilo original sin que se note.
        restore_opacity(tray);
    }
    REPLACING.store(replacing, Ordering::Relaxed);
    let Some(flyout) = flyout else {
        return;
    };
    let mut rect = RECT::default();
    if GetWindowRect(flyout, &mut rect).is_ok() {
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        let x =
            (anchor_x - w / 2).clamp(screen_left + 8, (screen_right - w - 8).max(screen_left + 8));
        let _ = SetWindowPos(
            flyout,
            None,
            x,
            anchor_top - h - 8,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Quita la transparencia añadida para abrir el panel; también al salir, por si
/// GlassBar se cerró a mitad de la apertura.
unsafe fn restore_opacity(tray: HWND) {
    let style = SAVED_STYLE.swap(0, Ordering::Relaxed);
    if style != 0 {
        SetWindowLongPtrW(tray, GWL_EXSTYLE, style);
    }
}

/// Pulsa "Mostrar iconos ocultos" mediante UI Automation. Es el primer botón
/// normal de la bandeja con nombre de una línea (el indicador de idioma tiene varias),
/// así no depende del idioma de Windows.
unsafe fn invoke_chevron(tray: HWND) -> bool {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Accessibility::*;
    let Some(bridge) = find_child(tray, "Windows.UI.Composition.DesktopWindowContentBridge") else {
        return false;
    };
    let result = (|| -> windows::core::Result<bool> {
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        let root = uia.ElementFromHandle(bridge)?;
        let condition = uia.CreatePropertyCondition(
            UIA_AutomationIdPropertyId,
            &VARIANT::from("SystemTrayIcon"),
        )?;
        let buttons = root.FindAll(TreeScope_Descendants, &condition)?;
        for i in 0..buttons.Length()? {
            let button = buttons.GetElement(i)?;
            if button.CurrentClassName()? != "SystemTray.NormalButton" {
                continue;
            }
            if button.CurrentName()?.to_string().contains('\n') {
                return Ok(false);
            }
            let invoke: IUIAutomationInvokePattern =
                button.GetCurrentPatternAs(UIA_InvokePatternId)?;
            invoke.Invoke()?;
            return Ok(true);
        }
        Ok(false)
    })();
    result.unwrap_or(false)
}

/// La ventana activa es el Inicio nativo. Puede abrirse aunque se filtre la tecla
/// Windows: con una ventana de administrador activa (el filtro no ve sus teclas) o
/// con Ctrl+Esc.
pub fn native_start_open() -> bool {
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        crate::apps::process_info(pid).is_some_and(|(exe, _)| {
            exe.rsplit('\\')
                .next()
                .is_some_and(|file| file.eq_ignore_ascii_case("StartMenuExperienceHost.exe"))
        })
    }
}

/// Ventana del panel de iconos ocultos, si está abierto. `FindWindowW` no la encuentra.
pub fn overflow_window() -> Option<HWND> {
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let found = &mut *(lparam.0 as *mut Option<HWND>);
        if IsWindowVisible(hwnd).as_bool()
            && class_of(hwnd) == "TopLevelWindowForOverflowXamlIsland"
        {
            *found = Some(hwnd);
            return false.into();
        }
        true.into()
    }
    let mut found = None;
    unsafe {
        let _ = EnumWindows(
            Some(visit),
            LPARAM(&mut found as *mut Option<HWND> as isize),
        );
    }
    found
}

fn find_child(parent: HWND, class: &str) -> Option<HWND> {
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let (class, found) = &mut *(lparam.0 as *mut (&str, Option<HWND>));
        if class_of(hwnd) == *class {
            *found = Some(hwnd);
            return false.into();
        }
        true.into()
    }
    let mut search = (class, None);
    unsafe {
        let _ = EnumChildWindows(
            Some(parent),
            Some(visit),
            LPARAM(&mut search as *mut (&str, Option<HWND>) as isize),
        );
    }
    search.1
}

fn class_of(hwnd: HWND) -> String {
    let mut name = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, &mut name) };
    String::from_utf16_lossy(&name[..len.max(0) as usize])
}

impl Drop for TaskbarReplacer {
    fn drop(&mut self) {
        self.restore();
    }
}
