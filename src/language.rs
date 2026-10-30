//! Panel del idioma del teclado, que se abre sobre el indicador del dock con el
//! estilo del panel rápido: un teclado por fila (abreviatura, nombre y una marca en
//! el actual) y, abajo, la configuración de idioma de Windows.
use crate::apps::{fill_rounded, stroke_rounded};
use crate::clock::{
    draw_icon_aligned, draw_text_on_baseline, draw_text_semibold, fit_text, line_metrics,
    text_width,
};
use crate::keyboard::Layout;
use tiny_skia::Pixmap;

pub const WIDTH: f32 = 320.0;
const PAD: f32 = 12.0;
const TITLE_H: f32 = 44.0;
const ROW: f32 = 48.0;
const FOOTER_H: f32 = 48.0;
const BADGE_W: f32 = 44.0;
const BADGE_H: f32 = 32.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// Teclado, por su posición.
    Layout(usize),
    /// "Más configuración de idioma".
    Settings,
}

#[derive(Default)]
pub struct LanguageState {
    pub layouts: Vec<Layout>,
    /// Teclado de la ventana activa.
    pub current: Option<isize>,
    pub hovered: Option<Hit>,
}

impl LanguageState {
    fn footer_y(&self) -> f32 {
        TITLE_H + self.layouts.len() as f32 * ROW + 8.0
    }
    pub fn height(&self) -> f32 {
        self.footer_y() + FOOTER_H
    }
    fn row_top(index: usize) -> f32 {
        TITLE_H + index as f32 * ROW
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<Hit> {
        if !(0.0..WIDTH).contains(&x) {
            return None;
        }
        if y >= TITLE_H && y < TITLE_H + self.layouts.len() as f32 * ROW {
            return Some(Hit::Layout(((y - TITLE_H) / ROW) as usize));
        }
        (y >= self.footer_y() && y < self.height()).then_some(Hit::Settings)
    }

    /// Flechas arriba y abajo: recorren los teclados, empezando por el actual.
    pub fn move_selection(&mut self, delta: isize) {
        let count = self.layouts.len() as isize;
        if count == 0 {
            return;
        }
        let from = match self.hovered {
            Some(Hit::Layout(i)) => i as isize,
            _ => self
                .layouts
                .iter()
                .position(|l| Some(l.id) == self.current)
                .map_or(-1, |i| i as isize),
        };
        self.hovered = Some(Hit::Layout((from + delta).rem_euclid(count) as usize));
    }

    pub fn draw(&self, image: &mut Pixmap, scale: f32) {
        image.fill(tiny_skia::Color::TRANSPARENT);
        let (w, h) = ((WIDTH - 2.0) * scale, (self.height() - 2.0) * scale);
        fill_rounded(image, [32, 32, 34, 246], scale, scale, w, h, 12.0 * scale);
        // Línea sobre el pie, como en el panel rápido.
        fill_rounded(
            image,
            [255, 255, 255, 20],
            scale,
            (self.footer_y() * scale).round(),
            w,
            scale.max(1.0),
            0.0,
        );
        stroke_rounded(
            image,
            [255, 255, 255, 34],
            scale,
            scale,
            w,
            h,
            12.0 * scale,
            scale * 0.75,
        );
        draw_text_semibold(
            image,
            "Idioma del teclado",
            (PAD + 4.0) * scale,
            15.0 * scale,
            14.0 * scale,
            1.0,
        );
        let size = 13.0 * scale;
        let (ascent, descent) = line_metrics(size);
        for (index, layout) in self.layouts.iter().enumerate() {
            let top = Self::row_top(index);
            let cy = top + ROW / 2.0;
            let current = Some(layout.id) == self.current;
            let hovered = self.hovered == Some(Hit::Layout(index));
            if hovered || current {
                fill_rounded(
                    image,
                    [255, 255, 255, if hovered { 20 } else { 10 }],
                    (PAD - 4.0) * scale,
                    (top + 3.0) * scale,
                    (WIDTH - PAD * 2.0 + 8.0) * scale,
                    (ROW - 6.0) * scale,
                    6.0 * scale,
                );
            }
            draw_badge(image, layout, PAD + 4.0 + BADGE_W / 2.0, cy, scale);
            let left = PAD + 4.0 + BADGE_W + 12.0;
            let room = (WIDTH - PAD - 28.0 - left) * scale;
            draw_text_on_baseline(
                image,
                &fit_text(&layout.name, size, room),
                left * scale,
                cy * scale + (ascent - descent) / 2.0,
                size,
                if current { 1.0 } else { 0.9 },
            );
            if current {
                draw_icon_aligned(
                    image,
                    '\u{E73E}',
                    (WIDTH - PAD - 10.0) * scale,
                    cy * scale,
                    12.0 * scale,
                    0.95,
                );
            }
        }
        let footer = self.footer_y();
        if self.hovered == Some(Hit::Settings) {
            fill_rounded(
                image,
                [255, 255, 255, 20],
                (PAD - 4.0) * scale,
                (footer + 8.0) * scale,
                (WIDTH - PAD * 2.0 + 8.0) * scale,
                (FOOTER_H - 16.0) * scale,
                5.0 * scale,
            );
        }
        draw_text_on_baseline(
            image,
            "Más configuración de idioma",
            (PAD + 4.0) * scale,
            (footer + FOOTER_H / 2.0) * scale + (ascent - descent) / 2.0,
            size,
            0.9,
        );
    }
}

/// Abreviatura del teclado en un recuadro, como la muestra el dock: idioma y,
/// debajo, la distribución si no es la normal.
fn draw_badge(image: &mut Pixmap, layout: &Layout, cx: f32, cy: f32, scale: f32) {
    fill_rounded(
        image,
        [255, 255, 255, 18],
        (cx - BADGE_W / 2.0) * scale,
        (cy - BADGE_H / 2.0) * scale,
        BADGE_W * scale,
        BADGE_H * scale,
        5.0 * scale,
    );
    let size = 11.0 * scale;
    let (ascent, descent) = line_metrics(size);
    let line = ascent + descent;
    let lines: Vec<&str> = std::iter::once(layout.language.as_str())
        .chain(layout.variant.as_deref())
        .collect();
    let top = cy * scale - line * lines.len() as f32 / 2.0;
    for (index, text) in lines.iter().enumerate() {
        draw_text_on_baseline(
            image,
            text,
            cx * scale - text_width(text, size) / 2.0,
            top + line * index as f32 + ascent,
            size,
            0.95,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(id: isize, language: &str) -> Layout {
        Layout {
            id,
            language: language.into(),
            variant: None,
            name: format!("Teclado {id}"),
        }
    }

    #[test]
    fn rows_and_footer_are_where_they_are_drawn() {
        let mut panel = LanguageState {
            layouts: vec![layout(1, "ESP"), layout(2, "ENG"), layout(3, "FRA")],
            current: Some(2),
            hovered: None,
        };
        assert_eq!(panel.height(), TITLE_H + 3.0 * ROW + 8.0 + FOOTER_H);
        assert_eq!(panel.hit_test(WIDTH / 2.0, TITLE_H / 2.0), None);
        for index in 0..3 {
            let y = LanguageState::row_top(index) + ROW / 2.0;
            assert_eq!(panel.hit_test(WIDTH / 2.0, y), Some(Hit::Layout(index)));
        }
        let footer = panel.footer_y() + FOOTER_H / 2.0;
        assert_eq!(panel.hit_test(WIDTH / 2.0, footer), Some(Hit::Settings));
        assert_eq!(panel.hit_test(WIDTH / 2.0, panel.height() + 1.0), None);

        // Las flechas empiezan en el teclado actual y dan la vuelta.
        panel.move_selection(1);
        assert_eq!(panel.hovered, Some(Hit::Layout(2)));
        panel.move_selection(1);
        assert_eq!(panel.hovered, Some(Hit::Layout(0)));
        panel.move_selection(-1);
        assert_eq!(panel.hovered, Some(Hit::Layout(2)));
    }
}
