//! Menú propio de una app del dock, con el estilo del de la barra de Windows 11:
//! panel oscuro redondeado, títulos de sección en gris, un icono por opción y
//! resaltado al pasar el cursor. Aquí está solo el contenido y su dibujo; la
//! ventana que lo muestra está en `main`.
use crate::apps::{fill_rounded, stroke_rounded};
use crate::clock::{draw_icon, draw_text, fit_text, text_width};
use tiny_skia::{Pixmap, PixmapPaint, Transform};

/// Opciones fijas del menú de una app.
pub const CMD_APP_OPEN: u32 = 10;
pub const CMD_APP_PIN: u32 = 11;
pub const CMD_APP_UNPIN: u32 = 12;
pub const CMD_APP_CLOSE: u32 = 13;
pub const CMD_APP_END_TASK: u32 = 14;
/// Elementos de la jump list: `CMD_JUMP + índice`, seguidos sección tras sección.
pub const CMD_JUMP: u32 = 100;

/// Glifos de Segoe Fluent Icons para las opciones fijas.
pub const GLYPH_PIN: char = '\u{E718}';
pub const GLYPH_UNPIN: char = '\u{E77A}';
pub const GLYPH_END_TASK: char = '\u{E71A}';
pub const GLYPH_CLOSE: char = '\u{E8BB}';

/// Lado de los iconos, en unidades lógicas (los iconos pequeños de Windows).
pub const ICON: f32 = 16.0;
const MIN_W: f32 = 240.0;
const MAX_W: f32 = 340.0;
const PAD: f32 = 4.0;
const ITEM_H: f32 = 32.0;
const HEADER_H: f32 = 28.0;
const SEPARATOR_H: f32 = 9.0;
const TEXT: f32 = 13.0;
const HEADER_TEXT: f32 = 12.0;
/// Margen del texto y del icono dentro de cada fila.
const ICON_X: f32 = 12.0;
const TEXT_X: f32 = ICON_X + ICON + 12.0;
const RADIUS: f32 = 8.0;

/// Icono de una opción: una imagen ya reducida a `ICON` por la escala, o un glifo.
pub enum Icon {
    Image(Option<Pixmap>),
    Glyph(char),
}

pub enum Entry {
    /// Título de sección ("Más visitado", "Tareas"…).
    Header(String),
    Item {
        icon: Icon,
        label: String,
        command: u32,
    },
    Separator,
}

#[derive(Default)]
pub struct AppMenu {
    pub entries: Vec<Entry>,
    /// Opción resaltada (índice en `entries`), con el ratón o con las flechas.
    pub hovered: Option<usize>,
    width: f32,
}

impl AppMenu {
    pub fn new(entries: Vec<Entry>) -> Self {
        // Tan ancho como la opción más larga, entre un mínimo y un máximo.
        let widest = entries
            .iter()
            .map(|entry| match entry {
                Entry::Item { label, .. } => text_width(label, TEXT) + TEXT_X + 16.0,
                Entry::Header(title) => text_width(title, HEADER_TEXT) + ICON_X * 2.0,
                Entry::Separator => 0.0,
            })
            .fold(0.0, f32::max);
        Self {
            entries,
            hovered: None,
            width: (widest + PAD * 2.0).clamp(MIN_W, MAX_W),
        }
    }

    /// Altura de cada tipo de fila.
    fn height_of(entry: &Entry) -> f32 {
        match entry {
            Entry::Header(_) => HEADER_H,
            Entry::Item { .. } => ITEM_H,
            Entry::Separator => SEPARATOR_H,
        }
    }

    /// Borde superior de cada fila.
    fn tops(&self) -> Vec<f32> {
        let mut y = PAD;
        self.entries
            .iter()
            .map(|entry| {
                let top = y;
                y += Self::height_of(entry);
                top
            })
            .collect()
    }

    /// Tamaño lógico del menú.
    pub fn size(&self) -> (f32, f32) {
        let height: f32 = self.entries.iter().map(Self::height_of).sum();
        (self.width, height + PAD * 2.0)
    }

