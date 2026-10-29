//! Hora con Segoe UI del sistema y antialias en escala de grises.
use fontdue::{Font, FontSettings};
use std::{cell::RefCell, sync::OnceLock};
use tiny_skia::{Pixmap, PixmapPaint, Transform};
use windows::Win32::System::SystemInformation::GetLocalTime;

fn system_font() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| {
        let windows = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
        let path = std::path::PathBuf::from(windows)
            .join("Fonts")
            .join("segoeui.ttf");
        let bytes = std::fs::read(path).expect("Segoe UI, fuente del sistema Windows");
        Font::from_bytes(bytes, FontSettings::default()).expect("Segoe UI válida")
    })
}

pub struct ClockRenderer {
    cached: RefCell<Option<(String, u32, Pixmap)>>,
}
impl ClockRenderer {
    pub fn new() -> Self {
        Self {
            cached: RefCell::new(None),
        }
    }
    pub fn draw(&self, target: &mut Pixmap, cx: f32, cy: f32, scale: f32) {
        let time = unsafe { GetLocalTime() };
        let text = format!("{:02}:{:02}", time.wHour, time.wMinute);
        let pixels = (13.0 * scale).round().max(8.0) as u32;
        let mut cached = self.cached.borrow_mut();
        if !cached
            .as_ref()
            .is_some_and(|(old, size, _)| old == &text && *size == pixels)
        {
            *cached = Some((text.clone(), pixels, rasterize(&text, pixels as f32)));
        }
        if let Some((_, _, image)) = &*cached {
            target.draw_pixmap(
                (cx - image.width() as f32 / 2.0).round() as i32,
                (cy - image.height() as f32 / 2.0).round() as i32,
                image.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
    }
}

fn rasterize(text: &str, size: f32) -> Pixmap {
    let font = system_font();
    let glyphs: Vec<_> = text.chars().map(|c| font.rasterize(c, size)).collect();
    let width = glyphs
        .iter()
        .map(|(m, _)| m.advance_width)
        .sum::<f32>()
        .ceil() as u32
        + 2;
    let top = glyphs
        .iter()
        .map(|(m, _)| m.ymin + m.height as i32)
        .max()
        .unwrap_or(1);
    let bottom = glyphs.iter().map(|(m, _)| m.ymin).min().unwrap_or(0);
    let mut image = Pixmap::new(width, (top - bottom).max(1) as u32).unwrap();
    let mut pen: f32 = 1.0;
    for (metrics, coverage) in glyphs {
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
    image
}

/// Texto de interfaz con la misma fuente que el reloj; `y` es el borde superior.
pub fn draw_text(target: &mut Pixmap, text: &str, x: f32, y: f32, size: f32, opacity: f32) {
    if text.is_empty() {
        return;
    }
    let image = rasterize(text, size);
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
    let measure = |s: &str| {
        s.chars()
            .map(|c| font.metrics(c, size).advance_width)
            .sum::<f32>()
    };
    if measure(text) <= width {
        return text.into();
    }
    let mut short = String::new();
    for c in text.chars() {
        let candidate = format!("{short}{c}…");
        if measure(&candidate) > width {
            break;
        }
        short.push(c);
    }
    short.push('…');
    short
}

#[cfg(test)]
mod tests {
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
