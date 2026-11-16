//! Hora y fecha con Segoe UI del sistema y antialias en escala de grises.
use crate::font::Font;
use std::{cell::RefCell, sync::OnceLock};
use tiny_skia::{Pixmap, PixmapPaint, Transform};
use windows::Win32::System::SystemInformation::GetLocalTime;

fn font_file(name: &str) -> Option<Font> {
    let windows = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
    let path = std::path::PathBuf::from(windows).join("Fonts").join(name);
    Font::from_file(&path)
}

fn system_font() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| font_file("segoeui.ttf").expect("Segoe UI, fuente del sistema Windows"))
}

/// Iconos del sistema: Segoe Fluent Icons (Windows 11) o Segoe MDL2 Assets (Windows 10).
fn icon_font() -> Option<&'static Font> {
    static FONT: OnceLock<Option<Font>> = OnceLock::new();
    FONT.get_or_init(|| font_file("SegoeIcons.ttf").or_else(|| font_file("segmdl2.ttf")))
        .as_ref()
}

/// Dibuja un icono de la fuente de iconos de Windows centrado en (`cx`, `cy`).
pub fn draw_icon(target: &mut Pixmap, icon: char, cx: f32, cy: f32, size: f32, opacity: f32) {
    let Some(font) = icon_font() else {
        return;
    };
    let image = rasterize_with(font, &icon.to_string(), size);
    let (w, h) = (image.width() as f32, image.height() as f32);
    blit_text(target, image, cx - w / 2.0, cy - h / 2.0, opacity);
}

/// Como `draw_icon`, pero centra el cuadro de la fuente y no el dibujo de cada
/// icono. Los iconos de Segoe Fluent están diseñados sobre ese cuadro, así que los
/// de un mismo grupo (red, volumen, batería) quedan alineados como en Windows.
pub fn draw_icon_aligned(
    target: &mut Pixmap,
    icon: char,
    cx: f32,
    cy: f32,
    size: f32,
    opacity: f32,
) {
    let Some(font) = icon_font() else {
        return;
    };
    let (image, top) = rasterize_line(font, &icon.to_string(), size);
    let advance = font.metrics(icon, size).advance_width;
    let (ascent, descent) = font
        .horizontal_line_metrics(size)
        .map(|m| (m.ascent, m.descent))
        .unwrap_or((size, 0.0));
    // El cuadro va de `ascent` sobre la línea base a `descent` (negativo) bajo ella.
    let baseline = cy + (ascent + descent) / 2.0;
    // La imagen empieza un píxel antes de la pluma (ver `rasterize_line`).
    blit_text(
        target,
        image,
        (cx - advance / 2.0 - 1.0).round(),
        (baseline - top as f32).round(),
        opacity,
    );
}

/// Segoe UI Semibold para encabezados; si falta, la normal.
fn semibold_font() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| font_file("seguisb.ttf").unwrap_or_else(|| system_font().clone()))
}

/// El minuto actual (con el día), para saber si lo dibujado con la hora sigue al día.
pub fn minute() -> u64 {
    let now = unsafe { GetLocalTime() };
    ((now.wYear as u64 * 13 + now.wMonth as u64) * 32 + now.wDay as u64) * 1440
        + now.wHour as u64 * 60
        + now.wMinute as u64
}

