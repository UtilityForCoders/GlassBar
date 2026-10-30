//! Panel rápido propio, que se abre sobre los iconos de red, volumen y batería:
//! Wi-Fi y Bluetooth, brillo y volumen, con el estilo del lanzador. La flecha de los
//! botones de Wi-Fi y Bluetooth abre la lista de redes o de dispositivos, y la del
//! volumen, la de salidas de audio y aplicaciones. "Más opciones" abre el panel de
//! Windows para lo que este aún no tiene.
use crate::apps::{fill_rounded, stroke_rounded};
use crate::audio::Mixer;
use crate::bluetooth::{Device, Kind};
use crate::clock::{
    draw_icon_aligned, draw_text, draw_text_on_baseline, draw_text_semibold, fit_text,
    line_metrics, text_width,
};
use crate::status::{draw_battery, volume_glyph, Battery, Volume};
use crate::system::{Radio, Snapshot};
use crate::textfield::TextField;
use crate::wifi::{Network, Security};
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
/// Las barras dejan sitio a la derecha para el porcentaje y, en la del volumen, la
/// flecha que abre las salidas de audio.
const SLIDER_W: f32 = WIDTH - PAD - 76.0 - SLIDER_X;
const AUDIO_BUTTON: f32 = 28.0;
const FOOTER_Y: f32 = 196.0;
const MORE_W: f32 = 112.0;
const BACKGROUND: [u8; 4] = [32, 32, 34, 246];
// Los controles activos usan el color de énfasis de Windows (`crate::accent`).
/// Zona de la flecha "›" a la derecha de los botones de Wi-Fi y Bluetooth.
const ARROW_W: f32 = 36.0;
/// Vista de lista (redes o dispositivos): más alta, con cabecera, lista desplazable
/// y pie.
pub const LIST_HEIGHT: f32 = 392.0;
const LIST_Y: f32 = 56.0;
const ROW: f32 = 52.0;
const LIST_FOOTER_Y: f32 = LIST_HEIGHT - 52.0;
/// Lo que crece la fila de una red nueva al desplegar su contraseña y botones.
const EDITOR_H: f32 = 96.0;
const FIELD_X: f32 = PAD + 40.0;
const BUTTON_W: f32 = 100.0;
/// Si Windows no confirma la conexión en este tiempo, se deja de mostrar
/// "Conectando…". Es mayor que el plazo de una red nueva, para ver antes su error.
const PENDING_FOR: Duration = Duration::from_secs(25);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Main,
    /// Redes Wi-Fi o dispositivos Bluetooth.
    List(Radio),
    /// Salidas de audio y aplicaciones que lo usan.
    Audio,
}

