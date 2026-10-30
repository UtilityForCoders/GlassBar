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

use crate::spring::Spring;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct AppEntry {
    pub name: String,
    pub command: String,
}

pub struct AppItem {
    pub entry: AppEntry,
    process_name: String,
    pub icon: Option<Pixmap>,
    /// Icono ya reducido al tamaño en pantalla.
    scaled: RefCell<Option<Pixmap>>,
    pub is_running: bool,
    /// Ventana abierta más reciente del programa, si tiene alguna.
    pub window: Option<isize>,
    /// Todas sus ventanas abiertas, de la más reciente a la más antigua.
    pub windows: Vec<isize>,
    pub hover_spring: Spring,
    pub press_spring: Spring,
    /// Desplazamiento horizontal mientras se reordena la barra.
    pub slide: Spring,
}

impl AppItem {
    pub fn new(entry: AppEntry) -> Self {
        let icon = crate::icons::load(&entry.command);
        let process_path =
            crate::pins::shortcut_target(&entry.command).unwrap_or_else(|| entry.command.clone());
        let process_name = std::path::Path::new(&process_path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            entry,
            process_name,
            icon,
            scaled: RefCell::new(None),
            is_running: false,
            window: None,
            windows: Vec::new(),
            hover_spring: Spring::new(1.0, 260.0, 18.0),
            press_spring: Spring::new(1.0, 340.0, 20.0),
            slide: Spring::new(0.0, 420.0, 36.0),
        }
    }

    /// Programa abierto sin anclar; las apps de Store toman icono y nombre de su paquete.
    fn open(window: &OpenWindow) -> Self {
        let shell_path = window
            .app_id
            .as_ref()
            .map(|id| format!("shell:AppsFolder\\{id}"));
        let name = shell_path
            .as_deref()
            .and_then(crate::icons::display_name)
            .or_else(|| file_description(&window.exe))
            .unwrap_or_else(|| {
                std::path::Path::new(&window.exe)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
        let icon = shell_path
            .as_deref()
            .and_then(crate::icons::load)
            .or_else(|| crate::icons::load(&window.exe));
        Self {
            entry: AppEntry {
                name,
                command: window.exe.clone(),
            },
            process_name: file_name(&window.exe).to_string(),
            icon,
            scaled: RefCell::new(None),
            is_running: true,
            window: Some(window.hwnd),
            windows: vec![window.hwnd],
            hover_spring: Spring::new(1.0, 260.0, 18.0),
            press_spring: Spring::new(1.0, 340.0, 20.0),
            slide: Spring::new(0.0, 420.0, 36.0),
        }
    }

    pub fn launch(&self) -> bool {
        launch_entry(&self.entry)
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

/// Memoria de los iconos de una app: el original y el reducido al tamaño del dock.
#[cfg(feature = "profile")]
impl AppItem {
    pub fn icon_bytes(&self) -> usize {
        let original = self.icon.as_ref().map_or(0, |i| i.data().len());
        let scaled = self.scaled.borrow().as_ref().map_or(0, |i| i.data().len());
        original + scaled
    }
}

pub fn launch_entry(entry: &AppEntry) -> bool {
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
    /// AppUserModelID de las apps empaquetadas (Store).
    pub app_id: Option<String>,
}

impl OpenWindow {
    fn belongs_to(&self, app: &AppItem) -> bool {
        (!app.process_name.is_empty()
            && file_name(&self.exe).eq_ignore_ascii_case(&app.process_name))
            || self.app_id.as_ref().is_some_and(|id| {
                app.entry
                    .command
                    .strip_prefix("shell:AppsFolder\\")
                    .is_some_and(|c| c.eq_ignore_ascii_case(id))
            })
    }
}

/// Descripción del archivo (p. ej. "Google Chrome"), como la muestra el Explorador.
pub fn file_description(exe: &str) -> Option<String> {
    let _p = crate::profile::scope("apps.file_description");
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
    let _p = crate::profile::scope("apps.update_running_apps");
    let windows = open_windows();
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

    let before: Vec<String> = open.iter().map(|a| a.entry.command.clone()).collect();
    open.retain_mut(|a| {
        a.windows = unclaimed
            .iter()
            .filter(|w| w.exe.eq_ignore_ascii_case(&a.entry.command))
            .map(|w| w.hwnd)
            .collect();
        a.window = a.windows.first().copied();
        a.window.is_some()
    });
    for w in unclaimed {
        if open
            .iter()
            .any(|a| a.entry.command.eq_ignore_ascii_case(&w.exe))
        {
            continue;
        }
        open.push(AppItem::open(w));
    }
    before.len() != open.len()
        || before
            .iter()
            .zip(open.iter())
            .any(|(b, a)| !b.eq_ignore_ascii_case(&a.entry.command))
}

/// Ventanas que la barra nativa mostraría, de la más reciente a la más antigua.
pub fn open_windows() -> Vec<OpenWindow> {
    let _p = crate::profile::scope("apps.open_windows (EnumWindows)");
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
        list.push(OpenWindow {
            hwnd: hwnd.0 as isize,
            exe,
            app_id,
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
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 || pid == GetCurrentProcessId() {
        return None;
    }
    let (exe, app_id) = process_info(pid)?;
    if !file_name(&exe).eq_ignore_ascii_case("ApplicationFrameHost.exe") {
        return Some((exe, app_id));
    }
    // Las apps de Store se alojan en un marco; el proceso real es el de la ventana hija.
    let mut search = (pid, 0u32);
    let _ = EnumChildWindows(
        Some(hwnd),
        Some(find_hosted_process),
        LPARAM(&mut search as *mut (u32, u32) as isize),
    );
    process_info(search.1)
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
    let _p = crate::profile::scope("apps.process_info");
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

    let white = [255, 255, 255, 235];
    fill_rounded(pixmap, white, x0, y0, tile_w, tile_w, r);
    fill_rounded(pixmap, white, x1, y0, tile_w, tile_w, r);
    fill_rounded(pixmap, white, x0, y1, tile_w, tile_w, r);
    fill_rounded(pixmap, white, x1, y1, tile_w, tile_w, r);
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
    let _p = crate::profile::scope("apps.downscale_icon");
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
