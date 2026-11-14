//! Ventana de configuración de GlassBar: lo que antes estaba en el menú del clic
//! derecho, en un panel con secciones e interruptores. Se abre con el icono de
//! GlassBar en el panel de apps en segundo plano (o en la bandeja de Windows).
//!
//! Cada opción se traduce a la misma orden que tenía en el menú (ver `appmenu` y
//! `style::Choice`), así el resultado es el mismo que antes.
use crate::apps::{fill_rounded, stroke_rounded};
use crate::clock::{draw_text_on_baseline, draw_text_semibold, line_metrics, text_width};
use crate::style::{Appearance, Choice, State, Styles};
use tiny_skia::Pixmap;

pub const WIDTH: f32 = 420.0;
const PAD: f32 = 16.0;
const TITLE_H: f32 = 58.0;
const SECTION_H: f32 = 34.0;
const ROW: f32 = 42.0;
/// Botón de cerrar, arriba a la derecha.
const CLOSE: f32 = 32.0;
/// Interruptor.
const SWITCH_W: f32 = 38.0;
const SWITCH_H: f32 = 20.0;
/// Selector de apariencia: "‹ Desenfoque ›".
const PICKER_W: f32 = 140.0;
const PICKER_H: f32 = 28.0;
/// Barra de la opacidad de los menús.
const SLIDER_W: f32 = 150.0;
/// Botón de las actualizaciones.
const BUTTON_W: f32 = 150.0;
const TEXT: f32 = 13.0;

/// Interruptores de la sección del dock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    AutoHide,
    TopBar,
    Autostart,
}

/// Lo que hay bajo el cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Close,
    Switch(Switch),
    /// Activar o quitar una situación (todas menos el escritorio).
    Enabled(State),
    /// Cambiar la apariencia de una situación a la anterior (`false`) o la siguiente.
    Appearance(State, bool),
    Opacity,
    Update,
    Exit,
}

/// Estado de las actualizaciones, como en el menú.
#[derive(Clone, Default, PartialEq)]
pub struct Updates {
    pub checking: bool,
    pub available: Option<String>,
    pub busy: bool,
}

#[derive(Clone)]
pub struct SettingsState {
    pub styles: Styles,
    pub autostart: bool,
    pub updates: Updates,
    pub hovered: Option<Hit>,
    /// Arrastrando la barra de la opacidad.
    pub dragging: bool,
}

/// Filas, de arriba abajo: título de sección o fila con su contenido.
#[derive(Clone, Copy, PartialEq)]
enum Line {
    Section(&'static str),
    Switch(Switch),
    Look(State),
    Opacity,
    Update,
    Exit,
}

fn lines() -> Vec<Line> {
    let mut lines = vec![
        Line::Section("Dock"),
        Line::Switch(Switch::AutoHide),
        Line::Switch(Switch::TopBar),
        Line::Switch(Switch::Autostart),
        Line::Section("Apariencia según lo que hay en pantalla"),
    ];
    lines.extend(State::ALL.iter().map(|&state| Line::Look(state)));
    lines.push(Line::Opacity);
    lines.push(Line::Section("GlassBar"));
    lines.push(Line::Update);
    lines.push(Line::Exit);
    lines
}

fn line_height(line: Line) -> f32 {
    match line {
        Line::Section(_) => SECTION_H,
        _ => ROW,
    }
}

/// Cada fila con su borde superior.
fn layout() -> Vec<(Line, f32)> {
    let mut top = TITLE_H;
    lines()
        .into_iter()
        .map(|line| {
            let at = top;
            top += line_height(line);
            (line, at)
        })
        .collect()
}

pub fn height() -> f32 {
    TITLE_H + lines().into_iter().map(line_height).sum::<f32>() + PAD / 2.0
}

fn switch_label(switch: Switch) -> &'static str {
    match switch {
        Switch::AutoHide => "Ocultar el dock automáticamente",
        Switch::TopBar => "Controles en una barra superior",
        Switch::Autostart => "Iniciar con Windows",
    }
}

/// Borde izquierdo de los controles alineados a la derecha.
fn right_edge() -> f32 {
    WIDTH - PAD
}

impl SettingsState {
    pub fn new(styles: Styles, autostart: bool, updates: Updates) -> Self {
        Self {
            styles,
            autostart,
            updates,
            hovered: None,
            dragging: false,
        }
    }