/// Filas de la vista de audio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioRow {
    Header(&'static str),
    /// Aviso en lugar de filas (sin aplicaciones, o sin salida por aplicación).
    Note(&'static str),
    /// Salida de audio, por su posición.
    Output(usize),
    /// Aplicación, por su posición.
    App(usize),
    /// Salida que se puede elegir para la aplicación desplegada; `None` es la
    /// predeterminada.
    Choice(usize, Option<usize>),
}
impl AudioRow {
    fn height(self) -> f32 {
        match self {
            Self::Header(_) => 32.0,
            Self::Note(_) => 40.0,
            Self::Output(_) => 44.0,
            Self::App(_) => ROW,
            Self::Choice(..) => 36.0,
        }
    }
}

/// Conexión o desconexión pedida y aún sin confirmar: de una red (por su nombre) o
/// de un dispositivo Bluetooth (por su dirección).
#[derive(Clone, Debug)]
pub struct Pending {
    pub key: String,
    /// `true` al conectar, `false` al desconectar.
    pub connect: bool,
    pub since: Instant,
}

/// Contraseña de una red nueva, desplegada en su fila.
pub struct JoinEditor {
    pub ssid: String,
    pub security: Security,
    pub field: TextField,
    pub error: Option<&'static str>,
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
    /// Flecha de un botón: abre su lista.
    List(Radio),
    /// Flecha junto al volumen: abre las salidas de audio.
    AudioList,
    /// Volver de una lista a la vista principal.
    Back,
    /// Red o dispositivo de la lista, por su posición.
    Row(usize),
    /// "Más configuración de…", en la configuración de Windows.
    Settings,
    /// Campo de contraseña de una red nueva, y sus botones.
    Password,
    JoinConfirm,
    JoinCancel,
    /// Barra de desplazamiento de la lista.
    Scrollbar,
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
    /// Desplazamiento de la lista.
    pub list_scroll: f32,
    /// Arrastre de la barra de desplazamiento: dónde se agarró y el desplazamiento
    /// que tenía la lista entonces.
    pub scroll_drag: Option<(f32, f32)>,
    pub pending: Option<Pending>,
    /// Red nueva cuya contraseña se está escribiendo.
    pub join: Option<JoinEditor>,
    /// Red nueva a la que se intenta conectar: si Windows dice que falló, su
    /// formulario vuelve a abrirse con el aviso.
    pub joining: Option<JoinEditor>,
    /// Aplicación con su lista de salidas desplegada (por la ruta de su ejecutable).
    pub audio_app: Option<String>,
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
fn settings_rect() -> (f32, f32, f32, f32) {
    (PAD - 6.0, LIST_FOOTER_Y + 10.0, 240.0, 32.0)
}
fn audio_button_rect() -> (f32, f32, f32, f32) {
    (
        WIDTH - PAD - AUDIO_BUTTON + 4.0,
        VOLUME_Y - AUDIO_BUTTON / 2.0,
        AUDIO_BUTTON,
        AUDIO_BUTTON,
    )
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
/// Partes del formulario de contraseña, relativas al borde superior de su fila.
fn field_rect() -> (f32, f32, f32, f32) {
    (FIELD_X, ROW + 2.0, WIDTH - PAD - FIELD_X, 32.0)
}
fn confirm_rect() -> (f32, f32, f32, f32) {
    (WIDTH - PAD - BUTTON_W, ROW + 58.0, BUTTON_W, 30.0)
}
fn cancel_rect() -> (f32, f32, f32, f32) {
    (
        WIDTH - PAD - BUTTON_W * 2.0 - 8.0,
        ROW + 58.0,
        BUTTON_W,
        30.0,
    )
}

impl QuickState {
    /// Alto del panel en la vista actual.
    pub fn height(&self) -> f32 {
        match self.view {
            View::Main => HEIGHT,
            View::List(_) | View::Audio => LIST_HEIGHT,
        }
    }
    pub fn mixer(&self) -> &Mixer {
        &self.system.audio
    }
    /// Filas de la vista de audio: salidas y aplicaciones, con las salidas posibles
    /// bajo la aplicación desplegada.
    pub fn audio_rows(&self) -> Vec<AudioRow> {
        let mixer = self.mixer();
        let mut rows = vec![AudioRow::Header("Salida")];
        rows.extend((0..mixer.outputs.len()).map(AudioRow::Output));
        rows.push(AudioRow::Header("Aplicaciones"));
        if !mixer.per_app {
            rows.push(AudioRow::Note("Esta versión de Windows no lo permite"));
        } else if mixer.apps.is_empty() {
            rows.push(AudioRow::Note("Ninguna aplicación está usando el audio"));
        }
        if mixer.per_app {
            for (index, app) in mixer.apps.iter().enumerate() {
                rows.push(AudioRow::App(index));
                if self.audio_app.as_deref() == Some(app.path.as_str()) {
                    rows.push(AudioRow::Choice(index, None));
                    rows.extend((0..mixer.outputs.len()).map(|o| AudioRow::Choice(index, Some(o))));
                }
            }
        }
        rows
    }
    /// Nombre de la salida que usa una aplicación.
    fn app_output_name(&self, output: Option<&str>) -> String {
        match output {
            None => "Predeterminada".into(),
            Some(id) => self
                .mixer()
                .outputs
                .iter()
                .find(|o| o.id == id)
                .map(|o| o.name.clone())
                .unwrap_or_else(|| "Salida desconectada".into()),
        }
    }
    pub fn networks(&self) -> &[Network] {
        self.system.networks.as_deref().unwrap_or_default()
    }
    pub fn devices(&self) -> &[Device] {
        &self.system.devices
    }
    /// Filas de la lista abierta.
    fn rows(&self) -> usize {
        match self.view {
            View::Main => 0,
            View::List(Radio::Wifi) => self.networks().len(),
            View::List(Radio::Bluetooth) => self.devices().len(),
            View::Audio => self.audio_rows().len(),
        }
    }
    /// La fila tiene el formulario de contraseña desplegado.
    fn expanded(&self, index: usize) -> bool {
        self.view == View::List(Radio::Wifi)
            && self
                .join
                .as_ref()
                .is_some_and(|j| self.networks().get(index).is_some_and(|n| n.ssid == j.ssid))
    }
    fn row_height(&self, index: usize) -> f32 {
        if self.view == View::Audio {
            return self.audio_rows().get(index).map_or(ROW, |r| r.height());
        }
        ROW + if self.expanded(index) { EDITOR_H } else { 0.0 }
    }
    /// Borde superior de la fila dentro de la lista, sin desplazar.
    fn row_top(&self, index: usize) -> f32 {
        if self.view == View::Audio {
            return self
                .audio_rows()
                .iter()
                .take(index)
                .map(|r| r.height())
                .sum();
        }
        (0..index).map(|i| self.row_height(i)).sum()
    }
    /// Despliega u oculta las salidas que se pueden elegir para una aplicación.
    pub fn toggle_audio_app(&mut self, app: usize) {
        let path = self.mixer().apps.get(app).map(|a| a.path.clone());
        self.audio_app = if self.audio_app == path { None } else { path };
        self.list_scroll = self.list_scroll.clamp(0.0, self.max_list_scroll());
        if let Some(index) = self
            .audio_rows()
            .iter()
            .position(|r| *r == AudioRow::App(app))
            .filter(|_| self.audio_app.is_some())
        {
            // Que se vean las salidas recién desplegadas.
            let rows = self.audio_rows();
            let bottom = self.row_top(index)
                + rows[index..]
                    .iter()
                    .take_while(
                        |r| matches!(r, AudioRow::App(i) | AudioRow::Choice(i, _) if *i == app),
                    )
                    .map(|r| r.height())
                    .sum::<f32>();
            if bottom - self.list_scroll > Self::list_height() {
                self.list_scroll = (bottom - Self::list_height()).min(self.row_top(index));
            }
        }
    }
    fn list_height() -> f32 {
        LIST_FOOTER_Y - LIST_Y
    }
    fn max_list_scroll(&self) -> f32 {
        (self.row_top(self.rows()) - Self::list_height()).max(0.0)
    }
    /// Desplaza la lista (positivo: hacia abajo).
    /// Tirador de la barra de desplazamiento (arriba y alto, en el panel); `None` si
    /// todo cabe y no hace falta barra.
    fn scroll_thumb(&self) -> Option<(f32, f32)> {
        let max = self.max_list_scroll();
        if self.view == View::Main || max <= 0.0 {
            return None;
        }
        let track = Self::list_height() - 8.0;
        let content = max + Self::list_height();
        let height = (track * Self::list_height() / content).max(24.0);
        Some((
            LIST_Y + 4.0 + (track - height) * self.list_scroll / max,
            height,
        ))
    }
    /// Empieza a arrastrar la barra en `y`. Un clic fuera del tirador lo lleva antes
    /// hasta ahí, como en Windows.
    pub fn grab_scrollbar(&mut self, y: f32) {
        let Some((top, height)) = self.scroll_thumb() else {
            return;
        };
        if !(top..top + height).contains(&y) {
            self.drag_scrollbar_from(top + height / 2.0, self.list_scroll, y);
        }
        self.scroll_drag = Some((y, self.list_scroll));
    }
    /// Sigue el arrastre de la barra con el cursor en `y`.
    pub fn drag_scrollbar(&mut self, y: f32) {
        if let Some((grab, start)) = self.scroll_drag {
            self.drag_scrollbar_from(grab, start, y);
        }
    }
    fn drag_scrollbar_from(&mut self, grab: f32, start: f32, y: f32) {
        let Some((_, height)) = self.scroll_thumb() else {
            return;
        };
        let room = (Self::list_height() - 8.0 - height).max(1.0);
        let max = self.max_list_scroll();
        self.list_scroll = (start + (y - grab) * max / room).clamp(0.0, max);
    }

    pub fn scroll_list(&mut self, delta: f32) {
        self.list_scroll = (self.list_scroll + delta).clamp(0.0, self.max_list_scroll());
    }
    /// Desplaza lo justo para que se vea la fila entera (con su formulario).
    fn reveal(&mut self, index: usize) {
        let bottom = self.row_top(index) + self.row_height(index);
        if bottom - self.list_scroll > Self::list_height() {
            self.list_scroll = bottom - Self::list_height();
        }
        self.list_scroll = self.list_scroll.min(self.row_top(index)).max(0.0);
    }

    /// Despliega la contraseña de una red nueva, o la pliega si ya lo estaba.
    pub fn toggle_join(&mut self, index: usize) {
        let Some(network) = self.networks().get(index).cloned() else {
            return;
        };
        if self.expanded(index) {
            self.join = None;
        } else if let Some(security) = network.security {
            self.join = Some(JoinEditor {
                ssid: network.ssid,
                security,
                field: TextField::default(),
                error: None,
            });
            self.reveal(index);
        }
        self.list_scroll = self.list_scroll.clamp(0.0, self.max_list_scroll());
    }
    /// Comprueba la contraseña escrita. Si vale, cierra el formulario y devuelve lo
    /// necesario para conectarse; si no, deja el aviso en el formulario.
    pub fn take_join(&mut self, now: Instant) -> Option<(String, Security, String)> {
        let join = self.join.as_mut()?;
        let password = join.field.text().to_string();
        if join.security.needs_password() {
            if let Err(error) = crate::wifi::check_password(&password) {
                join.error = Some(error);
                return None;
            }
        }
        let mut join = self.join.take()?;
        join.error = None;
        let request = (join.ssid.clone(), join.security, password);
        self.pending = Some(Pending {
            key: join.ssid.clone(),
            connect: true,
            since: now,
        });
        self.joining = Some(join);
        Some(request)
    }

    /// Tras leer el sistema: se olvida la conexión pendiente si Windows ya la confirma
    /// (o tardó demasiado), reabre el formulario de una red nueva que falló y la
    /// lista no queda desplazada más allá de su final.
    pub fn settle(&mut self, now: Instant) {
        if let Some(pending) = &self.pending {
            let network = self
                .networks()
                .iter()
                .any(|n| n.ssid == pending.key && n.connected == pending.connect);
            let device = self
                .devices()
                .iter()
                .any(|d| d.address.to_string() == pending.key && d.connected == pending.connect);
            if network || device || now.duration_since(pending.since) > PENDING_FOR {
                self.pending = None;
            }
        }
        if let Some(joining) = &self.joining {
            let failed = self.system.join_failed.as_deref() == Some(joining.ssid.as_str());
            let joined = self
                .networks()
                .iter()
                .any(|n| n.ssid == joining.ssid && n.connected);
            if failed {
                let mut join = self.joining.take();
                if let Some(join) = &mut join {
                    join.error = Some("No se pudo conectar. Revisa la contraseña.");
                    join.field.select_all();
                }
                self.join = join;
                self.pending = None;
                if let Some(index) = self.join_index() {
                    self.reveal(index);
                }
            } else if joined {
                self.joining = None;
            }
        }
        self.list_scroll = self.list_scroll.clamp(0.0, self.max_list_scroll());
    }
    /// Fila de la red con el formulario abierto.
    fn join_index(&self) -> Option<usize> {
        let join = self.join.as_ref()?;
        self.networks().iter().position(|n| n.ssid == join.ssid)
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
    /// El botón tiene flecha a su lista: Wi-Fi con adaptador, o Bluetooth.
    fn has_list(&self, radio: Radio) -> bool {
        match radio {
            Radio::Wifi => self.system.networks.is_some(),
            Radio::Bluetooth => self.system.bluetooth.is_some(),
        }
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<Hit> {
        let within =
            |rx: f32, ry: f32, w: f32, h: f32| x >= rx && x < rx + w && y >= ry && y < ry + h;
        if self.view != View::Main {
            let (bx, by, bw, bh) = back_rect();
            if within(bx, by, bw, bh) {
                return Some(Hit::Back);
            }
            let (sx, sy, sw, sh) = settings_rect();
            if within(sx, sy, sw, sh) {
                return Some(Hit::Settings);
            }
            let enabled = match self.view {
                View::List(radio) => self.radio(radio) == Some(true),
                _ => true,
            };
            if !enabled || !(LIST_Y..LIST_FOOTER_Y).contains(&y) {
                return None;
            }
            // La barra ocupa el borde derecho, a lo alto de toda la lista.
            if self.scroll_thumb().is_some() && x >= WIDTH - 14.0 {
                return Some(Hit::Scrollbar);
            }
            let content = y - LIST_Y + self.list_scroll;
            let index = (0..self.rows()).find(|&i| {
                let top = self.row_top(i);
                (top..top + self.row_height(i)).contains(&content)
            })?;
            if self.view == View::Audio {
                // Los títulos y avisos no se pulsan.
                let row = self.audio_rows()[index];
                let clickable = !matches!(row, AudioRow::Header(_) | AudioRow::Note(_));
                return (clickable && (PAD - 8.0..WIDTH - PAD + 8.0).contains(&x))
                    .then_some(Hit::Row(index));
            }
            let inside = content - self.row_top(index);
            if inside < ROW {
                return (PAD - 8.0..WIDTH - PAD + 8.0)
                    .contains(&x)
                    .then_some(Hit::Row(index));
            }
            let in_part = |(rx, ry, w, h): (f32, f32, f32, f32)| {
                x >= rx && x < rx + w && inside >= ry && inside < ry + h
            };
            let needs_password = self
                .join
                .as_ref()
                .is_some_and(|j| j.security.needs_password());
            return if needs_password && in_part(field_rect()) {
                Some(Hit::Password)
            } else if in_part(confirm_rect()) {
                Some(Hit::JoinConfirm)
            } else if in_part(cancel_rect()) {
                Some(Hit::JoinCancel)
            } else {
                None
            };
        }
        for radio in [Radio::Wifi, Radio::Bluetooth] {
            let ax = tile_x(radio) + TILE_W - ARROW_W;
            if self.has_list(radio) && within(ax, TILE_Y, ARROW_W, TILE_H) {
                return Some(Hit::List(radio));
            }
        }
        for radio in [Radio::Wifi, Radio::Bluetooth] {
            if self.radio(radio).is_some() && within(tile_x(radio), TILE_Y, TILE_W, TILE_H) {
                return Some(Hit::Tile(radio));
            }
        }
        let (ax, ay, aw, ah) = audio_button_rect();
        if within(ax, ay, aw, ah) {
            return Some(Hit::AudioList);
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
            View::List(_) | View::Audio => LIST_FOOTER_Y,
        };
        self.draw_chrome(image, footer_y, scale);
        if self.view != View::Main {
            self.draw_list(image, scale);
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
            (true, false) => crate::accent::current().dark,
            (true, true) => crate::accent::current().base,
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
        // Los botones se dividen, como en Windows: la flecha abre su lista.
        let arrow = self.has_list(radio);
        if arrow {
            let ax = x + TILE_W - ARROW_W;
            if self.hovered == Some(Hit::List(radio)) {
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

    /// Vista de lista: cabecera con "volver", lista desplazable y pie.
    fn draw_list(&self, image: &mut Pixmap, scale: f32) {
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
        let (title, settings) = match self.view {
            View::List(Radio::Wifi) => ("Wi-Fi", "Más configuración de Wi-Fi"),
            View::List(Radio::Bluetooth) => ("Bluetooth", "Más configuración de Bluetooth"),
            _ => ("Salida de audio", "Más configuración de sonido"),
        };
        draw_text_semibold(
            image,
            title,
            (PAD + 34.0) * scale,
            (by + 7.0) * scale,
            15.0 * scale,
            1.0,
        );

        let state = match self.view {
            View::List(radio) => (Some(radio), self.radio(radio), self.rows() == 0),
            _ => (None, Some(true), self.mixer().outputs.is_empty()),
        };
        let message = match state {
            (None, _, true) => Some("No hay salidas de audio"),
            (None, _, false) => None,
            (Some(radio), on, empty) => match (radio, on, empty) {
                (Radio::Wifi, Some(false), _) => Some("El Wi-Fi está desactivado"),
                (Radio::Wifi, None, _) => Some("No hay adaptador Wi-Fi"),
                (Radio::Wifi, Some(true), true) => Some("Buscando redes…"),
                (Radio::Bluetooth, Some(false), _) => Some("El Bluetooth está desactivado"),
                (Radio::Bluetooth, None, _) => Some("No hay Bluetooth"),
                (Radio::Bluetooth, Some(true), true) => Some("No hay dispositivos emparejados"),
                _ => None,
            },
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
            for index in 0..self.rows() {
                let top = self.row_top(index) - self.list_scroll;
                if top + self.row_height(index) < 0.0 || top > Self::list_height() {
                    continue;
                }
                match self.view {
                    View::Audio => self.draw_audio_row(&mut list, index, top, scale),
                    View::List(Radio::Wifi) => {
                        self.draw_network(&mut list, index, &self.networks()[index], top, scale)
                    }
                    View::List(Radio::Bluetooth) => {
                        self.draw_device(&mut list, index, &self.devices()[index], top, scale)
                    }
                    View::Main => {}
                }
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
        // Barra fina que se ensancha al pasar el cursor o arrastrarla, como en Windows.
        if let Some((top, height)) = self.scroll_thumb() {
            let active = self.hovered == Some(Hit::Scrollbar) || self.scroll_drag.is_some();
            let width = if active { 6.0 } else { 3.0 };
            fill_rounded(
                image,
                [255, 255, 255, if active { 140 } else { 90 }],
                (WIDTH - 6.0 - width) * scale,
                top * scale,
                width * scale,
                height * scale,
                width / 2.0 * scale,
            );
        }

        let (sx, sy, sw, sh) = settings_rect();
        if self.hovered == Some(Hit::Settings) {
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
            settings,
            (sx + 10.0) * scale,
            (sy + sh / 2.0) * scale + (ascent - descent) / 2.0,
            size,
            0.9,
        );
    }

    /// Fondo de una fila (al pasar el cursor o si está conectada), su nombre y su
    /// estado, y el botón de la acción a la derecha al pasar el cursor.
    #[allow(clippy::too_many_arguments)]
    fn draw_row_text(
        &self,
        image: &mut Pixmap,
        index: usize,
        top: f32,
        name: &str,
        state: &str,
        connected: bool,
        action: Option<&str>,
        scale: f32,
    ) {
        let hovered = self.hovered == Some(Hit::Row(index));
        if hovered || connected || self.expanded(index) {
            fill_rounded(
                image,
                [255, 255, 255, if hovered { 20 } else { 10 }],
                (PAD - 6.0) * scale,
                (top + 3.0) * scale,
                (WIDTH - PAD * 2.0 + 12.0) * scale,
                (self.row_height(index) - 6.0) * scale,
                6.0 * scale,
            );
        }
        let action = action.filter(|_| hovered);
        let action_w = action.map_or(0.0, |a| text_width(a, 12.0 * scale) / scale + 20.0);
        let left = PAD + 40.0;
        let room = (WIDTH - PAD - left - action_w - 4.0) * scale;
        let name = fit_text(name, 13.0 * scale, room);
        let y = (top + 9.0) * scale;
        if connected {
            draw_text_semibold(image, &name, left * scale, y, 13.0 * scale, 1.0);
        } else {
            draw_text(image, &name, left * scale, y, 13.0 * scale, 1.0);
        }
        draw_text(
            image,
            &fit_text(state, 12.0 * scale, room),
            left * scale,
            (top + 28.0) * scale,
            12.0 * scale,
            0.7,
        );
        if let Some(action) = action {
            let cy = top + ROW / 2.0;
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

    fn pending_for(&self, key: &str) -> Option<bool> {
        self.pending
            .as_ref()
            .filter(|p| p.key == key)
            .map(|p| p.connect)
    }

    fn draw_network(
        &self,
        image: &mut Pixmap,
        index: usize,
        network: &Network,
        top: f32,
        scale: f32,
    ) {
        let cy = top + ROW / 2.0;
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
        let action = match self.pending_for(&network.ssid) {
            Some(_) => None,
            None if network.connected => Some("Desconectar"),
            None if network.profile.is_some() || network.security.is_some() => Some("Conectar"),
            None => Some("Configurar"),
        };
        let action = action.filter(|_| !self.expanded(index));
        self.draw_row_text(
            image,
            index,
            top,
            &network.ssid,
            &self.network_state(network),
            network.connected,
            action,
            scale,
        );
        if let Some(join) = self.join.as_ref().filter(|_| self.expanded(index)) {
            self.draw_join(image, join, top, scale);
        }
    }

    /// Formulario de una red nueva: contraseña (oculta con puntos), aviso si lo hay, y
    /// los botones Cancelar y Conectar.
    fn draw_join(&self, image: &mut Pixmap, join: &JoinEditor, top: f32, scale: f32) {
        let size = 13.0 * scale;
        let (ascent, descent) = line_metrics(size);
        if join.security.needs_password() {
            let (fx, fy, fw, fh) = field_rect();
            let (fy, cy) = (top + fy, top + fy + fh / 2.0);
            fill_rounded(
                image,
                [255, 255, 255, 14],
                fx * scale,
                fy * scale,
                fw * scale,
                fh * scale,
                4.0 * scale,
            );
            stroke_rounded(
                image,
                [255, 255, 255, 30],
                fx * scale,
                fy * scale,
                fw * scale,
                fh * scale,
                4.0 * scale,
                scale * 0.75,
            );
            // Línea de color abajo: el campo tiene el teclado, como en Windows.
            fill_rounded(
                image,
                crate::accent::current().light,
                (fx + 1.0) * scale,
                (fy + fh - 2.0) * scale,
                (fw - 2.0) * scale,
                2.0 * scale,
                1.0 * scale,
            );
            let baseline = cy * scale + (ascent - descent) / 2.0;
            let text_x = (fx + 10.0) * scale;
            let count = join.field.text().chars().count();
            if count == 0 {
                draw_text_on_baseline(image, "Contraseña", text_x, baseline, size, 0.5);
            }
            // Puntos que caben; si no caben todos, los que rodean al cursor.
            let dot = text_width("•", size).max(1.0);
            let fits = (((fw - 20.0) * scale) / dot).floor().max(1.0) as usize;
            let caret = join.field.caret();
            let first = caret.saturating_sub(fits).min(count.saturating_sub(fits));
            let shown = (count - first).min(fits);
            if let Some((start, end)) = join.field.selection() {
                let (start, end) = (
                    start.clamp(first, first + shown),
                    end.clamp(first, first + shown),
                );
                fill_rounded(
                    image,
                    crate::accent::with_alpha(crate::accent::current().light, 110),
                    text_x + (start - first) as f32 * dot + scale,
                    cy * scale - (ascent + descent) / 2.0,
                    (end - start) as f32 * dot,
                    ascent + descent,
                    2.0 * scale,
                );
            }
            draw_text_on_baseline(image, &"•".repeat(shown), text_x, baseline, size, 1.0);
            if join.field.selection().is_none() {
                fill_rounded(
                    image,
                    [235, 240, 250, 220],
                    (text_x + (caret - first) as f32 * dot + scale).round(),
                    cy * scale - (ascent + descent) / 2.0,
                    scale.max(1.0),
                    ascent + descent,
                    0.0,
                );
            }
        }
        if let Some(error) = join.error {
            let y = top + ROW + 44.0;
            draw_icon_aligned(
                image,
                '\u{E7BA}',
                (FIELD_X + 6.0) * scale,
                (y + 6.0) * scale,
                12.0 * scale,
                0.9,
            );
            draw_text(
                image,
                &fit_text(error, 12.0 * scale, (WIDTH - PAD - FIELD_X - 20.0) * scale),
                (FIELD_X + 18.0) * scale,
                (y - 2.0) * scale,
                12.0 * scale,
                0.9,
            );
        }
        for (rect, label, hit, fill) in [
            (
                cancel_rect(),
                "Cancelar",
                Hit::JoinCancel,
                [255, 255, 255, 20],
            ),
            (
                confirm_rect(),
                "Conectar",
                Hit::JoinConfirm,
                crate::accent::current().dark,
            ),
        ] {
            let (x, y, w, h) = rect;
            let hovered = self.hovered == Some(hit);
            let fill = match (hovered, hit) {
                (true, Hit::JoinConfirm) => crate::accent::current().base,
                (true, _) => [255, 255, 255, 32],
                _ => fill,
            };
            fill_rounded(
                image,
                fill,
                x * scale,
                (top + y) * scale,
                w * scale,
                h * scale,
                5.0 * scale,
            );
            let width = text_width(label, size);
            draw_text_on_baseline(
                image,
                label,
                (x + w / 2.0) * scale - width / 2.0,
                (top + y + h / 2.0) * scale + (ascent - descent) / 2.0,
                size,
                0.95,
            );
        }
    }

    /// Segunda línea de cada red: "Conectado, protegida", "Abierta · Guardada"…
    fn network_state(&self, network: &Network) -> String {
        match self.pending_for(&network.ssid) {
            Some(true) => return "Conectando…".into(),
            Some(false) => return "Desconectando…".into(),
            None => {}
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

    fn draw_device(&self, image: &mut Pixmap, index: usize, device: &Device, top: f32, scale: f32) {
        // Auriculares, teclado, ratón, teléfono o el símbolo de Bluetooth.
        let glyph = match device.kind {
            Kind::Audio => '\u{E7F6}',
            Kind::Keyboard => '\u{E765}',
            Kind::Mouse => '\u{E962}',
            Kind::Phone => '\u{E8EA}',
            Kind::Other => '\u{E702}',
        };
        draw_icon_aligned(
            image,
            glyph,
            (PAD + 14.0) * scale,
            (top + ROW / 2.0) * scale,
            16.0 * scale,
            0.95,
        );
        let key = device.address.to_string();
        let action = match self.pending_for(&key) {
            Some(_) => None,
            None if device.audio && device.connected => Some("Desconectar"),
            None if device.audio => Some("Conectar"),
            None => Some("Configurar"),
        };
        self.draw_row_text(
            image,
            index,
            top,
            &device.name,
            &self.device_state(device),
            device.connected,
            action,
            scale,
        );
    }

    /// Segunda línea de cada dispositivo: "Conectado", "Emparejado"…
    fn device_state(&self, device: &Device) -> String {
        match self.pending_for(&device.address.to_string()) {
            Some(true) => "Conectando…".into(),
            Some(false) => "Desconectando…".into(),
            None if device.connected => "Conectado".into(),
            None => "Emparejado".into(),
        }
    }

    /// Flecha junto al volumen que abre las salidas de audio, como en Windows.
    fn draw_audio_button(&self, image: &mut Pixmap, scale: f32) {
        let (x, y, w, h) = audio_button_rect();
        if self.hovered == Some(Hit::AudioList) {
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
        draw_icon_aligned(
            image,
            '\u{E76C}',
            (x + w / 2.0) * scale,
            (y + h / 2.0) * scale,
            12.0 * scale,
            0.9,
        );
    }

    fn draw_audio_row(&self, image: &mut Pixmap, index: usize, top: f32, scale: f32) {
        let rows = self.audio_rows();
        let Some(&row) = rows.get(index) else {
            return;
        };
        let height = row.height();
        let cy = top + height / 2.0;
        let size = 13.0 * scale;
        let (ascent, descent) = line_metrics(size);
        let baseline = |cy: f32| cy * scale + (ascent - descent) / 2.0;
        let hovered = self.hovered == Some(Hit::Row(index));
        let highlight = |image: &mut Pixmap, left: f32, alpha: u8| {
            fill_rounded(
                image,
                [255, 255, 255, alpha],
                left * scale,
                (top + 2.0) * scale,
                (WIDTH - PAD + 6.0 - left) * scale,
                (height - 4.0) * scale,
                6.0 * scale,
            );
        };
        let check = |image: &mut Pixmap| {
            draw_icon_aligned(
                image,
                '\u{E73E}',
                (WIDTH - PAD - 10.0) * scale,
                cy * scale,
                12.0 * scale,
                0.95,
            );
        };
        let mixer = self.mixer();
        match row {
            AudioRow::Header(title) => {
                draw_text_semibold(
                    image,
                    title,
                    PAD * scale,
                    (top + 12.0) * scale,
                    12.0 * scale,
                    0.7,
                );
            }
            AudioRow::Note(text) => {
                draw_text(
                    image,
                    text,
                    PAD * scale,
                    (top + 11.0) * scale,
                    12.0 * scale,
                    0.55,
                );
            }
            AudioRow::Output(output) => {
                let Some(output) = mixer.outputs.get(output) else {
                    return;
                };
                let chosen = mixer.default.as_deref() == Some(output.id.as_str());
                if hovered || chosen {
                    highlight(image, PAD - 6.0, if hovered { 20 } else { 10 });
                }
                draw_icon_aligned(
                    image,
                    '\u{E7F5}',
                    (PAD + 14.0) * scale,
                    cy * scale,
                    16.0 * scale,
                    0.95,
                );
                let room = (WIDTH - PAD * 2.0 - 64.0) * scale;
                draw_text_on_baseline(
                    image,
                    &fit_text(&output.name, size, room),
                    (PAD + 40.0) * scale,
                    baseline(cy),
                    size,
                    1.0,
                );
                if chosen {
                    check(image);
                }
            }
            AudioRow::App(app) => {
                let Some(app) = mixer.apps.get(app) else {
                    return;
                };
                let open = self.audio_app.as_deref() == Some(app.path.as_str());
                if hovered || open {
                    highlight(image, PAD - 6.0, if hovered { 20 } else { 10 });
                }
                self.draw_app_icon(image, app, PAD + 14.0, cy, scale);
                let room = (WIDTH - PAD * 2.0 - 70.0) * scale;
                draw_text(
                    image,
                    &fit_text(&app.name, size, room),
                    (PAD + 40.0) * scale,
                    (top + 9.0) * scale,
                    size,
                    1.0,
                );
                draw_text(
                    image,
                    &fit_text(
                        &self.app_output_name(app.output.as_deref()),
                        12.0 * scale,
                        room,
                    ),
                    (PAD + 40.0) * scale,
                    (top + 28.0) * scale,
                    12.0 * scale,
                    0.7,
                );
                // Flecha hacia abajo o hacia arriba, según esté desplegada.
                draw_icon_aligned(
                    image,
                    if open { '\u{E70E}' } else { '\u{E70D}' },
                    (WIDTH - PAD - 10.0) * scale,
                    cy * scale,
                    11.0 * scale,
                    0.8,
                );
            }
            AudioRow::Choice(app, output) => {
                let Some(app) = mixer.apps.get(app) else {
                    return;
                };
                let id = output
                    .and_then(|o| mixer.outputs.get(o))
                    .map(|o| o.id.as_str());
                let chosen = app.output.as_deref() == id;
                if hovered {
                    highlight(image, PAD + 30.0, 20);
                }
                let name = match output.and_then(|o| mixer.outputs.get(o)) {
                    Some(output) => output.name.clone(),
                    None => "Predeterminada".into(),
                };
                let room = (WIDTH - PAD * 2.0 - 84.0) * scale;
                draw_text_on_baseline(
                    image,
                    &fit_text(&name, size, room),
                    (PAD + 40.0) * scale,
                    baseline(cy),
                    size,
                    if chosen { 1.0 } else { 0.85 },
                );
                if chosen {
                    check(image);
                }
            }
        }
    }

    /// Icono de una aplicación con audio; si no se pudo cargar, el de altavoz.
    fn draw_app_icon(
        &self,
        image: &mut Pixmap,
        app: &crate::audio::App,
        cx: f32,
        cy: f32,
        scale: f32,
    ) {
        let side = 24.0 * scale;
        let Some(icon) = &app.icon else {
            draw_icon_aligned(image, '\u{E767}', cx * scale, cy * scale, 16.0 * scale, 0.9);
            return;
        };
        let transform = tiny_skia::Transform::from_scale(
            side / icon.width() as f32,
            side / icon.height() as f32,
        )
        .post_translate(
            (cx * scale - side / 2.0).round(),
            (cy * scale - side / 2.0).round(),
        );
        image.draw_pixmap(
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
        let color = if muted {
            [150, 150, 155, 255]
        } else {
            crate::accent::current().light
        };
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
            (WIDTH - PAD - AUDIO_BUTTON - 4.0) * scale - text_width(&percent, 13.0 * scale),
            (cy - 9.0) * scale,
            13.0 * scale,
            0.9,
        );
        if slider == Slider::Volume {
            self.draw_audio_button(image, scale);
        }
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
                bluetooth: Some(true),
                brightness: Some(40),
                network: Some("Casa".into()),
                networks: Some(Vec::new()),
                ..Default::default()
            },
            volume: Some(Volume {
                level: 0.5,
                muted: false,
            }),
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
                security: Some(Security::Wpa2Personal { tkip: false }),
            })
            .collect()
    }

    fn row(i: f32) -> f32 {
        LIST_Y + (i + 0.5) * ROW
    }

    #[test]
    fn controls_are_found_where_they_are_drawn() {
        let quick = state();
        let middle_y = TILE_Y + TILE_H / 2.0;
        for radio in [Radio::Wifi, Radio::Bluetooth] {
            // La flecha, a la derecha del botón, abre su lista; el resto lo apaga.
            let arrow = tile_x(radio) + TILE_W - ARROW_W / 2.0;
            assert_eq!(quick.hit_test(arrow, middle_y), Some(Hit::List(radio)));
            assert_eq!(
                quick.hit_test(tile_x(radio) + 20.0, middle_y),
                Some(Hit::Tile(radio))
            );
        }
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
        quick.system.networks = None;
        quick.volume = None;
        let middle = SLIDER_X + SLIDER_W / 2.0;
        assert_eq!(quick.hit_test(middle, BRIGHTNESS_Y), None);
        assert_eq!(quick.hit_test(middle, VOLUME_Y), None);
        assert_eq!(quick.hit_test(PAD + 16.0, VOLUME_Y), None);
        let (x, y) = (tile_x(Radio::Bluetooth) + 10.0, TILE_Y + 10.0);
        assert_eq!(quick.hit_test(x, y), None);
        // Sin adaptador Wi-Fi ni Bluetooth no hay flechas: el botón entero es el botón.
        let arrow = tile_x(Radio::Wifi) + TILE_W - ARROW_W / 2.0;
        assert_eq!(
            quick.hit_test(arrow, TILE_Y + 10.0),
            Some(Hit::Tile(Radio::Wifi))
        );
    }

    #[test]
    fn slider_value_follows_the_cursor_and_stays_in_range() {
        assert_eq!(QuickState::slider_value(SLIDER_X), 0.0);
        assert_eq!(QuickState::slider_value(SLIDER_X + SLIDER_W), 1.0);
        assert_eq!(QuickState::slider_value(SLIDER_X + SLIDER_W / 4.0), 0.25);
        assert_eq!(QuickState::slider_value(-50.0), 0.0);
        assert_eq!(QuickState::slider_value(WIDTH + 50.0), 1.0);
    }

    #[test]
    fn list_rows_are_clickable_and_scroll() {
        let mut quick = state();
        quick.system.networks = Some(networks(10));
        quick.view = View::List(Radio::Wifi);
        assert_eq!(quick.height(), LIST_HEIGHT);
        assert_eq!(quick.hit_test(WIDTH / 2.0, row(0.0)), Some(Hit::Row(0)));
        assert_eq!(quick.hit_test(WIDTH / 2.0, row(2.0)), Some(Hit::Row(2)));
        let (bx, by, _, _) = back_rect();
        assert_eq!(quick.hit_test(bx + 4.0, by + 4.0), Some(Hit::Back));
        let (sx, sy, _, _) = settings_rect();
        assert_eq!(quick.hit_test(sx + 4.0, sy + 4.0), Some(Hit::Settings));

        // Al desplazar, la misma posición apunta a otra fila, y no se pasa del final.
        quick.scroll_list(ROW * 2.0);
        assert_eq!(quick.hit_test(WIDTH / 2.0, row(0.0)), Some(Hit::Row(2)));
        quick.scroll_list(10_000.0);
        assert_eq!(quick.list_scroll, 10.0 * ROW - QuickState::list_height());
        quick.scroll_list(-10_000.0);
        assert_eq!(quick.list_scroll, 0.0);

        // Con el Wi-Fi apagado la lista no responde.
        quick.system.wifi = Some(false);
        assert_eq!(quick.hit_test(WIDTH / 2.0, row(0.0)), None);

        // La de Bluetooth funciona igual, con los dispositivos.
        quick.view = View::List(Radio::Bluetooth);
        assert_eq!(quick.hit_test(WIDTH / 2.0, row(0.0)), None);
        quick.system.devices = vec![
            Device {
                name: "Auriculares".into(),
                address: 0xAABB,
                kind: Kind::Audio,
                connected: true,
                audio: true,
            },
            Device {
                name: "Teclado".into(),
                address: 0xCCDD,
                kind: Kind::Keyboard,
                connected: false,
                audio: false,
            },
        ];
        assert_eq!(quick.hit_test(WIDTH / 2.0, row(1.0)), Some(Hit::Row(1)));
        assert_eq!(quick.device_state(&quick.devices()[0].clone()), "Conectado");
        assert_eq!(
            quick.device_state(&quick.devices()[1].clone()),
            "Emparejado"
        );
        quick.pending = Some(Pending {
            key: 0xAABB.to_string(),
            connect: false,
            since: Instant::now(),
        });
        assert_eq!(
            quick.device_state(&quick.devices()[0].clone()),
            "Desconectando…"
        );
        // Windows confirma la desconexión.
        quick.system.devices[0].connected = false;
        quick.settle(Instant::now());
        assert!(quick.pending.is_none());
    }

    #[test]
    fn new_network_form_opens_in_its_row_and_moves_the_rest_down() {
        let mut quick = state();
        quick.system.networks = Some(networks(4));
        quick.view = View::List(Radio::Wifi);
        // "Red 1" no está guardada: su fila despliega el formulario.
        quick.toggle_join(1);
        assert!(quick.expanded(1));
        let form = LIST_Y + ROW;
        let (fx, fy, _, fh) = field_rect();
        assert_eq!(
            quick.hit_test(fx + 10.0, form + fy + fh / 2.0),
            Some(Hit::Password)
        );
        let (cx, cy, cw, ch) = confirm_rect();
        assert_eq!(
            quick.hit_test(cx + cw / 2.0, form + cy + ch / 2.0),
            Some(Hit::JoinConfirm)
        );
        let (kx, ky, kw, kh) = cancel_rect();
        assert_eq!(
            quick.hit_test(kx + kw / 2.0, form + ky + kh / 2.0),
            Some(Hit::JoinCancel)
        );
        // La fila siguiente queda debajo del formulario.
        assert_eq!(
            quick.hit_test(WIDTH / 2.0, row(2.0) + EDITOR_H),
            Some(Hit::Row(2))
        );
        assert!(confirm_rect().1 + confirm_rect().3 <= ROW + EDITOR_H);
        // Otro clic en la fila lo pliega.
        quick.toggle_join(1);
        assert!(quick.join.is_none());
        assert_eq!(quick.hit_test(WIDTH / 2.0, row(2.0)), Some(Hit::Row(2)));
    }

    #[test]
    fn joining_checks_the_password_and_reopens_the_form_if_windows_fails() {
        let mut quick = state();
        quick.system.networks = Some(networks(4));
        quick.view = View::List(Radio::Wifi);
        let now = Instant::now();
        quick.toggle_join(1);
        // Demasiado corta: no se envía y el formulario sigue abierto con el aviso.
        quick.join.as_mut().unwrap().field.insert("corta");
        assert_eq!(quick.take_join(now), None);
        assert!(quick.join.as_ref().unwrap().error.is_some());

        quick.join.as_mut().unwrap().field.insert("123");
        let request = quick.take_join(now).unwrap();
        assert_eq!(request.0, "Red 1");
        assert_eq!(request.2, "corta123");
        assert!(quick.join.is_none());
        assert_eq!(
            quick.network_state(&quick.networks()[1].clone()),
            "Conectando…"
        );

        // Windows no pudo conectarse: vuelve el formulario con el aviso.
        quick.system.join_failed = Some("Red 1".into());
        quick.settle(now);
        let join = quick.join.as_ref().unwrap();
        assert_eq!(join.ssid, "Red 1");
        assert!(join.error.is_some());
        assert_eq!(join.field.selected_text().as_deref(), Some("corta123"));
        assert!(quick.pending.is_none());

        // Segundo intento, esta vez conectado.
        quick.system.join_failed = None;
        quick.take_join(now).unwrap();
        quick.system.networks.as_mut().unwrap()[1].connected = true;
        quick.settle(now);
        assert!(quick.joining.is_none() && quick.pending.is_none());
    }

    #[test]
    fn pending_connection_times_out_and_states_read_like_windows() {
        let mut quick = state();
        quick.system.networks = Some(networks(3));
        let now = Instant::now();
        quick.pending = Some(Pending {
            key: "Red 1".into(),
            connect: true,
            since: now,
        });
        quick.settle(now);
        assert!(quick.pending.is_some());
        quick.settle(now + PENDING_FOR + Duration::from_secs(1));
        assert!(quick.pending.is_none());
        let list = quick.networks().to_vec();
        assert_eq!(quick.network_state(&list[0]), "Conectado, protegida");
        assert_eq!(quick.network_state(&list[1]), "Protegida");
        assert_eq!(quick.network_state(&list[2]), "Protegida · Guardada");
    }

    fn mixer() -> Mixer {
        let output = |id: &str, name: &str| crate::audio::Output {
            id: id.into(),
            name: name.into(),
        };
        let app = |name: &str, output: Option<&str>| crate::audio::App {
            name: name.into(),
            path: format!("C:\\{name}.exe"),
            pids: vec![1, 2],
            output: output.map(Into::into),
            icon: None,
        };
        Mixer {
            outputs: vec![output("a", "Altavoces"), output("b", "Auriculares")],
            default: Some("a".into()),
            apps: vec![app("Discord", Some("b")), app("Spotify", None)],
            per_app: true,
        }
    }

    #[test]
    fn audio_view_lists_outputs_then_apps_with_their_choices() {
        let mut quick = state();
        // La flecha junto al volumen abre la vista, y no pisa la barra.
        let (ax, ay, aw, ah) = audio_button_rect();
        assert_eq!(
            quick.hit_test(ax + aw / 2.0, ay + ah / 2.0),
            Some(Hit::AudioList)
        );
        assert!(SLIDER_X + SLIDER_W + 10.0 < ax);

        quick.system.audio = mixer();
        quick.view = View::Audio;
        assert_eq!(
            quick.audio_rows(),
            [
                AudioRow::Header("Salida"),
                AudioRow::Output(0),
                AudioRow::Output(1),
                AudioRow::Header("Aplicaciones"),
                AudioRow::App(0),
                AudioRow::App(1),
            ]
        );
        assert_eq!(quick.app_output_name(Some("b")), "Auriculares");
        assert_eq!(quick.app_output_name(None), "Predeterminada");
        assert_eq!(quick.app_output_name(Some("x")), "Salida desconectada");

        // Los títulos no se pulsan; las salidas y aplicaciones sí.
        let center = |index: usize| LIST_Y + quick.row_top(index) + quick.row_height(index) / 2.0;
        assert_eq!(quick.hit_test(WIDTH / 2.0, center(0)), None);
        assert_eq!(quick.hit_test(WIDTH / 2.0, center(2)), Some(Hit::Row(2)));
        assert_eq!(quick.hit_test(WIDTH / 2.0, center(4)), Some(Hit::Row(4)));

        // Al desplegar Discord aparecen "Predeterminada" y cada salida bajo ella.
        quick.toggle_audio_app(0);
        assert_eq!(
            quick.audio_rows()[4..8],
            [
                AudioRow::App(0),
                AudioRow::Choice(0, None),
                AudioRow::Choice(0, Some(0)),
                AudioRow::Choice(0, Some(1)),
            ]
        );
        assert_eq!(quick.audio_rows()[8], AudioRow::App(1));
        let center = |index: usize| {
            LIST_Y + quick.row_top(index) + quick.row_height(index) / 2.0 - quick.list_scroll
        };
        assert_eq!(quick.hit_test(WIDTH / 2.0, center(7)), Some(Hit::Row(7)));
        // Otro clic la pliega.
        quick.toggle_audio_app(0);
        assert_eq!(quick.audio_rows().len(), 6);
    }

    #[test]
    fn audio_view_explains_when_there_is_nothing_to_choose() {
        let mut quick = state();
        quick.view = View::Audio;
        quick.system.audio = Mixer {
            per_app: true,
            ..mixer()
        };
        quick.system.audio.apps.clear();
        assert_eq!(
            quick.audio_rows().last(),
            Some(&AudioRow::Note("Ninguna aplicación está usando el audio"))
        );
        // Sin la interfaz interna de Windows no se listan aplicaciones.
        quick.system.audio = Mixer {
            per_app: false,
            ..mixer()
        };
        let rows = quick.audio_rows();
        assert_eq!(
            rows.last(),
            Some(&AudioRow::Note("Esta versión de Windows no lo permite"))
        );
        assert!(!rows.iter().any(|r| matches!(r, AudioRow::App(_))));
    }

    #[test]
    fn scrollbar_appears_when_the_list_overflows_and_can_be_dragged() {
        let mut quick = state();
        quick.view = View::Audio;
        quick.system.audio = mixer();
        // Todo cabe: no hay barra.
        assert_eq!(quick.scroll_thumb(), None);
        assert_ne!(
            quick.hit_test(WIDTH - 8.0, LIST_Y + 10.0),
            Some(Hit::Scrollbar)
        );

        // Con muchas aplicaciones la lista no cabe y aparece la barra a la derecha.
        let many = (0..12).map(|i| crate::audio::App {
            name: format!("App {i}"),
            path: format!("C:\\app{i}.exe"),
            pids: vec![i],
            output: None,
            icon: None,
        });
        quick.system.audio.apps.extend(many);
        let (top, height) = quick.scroll_thumb().unwrap();
        assert_eq!(top, LIST_Y + 4.0);
        assert!(height < QuickState::list_height() - 8.0);
        assert_eq!(
            quick.hit_test(WIDTH - 8.0, LIST_Y + 10.0),
            Some(Hit::Scrollbar)
        );

        // Arrastrar el tirador hasta abajo lleva la lista a su final, y no más allá.
        quick.grab_scrollbar(top + 5.0);
        quick.drag_scrollbar(top + 5.0 + 10_000.0);
        assert_eq!(quick.list_scroll, quick.max_list_scroll());
        let (bottom_top, _) = quick.scroll_thumb().unwrap();
        assert!((bottom_top + height - (LIST_FOOTER_Y - 4.0)).abs() < 0.01);
        quick.drag_scrollbar(-10_000.0);
        assert_eq!(quick.list_scroll, 0.0);
        quick.scroll_drag = None;

        // Un clic en el carril, abajo, salta hasta ahí.
        quick.grab_scrollbar(LIST_FOOTER_Y - 6.0);
        assert!(quick.list_scroll > quick.max_list_scroll() * 0.8);
    }
}