/// Si el reloj muestra los segundos (ajuste de la configuración).
static SECONDS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_seconds(on: bool) {
    SECONDS.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// La hora en el formato regional del usuario (24 horas o 12 con "a. m."/"p. m."),
/// con segundos si están activados, como el reloj de la barra de Windows.
fn format_time(time: &windows::Win32::Foundation::SYSTEMTIME) -> String {
    use windows::Win32::Globalization::{GetTimeFormatEx, TIME_FORMAT_FLAGS, TIME_NOSECONDS};
    let seconds = SECONDS.load(std::sync::atomic::Ordering::Relaxed);
    let flags = if seconds {
        TIME_FORMAT_FLAGS(0)
    } else {
        TIME_NOSECONDS
    };
    let mut buffer = [0u16; 64];
    // Sin nombre de configuración regional ni formato: los del usuario.
    let len = unsafe {
        GetTimeFormatEx(
            windows::core::PCWSTR::null(),
            flags,
            Some(time),
            windows::core::PCWSTR::null(),
            Some(&mut buffer),
        )
    };
    if len <= 1 {
        return fallback_time(time.wHour, time.wMinute, seconds.then_some(time.wSecond));
    }
    String::from_utf16_lossy(&buffer[..len as usize - 1])
}

/// La hora en 24 horas, por si Windows no la da con el formato regional.
fn fallback_time(hour: u16, minute: u16, second: Option<u16>) -> String {
    match second {
        Some(second) => format!("{hour:02}:{minute:02}:{second:02}"),
        None => format!("{hour:02}:{minute:02}"),
    }
}

/// Opacidad de la fecha: queda por detrás de la hora.
const DATE_OPACITY: f32 = 0.75;

/// Tamaño de la fecha para una hora de `pixels`: unos dos puntos menos.
fn date_size(pixels: u32) -> f32 {
    (pixels as f32 * 11.0 / 13.0).round()
}

/// Hora y, debajo, la fecha corta, como la barra de Windows. Las dos líneas se
/// rasterizan solo cuando cambian (una vez por minuto).
pub struct ClockRenderer {
    cached: RefCell<Option<Clock>>,
}
struct Clock {
    time: String,
    date: String,
    pixels: u32,
    /// Cada línea con cuánto sube sobre su línea base.
    lines: [(Pixmap, i32); 2],
}
impl ClockRenderer {
    pub fn new() -> Self {
        Self {
            cached: RefCell::new(None),
        }
    }
    /// Si la hora mostrada ya no es la actual (cambió el minuto) y hay que redibujar.
    pub fn outdated(&self) -> bool {
        let time = format_time(&unsafe { GetLocalTime() });
        self.cached.borrow().as_ref().is_none_or(|c| c.time != time)
    }
    /// `room` es el ancho disponible: una hora más larga (12 horas con segundos)
    /// se dibuja más pequeña para no salirse.
    pub fn draw(&self, target: &mut Pixmap, cx: f32, cy: f32, scale: f32, room: f32) {
        let now = unsafe { GetLocalTime() };
        let time = format_time(&now);
        let date = format_date(&now, false);
        // La hora, más grande; la fecha, más pequeña y atenuada debajo.
        let pixels = (14.0 * scale).round().max(8.0) as u32;
        let mut cached = self.cached.borrow_mut();
        if !cached
            .as_ref()
            .is_some_and(|c| c.time == time && c.date == date && c.pixels == pixels)
        {
            let fitted = |text: &str, size: f32| {
                let line = rasterize_line(system_font(), text, size);
                let width = line.0.width() as f32;
                if width <= room {
                    return line;
                }
                rasterize_line(system_font(), text, (size * room / width).floor().max(6.0))
            };
            let lines = [
                fitted(&time, pixels as f32),
                fitted(&date, date_size(pixels)),
            ];
            *cached = Some(Clock {
                time,
                date,
                pixels,
                lines,
            });
        }
        let Some(clock) = &*cached else {
            return;
        };
        // Dos líneas seguidas centradas en `cy`, cada una sobre su línea base.
        let metrics = [
            line_metrics(clock.pixels as f32),
            line_metrics(date_size(clock.pixels)),
        ];
        let height: f32 = metrics.iter().map(|(a, d)| a + d).sum();
        let mut top = cy - height / 2.0;
        for (((image, rise), (ascent, descent)), opacity) in
            clock.lines.iter().zip(metrics).zip([1.0, DATE_OPACITY])
        {
            let baseline = top + ascent;
            target.draw_pixmap(
                (cx - image.width() as f32 / 2.0).round() as i32,
                (baseline - *rise as f32).round() as i32,
                image.as_ref(),
                &PixmapPaint {
                    opacity,
                    ..Default::default()
                },
                Transform::identity(),
                None,
            );
            top += ascent + descent;
        }
    }
}

/// Fecha de hoy en el formato regional del usuario: corta ("30/10/2026") o larga
/// ("jueves, 30 de octubre de 2026").
pub fn today(long: bool) -> String {
    format_date(&unsafe { GetLocalTime() }, long)
}

/// Día y hora en una línea, como la barra de menús de macOS: "jue 13 nov 22:09".
/// Los nombres salen de la configuración regional del usuario.
pub fn day_and_time() -> String {
    use windows::Win32::Globalization::GetDateFormatEx;
    let now = unsafe { GetLocalTime() };
    let mut buffer = [0u16; 64];
    let len = unsafe {
        GetDateFormatEx(
            windows::core::PCWSTR::null(),
            Default::default(),
            Some(&now),
            windows::core::w!("ddd d MMM"),
            Some(&mut buffer),
            windows::core::PCWSTR::null(),
        )
    };
    let day = if len > 1 {
        String::from_utf16_lossy(&buffer[..len as usize - 1]).replace('.', "")
    } else {
        format_date(&now, false)
    };
    format!("{day}  {}", format_time(&now))
}

fn format_date(date: &windows::Win32::Foundation::SYSTEMTIME, long: bool) -> String {
    use windows::Win32::Globalization::{GetDateFormatEx, DATE_LONGDATE, DATE_SHORTDATE};
    let flags = if long { DATE_LONGDATE } else { DATE_SHORTDATE };
    let mut buffer = [0u16; 128];
    // Sin nombre de configuración regional: la del usuario.
    let len = unsafe {
        GetDateFormatEx(
            windows::core::PCWSTR::null(),
            flags,
            Some(date),
            windows::core::PCWSTR::null(),
            Some(&mut buffer),
            windows::core::PCWSTR::null(),
        )
    };
    if len <= 1 {
        return format!("{:02}/{:02}/{}", date.wDay, date.wMonth, date.wYear);
    }
    String::from_utf16_lossy(&buffer[..len as usize - 1])
}

fn rasterize(text: &str, size: f32) -> Pixmap {
    rasterize_with(system_font(), text, size)
}

type Glyph = std::rc::Rc<(crate::font::Metrics, Vec<u8>)>;

/// Rasterizar un glifo es lo más caro de pintar texto; el lanzador redibuja
/// decenas de nombres en cada cambio, así que cada glifo se rasteriza una vez.
fn glyph(font: &Font, c: char, size: f32) -> Glyph {
    thread_local! {
        static CACHE: RefCell<std::collections::HashMap<(usize, char, u32), Glyph>> =
            RefCell::new(std::collections::HashMap::new());
    }
    let key = (font as *const Font as usize, c, size.to_bits());
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(glyph) = cache.get(&key) {
            return glyph.clone();
        }
        if cache.len() > 8192 {
            cache.clear();
        }
        let glyph = Glyph::new(font.rasterize(c, size));
        cache.insert(key, glyph.clone());
        glyph
    })
}

