//! Barra superior al estilo de macOS: arriba, Inicio, el nombre de la app activa,
//! la bandeja, el idioma, el estado y la hora; abajo, el dock solo con las apps.
//! El centro queda libre: ahí puede ir una isla como Notch Isla. Las ventanas
//! "siempre encima" de otros programas que la tocan quedan por encima de ella (ver
//! `priority::raise_below_overlapping`), así se ven y se pueden pulsar.
use crate::bar::HoverTarget;
use crate::clock::{draw_icon_aligned, draw_text_on_baseline, line_metrics, text_width};
use crate::status::{Item, Status};
use tiny_skia::Pixmap;

/// Alto de la barra, en unidades lógicas.
pub const HEIGHT: f32 = 28.0;
/// Margen a los lados de la barra.
const PAD: f32 = 6.0;
/// Botón de Inicio, a la izquierda.
const LAUNCHER_W: f32 = 34.0;
const LAUNCHER_ICON: f32 = 14.0;
/// Hueco del centro que no se ocupa: el nombre de la app no llega hasta él.
const CENTER_FREE: f32 = 660.0;
const OVERFLOW_W: f32 = 28.0;
const STATUS_STEP: f32 = 26.0;
const STATUS_ICON: f32 = 14.0;
/// Margen dentro de las zonas de texto (idioma y hora).
const TEXT_PAD: f32 = 9.0;
/// Franja del extremo derecho que muestra el escritorio.
const DESKTOP_W: f32 = 10.0;
/// Alto y radio del resaltado al pasar el cursor.
const HOVER_H: f32 = 22.0;
const HOVER_R: f32 = 6.0;
const TEXT: f32 = 12.5;

/// Lo que muestra la barra. `width` es el ancho del monitor en unidades lógicas.
#[derive(Clone, PartialEq)]
pub struct TopBar {
    pub width: f32,
    pub status: Status,
    pub keyboard: Option<crate::keyboard::Layout>,
    /// Nombre de la app activa ("Google Chrome", "Escritorio").
    pub app: String,
    /// Día y hora tal como se muestran (ver `clock::day_and_time`).
    pub clock: String,
    pub hovered: Option<HoverTarget>,
    pub appearance: crate::style::Appearance,
}

impl TopBar {
    pub fn new() -> Self {
        Self {
            width: 0.0,
            status: Status::default(),
            keyboard: None,
            app: String::new(),
            clock: crate::clock::day_and_time(),
            hovered: None,
            appearance: Default::default(),
        }
    }

    /// Zonas que se pueden pulsar, de izquierda a derecha, con sus bordes.
    pub fn zones(&self) -> Vec<(HoverTarget, f32, f32)> {
        let mut zones = vec![(HoverTarget::Launcher, PAD, PAD + LAUNCHER_W)];
        // De derecha a izquierda desde el borde.
        let mut right = self.width;
        let mut tail = Vec::new();
        let mut push = |target, width: f32, right: &mut f32| {
            tail.push((target, *right - width, *right));
            *right -= width;
        };
        push(HoverTarget::ShowDesktop, DESKTOP_W, &mut right);
        let clock = text_width(&self.clock, TEXT) + TEXT_PAD * 2.0;
        push(HoverTarget::Clock, clock, &mut right);
        // Centro de control: el idioma y los iconos de estado son un solo botón, que
        // abre el panel rápido (y desde él, el de idioma).
        let status = self.language_width() + self.status.items().len() as f32 * STATUS_STEP;
        push(HoverTarget::Status(0), status, &mut right);
        push(HoverTarget::Overflow, OVERFLOW_W, &mut right);
        tail.reverse();
        zones.extend(tail);
        zones
    }