    /// Opción bajo el punto (coordenadas lógicas del menú); los títulos y
    /// separadores no cuentan.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<usize> {
        if x < PAD || x >= self.width - PAD {
            return None;
        }
        self.tops()
            .into_iter()
            .zip(&self.entries)
            .position(|(top, entry)| {
                matches!(entry, Entry::Item { .. }) && y >= top && y < top + ITEM_H
            })
    }

    /// Orden de la opción `index`, si es una opción.
    pub fn command(&self, index: usize) -> Option<u32> {
        match self.entries.get(index)? {
            Entry::Item { command, .. } => Some(*command),
            _ => None,
        }
    }

    /// Mueve el resaltado con las flechas a la opción siguiente o anterior,
    /// saltando títulos y separadores. Sin resaltado, empieza por un extremo.
    pub fn move_hover(&mut self, forward: bool) {
        let items: Vec<usize> = (0..self.entries.len())
            .filter(|&i| self.command(i).is_some())
            .collect();
        let (Some(&first), Some(&last)) = (items.first(), items.last()) else {
            return;
        };
        let current = self
            .hovered
            .and_then(|h| items.iter().position(|&i| i == h));
        self.hovered = Some(match (current, forward) {
            (None, true) => first,
            (None, false) => last,
            (Some(i), true) => items[(i + 1).min(items.len() - 1)],
            (Some(i), false) => items[i.saturating_sub(1)],
        });
    }

    pub fn draw(&self, image: &mut Pixmap, scale: f32) {
        image.fill(tiny_skia::Color::TRANSPARENT);
        let (w, h) = self.size();
        let s = |v: f32| v * scale;
        // Panel oscuro como los menús de Windows 11.
        fill_rounded(
            image,
            [44, 44, 44, 250],
            s(0.5),
            s(0.5),
            s(w - 1.0),
            s(h - 1.0),
            s(RADIUS),
        );
        stroke_rounded(
            image,
            [255, 255, 255, 26],
            s(0.5),
            s(0.5),
            s(w - 1.0),
            s(h - 1.0),
            s(RADIUS),
            scale * 0.75,
        );
        for ((top, entry), index) in self.tops().into_iter().zip(&self.entries).zip(0..) {
            match entry {
                Entry::Header(title) => {
                    let title = fit_text(title, s(HEADER_TEXT), s(w - PAD * 2.0 - ICON_X * 2.0));
                    draw_text(
                        image,
                        &title,
                        s(PAD + ICON_X),
                        s(top + 9.0),
                        s(HEADER_TEXT),
                        0.6,
                    );
                }
                Entry::Separator => {
                    fill_rounded(
                        image,
                        [255, 255, 255, 22],
                        s(PAD),
                        s(top + SEPARATOR_H / 2.0).round(),
                        s(w - PAD * 2.0),
                        scale.max(1.0),
                        0.0,
                    );
                }
                Entry::Item { icon, label, .. } => {
                    if self.hovered == Some(index) {
                        fill_rounded(
                            image,
                            [255, 255, 255, 16],
                            s(PAD),
                            s(top + 2.0),
                            s(w - PAD * 2.0),
                            s(ITEM_H - 4.0),
                            s(4.0),
                        );
                    }
                    let (icon_x, center_y) = (s(PAD + ICON_X), s(top + ITEM_H / 2.0));
                    match icon {
                        Icon::Image(Some(picture)) => {
                            let side = s(ICON).round();
                            let factor = side / picture.width().max(1) as f32;
                            image.draw_pixmap(
                                0,
                                0,
                                picture.as_ref(),
                                &PixmapPaint {
                                    quality: tiny_skia::FilterQuality::Bicubic,
                                    ..Default::default()
                                },
                                Transform::from_scale(factor, factor).post_translate(
                                    icon_x.round(),
                                    (center_y - side / 2.0).round(),
                                ),
                                None,
                            );
                        }
                        Icon::Image(None) => {}
                        Icon::Glyph(glyph) => {
                            draw_icon(
                                image,
                                *glyph,
                                icon_x + s(ICON / 2.0),
                                center_y,
                                s(14.0),
                                0.9,
                            );
                        }
                    }
                    let label = fit_text(label, s(TEXT), s(w - PAD * 2.0 - TEXT_X - 12.0));
                    draw_text(
                        image,
                        &label,
                        s(PAD + TEXT_X),
                        center_y - s(TEXT * 0.62),
                        s(TEXT),
                        0.95,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str, command: u32) -> Entry {
        Entry::Item {
            icon: Icon::Glyph(GLYPH_PIN),
            label: label.into(),
            command,
        }
    }

    /// Como el menú de Chrome: una sección, un separador y las opciones de la app.
    fn chrome() -> AppMenu {
        AppMenu::new(vec![
            Entry::Header("Tareas".into()),
            item("Nueva ventana", CMD_JUMP),
            item("Nueva ventana de incógnito", CMD_JUMP + 1),
            Entry::Separator,
            item("Google Chrome", CMD_APP_OPEN),
            item("Desanclar de la barra de tareas", CMD_APP_UNPIN),
        ])
    }

    #[test]
    fn rows_stack_and_only_options_respond() {
        let menu = chrome();
        let (w, h) = menu.size();
        assert!((MIN_W..=MAX_W).contains(&w));
        assert_eq!(h, PAD * 2.0 + HEADER_H + ITEM_H * 4.0 + SEPARATOR_H);
        // El título y el separador no responden; cada opción, en su fila.
        assert_eq!(menu.hit_test(w / 2.0, PAD + HEADER_H / 2.0), None);
        let first = PAD + HEADER_H + ITEM_H / 2.0;
        assert_eq!(menu.hit_test(w / 2.0, first), Some(1));
        assert_eq!(menu.command(1), Some(CMD_JUMP));
        assert_eq!(menu.hit_test(w / 2.0, first + ITEM_H), Some(2));
        let separator = PAD + HEADER_H + ITEM_H * 2.0 + SEPARATOR_H / 2.0;
        assert_eq!(menu.hit_test(w / 2.0, separator), None);
        let unpin = PAD + HEADER_H + ITEM_H * 3.0 + SEPARATOR_H + ITEM_H / 2.0;
        assert_eq!(
            menu.hit_test(w / 2.0, unpin).and_then(|i| menu.command(i)),
            Some(CMD_APP_UNPIN)
        );
        // Fuera del panel, nada.
        assert_eq!(menu.hit_test(w / 2.0, h + 5.0), None);
        assert_eq!(menu.hit_test(-1.0, first), None);
    }

    #[test]
    fn arrows_skip_headers_and_separators() {
        let mut menu = chrome();
        menu.move_hover(true);
        assert_eq!(menu.hovered, Some(1));
        menu.move_hover(true);
        menu.move_hover(true);
        // Salta el separador: de "incógnito" a "Google Chrome".
        assert_eq!(menu.hovered, Some(4));
        menu.move_hover(true);
        menu.move_hover(true);
        assert_eq!(menu.hovered, Some(5));
        menu.move_hover(false);
        assert_eq!(menu.hovered, Some(4));
        // Sin resaltado, hacia arriba empieza por la última.
        menu.hovered = None;
        menu.move_hover(false);
        assert_eq!(menu.hovered, Some(5));
    }

    #[test]
    fn long_menus_grow_but_stay_within_bounds() {
        let long = "Universidad Privada Antenor Orrego | Carreras Universitarias | UPAO";
        let menu = AppMenu::new(vec![item(long, CMD_JUMP)]);
        assert_eq!(menu.size().0, MAX_W);
        let short = AppMenu::new(vec![item("Abrir", CMD_APP_OPEN)]);
        assert_eq!(short.size().0, MIN_W);
    }
}
