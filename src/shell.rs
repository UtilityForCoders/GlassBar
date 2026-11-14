//! Sustitución temporal de la barra nativa, incluso cuando Explorer vuelve a mostrarla.
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM};
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
        free_work_area();
    }
    /// Vuelve a activar el ocultar automático si alguien lo desactivó con GlassBar
    /// abierto (desde Configuración): si no, la barra oculta vuelve a reservar su
    /// franja y las ventanas maximizadas dejan un hueco abajo. Lo que se devuelve
    /// al salir es lo último que eligió el usuario.
    pub fn keep_work_area(&self) {
        if self.active {
            free_work_area();
        }
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
                let _ = ShowWindowAsync(hwnd, SW_SHOW);
            }
        }
        restore_work_area();
    }
}

// ---------------------------------------------------------------------------
// Área de trabajo: oculta, la barra nativa sigue reservando su franja inferior
// (las ventanas maximizadas dejan un hueco) salvo que tenga activado el ocultar
// automático. GlassBar lo activa mientras está abierto y lo devuelve al salir.
// ---------------------------------------------------------------------------

/// Archivo que indica que el ocultar automático lo activó GlassBar (el usuario lo
/// tenía desactivado). Sobrevive a un cierre forzado: si ni el vigilante llegó a
/// devolverlo, se devuelve al cerrar la siguiente vez.
fn autohide_marker() -> Option<std::path::PathBuf> {
    Some(
        std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("GlassBar")
            .join("native-autohide.txt"),
    )
}

fn appbar_data() -> windows::Win32::UI::Shell::APPBARDATA {
    windows::Win32::UI::Shell::APPBARDATA {
        cbSize: std::mem::size_of::<windows::Win32::UI::Shell::APPBARDATA>() as u32,
        hWnd: explorer_tray().unwrap_or_default(),
        ..Default::default()
    }
}

/// Estado de la barra nativa (`ABS_AUTOHIDE`, `ABS_ALWAYSONTOP`).
fn native_state() -> u32 {
    use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_GETSTATE};
    let mut data = appbar_data();
    unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) as u32 }
}

/// Cambia el estado de la barra nativa. Explorer lo guarda y recalcula el área de
/// trabajo de todos los monitores.
fn set_native_state(state: u32) {
    use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_SETSTATE};
    let mut data = appbar_data();
    data.lParam = LPARAM(state as isize);
    unsafe {
        SHAppBarMessage(ABM_SETSTATE, &mut data);
    }
}

/// Activa el ocultar automático de la barra nativa si estaba desactivado, para que
/// las ventanas maximizadas lleguen al borde inferior. Solo lo cambia si puede
/// apuntarlo antes, para no dejarlo cambiado sin saber que hay que devolverlo.
fn free_work_area() {
    use windows::Win32::UI::Shell::ABS_AUTOHIDE;
    let state = native_state();
    if state & ABS_AUTOHIDE != 0 {
        return;
    }
    let Some(path) = autohide_marker() else {
        return;
    };
    let written = path
        .parent()
        .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
        && std::fs::write(&path, "1").is_ok();
    if written {
        set_native_state(state | ABS_AUTOHIDE);
    }
}

/// Desactiva el ocultar automático si lo activó GlassBar. Sin el archivo no hace
/// nada: el usuario ya lo tenía así.
fn restore_work_area() {
    use windows::Win32::UI::Shell::ABS_AUTOHIDE;
    let Some(path) = autohide_marker().filter(|p| p.exists()) else {
        return;
    };
    let state = native_state();
    if state & ABS_AUTOHIDE != 0 {
        set_native_state(state & !ABS_AUTOHIDE);
    }
    let _ = std::fs::remove_file(path);
}

/// Vuelve a mostrar las barras nativas desde otro proceso (el vigilante), cuando
/// GlassBar ya no está. También quita la transparencia que versiones anteriores de
/// GlassBar dejaban en la barra principal si morían a mitad de abrir su panel de
/// iconos ocultos.
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
    // Con `--replace` el GlassBar nuevo puede haber arrancado ya y activado el
    // ocultar automático: entonces se le deja.
    if unsafe { FindWindowW(w!("GlassBarWindow"), None) }.is_err() {
        restore_work_area();
    }
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

/// Muestra el escritorio, o vuelve a traer las ventanas, como la franja del extremo
/// de la barra de Windows (Win+D). Lo hace Explorer; en otro hilo, que COM lo pide.
pub fn toggle_desktop() {
    std::thread::spawn(|| unsafe {
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::UI::Shell::{IShellDispatch4, Shell};
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        if let Ok(shell) =
            CoCreateInstance::<_, IShellDispatch4>(&Shell, None, CLSCTX_INPROC_SERVER)
        {
            let _ = shell.ToggleDesktop();
        }
        if initialized {
            CoUninitialize();
        }
    });
}