fn rasterize_with(font: &Font, text: &str, size: f32) -> Pixmap {
    rasterize_line(font, text, size).0
}

/// Texto rasterizado y cuántos píxeles sube su glifo más alto sobre la línea base
/// (la primera fila de la imagen).
fn rasterize_line(font: &Font, text: &str, size: f32) -> (Pixmap, i32) {
    let glyphs: Vec<Glyph> = text.chars().map(|c| glyph(font, c, size)).collect();
    let width = glyphs.iter().map(|g| g.0.advance_width).sum::<f32>().ceil() as u32 + 2;
    let top = glyphs
        .iter()
        .map(|g| g.0.ymin + g.0.height as i32)
        .max()
        .unwrap_or(1);
    let bottom = glyphs.iter().map(|g| g.0.ymin).min().unwrap_or(0);
    let mut image = Pixmap::new(width, (top - bottom).max(1) as u32).unwrap();
    let mut pen: f32 = 1.0;
    for glyph in &glyphs {
        let (metrics, coverage) = (&glyph.0, &glyph.1);
        for y in 0..metrics.height {
            for x in 0..metrics.width {
                let px = pen.round() as i32 + metrics.xmin + x as i32;
                let py = top - metrics.ymin - metrics.height as i32 + y as i32;
                if px >= 0 && py >= 0 && px < image.width() as i32 && py < image.height() as i32 {
                    let alpha = (coverage[y * metrics.width + x] as u16 * 235 / 255) as u8;
                    let i = ((py as u32 * image.width() + px as u32) * 4) as usize;
                    image.data_mut()[i..i + 4].copy_from_slice(&[alpha, alpha, alpha, alpha]);
                }
            }
        }
        pen += metrics.advance_width;
    }
    (image, top)
}

