//! Programas anclados e iconos proporcionados por Windows.

use std::cell::RefCell;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};
use windows::core::{w, PCWSTR};
use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GetForegroundWindow, GetWindow, GetWindowLongPtrW,
    GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible,
    SetForegroundWindow, ShowWindow, SwitchToThisWindow, GWL_EXSTYLE, GW_OWNER, SW_MINIMIZE,
    SW_RESTORE, SW_SHOWNORMAL, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

use crate::jumplist::WindowApp;
use crate::spring::Spring;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct AppEntry {
    pub name: String,
    pub command: String,
}

pub struct AppItem {
    pub entry: AppEntry,
    process_name: String,
    /// Con qué se agrupan las ventanas abiertas sin anclar: el ejecutable o, si la
    /// ventana es una app aparte (una app web), su AppUserModelID.
    key: String,
    pub icon: Option<Pixmap>,
    /// Icono ya reducido al tamaño en pantalla.
    scaled: RefCell<Option<Pixmap>>,
    pub is_running: bool,
    /// Ventana abierta más reciente del programa, si tiene alguna.
    pub window: Option<isize>,
    /// Todas sus ventanas abiertas, de la más reciente a la más antigua.
    pub windows: Vec<isize>,
    /// Alguna de sus ventanas pide atención (ver `attention`).
    pub attention: bool,
    /// Para las apps de Store abiertas sin anclar: su ruta de AppsFolder, con la que
    /// se abren (su .exe no se puede abrir directamente).
    shell_path: Option<String>,
    pub hover_spring: Spring,
    pub press_spring: Spring,
    /// Desplazamiento horizontal mientras se reordena la barra.
    pub slide: Spring,
}

impl AppItem {
    pub fn new(entry: AppEntry) -> Self {
        // Una orden con argumentos (la que declara la ventana de una app web) toma el
        // icono de su programa y no se reconoce por él: ese programa es el navegador.
        let (program, args) = split_command_line(&entry.command);
        let icon = crate::icons::load(&entry.command).or_else(|| {
            args.is_some()
                .then(|| crate::icons::load(&program))
                .flatten()
        });
        let process_path =
            crate::pins::shortcut_target(&entry.command).unwrap_or_else(|| entry.command.clone());
        let process_name = if args.is_some() {
            String::new()
        } else {
            std::path::Path::new(&process_path)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        Self {
            key: entry.command.clone(),
            entry,
            process_name,
            icon,
            scaled: RefCell::new(None),
            is_running: false,
            window: None,
            windows: Vec::new(),
            attention: false,
            shell_path: None,
            hover_spring: Spring::new(1.0, 260.0, 18.0),
            press_spring: Spring::new(1.0, 340.0, 20.0),
            slide: Spring::new(0.0, 420.0, 36.0),
        }
    }

    /// Programa abierto sin anclar; las apps de Store toman icono y nombre de su
    /// paquete, y las apps web los de su entrada del menú Inicio o los que declara su
    /// ventana.
    fn open(window: &OpenWindow) -> Self {
        let shell_path = match &window.web {
            Some(web) => {
                let apps_folder = format!("shell:AppsFolder\\{}", web.id);
                let registered = crate::jumplist::apps_folder_lookup(&web.id, &window.exe)
                    .is_some_and(|(registered, _)| registered)
                    || crate::icons::display_name(&apps_folder).is_some();
                relaunch_command(web, registered)
            }
            None => window
                .app_id
                .as_ref()
                .map(|id| format!("shell:AppsFolder\\{id}")),
        };
        let from_apps_folder = shell_path
            .as_deref()
            .filter(|p| p.starts_with("shell:AppsFolder\\"));
        let web = window.web.as_ref();
        let name = from_apps_folder
            .and_then(crate::icons::display_name)
            .or_else(|| web.and_then(|w| w.name.clone()))
            .or_else(|| web.and_then(|_| window_title(window.hwnd)))
            .or_else(|| file_description(&window.exe))
            .unwrap_or_else(|| {
                std::path::Path::new(&window.exe)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
        let icon = from_apps_folder
            .and_then(crate::icons::load)
            .or_else(|| {
                let (path, index) = icon_resource(web?.icon.as_deref()?);
                crate::icons::load_resource(&path, index)
            })
            .or_else(|| web.and_then(|_| window_icon(window.hwnd)))
            .or_else(|| crate::icons::load(&window.exe));
        // Una app web se abre por su entrada o su orden; sin ninguna, por su programa.
        let command = match web {
            Some(_) => shell_path.clone().unwrap_or_else(|| window.exe.clone()),
            None => window.exe.clone(),
        };
        Self {
            entry: AppEntry { name, command },
            process_name: file_name(&window.exe).to_string(),
            key: window.group_key().to_string(),
            icon,
            scaled: RefCell::new(None),
            is_running: true,
            window: Some(window.hwnd),
            windows: vec![window.hwnd],
            attention: false,
            shell_path,
            hover_spring: Spring::new(1.0, 260.0, 18.0),
            press_spring: Spring::new(1.0, 340.0, 20.0),
            slide: Spring::new(0.0, 420.0, 36.0),
        }
    }

    pub fn launch(&self) -> bool {
        launch_entry(&self.entry)
    }

    /// Cómo abrir otra vez el programa o anclarlo: las apps de Store por su ruta de
    /// AppsFolder y las demás por su ejecutable.
    pub fn pin_entry(&self) -> AppEntry {
        AppEntry {
            name: self.entry.name.clone(),
            command: self
                .shell_path
                .clone()
                .unwrap_or_else(|| self.entry.command.clone()),
        }
    }

    /// Procesos de sus ventanas que "Finalizar tarea" puede terminar, sin repetir.
    pub fn task_processes(&self) -> Vec<u32> {
        let mut pids = Vec::new();
        for &hwnd in &self.windows {
            if let Some((pid, exe, _)) = unsafe { window_process(HWND(hwnd as *mut _)) } {
                if can_end(pid, &exe) && !pids.contains(&pid) {
                    pids.push(pid);
                }
            }
        }
        pids
    }

    /// "Finalizar tarea": termina a la fuerza los procesos de sus ventanas, como el
    /// Administrador de tareas, aunque no respondan. Se pierde lo que no estuviera
    /// guardado. Los procesos con más permisos (como administrador) no se pueden.
    pub fn end_task(&self) {
        use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
        for pid in self.task_processes() {
            unsafe {
                if let Ok(process) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                    let _ = TerminateProcess(process, 1);
                    let _ = windows::Win32::Foundation::CloseHandle(process);
                }
            }
        }
    }

    /// Pide a cada ventana del programa que se cierre, como "Cerrar ventana" en la
    /// barra de Windows: el programa puede preguntar antes si hay cambios sin guardar.
    pub fn close_windows(&self) {
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SC_CLOSE, WM_SYSCOMMAND};
        for &hwnd in &self.windows {
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(hwnd as *mut _)),
                    WM_SYSCOMMAND,
                    windows::Win32::Foundation::WPARAM(SC_CLOSE as usize),
                    windows::Win32::Foundation::LPARAM(0),
                );
            }
        }
    }

    /// Trae al frente la ventana abierta o, si no hay ninguna, inicia el programa.
    pub fn activate(&self) -> bool {
        match self.window {
            Some(hwnd) if focus_window(hwnd) => false,
            _ => self.launch(),
        }
    }
}

