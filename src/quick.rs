//! Panel rápido propio, que se abre sobre los iconos de red, volumen y batería:
//! Wi-Fi y Bluetooth, brillo y volumen, con el estilo del lanzador. "Más opciones"
//! abre el panel de Windows para lo que este aún no tiene.
use crate::apps::{fill_rounded, stroke_rounded};
use crate::clock::{
    draw_icon_aligned, draw_text, draw_text_on_baseline, draw_text_semibold, fit_text,
    line_metrics, text_width,
};
use crate::status::{draw_battery, volume_glyph, Battery, Volume};
use crate::system::{Radio, Snapshot};
use tiny_skia::Pixmap;

pub const WIDTH: f32 = 340.0;
pub const HEIGHT: f32 = 244.0;
const PAD: f32 = 16.0;
const TILE_Y: f32 = 16.0;
const TILE_H: f32 = 60.0;
const TILE_GAP: f32 = 12.0;
const TILE_W: f32 = (WIDTH - PAD * 2.0 - TILE_GAP) / 2.0;
/// Centro vertical de las filas de brillo y volumen.
const BRIGHTNESS_Y: f32 = 112.0;
const VOLUME_Y: f32 = 160.0;
const ROW_H: f32 = 40.0;
const SLIDER_X: f32 = PAD + 40.0;
const SLIDER_W: f32 = WIDTH - PAD - 44.0 - SLIDER_X;
const FOOTER_Y: f32 = 196.0;
const MORE_W: f32 = 112.0;
const BACKGROUND: [u8; 4] = [32, 32, 34, 246];
/// Azul de los controles activos, como el color de énfasis por defecto de Windows.
const ACCENT: [u8; 4] = [76, 160, 255, 255];
const TILE_ON: [u8; 4] = [0, 95, 184, 255];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slider {
    Brightness,
    Volume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Tile(Radio),
    /// Icono del altavoz: silencia o quita el silencio.
    Mute,
    Slider(Slider),
    More,
}

#[derive(Default)]
pub struct QuickState {
    pub system: Snapshot,
    pub volume: Option<Volume>,
    pub battery: Option<Battery>,
    pub hovered: Option<Hit>,
    /// Barra que se está arrastrando.
    pub dragging: Option<Slider>,
}

fn tile_x(radio: Radio) -> f32 {
    match radio {
        Radio::Wifi => PAD,
        Radio::Bluetooth => PAD + TILE_W + TILE_GAP,
    }
}
fn more_rect() -> (f32, f32, f32, f32) {
    (WIDTH - PAD - MORE_W, FOOTER_Y + 8.0, MORE_W, 32.0)
}
fn row_y(slider: Slider) -> f32 {
    match slider {
        Slider::Brightness => BRIGHTNESS_Y,
        Slider::Volume => VOLUME_Y,
    }
}

