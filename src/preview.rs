//! Vista previa de una ventana al pasar el cursor por su programa en el dock: el
//! título y, debajo, su miniatura en vivo (la dibuja Windows con DWM, como en su
//! barra). Aquí está su tamaño y el dibujo del marco; la ventana vive en `main`.
use crate::apps::{fill_rounded, stroke_rounded};
use crate::clock::{draw_icon_aligned, draw_text_on_baseline, fit_text, line_metrics};
use tiny_skia::Pixmap;

const PAD: f32 = 8.0;
const TITLE_H: f32 = 32.0;
/// La miniatura cabe en este recuadro conservando la proporción de la ventana.
const MAX_W: f32 = 240.0;
const MAX_H: f32 = 150.0;
const MIN_W: f32 = 180.0;
const CLOSE: f32 = 26.0;

/// Medidas lógicas del panel; la miniatura y el botón de cerrar, relativos a él.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub width: f32,
    pub height: f32,
    pub thumb: (f32, f32, f32, f32),
    pub close: (f32, f32, f32, f32),
}

/// Medidas para una ventana de `source` píxeles (los de la ventana real), con la
/// escala de la pantalla: la miniatura se reduce hasta caber, sin agrandarse.
pub fn geometry(source: (f32, f32), scale: f32) -> Geometry {
    let (w, h) = ((source.0 / scale).max(1.0), (source.1 / scale).max(1.0));
    let fit = (MAX_W / w).min(MAX_H / h).min(1.0);
    let (thumb_w, thumb_h) = (w * fit, h * fit);
    let width = (thumb_w + PAD * 2.0).max(MIN_W);
    Geometry {
        width,
        height: TITLE_H + thumb_h + PAD,
        // Centrada si el panel es más ancho que la miniatura.
        thumb: ((width - thumb_w) / 2.0, TITLE_H, thumb_w, thumb_h),
        close: (
            width - PAD - CLOSE + 2.0,
            (TITLE_H - CLOSE) / 2.0,
            CLOSE,
            CLOSE,
        ),
    }
}

impl Geometry {
    pub fn in_close(&self, x: f32, y: f32) -> bool {
        let (cx, cy, cw, ch) = self.close;
        x >= cx && x < cx + cw && y >= cy && y < cy + ch
    }
}

/// Fondo, título y botón de cerrar; la miniatura la pone Windows encima.
pub fn draw(image: &mut Pixmap, geometry: &Geometry, title: &str, close_hovered: bool, scale: f32) {
    image.fill(tiny_skia::Color::TRANSPARENT);
    let (w, h) = (
        (geometry.width - 2.0) * scale,
        (geometry.height - 2.0) * scale,
    );
    fill_rounded(image, [32, 32, 34, 246], scale, scale, w, h, 8.0 * scale);
    stroke_rounded(
        image,
        [255, 255, 255, 34],
        scale,
        scale,
        w,
        h,
        8.0 * scale,
        scale * 0.75,
    );
    let size = 12.0 * scale;
    let (ascent, descent) = line_metrics(size);
    let room = (geometry.close.0 - PAD * 2.0) * scale;
    draw_text_on_baseline(
        image,
        &fit_text(title, size, room),
        (PAD + 4.0) * scale,
        TITLE_H / 2.0 * scale + (ascent - descent) / 2.0,
        size,
        0.95,
    );
    let (x, y, cw, ch) = geometry.close;
    if close_hovered {
        // Rojo de cerrar, como el de las ventanas de Windows.
        fill_rounded(
            image,
            [196, 43, 28, 255],
            x * scale,
            y * scale,
            cw * scale,
            ch * scale,
            5.0 * scale,
        );
    }
    draw_icon_aligned(
        image,
        '\u{E8BB}',
        (x + cw / 2.0) * scale,
        (y + ch / 2.0) * scale,
        10.0 * scale,
        0.9,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_keeps_its_proportion_and_fits() {
        // Ventana de 1920 × 1080 a 100 %: se reduce a 240 de ancho, 16:9.
        let g = geometry((1920.0, 1080.0), 1.0);
        assert_eq!(g.thumb.2, 240.0);
        assert_eq!(g.thumb.3, 135.0);
        assert_eq!(g.width, 240.0 + PAD * 2.0);
        assert_eq!(g.height, TITLE_H + 135.0 + PAD);
        // Ventana alta (vertical): manda el alto.
        let g = geometry((600.0, 1200.0), 1.0);
        assert_eq!(g.thumb.3, MAX_H);
        assert_eq!(g.thumb.2, 75.0);
        // Estrecha: el panel no baja del mínimo y la miniatura queda centrada.
        assert_eq!(g.width, MIN_W);
        assert_eq!(g.thumb.0, (MIN_W - 75.0) / 2.0);
        // A 150 % la ventana real mide más píxeles, pero el panel es igual.
        assert_eq!(
            geometry((2880.0, 1620.0), 1.5),
            geometry((1920.0, 1080.0), 1.0)
        );
        // Una ventana pequeña no se agranda.
        assert_eq!(geometry((200.0, 100.0), 1.0).thumb.2, 200.0);
        // El botón de cerrar está en la barra del título, a la derecha.
        let g = geometry((1920.0, 1080.0), 1.0);
        let (x, y, w, h) = g.close;
        assert!(g.in_close(x + w / 2.0, y + h / 2.0));
        assert!(!g.in_close(10.0, 10.0));
        assert!(x + w <= g.width && y + h <= TITLE_H);
    }
}