/// Restaura y enfoca la ventana; si ya está al frente, la minimiza como la barra nativa.
pub fn focus_window(hwnd: isize) -> bool {
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return false;
        }
        let minimized = IsIconic(hwnd).as_bool();
        if GetForegroundWindow() == hwnd && !minimized {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
            return true;
        }
        if minimized {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        if !SetForegroundWindow(hwnd).as_bool() {
            SwitchToThisWindow(hwnd, true);
        }
        true
    }
}

pub fn launch_entry(entry: &AppEntry) -> bool {
    // La orden que declara la ventana de una app web lleva argumentos
    // ("chrome_proxy.exe --app-id=…"): se separan, salvo que sea un archivo.
    if !std::path::Path::new(&entry.command).exists() {
        if let (program, Some(args)) = split_command_line(&entry.command) {
            return shell_execute("open", &program, Some(&args));
        }
    }
    let command: Vec<u16> = OsStr::new(&entry.command)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let shell_app = entry.command.starts_with("shell:AppsFolder\\");
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            if shell_app {
                w!("explorer.exe")
            } else {
                PCWSTR(command.as_ptr())
            },
            if shell_app {
                PCWSTR(command.as_ptr())
            } else {
                PCWSTR::null()
            },
            None,
            SW_SHOWNORMAL,
        )
        .0 as isize
            > 32
    }
}

/// Pide a Windows `verb` sobre `file` con `args`. Devuelve si lo aceptó.
fn shell_execute(verb: &str, file: &str, args: Option<&str>) -> bool {
    let wide = |text: &str| -> Vec<u16> { OsStr::new(text).encode_wide().chain(Some(0)).collect() };
    let (verb, file) = (wide(verb), wide(file));
    let args = args.map(wide);
    unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            args.as_ref().map_or(PCWSTR::null(), |a| PCWSTR(a.as_ptr())),
            None,
            SW_SHOWNORMAL,
        )
        .0 as isize
            > 32
    }
}

/// Abre la app como administrador (Windows pide confirmación), con la acción
/// "runas" de su menú de Shell: así funciona también con las entradas de
/// AppsFolder, que no son archivos. Si no la tiene, se pide a `ShellExecute`.
pub fn launch_entry_as_admin(entry: &AppEntry) -> bool {
    with_shell_menu(&entry.command, |menu, _| unsafe {
        let id = runas_id(menu)?;
        let info = windows::Win32::UI::Shell::CMINVOKECOMMANDINFO {
            cbSize: std::mem::size_of::<windows::Win32::UI::Shell::CMINVOKECOMMANDINFO>() as u32,
            lpVerb: windows::core::PCSTR(id as usize as *const u8),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        menu.InvokeCommand(&info).ok()
    })
    .flatten()
    .is_some()
        || shell_execute("runas", &entry.command, None)
}

/// Windows ofrece "Ejecutar como administrador" para la app: su menú de Shell
/// tiene la acción "runas", como mira el Inicio.
pub fn can_run_as_admin(command: &str) -> bool {
    with_shell_menu(command, |menu, _| unsafe { runas_id(menu) })
        .flatten()
        .is_some()
}

/// Primer número de orden que se pide al menú de Shell.
const SHELL_MENU_FIRST: u32 = 1;

/// Construye el menú de Shell del elemento `command` (ruta o `shell:AppsFolder\…`)
/// y llama a `f` con él y con su primer número de orden.
fn with_shell_menu<T>(
    command: &str,
    f: impl FnOnce(&windows::Win32::UI::Shell::IContextMenu, u32) -> T,
) -> Option<T> {
    use windows::core::HSTRING;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{
        BHID_SFUIObject, IContextMenu, IShellItem, SHCreateItemFromParsingName, CMF_NORMAL,
    };
    use windows::Win32::UI::WindowsAndMessaging::{CreatePopupMenu, DestroyMenu};
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let result = (|| {
            let path = HSTRING::from(command);
            let item: IShellItem = SHCreateItemFromParsingName(&path, None).ok()?;
            let menu: IContextMenu = item.BindToHandler(None, &BHID_SFUIObject).ok()?;
            let popup = CreatePopupMenu().ok()?;
            let built = menu
                .QueryContextMenu(popup, 0, SHELL_MENU_FIRST, 0x7FFF, CMF_NORMAL)
                .is_ok();
            let result = built.then(|| f(&menu, SHELL_MENU_FIRST));
            let _ = DestroyMenu(popup);
            result
        })();
        if initialized {
            CoUninitialize();
        }
        result
    }
}

