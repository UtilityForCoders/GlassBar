//! Panel rápido propio, que se abre sobre los iconos de red, volumen y batería:
//! Wi-Fi y Bluetooth, brillo y volumen, con el estilo del lanzador. La flecha del
//! botón de Wi-Fi abre la lista de redes. "Más opciones" abre el panel de Windows
//! para lo que este aún no tiene.
use crate::apps::{fill_rounded, stroke_rounded};
use crate::clock::{
    draw_icon_aligned, draw_text, draw_text_on_baseline, draw_text_semibold, fit_text,
    line_metrics, text_width,
};
use crate::status::{draw_battery, volume_glyph, Battery, Volume};
use crate::system::{Radio, Snapshot};
use crate::wifi::Network;
use std::time::{Duration, Instant};
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
/// Zona de la flecha "›" a la derecha del botón de Wi-Fi.
const ARROW_W: f32 = 36.0;
/// Vista de redes Wi-Fi: más alta, con cabecera, lista desplazable y pie.
pub const WIFI_HEIGHT: f32 = 392.0;
const LIST_Y: f32 = 56.0;
const NET_ROW: f32 = 52.0;
const WIFI_FOOTER_Y: f32 = WIFI_HEIGHT - 52.0;
/// Si Windows no confirma la conexión en este tiempo, se deja de mostrar "Conectando…".
const PENDING_FOR: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Main,
    Wifi,
}

/// Conexión o desconexión pedida y aún sin confirmar.
#[derive(Clone, Debug)]
pub struct Pending {
    pub ssid: String,
    /// `true` al conectar, `false` al desconectar.
    pub connect: bool,
    pub since: Instant,
}

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
    /// Flecha del botón de Wi-Fi: abre la lista de redes.
    WifiList,
    /// Volver de la lista de redes a la vista principal.
    Back,
    /// Red de la lista, por su posición.
    Network(usize),
    /// "Más configuración de Wi-Fi", en la configuración de Windows.
    WifiSettings,
}

#[derive(Default)]
pub struct QuickState {
    pub system: Snapshot,
    pub volume: Option<Volume>,
    pub battery: Option<Battery>,
    pub hovered: Option<Hit>,
    /// Barra que se está arrastrando.
    pub dragging: Option<Slider>,
    pub view: View,
    /// Desplazamiento de la lista de redes.
    pub list_scroll: f32,
    pub pending: Option<Pending>,
}

