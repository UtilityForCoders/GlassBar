//! Panel propio de las apps en segundo plano (los iconos de la bandeja de los demás
//! programas), que se abre sobre la flecha del dock: una cuadrícula de iconos como
//! la de Windows 11, con el estilo de los demás paneles de GlassBar.
use crate::apps::{fill_rounded, stroke_rounded};
use crate::clock::{draw_text, text_width};
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

/// Iconos por fila, como en Windows.
const COLUMNS: usize = 5;
const CELL: f32 = 40.0;
const PAD: f32 = 8.0;
/// Lado del icono dentro de su celda (el de los iconos pequeños de Windows).
const ICON: f32 = 16.0;
const EMPTY_W: f32 = 220.0;
const EMPTY_H: f32 = 48.0;
const EMPTY_TEXT: &str = "No hay apps en segundo plano";

pub struct TrayIcon {
    pub image: Option<Pixmap>,
    pub tip: String,
}

#[derive(Default)]
pub struct TrayPanel {
    pub icons: Vec<TrayIcon>,
    pub hovered: Option<usize>,
}

impl TrayPanel {
    fn columns(&self) -> usize {
        self.icons.len().clamp(1, COLUMNS)
    }
    fn rows(&self) -> usize {
        self.icons.len().div_ceil(COLUMNS)
    }
    /// Tamaño lógico del panel.
    pub fn size(&self) -> (f32, f32) {
        if self.icons.is_empty() {
            return (EMPTY_W, EMPTY_H);
        }
        (
            PAD * 2.0 + self.columns() as f32 * CELL,
            PAD * 2.0 + self.rows() as f32 * CELL,
        )
    }
    /// Esquina superior izquierda de la celda `index`.
    pub fn cell(&self, index: usize) -> (f32, f32) {
        (
            PAD + (index % COLUMNS) as f32 * CELL,
            PAD + (index / COLUMNS) as f32 * CELL,
        )
    }
    pub fn hit_test(&self, x: f32, y: f32) -> Option<usize> {
        if x < PAD || y < PAD {
            return None;
        }
        let (column, row) = (((x - PAD) / CELL) as usize, ((y - PAD) / CELL) as usize);
        let index = row * COLUMNS + column;
        (column < self.columns() && index < self.icons.len()).then_some(index)
    }

    pub fn draw(&self, image: &mut Pixmap, scale: f32) {
        image.fill(tiny_skia::Color::TRANSPARENT);
        let (w, h) = self.size();
        let (w, h) = ((w - 2.0) * scale, (h - 2.0) * scale);
        fill_rounded(image, [32, 32, 34, 246], scale, scale, w, h, 10.0 * scale);
        stroke_rounded(
            image,
            [255, 255, 255, 34],
            scale,
            scale,
            w,
            h,
            10.0 * scale,
            scale * 0.75,
        );
        if self.icons.is_empty() {
            let size = 12.0 * scale;
            draw_text(
                image,
                EMPTY_TEXT,
                (EMPTY_W * scale - text_width(EMPTY_TEXT, size)) / 2.0,
                (EMPTY_H / 2.0 - 8.0) * scale,
                size,
                0.6,
            );
            return;
        }
        let side = (ICON * scale).round().max(1.0);
        for (index, icon) in self.icons.iter().enumerate() {
            let (x, y) = self.cell(index);
            if self.hovered == Some(index) {
                fill_rounded(
                    image,
                    [255, 255, 255, 22],
                    (x + 2.0) * scale,
                    (y + 2.0) * scale,
                    (CELL - 4.0) * scale,
                    (CELL - 4.0) * scale,
                    6.0 * scale,
                );
            }
            let Some(picture) = &icon.image else {
                continue;
            };
            let left = ((x + CELL / 2.0) * scale - side / 2.0).round();
            let top = ((y + CELL / 2.0) * scale - side / 2.0).round();
            // Del tamaño justo se copia tal cual; si no, se escala con suavizado.
            let exact = picture.width() as f32 == side;
            let transform = Transform::from_scale(
                side / picture.width() as f32,
                side / picture.height() as f32,
            )
            .post_translate(left, top);
            image.draw_pixmap(
                0,
                0,
                picture.as_ref(),
                &PixmapPaint {
                    quality: if exact {
                        FilterQuality::Nearest
                    } else {
                        FilterQuality::Bicubic
                    },
                    ..Default::default()
                },
                transform,
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(count: usize) -> TrayPanel {
        TrayPanel {
            icons: (0..count)
                .map(|i| TrayIcon {
                    image: None,
                    tip: format!("App {i}"),
                })
                .collect(),
            hovered: None,
        }
    }

    #[test]
    fn icons_fill_rows_of_five_like_windows() {
        // 15 iconos: tres filas de cinco, como en la captura del panel de Windows.
        let full = panel(15);
        assert_eq!(
            full.size(),
            (PAD * 2.0 + 5.0 * CELL, PAD * 2.0 + 3.0 * CELL)
        );
        // Pocos iconos: el panel solo es tan ancho como ellos.
        assert_eq!(panel(2).size(), (PAD * 2.0 + 2.0 * CELL, PAD * 2.0 + CELL));
        assert_eq!(panel(0).size(), (EMPTY_W, EMPTY_H));

        // Cada celda responde a su icono; los huecos de la última fila, no.
        let partial = panel(7);
        for index in 0..7 {
            let (x, y) = partial.cell(index);
            assert_eq!(
                partial.hit_test(x + CELL / 2.0, y + CELL / 2.0),
                Some(index)
            );
        }
        let (x, y) = partial.cell(8);
        assert_eq!(partial.hit_test(x + CELL / 2.0, y + CELL / 2.0), None);
        assert_eq!(partial.hit_test(PAD / 2.0, PAD / 2.0), None);
    }
}