    fn switch_on(&self, switch: Switch) -> bool {
        match switch {
            Switch::AutoHide => !self.styles.always_visible(),
            Switch::TopBar => self.styles.top_bar(),
            Switch::Autostart => self.autostart,
        }
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<Hit> {
        if (WIDTH - PAD - CLOSE..WIDTH - PAD).contains(&x) && (13.0..13.0 + CLOSE).contains(&y) {
            return Some(Hit::Close);
        }
        let (line, top) = layout()
            .into_iter()
            .find(|(line, top)| (*top..*top + line_height(*line)).contains(&y))?;
        let right = right_edge();
        match line {
            Line::Section(_) => None,
            // Toda la fila cambia el interruptor.
            Line::Switch(switch) => Some(Hit::Switch(switch)),
            Line::Look(state) => {
                let picker = right - PICKER_W;
                if (picker..right).contains(&x) {
                    let next = x >= picker + PICKER_W / 2.0;
                    return Some(Hit::Appearance(state, next));
                }
                let switch = picker - 12.0 - SWITCH_W;
                if state != State::Desktop && (switch - 6.0..picker - 6.0).contains(&x) {
                    return Some(Hit::Enabled(state));
                }
                None
            }
            Line::Opacity => {
                let left = right - SLIDER_W - 44.0;
                ((left - 6.0..right).contains(&x)).then_some(Hit::Opacity)
            }
            Line::Update => {
                let busy = self.updates.busy || self.updates.checking;
                let inside = (right - BUTTON_W..right).contains(&x);
                (inside && !busy).then_some(Hit::Update)
            }
            Line::Exit => {
                let _ = top;
                Some(Hit::Exit)
            }
        }
    }

    /// El cursor está en la cabecera (fuera de la ✕): desde ahí se arrastra la ventana.
    pub fn in_title(x: f32, y: f32) -> bool {
        (0.0..TITLE_H).contains(&y) && (0.0..WIDTH - PAD - CLOSE).contains(&x)
    }

    /// Valor de la opacidad (0 a 1) para el cursor en `x`.
    pub fn opacity_at(x: f32) -> f32 {
        let left = right_edge() - SLIDER_W - 44.0;
        ((x - left) / SLIDER_W).clamp(0.0, 1.0)
    }

    /// La orden del menú que corresponde a pulsar `hit`, si la tiene. Cerrar y la
    /// barra de la opacidad los atiende la ventana.
    pub fn command(&self, hit: Hit) -> Option<u32> {
        use crate::appmenu::{CMD_AUTOHIDE, CMD_AUTOSTART, CMD_EXIT, CMD_TOP_BAR, CMD_UPDATE};
        Some(match hit {
            Hit::Switch(Switch::AutoHide) => CMD_AUTOHIDE,
            Hit::Switch(Switch::TopBar) => CMD_TOP_BAR,
            Hit::Switch(Switch::Autostart) => CMD_AUTOSTART,
            Hit::Enabled(state) => Choice::Toggle(state).command(),
            Hit::Appearance(state, next) => {
                let all = Appearance::ALL;
                let current = self.styles.look(state).appearance;
                let at = all.iter().position(|a| *a == current).unwrap_or(0);
                let to = if next {
                    (at + 1) % all.len()
                } else {
                    (at + all.len() - 1) % all.len()
                };
                Choice::Set(state, all[to]).command()
            }
            Hit::Update if self.updates.available.is_some() => CMD_UPDATE,
            Hit::Update => crate::appmenu::CMD_UPDATE_CHECK,
            Hit::Exit => CMD_EXIT,
            Hit::Close | Hit::Opacity => return None,
        })
    }

    pub fn draw(&self, image: &mut Pixmap, scale: f32) {
        let s = |v: f32| v * scale;
        image.fill(tiny_skia::Color::TRANSPARENT);
        let (w, h) = (s(WIDTH - 2.0), s(height() - 2.0));
        fill_rounded(image, crate::glass::panel(), scale, scale, w, h, s(12.0));
        stroke_rounded(
            image,
            [255, 255, 255, 34],
            scale,
            scale,
            w,
            h,
            s(12.0),
            scale * 0.75,
        );
        draw_text_semibold(
            image,
            "Configuración de GlassBar",
            s(PAD + 2.0),
            s(14.0),
            s(15.0),
            1.0,
        );
        let version = format!("Versión {}", env!("CARGO_PKG_VERSION"));
        draw_text_on_baseline(image, &version, s(PAD + 2.0), s(48.0), s(11.5), 0.6);
        // Cerrar.
        let close_x = WIDTH - PAD - CLOSE / 2.0;
        if self.hovered == Some(Hit::Close) {
            fill_rounded(
                image,
                [255, 255, 255, 22],
                s(close_x - CLOSE / 2.0),
                s(13.0),
                s(CLOSE),
                s(CLOSE),
                s(6.0),
            );
        }
        crate::clock::draw_icon_aligned(
            image,
            crate::appmenu::GLYPH_CLOSE,
            s(close_x),
            s(13.0 + CLOSE / 2.0),
            s(12.0),
            0.9,
        );

        let size = s(TEXT);
        let (ascent, descent) = line_metrics(size);
        let right = right_edge();
        for (line, top) in layout() {
            let cy = top + line_height(line) / 2.0;
            let baseline = s(cy) + (ascent - descent) / 2.0;
            let row_hover = |hit: Hit| self.hovered == Some(hit);
            match line {
                Line::Section(title) => {
                    let small = s(11.5);
                    draw_text_on_baseline(image, title, s(PAD + 2.0), s(top + 24.0), small, 0.6);
                }
                Line::Switch(switch) => {
                    let hit = Hit::Switch(switch);
                    if row_hover(hit) {
                        self.row_highlight(image, top, scale);
                    }
                    draw_text_on_baseline(
                        image,
                        switch_label(switch),
                        s(PAD + 2.0),
                        baseline,
                        size,
                        0.95,
                    );
                    draw_switch(
                        image,
                        right - SWITCH_W,
                        cy,
                        self.switch_on(switch),
                        true,
                        scale,
                    );
                }
                Line::Look(state) => {
                    let look = self.styles.look(state);
                    crate::clock::draw_icon_aligned(
                        image,
                        state.glyph(),
                        s(PAD + 10.0),
                        s(cy),
                        s(14.0),
                        0.8,
                    );
                    draw_text_on_baseline(image, state.name(), s(PAD + 28.0), baseline, size, 0.95);
                    let picker = right - PICKER_W;
                    if state != State::Desktop {
                        let x = picker - 12.0 - SWITCH_W;
                        draw_switch(image, x, cy, look.enabled, true, scale);
                    }
                    self.draw_picker(
                        image,
                        state,
                        look.appearance,
                        look.enabled,
                        picker,
                        cy,
                        scale,
                    );
                }
                Line::Opacity => {
                    draw_text_on_baseline(
                        image,
                        "Opacidad de los menús",
                        s(PAD + 2.0),
                        baseline,
                        size,
                        0.95,
                    );
                    let value = self.styles.menu_opacity() as f32 / 100.0;
                    let left = right - SLIDER_W - 44.0;
                    draw_slider(image, left, cy, value, scale);
                    let label = format!("{} %", self.styles.menu_opacity());
                    let tx = s(right) - text_width(&label, size);
                    draw_text_on_baseline(image, &label, tx, baseline, size, 0.8);
                }
                Line::Update => {
                    draw_text_on_baseline(
                        image,
                        "Actualizaciones",
                        s(PAD + 2.0),
                        baseline,
                        size,
                        0.95,
                    );
                    let label = match (&self.updates.available, self.updates.busy) {
                        (_, true) => "Descargando…".to_string(),
                        (Some(version), false) => format!("Actualizar a v{version}"),
                        (None, false) if self.updates.checking => "Buscando…".to_string(),
                        (None, false) => "Buscar ahora".to_string(),
                    };
                    let enabled = !(self.updates.busy || self.updates.checking);
                    let left = right - BUTTON_W;
                    let alpha = if row_hover(Hit::Update) { 34 } else { 18 };
                    fill_rounded(
                        image,
                        [255, 255, 255, alpha],
                        s(left),
                        s(cy - PICKER_H / 2.0),
                        s(BUTTON_W),
                        s(PICKER_H),
                        s(6.0),
                    );
                    let tx = s(left + BUTTON_W / 2.0) - text_width(&label, size) / 2.0;
                    let opacity = if enabled { 0.95 } else { 0.5 };
                    draw_text_on_baseline(image, &label, tx, baseline, size, opacity);
                }
                Line::Exit => {
                    if row_hover(Hit::Exit) {
                        self.row_highlight(image, top, scale);
                    }
                    draw_text_on_baseline(
                        image,
                        "Salir de GlassBar",
                        s(PAD + 2.0),
                        baseline,
                        size,
                        0.95,
                    );
                    crate::clock::draw_icon_aligned(
                        image,
                        '\u{E7E8}',
                        s(right - 10.0),
                        s(cy),
                        s(14.0),
                        0.8,
                    );
                }
            }
        }
    }

    fn row_highlight(&self, image: &mut Pixmap, top: f32, scale: f32) {
        fill_rounded(
            image,
            [255, 255, 255, 16],
            (PAD - 8.0) * scale,
            (top + 3.0) * scale,
            (WIDTH - PAD * 2.0 + 16.0) * scale,
            (ROW - 6.0) * scale,
            6.0 * scale,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_picker(
        &self,
        image: &mut Pixmap,
        state: State,
        appearance: Appearance,
        enabled: bool,
        left: f32,
        cy: f32,
        scale: f32,
    ) {
        let s = |v: f32| v * scale;
        let hovered = matches!(self.hovered, Some(Hit::Appearance(st, _)) if st == state);
        fill_rounded(
            image,
            [255, 255, 255, if hovered { 30 } else { 16 }],
            s(left),
            s(cy - PICKER_H / 2.0),
            s(PICKER_W),
            s(PICKER_H),
            s(6.0),
        );
        let opacity = if enabled { 0.95 } else { 0.5 };
        let size = s(TEXT);
        let (ascent, descent) = line_metrics(size);
        let baseline = s(cy) + (ascent - descent) / 2.0;
        let name = appearance.name();
        let tx = s(left + PICKER_W / 2.0) - text_width(name, size) / 2.0;
        draw_text_on_baseline(image, name, tx, baseline, size, opacity);
        for (glyph, x) in [
            ('\u{E76B}', left + 12.0),
            ('\u{E76C}', left + PICKER_W - 12.0),
        ] {
            crate::clock::draw_icon_aligned(image, glyph, s(x), s(cy), s(10.0), opacity * 0.8);
        }
    }
}

/// Interruptor como los de Windows 11: con el color de énfasis si está encendido.
fn draw_switch(image: &mut Pixmap, left: f32, cy: f32, on: bool, enabled: bool, scale: f32) {
    let s = |v: f32| v * scale;
    let top = cy - SWITCH_H / 2.0;
    let alpha = if enabled { 255 } else { 120 };
    if on {
        let accent = crate::accent::with_alpha(crate::accent::current().light, alpha);
        fill_rounded(
            image,
            accent,
            s(left),
            s(top),
            s(SWITCH_W),
            s(SWITCH_H),
            s(SWITCH_H / 2.0),
        );
    } else {
        stroke_rounded(
            image,
            [255, 255, 255, 150],
            s(left + 0.5),
            s(top + 0.5),
            s(SWITCH_W - 1.0),
            s(SWITCH_H - 1.0),
            s(SWITCH_H / 2.0),
            scale,
        );
    }
    let knob = if on { 14.0 } else { 12.0 };
    let cx = if on {
        left + SWITCH_W - SWITCH_H / 2.0
    } else {
        left + SWITCH_H / 2.0
    };
    let color = if on {
        [20, 20, 22, 255]
    } else {
        [255, 255, 255, 200]
    };
    fill_rounded(
        image,
        color,
        s(cx - knob / 2.0),
        s(cy - knob / 2.0),
        s(knob),
        s(knob),
        s(knob / 2.0),
    );
}

/// Barra con el color de énfasis hasta `value` (0 a 1) y su tirador.
fn draw_slider(image: &mut Pixmap, left: f32, cy: f32, value: f32, scale: f32) {
    let s = |v: f32| v * scale;
    fill_rounded(
        image,
        [255, 255, 255, 60],
        s(left),
        s(cy - 2.0),
        s(SLIDER_W),
        s(4.0),
        s(2.0),
    );
    let accent = crate::accent::current().light;
    let filled = SLIDER_W * value.clamp(0.0, 1.0);
    fill_rounded(
        image,
        accent,
        s(left),
        s(cy - 2.0),
        s(filled),
        s(4.0),
        s(2.0),
    );
    let knob = 16.0;
    fill_rounded(
        image,
        [255, 255, 255, 255],
        s(left + filled - knob / 2.0),
        s(cy - knob / 2.0),
        s(knob),
        s(knob),
        s(knob / 2.0),
    );
    fill_rounded(
        image,
        accent,
        s(left + filled - 4.0),
        s(cy - 4.0),
        s(8.0),
        s(8.0),
        s(4.0),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> SettingsState {
        SettingsState::new(Styles::default(), false, Updates::default())
    }

    fn row_center(wanted: Line) -> f32 {
        let (line, top) = layout().into_iter().find(|(l, _)| *l == wanted).unwrap();
        top + line_height(line) / 2.0
    }

    #[test]
    fn rows_map_to_the_old_menu_commands() {
        use crate::appmenu::{CMD_AUTOHIDE, CMD_EXIT, CMD_TOP_BAR, CMD_UPDATE_CHECK};
        let settings = state();
        let hit = |line, x| settings.hit_test(x, row_center(line));
        let at = |line, x| hit(line, x).and_then(|h| settings.command(h));
        assert_eq!(at(Line::Switch(Switch::AutoHide), 40.0), Some(CMD_AUTOHIDE));
        assert_eq!(
            at(Line::Switch(Switch::TopBar), WIDTH - 30.0),
            Some(CMD_TOP_BAR)
        );
        assert_eq!(at(Line::Exit, 60.0), Some(CMD_EXIT));
        assert_eq!(at(Line::Update, WIDTH - PAD - 10.0), Some(CMD_UPDATE_CHECK));
        assert_eq!(hit(Line::Update, 40.0), None);
        assert_eq!(hit(Line::Section("Dock"), 40.0), None);
    }

    #[test]
    fn the_picker_cycles_through_the_appearances() {
        let settings = state();
        let y = row_center(Line::Look(State::Maximized));
        let right = right_edge();
        let next = settings.hit_test(right - 10.0, y);
        let previous = settings.hit_test(right - PICKER_W + 10.0, y);
        assert_eq!(next, Some(Hit::Appearance(State::Maximized, true)));
        assert_eq!(previous, Some(Hit::Appearance(State::Maximized, false)));
        // Normal -> Opaca hacia delante y Normal -> Acrílico hacia atrás.
        assert_eq!(
            settings.command(next.unwrap()),
            Some(Choice::Set(State::Maximized, Appearance::Opaque).command())
        );
        assert_eq!(
            settings.command(previous.unwrap()),
            Some(Choice::Set(State::Maximized, Appearance::Acrylic).command())
        );
        // El escritorio no se puede desactivar.
        let switch_x = right - PICKER_W - 12.0 - SWITCH_W / 2.0;
        let desktop = row_center(Line::Look(State::Desktop));
        assert_eq!(settings.hit_test(switch_x, desktop), None);
        assert_eq!(
            settings.hit_test(switch_x, y),
            Some(Hit::Enabled(State::Maximized))
        );
    }

    #[test]
    fn the_header_moves_the_window_but_not_the_close_button() {
        assert!(SettingsState::in_title(40.0, 20.0));
        assert!(SettingsState::in_title(WIDTH - PAD - CLOSE - 1.0, 50.0));
        assert!(!SettingsState::in_title(WIDTH - PAD - 10.0, 25.0));
        assert!(!SettingsState::in_title(40.0, TITLE_H + 1.0));
    }

    #[test]
    fn close_and_opacity() {
        let settings = state();
        assert_eq!(
            settings.hit_test(WIDTH - PAD - 10.0, 25.0),
            Some(Hit::Close)
        );
        let y = row_center(Line::Opacity);
        assert_eq!(
            settings.hit_test(right_edge() - 60.0, y),
            Some(Hit::Opacity)
        );
        assert_eq!(SettingsState::opacity_at(-100.0), 0.0);
        assert_eq!(SettingsState::opacity_at(1000.0), 1.0);
    }
}