fn tile_x(radio: Radio) -> f32 {
    match radio {
        Radio::Wifi => PAD,
        Radio::Bluetooth => PAD + TILE_W + TILE_GAP,
    }
}
fn back_rect() -> (f32, f32, f32, f32) {
    (PAD - 6.0, 12.0, 32.0, 32.0)
}
fn wifi_settings_rect() -> (f32, f32, f32, f32) {
    (PAD - 6.0, WIFI_FOOTER_Y + 10.0, 220.0, 32.0)
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
    /// Alto del panel en la vista actual.
    pub fn height(&self) -> f32 {
        match self.view {
            View::Main => HEIGHT,
            View::Wifi => WIFI_HEIGHT,
        }
    }
    pub fn networks(&self) -> &[Network] {
        self.system.networks.as_deref().unwrap_or_default()
    }
    fn list_height() -> f32 {
        WIFI_FOOTER_Y - LIST_Y
    }
    fn max_list_scroll(&self) -> f32 {
        (self.networks().len() as f32 * NET_ROW - Self::list_height()).max(0.0)
    }
    /// Desplaza la lista de redes (positivo: hacia abajo).
    pub fn scroll_list(&mut self, delta: f32) {
        self.list_scroll = (self.list_scroll + delta).clamp(0.0, self.max_list_scroll());
    }
    /// Tras leer el sistema: se olvida la conexión pendiente si Windows ya la confirma
    /// (o tardó demasiado), y la lista no queda desplazada más allá de su final.
    pub fn settle(&mut self, now: Instant) {
        if let Some(pending) = &self.pending {
            let done = self
                .networks()
                .iter()
                .any(|n| n.ssid == pending.ssid && n.connected == pending.connect);
            if done || now.duration_since(pending.since) > PENDING_FOR {
                self.pending = None;
            }
        }
        self.list_scroll = self.list_scroll.clamp(0.0, self.max_list_scroll());
    }

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
        if self.view == View::Wifi {
            let (bx, by, bw, bh) = back_rect();
            if within(bx, by, bw, bh) {
                return Some(Hit::Back);
            }
            let (sx, sy, sw, sh) = wifi_settings_rect();
            if within(sx, sy, sw, sh) {
                return Some(Hit::WifiSettings);
            }
            if self.system.wifi != Some(true) || !(LIST_Y..WIFI_FOOTER_Y).contains(&y) {
                return None;
            }
            let index = ((y - LIST_Y + self.list_scroll) / NET_ROW).floor() as usize;
            return (index < self.networks().len() && (PAD - 8.0..WIDTH - PAD + 8.0).contains(&x))
                .then_some(Hit::Network(index));
        }
        if self.system.networks.is_some()
            && within(
                tile_x(Radio::Wifi) + TILE_W - ARROW_W,
                TILE_Y,
                ARROW_W,
                TILE_H,
            )
        {
            return Some(Hit::WifiList);
        }
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
        let footer_y = match self.view {
            View::Main => FOOTER_Y,
            View::Wifi => WIFI_FOOTER_Y,
        };
        self.draw_chrome(image, footer_y, scale);
        if self.view == View::Wifi {
            self.draw_wifi(image, scale);
            return;
        }
        self.draw_tile(image, Radio::Wifi, scale);
        self.draw_tile(image, Radio::Bluetooth, scale);
        self.draw_slider(image, Slider::Brightness, scale);
        self.draw_slider(image, Slider::Volume, scale);
        self.draw_footer(image, scale);
    }

    /// Fondo, borde y pie (más oscuro desde `footer_y`), comunes a las dos vistas.
    fn draw_chrome(&self, image: &mut Pixmap, footer_y: f32, scale: f32) {
        image.fill(tiny_skia::Color::TRANSPARENT);
        let (w, h) = ((WIDTH - 2.0) * scale, (self.height() - 2.0) * scale);
        fill_rounded(image, BACKGROUND, scale, scale, w, h, 12.0 * scale);
        // El pie, algo más oscuro y separado por una línea, como en Windows: el mismo
        // contorno redondeado del panel, recortado desde la línea hacia abajo.
        if let Some(mut footer) = Pixmap::new(image.width(), image.height()) {
            fill_rounded(&mut footer, [0, 0, 0, 40], scale, scale, w, h, 12.0 * scale);
            let cut = ((footer_y * scale).round() as usize).min(footer.height() as usize);
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
            (footer_y * scale).round(),
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
        // El botón de Wi-Fi se divide, como en Windows: la flecha abre la lista.
        let arrow = radio == Radio::Wifi && self.system.networks.is_some();
        if arrow {
            let ax = x + TILE_W - ARROW_W;
            if self.hovered == Some(Hit::WifiList) {
                fill_rounded(
                    image,
                    [255, 255, 255, 24],
                    ax * scale,
                    y * scale,
                    ARROW_W * scale,
                    TILE_H * scale,
                    6.0 * scale,
                );
            }
            fill_rounded(
                image,
                [255, 255, 255, if on { 60 } else { 24 }],
                ax * scale,
                (y + 12.0) * scale,
                scale.max(1.0),
                (TILE_H - 24.0) * scale,
                0.0,
            );
            draw_icon_aligned(
                image,
                '\u{E76C}',
                (ax + ARROW_W / 2.0) * scale,
                (y + TILE_H / 2.0) * scale,
                12.0 * scale,
                0.9,
            );
        }
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
        let room = (TILE_W - 50.0 - if arrow { ARROW_W } else { 0.0 }) * scale;
        draw_text(
            image,
            &fit_text(&subtitle, 12.0 * scale, room),
            (x + 42.0) * scale,
            (y + 33.0) * scale,
            12.0 * scale,
            0.75 * faded,
        );
    }

    /// Vista de redes: cabecera con "volver", lista desplazable y pie.
    fn draw_wifi(&self, image: &mut Pixmap, scale: f32) {
        let (bx, by, bw, bh) = back_rect();
        if self.hovered == Some(Hit::Back) {
            fill_rounded(
                image,
                [255, 255, 255, 20],
                bx * scale,
                by * scale,
                bw * scale,
                bh * scale,
                5.0 * scale,
            );
        }
        draw_icon_aligned(
            image,
            '\u{E72B}',
            (bx + bw / 2.0) * scale,
            (by + bh / 2.0) * scale,
            13.0 * scale,
            0.9,
        );
        draw_text_semibold(
            image,
            "Wi-Fi",
            (PAD + 34.0) * scale,
            (by + 7.0) * scale,
            15.0 * scale,
            1.0,
        );

        let message = match (self.system.wifi, self.networks().is_empty()) {
            (Some(false), _) => Some("El Wi-Fi está desactivado"),
            (None, _) => Some("No hay adaptador Wi-Fi"),
            (Some(true), true) => Some("Buscando redes…"),
            _ => None,
        };
        if let Some(message) = message {
            let size = 13.0 * scale;
            draw_text(
                image,
                message,
                WIDTH / 2.0 * scale - text_width(message, size) / 2.0,
                (LIST_Y + 40.0) * scale,
                size,
                0.6,
            );
        } else if let Some(mut list) = Pixmap::new(
            (WIDTH * scale).ceil() as u32,
            (Self::list_height() * scale).ceil() as u32,
        ) {
            // Las filas van en una capa del alto de la lista: lo que se sale al
            // desplazarla queda recortado.
            for (index, network) in self.networks().iter().enumerate() {
                let top = index as f32 * NET_ROW - self.list_scroll;
                if top + NET_ROW < 0.0 || top > Self::list_height() {
                    continue;
                }
                self.draw_network(&mut list, index, network, top, scale);
            }
            image.draw_pixmap(
                0,
                (LIST_Y * scale).round() as i32,
                list.as_ref(),
                &tiny_skia::PixmapPaint::default(),
                tiny_skia::Transform::identity(),
                None,
            );
        }

        let (sx, sy, sw, sh) = wifi_settings_rect();
        if self.hovered == Some(Hit::WifiSettings) {
            fill_rounded(
                image,
                [255, 255, 255, 20],
                sx * scale,
                sy * scale,
                sw * scale,
                sh * scale,
                5.0 * scale,
            );
        }
        let size = 13.0 * scale;
        let (ascent, descent) = line_metrics(size);
        draw_text_on_baseline(
            image,
            "Más configuración de Wi-Fi",
            (sx + 10.0) * scale,
            (sy + sh / 2.0) * scale + (ascent - descent) / 2.0,
            size,
            0.9,
        );
    }

    fn draw_network(
        &self,
        image: &mut Pixmap,
        index: usize,
        network: &Network,
        top: f32,
        scale: f32,
    ) {
        let hovered = self.hovered == Some(Hit::Network(index));
        if hovered || network.connected {
            fill_rounded(
                image,
                [255, 255, 255, if hovered { 20 } else { 10 }],
                (PAD - 6.0) * scale,
                (top + 3.0) * scale,
                (WIDTH - PAD * 2.0 + 12.0) * scale,
                (NET_ROW - 6.0) * scale,
                6.0 * scale,
            );
        }
        let cy = top + NET_ROW / 2.0;
        draw_signal(
            image,
            (PAD + 14.0) * scale,
            cy * scale,
            18.0 * scale,
            network.signal,
        );
        if network.secured {
            // Candado pequeño en la esquina del icono, como en Windows.
            draw_icon_aligned(
                image,
                '\u{E72E}',
                (PAD + 23.0) * scale,
                (cy + 6.0) * scale,
                9.0 * scale,
                0.95,
            );
        }
        let action = match (&self.pending, network) {
            (Some(p), _) if p.ssid == network.ssid => None,
            (_, n) if n.connected => Some("Desconectar"),
            (_, n) if n.profile.is_some() => Some("Conectar"),
            _ => Some("Configurar"),
        };
        let action = action.filter(|_| hovered);
        let action_w = action.map_or(0.0, |a| text_width(a, 12.0 * scale) / scale + 20.0);
        let left = PAD + 40.0;
        let room = (WIDTH - PAD - left - action_w - 4.0) * scale;
        let name = fit_text(&network.ssid, 13.0 * scale, room);
        if network.connected {
            draw_text_semibold(
                image,
                &name,
                left * scale,
                (top + 9.0) * scale,
                13.0 * scale,
                1.0,
            );
        } else {
            draw_text(
                image,
                &name,
                left * scale,
                (top + 9.0) * scale,
                13.0 * scale,
                1.0,
            );
        }
        draw_text(
            image,
            &fit_text(&self.network_state(network), 12.0 * scale, room),
            left * scale,
            (top + 28.0) * scale,
            12.0 * scale,
            0.7,
        );
        if let Some(action) = action {
            let size = 12.0 * scale;
            let width = text_width(action, size) + 20.0 * scale;
            let x = (WIDTH - PAD) * scale - width;
            fill_rounded(
                image,
                [255, 255, 255, 24],
                x,
                (cy - 13.0) * scale,
                width,
                26.0 * scale,
                5.0 * scale,
            );
            let (ascent, descent) = line_metrics(size);
            draw_text_on_baseline(
                image,
                action,
                x + 10.0 * scale,
                cy * scale + (ascent - descent) / 2.0,
                size,
                0.95,
            );
        }
    }

    /// Segunda línea de cada red: "Conectado, protegida", "Abierta · Guardada"…
    fn network_state(&self, network: &Network) -> String {
        if let Some(pending) = self.pending.as_ref().filter(|p| p.ssid == network.ssid) {
            return if pending.connect {
                "Conectando…".into()
            } else {
                "Desconectando…".into()
            };
        }
        let security = if network.secured {
            "protegida"
        } else {
            "abierta"
        };
        if network.connected {
            return format!("Conectado, {security}");
        }
        let mut state = if network.secured {
            "Protegida".to_string()
        } else {
            "Abierta".to_string()
        };
        if network.profile.is_some() {
            state.push_str(" · Guardada");
        }
        state
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

/// Icono de Wi-Fi con tantas ondas encendidas como señal tenga la red: el icono
/// completo atenuado y, encima, la parte que corresponde a la señal, recortada en
/// círculo desde el punto de abajo (de donde salen las ondas).
fn draw_signal(image: &mut Pixmap, cx: f32, cy: f32, size: f32, signal: u8) {
    let side = (size * 2.0).ceil() as u32;
    let Some(mut layer) = Pixmap::new(side, side) else {
        return;
    };
    let center = side as f32 / 2.0;
    draw_icon_aligned(&mut layer, '\u{E701}', center, center, size, 1.0);
    let (width, height) = (layer.width() as usize, layer.height() as usize);
    let inked = |row: usize, data: &[u8]| {
        data[row * width * 4..(row + 1) * width * 4]
            .chunks(4)
            .any(|px| px[3] > 40)
    };
    let (Some(bottom), Some(top)) = (
        (0..height).rev().find(|&r| inked(r, layer.data())),
        (0..height).find(|&r| inked(r, layer.data())),
    ) else {
        return;
    };
    let paint = |opacity: f32| tiny_skia::PixmapPaint {
        opacity,
        ..Default::default()
    };
    let (x, y) = ((cx - center).round() as i32, (cy - center).round() as i32);
    let identity = tiny_skia::Transform::identity();
    image.draw_pixmap(x, y, layer.as_ref(), &paint(0.3), identity, None);
    // Punto y tres ondas, como en Windows: de una a cuatro partes encendidas.
    let bars = match signal {
        75.. => 4.0,
        50.. => 3.0,
        25.. => 2.0,
        _ => 1.0,
    };
    let full = (bottom - top) as f32 + 1.0;
    let radius = full * (bars / 4.0) + full * 0.06;
    let origin = (center, bottom as f32);
    let data = layer.data_mut();
    for row in 0..height {
        for col in 0..width {
            let (dx, dy) = (col as f32 + 0.5 - origin.0, row as f32 + 0.5 - origin.1);
            if dx * dx + dy * dy > radius * radius {
                let i = (row * width + col) * 4;
                data[i..i + 4].fill(0);
            }
        }
    }
    image.draw_pixmap(x, y, layer.as_ref(), &paint(1.0), identity, None);
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
                networks: Some(Vec::new()),
            },
            volume: Some(Volume {
                level: 0.5,
                muted: false,
            }),
            battery: None,
            ..Default::default()
        }
    }

    fn networks(count: usize) -> Vec<Network> {
        (0..count)
            .map(|i| Network {
                ssid: format!("Red {i}"),
                profile: (i % 2 == 0).then(|| format!("Red {i}")),
                signal: 90 - i as u8,
                secured: true,
                connected: i == 0,
            })
            .collect()
    }

    #[test]
    fn wifi_list_opens_from_the_arrow_and_its_rows_are_clickable() {
        let mut quick = state();
        // La flecha está a la derecha del botón de Wi-Fi; el resto del botón lo apaga.
        let arrow_x = tile_x(Radio::Wifi) + TILE_W - ARROW_W / 2.0;
        let middle_y = TILE_Y + TILE_H / 2.0;
        assert_eq!(quick.hit_test(arrow_x, middle_y), Some(Hit::WifiList));
        assert_eq!(
            quick.hit_test(tile_x(Radio::Wifi) + 20.0, middle_y),
            Some(Hit::Tile(Radio::Wifi))
        );
        // Sin adaptador Wi-Fi no hay lista que abrir.
        quick.system.networks = None;
        assert_eq!(
            quick.hit_test(arrow_x, middle_y),
            Some(Hit::Tile(Radio::Wifi))
        );

        quick.system.networks = Some(networks(10));
        quick.view = View::Wifi;
        assert_eq!(quick.height(), WIFI_HEIGHT);
        let row = |i: f32| LIST_Y + (i + 0.5) * NET_ROW;
        assert_eq!(quick.hit_test(WIDTH / 2.0, row(0.0)), Some(Hit::Network(0)));
        assert_eq!(quick.hit_test(WIDTH / 2.0, row(2.0)), Some(Hit::Network(2)));
        let (bx, by, _, _) = back_rect();
        assert_eq!(quick.hit_test(bx + 4.0, by + 4.0), Some(Hit::Back));
        let (sx, sy, _, _) = wifi_settings_rect();
        assert_eq!(quick.hit_test(sx + 4.0, sy + 4.0), Some(Hit::WifiSettings));

        // Al desplazar, la misma posición apunta a otra red, y no se pasa del final.
        quick.scroll_list(NET_ROW * 2.0);
        assert_eq!(quick.hit_test(WIDTH / 2.0, row(0.0)), Some(Hit::Network(2)));
        quick.scroll_list(10_000.0);
        assert_eq!(
            quick.list_scroll,
            10.0 * NET_ROW - QuickState::list_height()
        );
        quick.scroll_list(-10_000.0);
        assert_eq!(quick.list_scroll, 0.0);

        // Con el Wi-Fi apagado la lista no responde.
        quick.system.wifi = Some(false);
        assert_eq!(quick.hit_test(WIDTH / 2.0, row(0.0)), None);
    }

    #[test]
    fn pending_connection_clears_when_windows_confirms_or_times_out() {
        let mut quick = state();
        quick.system.networks = Some(networks(3));
        let now = Instant::now();
        quick.pending = Some(Pending {
            ssid: "Red 2".into(),
            connect: true,
            since: now,
        });
        assert_eq!(
            quick.network_state(&quick.networks()[2].clone()),
            "Conectando…"
        );
        quick.settle(now);
        assert!(quick.pending.is_some());
        // Windows la da por conectada.
        quick.system.networks.as_mut().unwrap()[2].connected = true;
        quick.settle(now);
        assert!(quick.pending.is_none());

        quick.pending = Some(Pending {
            ssid: "Red 1".into(),
            connect: true,
            since: now,
        });
        quick.settle(now + PENDING_FOR + Duration::from_secs(1));
        assert!(quick.pending.is_none());
        // Estado de cada red cuando no hay nada pendiente.
        let list = quick.networks().to_vec();
        assert_eq!(quick.network_state(&list[0]), "Conectado, protegida");
        assert_eq!(quick.network_state(&list[1]), "Protegida");
        let saved = Network {
            ssid: "Biblioteca".into(),
            profile: Some("Biblioteca".into()),
            signal: 50,
            secured: false,
            connected: false,
        };
        assert_eq!(quick.network_state(&saved), "Abierta · Guardada");
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