    /// Lo que ocupa el idioma al principio del centro de control: su texto con
    /// margen a los dos lados, para que no quede pegado al icono de red.
    fn language_width(&self) -> f32 {
        self.keyboard
            .as_ref()
            .map_or(0.0, |k| text_width(&k.language, TEXT) + TEXT_PAD * 2.0)
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<HoverTarget> {
        if !(0.0..HEIGHT).contains(&y) {
            return None;
        }
        self.zones()
            .into_iter()
            .find(|(_, left, right)| (*left..*right).contains(&x))
            .map(|(target, _, _)| target)
    }

    /// Centro horizontal de `target`, en unidades lógicas: los paneles se abren bajo él.
    pub fn center(&self, target: HoverTarget) -> Option<f32> {
        let target = match target {
            HoverTarget::Status(_) | HoverTarget::Language => HoverTarget::Status(0),
            t => t,
        };
        self.zones()
            .into_iter()
            .find(|(t, _, _)| *t == target)
            .map(|(_, left, right)| (left + right) / 2.0)
    }

    pub fn draw(&self, pixmap: &mut Pixmap, scale: f32) {
        use crate::style::Appearance;
        let s = |v: f32| v * scale;
        pixmap.fill(tiny_skia::Color::TRANSPARENT);
        let (w, h) = (pixmap.width() as f32, pixmap.height() as f32);
        // Sin desenfoque detrás: las apariencias que lo usan quedan como el cristal normal.
        let (fill, line) = match self.appearance {
            Appearance::Opaque => ([32, 32, 35, 255], [255, 255, 255, 24]),
            Appearance::Clear => ([0, 0, 0, 1], [0, 0, 0, 0]),
            _ => ([28, 30, 35, 178], [255, 255, 255, 22]),
        };
        crate::apps::fill_rounded(pixmap, fill, 0.0, 0.0, w, h, 0.0);
        if line[3] > 0 {
            crate::apps::fill_rounded(pixmap, line, 0.0, h - scale, w, scale, 0.0);
        }
        let cy = s(HEIGHT / 2.0);
        if let Some(hovered) = self.hovered {
            if let Some((_, left, right)) = self
                .zones()
                .into_iter()
                .find(|(t, _, _)| Some(*t) == Some(hovered))
            {
                crate::apps::fill_rounded(
                    pixmap,
                    [255, 255, 255, 30],
                    s(left + 1.0),
                    cy - s(HOVER_H / 2.0),
                    s(right - left - 2.0),
                    s(HOVER_H),
                    s(HOVER_R),
                );
            }
        }
        let size = s(TEXT);
        let (ascent, descent) = line_metrics(size);
        let baseline = cy + (ascent - descent) / 2.0;
        for (target, left, right) in self.zones() {
            let cx = s((left + right) / 2.0);
            match target {
                HoverTarget::Launcher => {
                    crate::apps::draw_windows_start_icon(pixmap, cx, cy, LAUNCHER_ICON, scale);
                }
                HoverTarget::Overflow => crate::bar::draw_chevron(pixmap, cx, cy, scale, true),
                HoverTarget::Status(_) => {
                    if let Some(keyboard) = &self.keyboard {
                        let x = s(left + TEXT_PAD);
                        draw_text_on_baseline(pixmap, &keyboard.language, x, baseline, size, 0.95);
                    }
                    let icons = left + self.language_width();
                    for (index, item) in self.status.items().into_iter().enumerate() {
                        let icon_x = s(icons + (index as f32 + 0.5) * STATUS_STEP);
                        if let (Item::Battery, Some(battery)) = (item, self.status.battery) {
                            crate::status::draw_battery(
                                pixmap,
                                battery,
                                icon_x,
                                cy,
                                s(STATUS_ICON),
                                0.95,
                            );
                            continue;
                        }
                        let (glyph, opacity) = self.status.glyph(item);
                        draw_icon_aligned(
                            pixmap,
                            glyph,
                            icon_x,
                            cy,
                            s(STATUS_ICON),
                            opacity * 0.95,
                        );
                    }
                }
                HoverTarget::Clock => {
                    let x = s(left + TEXT_PAD);
                    draw_text_on_baseline(pixmap, &self.clock, x, baseline, size, 0.95);
                }
                _ => {}
            }
        }
        // El nombre de la app activa, junto a Inicio y sin llegar al centro.
        let name_left = PAD + LAUNCHER_W + 4.0;
        let room = (self.width - CENTER_FREE) / 2.0 - name_left;
        if room > 40.0 && !self.app.is_empty() {
            let name = crate::clock::fit_text(&self.app, size, s(room));
            crate::clock::draw_text_semibold(
                pixmap,
                &name,
                s(name_left),
                cy - (ascent + descent) / 2.0,
                size,
                1.0,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar() -> TopBar {
        TopBar {
            width: 1920.0,
            clock: "jue 13 nov  22:09".into(),
            ..TopBar::new()
        }
    }

    #[test]
    fn launcher_on_the_left_and_the_rest_on_the_right() {
        let bar = bar();
        let y = HEIGHT / 2.0;
        assert_eq!(bar.hit_test(PAD + 5.0, y), Some(HoverTarget::Launcher));
        assert_eq!(bar.hit_test(1919.0, y), Some(HoverTarget::ShowDesktop));
        assert_eq!(bar.hit_test(960.0, y), None);
        let zones = bar.zones();
        let order: Vec<_> = zones.iter().map(|(t, _, _)| *t).collect();
        assert_eq!(
            order,
            [
                HoverTarget::Launcher,
                HoverTarget::Overflow,
                HoverTarget::Status(0),
                HoverTarget::Clock,
                HoverTarget::ShowDesktop
            ]
        );
        // Contiguas y hasta el borde derecho.
        for pair in zones[1..].windows(2) {
            assert_eq!(pair[0].2, pair[1].1);
        }
        assert_eq!(zones.last().unwrap().2, 1920.0);
    }

    #[test]
    fn every_status_icon_opens_the_same_panel() {
        let bar = bar();
        let center = bar.center(HoverTarget::Status(0)).unwrap();
        assert_eq!(bar.center(HoverTarget::Status(2)), Some(center));
        assert_eq!(bar.hit_test(center, 1.0), Some(HoverTarget::Status(0)));
        assert_eq!(bar.hit_test(center, HEIGHT + 1.0), None);
    }

    #[test]
    fn the_language_is_part_of_the_control_center() {
        let mut bar = bar();
        let without = bar.zones();
        bar.keyboard = Some(crate::keyboard::Layout {
            id: 1,
            language: "ESP".into(),
            variant: None,
            name: "Español".into(),
        });
        let with = bar.zones();
        assert_eq!(with.len(), without.len());
        assert!(!with.iter().any(|(t, _, _)| *t == HoverTarget::Language));
        let (_, left, _) = with[2];
        assert_eq!(bar.hit_test(left + 2.0, 10.0), Some(HoverTarget::Status(0)));
        assert_eq!(
            bar.center(HoverTarget::Language),
            bar.center(HoverTarget::Status(0))
        );
        // El idioma deja un margen antes del primer icono.
        assert!(bar.language_width() >= text_width("ESP", TEXT) + TEXT_PAD * 2.0);
    }

    #[test]
    fn the_right_side_stays_out_of_the_center() {
        let bar = bar();
        let first_right = bar.zones()[1].1;
        assert!(first_right > (1920.0 + CENTER_FREE) / 2.0);
    }
}