/// Número de orden (relativo) de la acción "runas" del menú de Shell, si la tiene.
unsafe fn runas_id(menu: &windows::Win32::UI::Shell::IContextMenu) -> Option<u32> {
    use windows::core::PSTR;
    use windows::Win32::UI::Shell::GCS_VERBW;
    // Se pregunta el nombre de cada orden; sus números son relativos al primero.
    (0..0x400u32).find(|&offset| {
        let mut verb = [0u16; 64];
        menu.GetCommandString(
            offset as usize,
            GCS_VERBW,
            None,
            PSTR(verb.as_mut_ptr().cast()),
            verb.len() as u32,
        )
        .is_ok()
            && String::from_utf16_lossy(&verb[..verb.iter().position(|&c| c == 0).unwrap_or(0)])
                .eq_ignore_ascii_case("runas")
    })
}

/// Abre la carpeta del programa o acceso directo con él seleccionado.
pub fn open_entry_location(entry: &AppEntry) -> bool {
    let explorer = std::env::var("WINDIR")
        .map(|p| format!("{p}\\explorer.exe"))
        .unwrap_or_else(|_| "explorer.exe".into());
    shell_execute(
        "open",
        &explorer,
        Some(&format!("/select,\"{}\"", entry.command)),
    )
}

/// Abre "Aplicaciones instaladas" de Configuración, desde donde se desinstala.
pub fn open_uninstall() -> bool {
    shell_execute("open", "ms-settings:appsfeatures", None)
}

pub fn default_apps() -> Vec<AppItem> {
    vec![AppItem::new(AppEntry {
        name: "Explorador".into(),
        command: std::env::var("WINDIR")
            .map(|p| format!("{p}\\explorer.exe"))
            .unwrap_or_else(|_| "explorer.exe".into()),
    })]
}

pub struct OpenWindow {
    pub hwnd: isize,
    pub exe: String,
    /// AppUserModelID de las apps empaquetadas (Store) o de las apps web.
    pub app_id: Option<String>,
    /// La ventana es una app aparte del programa que la abre: una app web instalada
    /// desde el navegador (PWA), que comparte proceso con él. Se agrupa por su
    /// AppUserModelID, no por el ejecutable, como en la barra de Windows.
    pub web: Option<WindowApp>,
}

impl OpenWindow {
    /// Con qué se agrupa entre los programas abiertos sin anclar.
    fn group_key(&self) -> &str {
        match (&self.web, &self.app_id) {
            (Some(_), Some(id)) => id,
            _ => &self.exe,
        }
    }

    fn belongs_to(&self, app: &AppItem) -> bool {
        let by_id = self.app_id.as_ref().is_some_and(|id| {
            app.entry
                .command
                .strip_prefix("shell:AppsFolder\\")
                .is_some_and(|c| c.eq_ignore_ascii_case(id))
        });
        // Una app web no es del navegador anclado: solo de su propia entrada.
        if let Some(web) = &self.web {
            return by_id
                || web
                    .relaunch
                    .as_ref()
                    .is_some_and(|r| r.eq_ignore_ascii_case(&app.entry.command));
        }
        (!app.process_name.is_empty()
            && file_name(&self.exe).eq_ignore_ascii_case(&app.process_name))
            || by_id
    }
}

/// Si la ventana que declara el AppUserModelID `window_id` es una app aparte del
/// programa que la abre, cuyo AppUserModelID es `exe_id` (si el menú Inicio lo
/// tiene). Lo es si el menú Inicio la tiene como app propia (`registered`), como las
/// apps web que instalan Chrome y Edge, o si su identificador es el de una app web
/// de Chromium ("Chrome._crx_…", "MSEdge._crx_…"). Las ventanas de un perfil del
/// navegador ("Chrome.UserData.Profile1") y las que repiten el de su programa siguen
/// con él.
pub fn is_own_app(window_id: &str, exe_id: Option<&str>, registered: bool) -> bool {
    if exe_id.is_some_and(|e| e.eq_ignore_ascii_case(window_id)) {
        return false;
    }
    (registered && exe_id.is_some()) || window_id.to_ascii_lowercase().contains("._crx_")
}