impl QuickState {
    /// Valor actual de una barra, de 0 a 1; `None` si no está disponible.
    pub fn value(&self, slider: Slider) -> Option<f32> {
        match slider {
            Slider::Brightness => self.system.brightness.map(|b| b as f32 / 100.0),
            Slider::Volume => self.volume.map(|v| v.level),
        }
    }
    fn radio(&self, radio: Radio) -> Option<bool> {
        match radio {
            Radio::Wifi => self.system.wifi,
            Radio::Bluetooth => self.system.bluetooth,
        }
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<Hit> {
        let within =
            |rx: f32, ry: f32, w: f32, h: f32| x >= rx && x < rx + w && y >= ry && y < ry + h;
        for radio in [Radio::Wifi, Radio::Bluetooth] {
            if self.radio(radio).is_some() && within(tile_x(radio), TILE_Y, TILE_W, TILE_H) {
                return Some(Hit::Tile(radio));
            }
        }
        for slider in [Slider::Brightness, Slider::Volume] {
            let top = row_y(slider) - ROW_H / 2.0;
            if self.value(slider).is_none() {
                continue;
            }
            if slider == Slider::Volume && within(PAD, top, 32.0, ROW_H) {
                return Some(Hit::Mute);
            }
            // Un margen a los lados para poder llevarla hasta 0 y 100 sin afinar.
            if within(SLIDER_X - 10.0, top, SLIDER_W + 20.0, ROW_H) {
                return Some(Hit::Slider(slider));
            }
        }
        let (mx, my, mw, mh) = more_rect();
        within(mx, my, mw, mh).then_some(Hit::More)
    }

    /// Valor de una barra para la posición `x` del cursor.
    pub fn slider_value(x: f32) -> f32 {
        ((x - SLIDER_X) / SLIDER_W).clamp(0.0, 1.0)
    }

    pub fn draw(&self, image: &mut Pixmap, scale: f32) {
        image.fill(tiny_skia::Color::TRANSPARENT);
        let (w, h) = ((WIDTH - 2.0) * scale, (HEIGHT - 2.0) * scale);
        fill_rounded(image, BACKGROUND, scale, scale, w, h, 12.0 * scale);
        // El pie, algo más oscuro y separado por una línea, como en Windows: el mismo
        // contorno redondeado del panel, recortado desde la línea hacia abajo.
        if let Some(mut footer) = Pixmap::new(image.width(), image.height()) {
            fill_rounded(&mut footer, [0, 0, 0, 40], scale, scale, w, h, 12.0 * scale);
            let cut = ((FOOTER_Y * scale).round() as usize).min(footer.height() as usize);
            let row = footer.width() as usize * 4;
            footer.data_mut()[..cut * row].fill(0);
            image.draw_pixmap(
                0,
                0,
                footer.as_ref(),
                &tiny_skia::PixmapPaint::default(),
                tiny_skia::Transform::identity(),
                None,
            );
        }
        fill_rounded(
            image,
            [255, 255, 255, 20],
            scale,
            (FOOTER_Y * scale).round(),
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
        self.draw_tile(image, Radio::Wifi, scale);
        self.draw_tile(image, Radio::Bluetooth, scale);
        self.draw_slider(image, Slider::Brightness, scale);
        self.draw_slider(image, Slider::Volume, scale);
        self.draw_footer(image, scale);
    }

    fn draw_tile(&self, image: &mut Pixmap, radio: Radio, scale: f32) {
        let (x, y) = (tile_x(radio), TILE_Y);
        let state = self.radio(radio);
        let on = state == Some(true);
        let hovered = self.hovered == Some(Hit::Tile(radio));
        let fill = match (on, hovered) {
            (true, false) => TILE_ON,
            (true, true) => [24, 112, 200, 255],
            (false, false) => [255, 255, 255, 14],
            (false, true) => [255, 255, 255, 24],
        };
        fill_rounded(
            image,
            fill,
            x * scale,
            y * scale,
            TILE_W * scale,
            TILE_H * scale,
            6.0 * scale,
        );
        if !on {
            stroke_rounded(
                image,
                [255, 255, 255, 24],
                x * scale,
                y * scale,
                TILE_W * scale,
                TILE_H * scale,
                6.0 * scale,
                scale * 0.75,
            );
        }
        let (glyph, title) = match radio {
            Radio::Wifi => ('\u{E701}', "Wi-Fi"),
            Radio::Bluetooth => ('\u{E702}', "Bluetooth"),
        };
        let subtitle = match (radio, state) {
            (_, None) => "No disponible".to_string(),
            (_, Some(false)) => "Desactivado".to_string(),
            (Radio::Wifi, Some(true)) => self
                .system
                .network
                .clone()
                .unwrap_or_else(|| "Sin conectar".into()),
            (Radio::Bluetooth, Some(true)) => "Activado".into(),
        };
        let faded = if state.is_none() { 0.45 } else { 1.0 };
        draw_icon_aligned(
            image,
            glyph,
            (x + 22.0) * scale,
            (y + TILE_H / 2.0) * scale,
            16.0 * scale,
            faded,
        );
        draw_text_semibold(
            image,
            title,
            (x + 42.0) * scale,
            (y + 13.0) * scale,
            13.0 * scale,
            faded,
        );
        let room = (TILE_W - 50.0) * scale;
        draw_text(
            image,
            &fit_text(&subtitle, 12.0 * scale, room),
            (x + 42.0) * scale,
            (y + 33.0) * scale,
            12.0 * scale,
            0.75 * faded,
        );
    }

    fn draw_slider(&self, image: &mut Pixmap, slider: Slider, scale: f32) {
        let cy = row_y(slider);
        let value = self.value(slider);
        let enabled = value.is_some();
        let glyph = match (slider, self.volume) {
            (Slider::Brightness, _) => '\u{E706}',
            (Slider::Volume, Some(v)) if v.muted => '\u{E74F}',
            (Slider::Volume, Some(v)) => volume_glyph(v.level),
            (Slider::Volume, None) => '\u{E74F}',
        };
        if slider == Slider::Volume && self.hovered == Some(Hit::Mute) {
            fill_rounded(
                image,
                [255, 255, 255, 20],
                (PAD + 2.0) * scale,
                (cy - 14.0) * scale,
                28.0 * scale,
                28.0 * scale,
                5.0 * scale,
            );
        }
        draw_icon_aligned(
            image,
            glyph,
            (PAD + 16.0) * scale,
            cy * scale,
            16.0 * scale,
            if enabled { 0.95 } else { 0.4 },
        );
        // Pista, parte llena y tirador: un círculo oscuro con un punto de color que
        // crece al pasar el cursor o arrastrar, como los de Windows 11.
        fill_rounded(
            image,
            [255, 255, 255, 60],
            SLIDER_X * scale,
            (cy - 2.0) * scale,
            SLIDER_W * scale,
            4.0 * scale,
            2.0 * scale,
        );
        let Some(value) = value else {
            draw_text(
                image,
                "No disponible",
                (SLIDER_X + SLIDER_W / 2.0) * scale
                    - text_width("No disponible", 12.0 * scale) / 2.0,
                (cy - 22.0) * scale,
                12.0 * scale,
                0.5,
            );
            return;
        };
        let muted = slider == Slider::Volume && self.volume.is_some_and(|v| v.muted);
        let color = if muted { [150, 150, 155, 255] } else { ACCENT };
        let thumb = SLIDER_X + SLIDER_W * value;
        fill_rounded(
            image,
            color,
            SLIDER_X * scale,
            (cy - 2.0) * scale,
            (thumb - SLIDER_X) * scale,
            4.0 * scale,
            2.0 * scale,
        );
        fill_rounded(
            image,
            [69, 69, 72, 255],
            (thumb - 10.0) * scale,
            (cy - 10.0) * scale,
            20.0 * scale,
            20.0 * scale,
            10.0 * scale,
        );
        let active = self.dragging == Some(slider) || self.hovered == Some(Hit::Slider(slider));
        let dot = if active { 7.0 } else { 5.5 };
        fill_rounded(
            image,
            color,
            (thumb - dot) * scale,
            (cy - dot) * scale,
            dot * 2.0 * scale,
            dot * 2.0 * scale,
            dot * scale,
        );
        let percent = format!("{}", (value * 100.0).round() as u32);
        draw_text(
            image,
            &percent,
            (WIDTH - PAD) * scale - text_width(&percent, 13.0 * scale),
            (cy - 9.0) * scale,
            13.0 * scale,
            0.9,
        );
    }

    fn draw_footer(&self, image: &mut Pixmap, scale: f32) {
        let cy = FOOTER_Y + (HEIGHT - FOOTER_Y) / 2.0;
        // Los textos del pie se apoyan en una línea base centrada en la fila, para que
        // queden a la altura de los iconos sin importar las letras que lleven.
        let size = 13.0 * scale;
        let (ascent, descent) = line_metrics(size);
        let baseline = cy * scale + (ascent - descent) / 2.0;
        let (x, y, w, h) = more_rect();
        if let Some(battery) = self.battery {
            draw_battery(
                image,
                battery,
                (PAD + 12.0) * scale,
                cy * scale,
                16.0 * scale,
                0.9,
            );
            // Lo que no quepa antes del botón se acorta con "…".
            let left = PAD + 28.0;
            let text = format!("{} % · {}", battery.percent, battery.state());
            draw_text_on_baseline(
                image,
                &fit_text(&text, size, (x - 8.0 - left) * scale),
                left * scale,
                baseline,
                size,
                0.9,
            );
        }
        if self.hovered == Some(Hit::More) {
            fill_rounded(
                image,
                [255, 255, 255, 20],
                x * scale,
                y * scale,
                w * scale,
                h * scale,
                5.0 * scale,
            );
        }
        let label = "Más opciones";
        let text_w = text_width(label, size);
        // Texto y flecha "›" centrados en el botón y a la misma altura.
        let left = (x + w / 2.0) * scale - (text_w + 14.0 * scale) / 2.0;
        draw_text_on_baseline(image, label, left, baseline, size, 0.9);
        draw_icon_aligned(
            image,
            '\u{E76C}',
            left + text_w + 9.0 * scale,
            cy * scale,
            10.0 * scale,
            0.8,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> QuickState {
        QuickState {
            system: Snapshot {
                wifi: Some(true),
                bluetooth: Some(false),
                brightness: Some(40),
                network: Some("Casa".into()),
            },
            volume: Some(Volume {
                level: 0.5,
                muted: false,
            }),
            battery: None,
            hovered: None,
            dragging: None,
        }
    }

    #[test]
    fn controls_are_found_where_they_are_drawn() {
        let quick = state();
        let tile_center = |radio| (tile_x(radio) + TILE_W / 2.0, TILE_Y + TILE_H / 2.0);
        let (x, y) = tile_center(Radio::Wifi);
        assert_eq!(quick.hit_test(x, y), Some(Hit::Tile(Radio::Wifi)));
        let (x, y) = tile_center(Radio::Bluetooth);
        assert_eq!(quick.hit_test(x, y), Some(Hit::Tile(Radio::Bluetooth)));
        let middle = SLIDER_X + SLIDER_W / 2.0;
        assert_eq!(
            quick.hit_test(middle, BRIGHTNESS_Y),
            Some(Hit::Slider(Slider::Brightness))
        );
        assert_eq!(
            quick.hit_test(middle, VOLUME_Y),
            Some(Hit::Slider(Slider::Volume))
        );
        assert_eq!(quick.hit_test(PAD + 16.0, VOLUME_Y), Some(Hit::Mute));
        let (mx, my, mw, mh) = more_rect();
        assert_eq!(
            quick.hit_test(mx + mw / 2.0, my + mh / 2.0),
            Some(Hit::More)
        );
        assert_eq!(quick.hit_test(PAD + 4.0, FOOTER_Y + 24.0), None);
        // Todo cabe en el panel y no se solapa.
        assert!(TILE_Y + TILE_H < BRIGHTNESS_Y - ROW_H / 2.0);
        assert!(BRIGHTNESS_Y + ROW_H / 2.0 <= VOLUME_Y - ROW_H / 2.0);
        assert!(VOLUME_Y + ROW_H / 2.0 <= FOOTER_Y);
        assert!(my + mh < HEIGHT);
    }

    #[test]
    fn unavailable_controls_cannot_be_used() {
        let mut quick = state();
        quick.system.brightness = None;
        quick.system.bluetooth = None;
        quick.volume = None;
        let middle = SLIDER_X + SLIDER_W / 2.0;
        assert_eq!(quick.hit_test(middle, BRIGHTNESS_Y), None);
        assert_eq!(quick.hit_test(middle, VOLUME_Y), None);
        assert_eq!(quick.hit_test(PAD + 16.0, VOLUME_Y), None);
        let (x, y) = (tile_x(Radio::Bluetooth) + 10.0, TILE_Y + 10.0);
        assert_eq!(quick.hit_test(x, y), None);
    }

    #[test]
    fn slider_value_follows_the_cursor_and_stays_in_range() {
        assert_eq!(QuickState::slider_value(SLIDER_X), 0.0);
        assert_eq!(QuickState::slider_value(SLIDER_X + SLIDER_W), 1.0);
        assert_eq!(QuickState::slider_value(SLIDER_X + SLIDER_W / 4.0), 0.25);
        assert_eq!(QuickState::slider_value(-50.0), 0.0);
        assert_eq!(QuickState::slider_value(WIDTH + 50.0), 1.0);
    }
}