/// Texto de interfaz con la misma fuente que el reloj; `y` es el borde superior.
pub fn draw_text(target: &mut Pixmap, text: &str, x: f32, y: f32, size: f32, opacity: f32) {
    blit_text(target, rasterize(text, size), x, y, opacity);
}

/// Como `draw_text`, pero apoyado en la línea base `baseline`: el texto no sube ni
/// baja según las letras que lleve (p. ej. "a" frente a "A"), como al escribir.
pub fn draw_text_on_baseline(
    target: &mut Pixmap,
    text: &str,
    x: f32,
    baseline: f32,
    size: f32,
    opacity: f32,
) {
    let (image, top) = rasterize_line(system_font(), text, size);
    blit_text(target, image, x, baseline - top as f32, opacity);
}

/// Cuánto sube y baja una línea de Segoe UI respecto a su línea base.
pub fn line_metrics(size: f32) -> (f32, f32) {
    system_font()
        .horizontal_line_metrics(size)
        .map(|m| (m.ascent, -m.descent))
        .unwrap_or((size * 1.08, size * 0.25))
}

/// Como `draw_text`, en Segoe UI Semibold.
pub fn draw_text_semibold(
    target: &mut Pixmap,
    text: &str,
    x: f32,
    y: f32,
    size: f32,
    opacity: f32,
) {
    blit_text(
        target,
        rasterize_with(semibold_font(), text, size),
        x,
        y,
        opacity,
    );
}

/// Como `draw_text_on_baseline`, en Segoe UI Semibold.
pub fn draw_text_semibold_on_baseline(
    target: &mut Pixmap,
    text: &str,
    x: f32,
    baseline: f32,
    size: f32,
    opacity: f32,
) {
    let (image, top) = rasterize_line(semibold_font(), text, size);
    blit_text(target, image, x, baseline - top as f32, opacity);
}

fn blit_text(target: &mut Pixmap, image: Pixmap, x: f32, y: f32, opacity: f32) {
    if image.width() <= 2 {
        return;
    }
    target.draw_pixmap(
        x.round() as i32,
        y.round() as i32,
        image.as_ref(),
        &PixmapPaint {
            opacity,
            ..Default::default()
        },
        Transform::identity(),
        None,
    );
}
pub fn text_width(text: &str, size: f32) -> f32 {
    text.chars()
        .map(|c| system_font().metrics(c, size).advance_width)
        .sum()
}
pub fn fit_text(text: &str, size: f32, width: f32) -> String {
    let font = system_font();
    let advance = |c: char| font.metrics(c, size).advance_width;
    if text.chars().map(advance).sum::<f32>() <= width {
        return text.into();
    }
    // Las letras que quepan junto con "…", sumando anchos sin crear textos de prueba.
    let room = width - advance('…');
    let mut used = 0.0;
    let mut short: String = text
        .chars()
        .take_while(|&c| {
            used += advance(c);
            used <= room
        })
        .collect();
    short.push('…');
    short
}

#[cfg(test)]
mod tests {
    #[test]
    fn time_follows_the_regional_format_and_the_seconds_setting() {
        use windows::Win32::Foundation::SYSTEMTIME;
        let time = SYSTEMTIME {
            wYear: 2026,
            wMonth: 11,
            wDay: 16,
            wHour: 21,
            wMinute: 5,
            wSecond: 7,
            ..Default::default()
        };
        // El formato es el del usuario (24 horas o 12 con a. m./p. m.): lo seguro es
        // que lleva los minutos y, solo con el ajuste, los segundos.
        super::set_seconds(false);
        let short = super::format_time(&time);
        assert!(short.contains("05") && !short.contains("07"), "{short}");
        super::set_seconds(true);
        let long = super::format_time(&time);
        assert!(long.contains("05") && long.contains("07"), "{long}");
        super::set_seconds(false);
        // Sin formato regional, 24 horas.
        assert_eq!(super::fallback_time(9, 5, None), "09:05");
        assert_eq!(super::fallback_time(21, 5, Some(7)), "21:05:07");
    }

    #[test]
    fn clock_uses_smooth_system_glyphs() {
        let image = super::rasterize("09:41", 13.0);
        assert!(image.width() > 20 && image.width() < 60);
        assert!(image
            .pixels()
            .iter()
            .any(|p| p.alpha() > 0 && p.alpha() < 200));
        assert!(image.height() <= 16);
    }
}