/// Cómo volver a abrir una app web: por su entrada del menú Inicio si la tiene (así
/// se abre la app y no el navegador, y conserva icono y nombre) o con la orden que
/// declara su ventana. Sin ninguna de las dos, `None`: se abre su programa.
pub fn relaunch_command(app: &WindowApp, in_apps_folder: bool) -> Option<String> {
    if in_apps_folder {
        return Some(format!("shell:AppsFolder\\{}", app.id));
    }
    app.relaunch.clone()
}

/// Separa una orden en programa y argumentos: `"C:\a b\x.exe" --y` o
/// `C:\a\x.exe --y`. Sin argumentos, el segundo es `None`.
pub fn split_command_line(command: &str) -> (String, Option<String>) {
    let command = command.trim();
    let (program, rest) = if let Some(quoted) = command.strip_prefix('"') {
        match quoted.find('"') {
            Some(end) => (&quoted[..end], &quoted[end + 1..]),
            None => (quoted, ""),
        }
    } else {
        match command.to_ascii_lowercase().find(".exe ") {
            Some(at) => command.split_at(at + 4),
            None => (command, ""),
        }
    };
    let rest = rest.trim();
    (
        program.to_string(),
        (!rest.is_empty()).then(|| rest.to_string()),
    )
}

/// Separa un recurso de icono "ruta,índice"; sin índice, el del archivo (-1).
fn icon_resource(resource: &str) -> (String, i32) {
    let resource = resource.trim().trim_start_matches('@');
    match resource.rsplit_once(',') {
        Some((path, index)) if index.trim().parse::<i32>().is_ok() => (
            path.trim().trim_matches('"').to_string(),
            index.trim().parse().unwrap_or(0),
        ),
        _ => (resource.trim_matches('"').to_string(), -1),
    }
}

/// Título de la ventana.
fn window_title(hwnd: isize) -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
    let mut text = [0u16; 512];
    let len = unsafe { GetWindowTextW(HWND(hwnd as *mut _), &mut text) } as usize;
    let title = String::from_utf16_lossy(&text[..len]).trim().to_string();
    (!title.is_empty()).then_some(title)
}

/// El icono que muestra la ventana (el de su barra de título).
fn window_icon(hwnd: isize) -> Option<Pixmap> {
    use windows::Win32::Foundation::WPARAM;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassLongPtrW, SendMessageTimeoutW, GCLP_HICON, HICON, ICON_BIG, SMTO_ABORTIFHUNG,
        WM_GETICON,
    };
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        let mut result = 0usize;
        let _ = SendMessageTimeoutW(
            hwnd,
            WM_GETICON,
            WPARAM(ICON_BIG as usize),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            200,
            Some(&mut result),
        );
        if result == 0 {
            result = GetClassLongPtrW(hwnd, GCLP_HICON);
        }
        (result != 0)
            .then(|| crate::icons::from_hicon(HICON(result as *mut _), 128))
            .flatten()
    }
}

/// Descripción del archivo (p. ej. "Google Chrome"), como la muestra el Explorador.
pub fn file_description(exe: &str) -> Option<String> {
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };
    let wide: Vec<u16> = exe.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(wide.as_ptr()), None, size, data.as_mut_ptr().cast()).ok()?;
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        if !VerQueryValueW(
            data.as_ptr().cast(),
            w!("\\VarFileInfo\\Translation"),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            || len < 4
        {
            return None;
        }
        let pair = std::slice::from_raw_parts(ptr as *const u16, 2);
        let key = format!(
            "\\StringFileInfo\\{:04x}{:04x}\\FileDescription",
            pair[0], pair[1]
        );
        let key: Vec<u16> = key.encode_utf16().chain(Some(0)).collect();
        if !VerQueryValueW(
            data.as_ptr().cast(),
            PCWSTR(key.as_ptr()),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            || len == 0
        {
            return None;
        }
        let text = std::slice::from_raw_parts(ptr as *const u16, len as usize);
        let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
        let name = String::from_utf16_lossy(&text[..end]).trim().to_string();
        (!name.is_empty()).then_some(name)
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Marca los programas anclados con ventana abierta y mantiene en `open` los demás,
/// en orden estable. Devuelve `true` si cambió la lista de programas abiertos.
pub fn update_running_apps(pinned: &mut [AppItem], open: &mut Vec<AppItem>) -> bool {
    assign_windows(&open_windows(), pinned, open)
}

/// `update_running_apps` con las ventanas `windows` ya leídas.
fn assign_windows(windows: &[OpenWindow], pinned: &mut [AppItem], open: &mut Vec<AppItem>) -> bool {
    let mut claimed = vec![false; windows.len()];
    for app in pinned.iter_mut() {
        app.window = None;
        app.windows.clear();
        for (i, w) in windows.iter().enumerate() {
            if w.belongs_to(app) {
                app.window.get_or_insert(w.hwnd);
                app.windows.push(w.hwnd);
                claimed[i] = true;
            }
        }
        app.is_running = app.window.is_some();
    }
    let unclaimed: Vec<&OpenWindow> = windows
        .iter()
        .zip(&claimed)
        .filter(|(_, c)| !**c)
        .map(|(w, _)| w)
        .collect();

    let before: Vec<String> = open.iter().map(|a| a.key.clone()).collect();
    open.retain_mut(|a| {
        a.windows = unclaimed
            .iter()
            .filter(|w| w.group_key().eq_ignore_ascii_case(&a.key))
            .map(|w| w.hwnd)
            .collect();
        a.window = a.windows.first().copied();
        a.window.is_some()
    });
    for w in unclaimed {
        if open
            .iter()
            .any(|a| a.key.eq_ignore_ascii_case(w.group_key()))
        {
            continue;
        }
        open.push(AppItem::open(w));
    }
    before.len() != open.len()
        || before
            .iter()
            .zip(open.iter())
            .any(|(b, a)| !b.eq_ignore_ascii_case(&a.key))
}

/// Ventanas que la barra nativa mostraría, de la más reciente a la más antigua.
pub fn open_windows() -> Vec<OpenWindow> {
    let mut list: Vec<OpenWindow> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(collect_window),
            LPARAM(&mut list as *mut Vec<OpenWindow> as isize),
        );
    }
    list
}

unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let list = &mut *(lparam.0 as *mut Vec<OpenWindow>);
    if let Some((exe, app_id)) = taskbar_window_process(hwnd) {
        let hwnd = hwnd.0 as isize;
        // Las apps web comparten proceso con el navegador: solo su ventana dice cuál es.
        let web = match app_id {
            None => crate::jumplist::window_app(hwnd).filter(|web| {
                let (registered, exe_id) =
                    crate::jumplist::apps_folder_lookup(&web.id, &exe).unwrap_or_default();
                is_own_app(&web.id, exe_id.as_deref(), registered)
            }),
            Some(_) => None,
        };
        list.push(OpenWindow {
            hwnd,
            exe,
            app_id: web.as_ref().map(|w| w.id.clone()).or(app_id),
            web,
        });
    }
    true.into()
}

unsafe fn taskbar_window_process(hwnd: HWND) -> Option<(String, Option<String>)> {
    if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
        return None;
    }
    if GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid()) {
        return None;
    }
    let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    if ex & WS_EX_APPWINDOW.0 == 0 && ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) != 0 {
        return None;
    }
    // Ventanas ocultas por DWM: otros escritorios virtuales y apps de Store suspendidas.
    let mut cloaked = 0u32;
    if DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        &mut cloaked as *mut u32 as *mut _,
        std::mem::size_of::<u32>() as u32,
    )
    .is_ok()
        && cloaked != 0
    {
        return None;
    }
    let (pid, exe, app_id) = window_process(hwnd)?;
    if pid == GetCurrentProcessId() {
        return None;
    }
    Some((exe, app_id))
}

/// Proceso real de una ventana (número, ejecutable y AppUserModelID). Las apps de
/// Store se alojan en un marco de ApplicationFrameHost, compartido por todas: su
/// proceso es el de la ventana hija.
pub unsafe fn window_process(hwnd: HWND) -> Option<(u32, String, Option<String>)> {
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return None;
    }
    let (exe, app_id) = process_info(pid)?;
    if !file_name(&exe).eq_ignore_ascii_case("ApplicationFrameHost.exe") {
        return Some((pid, exe, app_id));
    }
    let mut search = (pid, 0u32);
    let _ = EnumChildWindows(
        Some(hwnd),
        Some(find_hosted_process),
        LPARAM(&mut search as *mut (u32, u32) as isize),
    );
    let (exe, app_id) = process_info(search.1)?;
    Some((search.1, exe, app_id))
}

/// "Finalizar tarea" está activado en Windows (Configuración > Sistema > Para
/// programadores). Windows 11 lo guarda en `TaskbarDeveloperSettings`; antes, en
/// `Advanced`.
pub fn end_task_enabled() -> bool {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let read = |key: PCWSTR| unsafe {
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            w!("TaskbarEndTask"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
        .is_ok()
        .then_some(value)
    };
    read(w!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced\\TaskbarDeveloperSettings"
    ))
    .or_else(|| {
        read(w!(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced"
        ))
    }) == Some(1)
}

/// Si "Finalizar tarea" puede terminar el proceso `exe` (`pid`): nunca el Explorador,
/// que es el escritorio y la barra, ni GlassBar.
fn can_end(pid: u32, exe: &str) -> bool {
    pid != unsafe { GetCurrentProcessId() } && !file_name(exe).eq_ignore_ascii_case("explorer.exe")
}

unsafe extern "system" fn find_hosted_process(child: HWND, lparam: LPARAM) -> BOOL {
    let search = &mut *(lparam.0 as *mut (u32, u32));
    let mut pid = 0;
    GetWindowThreadProcessId(child, Some(&mut pid));
    if pid != 0 && pid != search.0 {
        search.1 = pid;
        return false.into();
    }
    true.into()
}

/// Ruta del ejecutable y, si es una app empaquetada, su AppUserModelID.
pub fn process_info(pid: u32) -> Option<(String, Option<String>)> {
    use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
    if pid == 0 {
        return None;
    }
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        // Se mira cada segundo por ventana: casi todas las rutas caben en el búfer
        // de la pila, y solo las muy largas piden el máximo de Windows.
        let read = |buf: &mut [u16]| {
            let mut len = buf.len() as u32;
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok()
            .then(|| String::from_utf16_lossy(&buf[..len as usize]))
        };
        let exe = read(&mut [0u16; 1024]).or_else(|| read(&mut vec![0u16; 32768]));
        let mut id = [0u16; 256];
        let mut id_len = id.len() as u32;
        let app_id =
            (GetApplicationUserModelId(process, &mut id_len, Some(PWSTR(id.as_mut_ptr()))).is_ok())
                .then(|| {
                    let end = id.iter().position(|&c| c == 0).unwrap_or(id.len());
                    String::from_utf16_lossy(&id[..end])
                })
                .filter(|id| !id.is_empty());
        let _ = CloseHandle(process);
        Some((exe?, app_id))
    }
}

