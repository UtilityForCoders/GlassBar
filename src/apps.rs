//! Programas anclados e iconos proporcionados por Windows.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

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
    pub is_running: bool,
    pub hover_spring: Spring,
    pub press_spring: Spring,
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
            is_running: false,
            hover_spring: Spring::new(1.0, 260.0, 18.0),
            press_spring: Spring::new(1.0, 340.0, 20.0),
        }
    }

    pub fn launch(&self) -> bool {
        launch_entry(&self.entry)
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

/// Comprueba en segundo plano cuáles aplicaciones están abiertas
pub fn update_running_apps(apps: &mut [AppItem]) {
    let mut running_exes: Vec<String> = Vec::new();

    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        let mut ok = Process32FirstW(snapshot, &mut entry).is_ok();
        while ok {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
            running_exes.push(name);
            ok = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }

    for app in apps.iter_mut() {
        app.is_running = running_exes
            .iter()
            .any(|r| r.eq_ignore_ascii_case(&app.process_name));
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

pub fn draw_app_icon(pixmap: &mut Pixmap, app: &AppItem, cx: f32, cy: f32, size: f32, scale: f32) {
    if let Some(icon) = &app.icon {
        let side = size * scale;
        let transform =
            Transform::from_scale(side / icon.width() as f32, side / icon.height() as f32)
                .post_translate(cx - side / 2.0, cy - side / 2.0);
        pixmap.draw_pixmap(
            0,
            0,
            icon.as_ref(),
            &tiny_skia::PixmapPaint {
                quality: tiny_skia::FilterQuality::Bicubic,
                ..Default::default()
            },
            transform,
            None,
        );
    }
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