// ---------------------------------------------------------------------------
// DIBUJO DE ICONOS VECTORIALES FLUENT (IDÉNTICOS A LA REFERENCIA)
// ---------------------------------------------------------------------------

/// Botón Inicio estilo Windows 11 Fluent (4 cuadros blancos redondeados)
pub fn draw_windows_start_icon(pixmap: &mut Pixmap, cx: f32, cy: f32, size: f32, scale: f32) {
    let s = size * scale;
    let gap = 2.4 * scale;
    let tile_w = (s - gap) / 2.0;
    let r = 2.0 * scale;

    let x0 = cx - s / 2.0;
    let y0 = cy - s / 2.0;
    let x1 = x0 + tile_w + gap;
    let y1 = y0 + tile_w + gap;

    // El logo de Windows 11 en azul: un solo degradado de cian claro (arriba a la
    // izquierda) a azul (abajo a la derecha) que atraviesa los cuatro cuadros.
    let stops = [(0.0, [112, 224, 255, 255]), (1.0, [0, 120, 240, 255])];
    let (from, to) = ((x0, y0), (x0 + s, y0 + s));
    for (tx, ty) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
        fill_rounded_gradient(pixmap, &stops, from, to, tx, ty, tile_w, tile_w, r);
    }
}

/// `base` es el tamaño en reposo; `size` incluye la animación de hover o pulsación.
pub fn draw_app_icon(
    pixmap: &mut Pixmap,
    app: &AppItem,
    cx: f32,
    cy: f32,
    base: f32,
    size: f32,
    scale: f32,
) {
    let Some(icon) = &app.icon else {
        return;
    };
    let side = (base * scale).round().max(1.0) as u32;
    let mut cache = app.scaled.borrow_mut();
    if !cache.as_ref().is_some_and(|c| c.width() == side) {
        *cache = Some(downscale(icon, side));
    }
    let Some(image) = cache.as_ref() else {
        return;
    };
    let factor = size / base;
    if (factor - 1.0).abs() < 0.002 {
        // En reposo se copia píxel a píxel, sin volver a filtrar.
        let half = side as f32 / 2.0;
        pixmap.draw_pixmap(
            (cx - half).round() as i32,
            (cy - half).round() as i32,
            image.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    } else {
        let drawn = side as f32 * factor;
        let transform = Transform::from_scale(factor, factor)
            .post_translate(cx - drawn / 2.0, cy - drawn / 2.0);
        pixmap.draw_pixmap(
            0,
            0,
            image.as_ref(),
            &tiny_skia::PixmapPaint {
                quality: tiny_skia::FilterQuality::Bicubic,
                ..Default::default()
            },
            transform,
            None,
        );
    }
}

/// Reduce promediando el área de origen que cubre cada píxel; evita el
/// dentado del bicúbico al reducir varias veces. No amplía iconos pequeños.
pub fn downscale(src: &Pixmap, side: u32) -> Pixmap {
    let (sw, sh) = (src.width(), src.height());
    if sw <= side && sh <= side {
        return src.clone();
    }
    // Pesos de cobertura de cada columna/fila de origen sobre la de destino.
    let spans = |from: u32| -> Vec<Vec<(usize, f32)>> {
        let ratio = from as f32 / side as f32;
        (0..side)
            .map(|d| {
                let (start, end) = (d as f32 * ratio, (d + 1) as f32 * ratio);
                (start.floor() as u32..(end.ceil() as u32).min(from))
                    .map(|s| {
                        let w = (end.min(s as f32 + 1.0) - start.max(s as f32)) / ratio;
                        (s as usize, w)
                    })
                    .filter(|(_, w)| *w > 0.0)
                    .collect()
            })
            .collect()
    };
    let (cols, rows) = (spans(sw), spans(sh));
    let data = src.data();
    let mut out = Pixmap::new(side, side).expect("tamaño de icono válido");
    let dst = out.data_mut();
    for (dy, row) in rows.iter().enumerate() {
        for (dx, col) in cols.iter().enumerate() {
            let mut acc = [0f32; 4];
            for &(sy, wy) in row {
                for &(sx, wx) in col {
                    let i = (sy * sw as usize + sx) * 4;
                    let w = wx * wy;
                    for c in 0..4 {
                        acc[c] += data[i + c] as f32 * w;
                    }
                }
            }
            let i = (dy * side as usize + dx) * 4;
            let alpha = acc[3].round().clamp(0.0, 255.0);
            dst[i + 3] = alpha as u8;
            for c in 0..3 {
                // Premultiplicado: ningún canal puede superar al alfa.
                dst[i + c] = acc[c].round().clamp(0.0, alpha) as u8;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// UTILIDADES GEOMÉTRICAS BÁSICAS
// ---------------------------------------------------------------------------

pub fn fill_rounded(
    pixmap: &mut Pixmap,
    [r, g, b, a]: [u8; 4],
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rad: f32,
) {
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color_rgba8(r, g, b, a);
    if let Some(path) = rounded_rect(x, y, w, h, rad) {
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

/// Como `fill_rounded`, con un degradado lineal de `from` (en `start`) a `to` (en
/// `end`), con paradas intermedias opcionales en `stops`: `(posición 0..1, color)`.
#[allow(clippy::too_many_arguments)]
pub fn fill_rounded_gradient(
    pixmap: &mut Pixmap,
    stops: &[(f32, [u8; 4])],
    start: (f32, f32),
    end: (f32, f32),
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rad: f32,
) {
    let stops = stops
        .iter()
        .map(|&(at, [r, g, b, a])| {
            tiny_skia::GradientStop::new(at, tiny_skia::Color::from_rgba8(r, g, b, a))
        })
        .collect();
    let Some(shader) = tiny_skia::LinearGradient::new(
        tiny_skia::Point::from_xy(start.0, start.1),
        tiny_skia::Point::from_xy(end.0, end.1),
        stops,
        tiny_skia::SpreadMode::Pad,
        Transform::identity(),
    ) else {
        return;
    };
    let paint = Paint {
        anti_alias: true,
        shader,
        ..Default::default()
    };
    if let Some(path) = rounded_rect(x, y, w, h, rad) {
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

pub fn stroke_rounded(
    pixmap: &mut Pixmap,
    [r, g, b, a]: [u8; 4],
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rad: f32,
    width: f32,
) {
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color_rgba8(r, g, b, a);
    let mut stroke = Stroke::default();
    stroke.width = width;
    if let Some(path) = rounded_rect(x, y, w, h, rad) {
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

pub fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    const KAPPA: f32 = 0.552_284_8;
    let r = r.min(w / 2.0).min(h / 2.0);
    let c = r * KAPPA;
    let (x1, y1) = (x + w, y + h);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x1 - r, y);
    pb.cubic_to(x1 - r + c, y, x1, y + r - c, x1, y + r);
    pb.line_to(x1, y1 - r);
    pb.cubic_to(x1, y1 - r + c, x1 - r + c, y1, x1 - r, y1);
    pb.line_to(x + r, y1);
    pb.cubic_to(x + r - c, y1, x, y1 - r + c, x, y1 - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - c, x + r - c, y, x + r, y);
    pb.close();
    pb.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_task_never_ends_explorer_or_glassbar() {
        // GlassBar mismo.
        assert!(!can_end(std::process::id(), r"C:\GlassBar\glassbar.exe"));
        // El Explorador es el escritorio y la barra: terminarlo dejaría Windows sin ellos.
        assert!(!can_end(1234, r"C:\Windows\explorer.exe"));
        assert!(!can_end(1234, r"C:\WINDOWS\EXPLORER.EXE"));
        // Cualquier otro programa, sí.
        assert!(can_end(
            1234,
            r"C:\Program Files\Google\Chrome\Application\chrome.exe"
        ));
    }

    #[test]
    fn pinning_from_the_menu_keeps_how_to_open_the_app() {
        // App de Store abierta sin anclar: se ancla y se reabre por su AppUserModelID,
        // y la versión anclada sigue reconociendo su ventana.
        let store = OpenWindow {
            hwnd: 0,
            exe: r"C:\Program Files\WindowsApps\WhatsApp_1.0_x64\WhatsApp.Root.exe".into(),
            app_id: Some("5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App".into()),
            web: None,
        };
        let entry = AppItem::open(&store).pin_entry();
        assert_eq!(
            entry.command,
            r"shell:AppsFolder\5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App"
        );
        assert!(store.belongs_to(&AppItem::new(entry)));

        // Programa de escritorio: por su ejecutable.
        let desktop = OpenWindow {
            hwnd: 0,
            exe: r"C:\Program Files\Google\Chrome\Application\chrome.exe".into(),
            app_id: None,
            web: None,
        };
        let entry = AppItem::open(&desktop).pin_entry();
        assert_eq!(entry.command, desktop.exe);
        assert!(desktop.belongs_to(&AppItem::new(entry)));
    }

    const CHROME: &str = r"C:\Program Files\Google\Chrome\Application\chrome.exe";

    /// Ventana de una app web de Chrome que el menú Inicio no tiene (así no depende
    /// de lo instalado en el equipo): se reabre con la orden que declara.
    fn web_window(hwnd: isize, id: &str) -> OpenWindow {
        OpenWindow {
            hwnd,
            exe: CHROME.into(),
            app_id: Some(id.into()),
            web: Some(WindowApp {
                id: id.into(),
                relaunch: Some(format!(
                    r#""C:\Program Files\Google\Chrome\Application\chrome_proxy.exe" --app-id={id}"#
                )),
                name: Some("WhatsApp Web".into()),
                icon: None,
            }),
        }
    }

    fn chrome_window(hwnd: isize) -> OpenWindow {
        OpenWindow {
            hwnd,
            exe: CHROME.into(),
            app_id: None,
            web: None,
        }
    }

    #[test]
    fn web_apps_are_apps_of_their_own_but_browser_profiles_are_not() {
        // WhatsApp Web instalada desde Chrome: el menú Inicio la tiene aparte.
        let pwa = "Chrome._crx_hnpfjaidbinmjnm.UserData.Profile1";
        assert!(is_own_app(pwa, Some("Chrome"), true));
        // Aunque aún no se haya leído el menú Inicio, su identificador lo dice.
        assert!(is_own_app(pwa, None, false));
        assert!(is_own_app("MSEdge._crx_abcdef", Some("MSEdge"), false));
        // Una ventana de otro perfil de Chrome sigue siendo Chrome.
        assert!(!is_own_app(
            "Chrome.UserData.Profile1",
            Some("Chrome"),
            false
        ));
        // Programas que declaran el mismo identificador que su entrada (VS Code).
        let code = "Microsoft.VisualStudioCode";
        assert!(!is_own_app(code, Some(code), true));
        // Programas cuyo ejecutable no está en el menú Inicio (Slack abre Update.exe).
        assert!(!is_own_app("com.squirrel.slack.slack", None, true));
    }

    #[test]
    fn web_apps_reopen_from_the_start_menu_or_their_own_command() {
        let mut app = WindowApp {
            id: "Chrome._crx_hnpf.UserData.Profile1".into(),
            relaunch: Some(r#"chrome_proxy.exe --app-id=hnpf"#.into()),
            ..Default::default()
        };
        assert_eq!(
            relaunch_command(&app, true).as_deref(),
            Some(r"shell:AppsFolder\Chrome._crx_hnpf.UserData.Profile1")
        );
        assert_eq!(
            relaunch_command(&app, false).as_deref(),
            Some("chrome_proxy.exe --app-id=hnpf")
        );
        app.relaunch = None;
        assert_eq!(relaunch_command(&app, false), None);
    }

    #[test]
    fn splits_commands_into_program_and_arguments() {
        assert_eq!(
            split_command_line(
                r#""C:\Program Files\Google\Chrome\Application\chrome_proxy.exe" --profile-directory="Profile 1" --app-id=x"#
            ),
            (
                r"C:\Program Files\Google\Chrome\Application\chrome_proxy.exe".to_string(),
                Some(r#"--profile-directory="Profile 1" --app-id=x"#.to_string())
            )
        );
        assert_eq!(
            split_command_line(r"C:\Apps\x.EXE --y"),
            (r"C:\Apps\x.EXE".to_string(), Some("--y".to_string()))
        );
        assert_eq!(
            split_command_line(r"C:\Program Files\App\app.exe"),
            (r"C:\Program Files\App\app.exe".to_string(), None)
        );
        assert_eq!(
            split_command_line(r"shell:AppsFolder\Chrome._crx_x"),
            (r"shell:AppsFolder\Chrome._crx_x".to_string(), None)
        );
    }

    #[test]
    fn reads_icon_resources() {
        assert_eq!(
            icon_resource(r"C:\Users\a\Web Applications\WhatsApp Web.ico,0"),
            (
                r"C:\Users\a\Web Applications\WhatsApp Web.ico".to_string(),
                0
            )
        );
        assert_eq!(
            icon_resource(r#""C:\a\b.dll",-12"#),
            (r"C:\a\b.dll".to_string(), -12)
        );
        assert_eq!(
            icon_resource(r"C:\a\b.ico"),
            (r"C:\a\b.ico".to_string(), -1)
        );
    }

    #[test]
    fn web_apps_group_apart_from_the_browser() {
        let id = "Chrome._crx_glassbartest";
        let windows = [
            web_window(1, id),
            chrome_window(2),
            web_window(3, id),
            chrome_window(4),
        ];
        // Sin anclar: Chrome y la app web salen aparte; en la pasada siguiente, cada
        // una con todas sus ventanas.
        let mut open = Vec::new();
        assert!(assign_windows(&windows, &mut [], &mut open));
        assert!(!assign_windows(&windows, &mut [], &mut open));
        assert_eq!(open.len(), 2);
        assert_eq!(open[0].windows, vec![1, 3]);
        assert_eq!(open[0].entry.name, "WhatsApp Web");
        assert_eq!(open[1].windows, vec![2, 4]);

        // Con Chrome anclado, sus ventanas van a él, pero no las de la app web.
        let mut pinned = vec![AppItem::new(AppEntry {
            name: "Chrome".into(),
            command: CHROME.into(),
        })];
        let mut open = Vec::new();
        assign_windows(&windows, &mut pinned, &mut open);
        assign_windows(&windows, &mut pinned, &mut open);
        assert_eq!(pinned[0].windows, vec![2, 4]);
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].windows, vec![1, 3]);

        // Anclar la app web: se reabre con su orden (no con Chrome) y la versión
        // anclada se queda con sus ventanas.
        let entry = open[0].pin_entry();
        assert!(entry.command.contains("chrome_proxy.exe"));
        assert!(entry.command.contains(id));
        pinned.push(AppItem::new(entry));
        let mut open = Vec::new();
        assign_windows(&windows, &mut pinned, &mut open);
        assert_eq!(pinned[0].windows, vec![2, 4]);
        assert_eq!(pinned[1].windows, vec![1, 3]);
        assert!(open.is_empty());

        // Anclada por su entrada del menú Inicio, también.
        let pinned_start = AppItem::new(AppEntry {
            name: "WhatsApp Web".into(),
            command: format!(r"shell:AppsFolder\{id}"),
        });
        assert!(windows[0].belongs_to(&pinned_start));
        assert!(!windows[1].belongs_to(&pinned_start));
    }
}
