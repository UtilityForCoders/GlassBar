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
/// Botones de minimizar y cerrar, arriba a la derecha.
const CLOSE: f32 = 32.0;
const BUTTONS_TOP: f32 = 13.0;
/// Borde izquierdo de cerrar y de minimizar, que va justo a su izquierda.
const CLOSE_LEFT: f32 = WIDTH - PAD - CLOSE;
const MINIMIZE_LEFT: f32 = CLOSE_LEFT - CLOSE - 2.0;
/// Icono de minimizar (Segoe Fluent Icons / MDL2 Assets).
const GLYPH_MINIMIZE: char = '\u{E921}';
/// Interruptor.
const SWITCH_W: f32 = 38.0;
const SWITCH_H: f32 = 20.0;
/// Selector de apariencia: "‹ Desenfoque ›".
const PICKER_W: f32 = 140.0;
const PICKER_H: f32 = 28.0;
/// Barra de la opacidad de los menús.
const SLIDER_W: f32 = 150.0;
/// Muestra del color personalizado del logo, que abre el selector de color.
const SWATCH: f32 = 28.0;
/// Botón de las actualizaciones.
const BUTTON_W: f32 = 150.0;
/// Botones de la copia de la configuración, uno junto al otro.
const BACKUP_W: f32 = 72.0;
const BACKUP_GAP: f32 = 6.0;
const TEXT: f32 = 13.0;
/// Pestañas de las páginas, bajo el título.
const TABS_TOP: f32 = TITLE_H + 4.0;
const TABS_H: f32 = 32.0;
const TAB_SIZE: f32 = 12.5;
/// Borde superior de las filas de la página.
const CONTENT_TOP: f32 = TABS_TOP + TABS_H + 8.0;

/// Páginas de la configuración, en el orden de las pestañas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Dock,
    Appearance,
    Notifications,
    GlassBar,
}

impl Page {
    pub const ALL: [Page; 4] = [
        Page::Dock,
        Page::Appearance,
        Page::Notifications,
        Page::GlassBar,
    ];
    fn name(self) -> &'static str {
        match self {
            Page::Dock => "Dock",
            Page::Appearance => "Apariencia",
            Page::Notifications => "Notificaciones",
            Page::GlassBar => "GlassBar",
        }
    }
}

/// Ancho de cada pestaña.
fn tab_width() -> f32 {
    (WIDTH - PAD * 2.0) / Page::ALL.len() as f32
}

/// Interruptores de la sección del dock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    AutoHide,
    TopBar,
    ClockSeconds,
    Notifications,
    Autostart,
}

/// Lo que hay bajo el cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Minimize,
    Close,
    /// Pestaña de una página.
    Page(Page),
    Switch(Switch),
    /// Activar o quitar una situación (todas menos el escritorio).
    Enabled(State),
    /// Cambiar la apariencia de una situación a la anterior (`false`) o la siguiente.
    Appearance(State, bool),
    /// Tamaño del dock: el anterior (`false`) o el siguiente.
    DockSize(bool),
    /// Color del logo de Inicio: el anterior (`false`) o el siguiente.
    StartColor(bool),
    /// Elegir el color personalizado del logo con el selector de Windows.
    PickColor,
    /// Una parte del selector de color abierto.
    Picker(crate::colorpick::Part),
    Opacity,
    /// Reactivar las apps silenciadas desde las notificaciones.
    Unmute,
    Update,
    /// Guardar la configuración en un archivo, o sustituirla por la de uno.
    Export,
    Import,
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
    /// Página que se ve.
    pub page: Page,
    pub hovered: Option<Hit>,
    /// Arrastrando la barra de la opacidad.
    pub dragging: bool,
    /// Selector del color personalizado del logo, abierto sobre la página.
    pub picker: Option<crate::colorpick::ColorPicker>,
}

/// Filas, de arriba abajo: título de sección o fila con su contenido.
#[derive(Clone, Copy, PartialEq)]
enum Line {
    Section(&'static str),
    Switch(Switch),
    DockSize,
    /// Color del logo de Inicio.
    StartColor,
    Look(State),
    Opacity,
    /// Apps silenciadas desde las notificaciones y el botón para reactivarlas.
    Muted,
    Update,
    /// Copia de la configuración: "Exportar" e "Importar".
    Backup,
    Exit,
}

/// Filas de cada página.
fn lines(page: Page) -> Vec<Line> {
    match page {
        Page::Dock => vec![
            Line::Switch(Switch::AutoHide),
            Line::DockSize,
            Line::Switch(Switch::TopBar),
            Line::Switch(Switch::ClockSeconds),
            Line::Switch(Switch::Autostart),
        ],
        Page::Appearance => {
            let mut lines = vec![Line::Section("Según lo que hay en pantalla")];
            lines.extend(State::ALL.iter().map(|&state| Line::Look(state)));
            lines.push(Line::Opacity);
            lines.push(Line::StartColor);
            lines
        }
        Page::Notifications => vec![Line::Switch(Switch::Notifications), Line::Muted],
        Page::GlassBar => vec![Line::Update, Line::Backup, Line::Exit],
    }
}

fn line_height(line: Line) -> f32 {
    match line {
        Line::Section(_) => SECTION_H,
        _ => ROW,
    }
}

/// Cada fila de `page` con su borde superior.
fn layout(page: Page) -> Vec<(Line, f32)> {
    let mut top = CONTENT_TOP;
    lines(page)
        .into_iter()
        .map(|line| {
            let at = top;
            top += line_height(line);
            (line, at)
        })
        .collect()
}

/// Alto de la ventana: el de la página más larga, para que no cambie de tamaño al
/// pasar de una a otra.
pub fn height() -> f32 {
    let longest = Page::ALL
        .iter()
        .map(|&page| lines(page).into_iter().map(line_height).sum::<f32>())
        .fold(0.0, f32::max);
    CONTENT_TOP + longest + PAD / 2.0
}

fn switch_label(switch: Switch) -> &'static str {
    match switch {
        Switch::AutoHide => "Ocultar el dock automáticamente",
        Switch::TopBar => "Controles en una barra superior",
        Switch::ClockSeconds => "Mostrar los segundos en el reloj",
        Switch::Notifications => "Notificaciones arriba a la derecha",
        Switch::Autostart => "Iniciar con Windows",
    }
}

/// Borde izquierdo de los controles alineados a la derecha.
fn right_edge() -> f32 {
    WIDTH - PAD
}

/// Borde izquierdo de la muestra del color personalizado, a la izquierda del selector.
fn swatch_left() -> f32 {
    right_edge() - PICKER_W - 8.0 - SWATCH
}

/// Borde izquierdo de "Exportar" y de "Importar".
fn backup_buttons() -> [(Hit, f32); 2] {
    let right = right_edge();
    [
        (Hit::Export, right - BACKUP_W * 2.0 - BACKUP_GAP),
        (Hit::Import, right - BACKUP_W),
    ]
}

impl SettingsState {
    pub fn new(styles: Styles, autostart: bool, updates: Updates) -> Self {
        Self {
            styles,
            autostart,
            updates,
            page: Page::default(),
            hovered: None,
            dragging: false,
            picker: None,
        }
    }

    fn switch_on(&self, switch: Switch) -> bool {
        match switch {
            Switch::AutoHide => !self.styles.always_visible(),
            Switch::TopBar => self.styles.top_bar(),
            Switch::ClockSeconds => self.styles.clock_seconds(),
            Switch::Notifications => self.styles.notifications(),
            Switch::Autostart => self.autostart,
        }
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<Hit> {
        // Con el selector de color abierto, solo cuenta él.
        if let Some(picker) = &self.picker {
            return picker.hit(x, y).map(Hit::Picker);
        }
        if (BUTTONS_TOP..BUTTONS_TOP + CLOSE).contains(&y) {
            if (CLOSE_LEFT..CLOSE_LEFT + CLOSE).contains(&x) {
                return Some(Hit::Close);
            }
            if (MINIMIZE_LEFT..MINIMIZE_LEFT + CLOSE).contains(&x) {
                return Some(Hit::Minimize);
            }
        }
        if (TABS_TOP..TABS_TOP + TABS_H).contains(&y) {
            let at = ((x - PAD) / tab_width()).floor();
            let inside = x >= PAD && at < Page::ALL.len() as f32;
            return inside.then(|| Hit::Page(Page::ALL[at as usize]));
        }
        let (line, top) = layout(self.page)
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
            Line::DockSize => {
                let picker = right - PICKER_W;
                (picker..right)
                    .contains(&x)
                    .then_some(Hit::DockSize(x >= picker + PICKER_W / 2.0))
            }
            Line::StartColor => {
                let picker = right - PICKER_W;
                if (swatch_left()..swatch_left() + SWATCH).contains(&x) {
                    return Some(Hit::PickColor);
                }
                (picker..right)
                    .contains(&x)
                    .then_some(Hit::StartColor(x >= picker + PICKER_W / 2.0))
            }
            Line::Opacity => {
                let left = right - SLIDER_W - 44.0;
                ((left - 6.0..right).contains(&x)).then_some(Hit::Opacity)
            }
            Line::Muted => {
                let inside = (right - BUTTON_W..right).contains(&x);
                (inside && self.styles.muted_count() > 0).then_some(Hit::Unmute)
            }
            Line::Update => {
                let busy = self.updates.busy || self.updates.checking;
                let inside = (right - BUTTON_W..right).contains(&x);
                (inside && !busy).then_some(Hit::Update)
            }
            Line::Backup => backup_buttons()
                .into_iter()
                .find(|(_, left)| (*left..*left + BACKUP_W).contains(&x))
                .map(|(hit, _)| hit),
            Line::Exit => {
                let _ = top;
                Some(Hit::Exit)
            }
        }
    }

    /// El cursor está en la cabecera (fuera de minimizar y de la ✕): desde ahí se
    /// arrastra la ventana.
    pub fn in_title(x: f32, y: f32) -> bool {
        (0.0..TITLE_H).contains(&y) && (0.0..MINIMIZE_LEFT).contains(&x)
    }

    /// Valor de la opacidad (0 a 1) para el cursor en `x`.
    pub fn opacity_at(x: f32) -> f32 {
        let left = right_edge() - SLIDER_W - 44.0;
        ((x - left) / SLIDER_W).clamp(0.0, 1.0)
    }

    /// La orden del menú que corresponde a pulsar `hit`, si la tiene. Cerrar y la
    /// barra de la opacidad los atiende la ventana.
    pub fn command(&self, hit: Hit) -> Option<u32> {
        use crate::appmenu::{
            CMD_AUTOHIDE, CMD_AUTOSTART, CMD_CLOCK_SECONDS, CMD_EXIT, CMD_EXPORT, CMD_IMPORT,
            CMD_NOTIFICATIONS, CMD_TOP_BAR, CMD_UNMUTE_APPS, CMD_UPDATE,
        };
        Some(match hit {
            Hit::Switch(Switch::AutoHide) => CMD_AUTOHIDE,
            Hit::Switch(Switch::TopBar) => CMD_TOP_BAR,
            Hit::Switch(Switch::ClockSeconds) => CMD_CLOCK_SECONDS,
            Hit::Switch(Switch::Notifications) => CMD_NOTIFICATIONS,
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
            Hit::DockSize(true) => crate::appmenu::CMD_DOCK_BIGGER,
            Hit::DockSize(false) => crate::appmenu::CMD_DOCK_SMALLER,
            Hit::StartColor(true) => crate::appmenu::CMD_START_COLOR_NEXT,
            Hit::StartColor(false) => crate::appmenu::CMD_START_COLOR_PREV,
            // El selector de color lo abre la ventana: espera a que se cierre.
            Hit::PickColor | Hit::Picker(_) => return None,
            Hit::Update if self.updates.available.is_some() => CMD_UPDATE,
            Hit::Update => crate::appmenu::CMD_UPDATE_CHECK,
            Hit::Unmute => CMD_UNMUTE_APPS,
            Hit::Export => CMD_EXPORT,
            Hit::Import => CMD_IMPORT,
            Hit::Exit => CMD_EXIT,
            Hit::Minimize | Hit::Close | Hit::Opacity | Hit::Page(_) => return None,
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
        // Minimizar y cerrar.
        for (hit, left, glyph) in [
            (Hit::Minimize, MINIMIZE_LEFT, GLYPH_MINIMIZE),
            (Hit::Close, CLOSE_LEFT, crate::appmenu::GLYPH_CLOSE),
        ] {
            if self.hovered == Some(hit) {
                fill_rounded(
                    image,
                    [255, 255, 255, 22],
                    s(left),
                    s(BUTTONS_TOP),
                    s(CLOSE),
                    s(CLOSE),
                    s(6.0),
                );
            }
            crate::clock::draw_icon_aligned(
                image,
                glyph,
                s(left + CLOSE / 2.0),
                s(BUTTONS_TOP + CLOSE / 2.0),
                s(12.0),
                0.9,
            );
        }

        self.draw_tabs(image, scale);

        let size = s(TEXT);
        let (ascent, descent) = line_metrics(size);
        let right = right_edge();
        for (line, top) in layout(self.page) {
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
                    let hovered =
                        matches!(self.hovered, Some(Hit::Appearance(st, _)) if st == state);
                    draw_picker(
                        image,
                        look.appearance.name(),
                        hovered,
                        look.enabled,
                        picker,
                        cy,
                        scale,
                    );
                }
                Line::DockSize => {
                    draw_text_on_baseline(
                        image,
                        "Tamaño del dock",
                        s(PAD + 2.0),
                        baseline,
                        size,
                        0.95,
                    );
                    let hovered = matches!(self.hovered, Some(Hit::DockSize(_)));
                    draw_picker(
                        image,
                        self.styles.dock_size_name(),
                        hovered,
                        true,
                        right - PICKER_W,
                        cy,
                        scale,
                    );
                }
                Line::StartColor => {
                    // El logo con el color elegido, como muestra.
                    crate::apps::draw_windows_start_icon(image, s(PAD + 10.0), s(cy), 14.0, scale);
                    draw_text_on_baseline(
                        image,
                        "Color del logo de Inicio",
                        s(PAD + 28.0),
                        baseline,
                        size,
                        0.95,
                    );
                    // Muestra del color personalizado: al pulsarla se elige otro.
                    let [r, g, b] = self.styles.start_custom();
                    let swatch = swatch_left();
                    let hover = self.hovered == Some(Hit::PickColor);
                    fill_rounded(
                        image,
                        [255, 255, 255, if hover { 30 } else { 16 }],
                        s(swatch),
                        s(cy - PICKER_H / 2.0),
                        s(SWATCH),
                        s(PICKER_H),
                        s(6.0),
                    );
                    fill_rounded(
                        image,
                        [r, g, b, 255],
                        s(swatch + 5.0),
                        s(cy - PICKER_H / 2.0 + 5.0),
                        s(SWATCH - 10.0),
                        s(PICKER_H - 10.0),
                        s(4.0),
                    );
                    let hovered = matches!(self.hovered, Some(Hit::StartColor(_)));
                    draw_picker(
                        image,
                        self.styles.start_color_name(),
                        hovered,
                        true,
                        right - PICKER_W,
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
                Line::Muted => {
                    draw_text_on_baseline(
                        image,
                        "Apps silenciadas",
                        s(PAD + 2.0),
                        baseline,
                        size,
                        0.95,
                    );
                    let count = self.styles.muted_count();
                    let label = match count {
                        0 => "Ninguna".to_string(),
                        1 => "Reactivar 1 app".to_string(),
                        n => format!("Reactivar {n} apps"),
                    };
                    let left = right - BUTTON_W;
                    let alpha = if row_hover(Hit::Unmute) { 34 } else { 18 };
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
                    let opacity = if count > 0 { 0.95 } else { 0.5 };
                    draw_text_on_baseline(image, &label, tx, baseline, size, opacity);
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
                Line::Backup => {
                    draw_text_on_baseline(
                        image,
                        "Copia de la configuración",
                        s(PAD + 2.0),
                        baseline,
                        size,
                        0.95,
                    );
                    for (hit, left) in backup_buttons() {
                        let alpha = if row_hover(hit) { 34 } else { 18 };
                        fill_rounded(
                            image,
                            [255, 255, 255, alpha],
                            s(left),
                            s(cy - PICKER_H / 2.0),
                            s(BACKUP_W),
                            s(PICKER_H),
                            s(6.0),
                        );
                        let label = if hit == Hit::Export {
                            "Exportar"
                        } else {
                            "Importar"
                        };
                        let tx = s(left + BACKUP_W / 2.0) - text_width(label, size) / 2.0;
                        draw_text_on_baseline(image, label, tx, baseline, size, 0.95);
                    }
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
        if let Some(picker) = &self.picker {
            let hovered = match self.hovered {
                Some(Hit::Picker(part)) => Some(part),
                _ => None,
            };
            picker.draw(image, scale, (WIDTH, height()), hovered);
        }
    }

    /// Pestañas de las páginas: un carril con la de la página que se ve resaltada.
    fn draw_tabs(&self, image: &mut Pixmap, scale: f32) {
        let s = |v: f32| v * scale;
        fill_rounded(
            image,
            [255, 255, 255, 12],
            s(PAD),
            s(TABS_TOP),
            s(WIDTH - PAD * 2.0),
            s(TABS_H),
            s(8.0),
        );
        let size = s(TAB_SIZE);
        let (ascent, descent) = line_metrics(size);
        let baseline = s(TABS_TOP + TABS_H / 2.0) + (ascent - descent) / 2.0;
        let width = tab_width();
        for (i, page) in Page::ALL.into_iter().enumerate() {
            let left = PAD + i as f32 * width;
            let selected = page == self.page;
            let hovered = self.hovered == Some(Hit::Page(page));
            if selected || hovered {
                fill_rounded(
                    image,
                    [255, 255, 255, if selected { 34 } else { 16 }],
                    s(left + 3.0),
                    s(TABS_TOP + 3.0),
                    s(width - 6.0),
                    s(TABS_H - 6.0),
                    s(6.0),
                );
            }
            let name = page.name();
            let tx = s(left + width / 2.0) - text_width(name, size) / 2.0;
            let opacity = if selected { 1.0 } else { 0.7 };
            draw_text_on_baseline(image, name, tx, baseline, size, opacity);
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
}

/// Selector "‹ nombre ›" de la configuración.
fn draw_picker(
    image: &mut Pixmap,
    name: &str,
    hovered: bool,
    enabled: bool,
    left: f32,
    cy: f32,
    scale: f32,
) {
    {
        let s = |v: f32| v * scale;
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

    /// La página de `wanted` y el centro de su fila.
    fn find(wanted: Line) -> (Page, f32) {
        Page::ALL
            .into_iter()
            .find_map(|page| {
                let (line, top) = layout(page).into_iter().find(|(l, _)| *l == wanted)?;
                Some((page, top + line_height(line) / 2.0))
            })
            .unwrap()
    }

    fn row_center(wanted: Line) -> f32 {
        find(wanted).1
    }

    /// La configuración abierta en la página de `line`.
    fn on(line: Line) -> SettingsState {
        let mut settings = state();
        settings.page = find(line).0;
        settings
    }

    #[test]
    fn rows_map_to_the_old_menu_commands() {
        use crate::appmenu::{CMD_AUTOHIDE, CMD_EXIT, CMD_TOP_BAR, CMD_UPDATE_CHECK};
        let hit = |line, x| on(line).hit_test(x, row_center(line));
        let at = |line, x| hit(line, x).and_then(|h| state().command(h));
        assert_eq!(at(Line::Switch(Switch::AutoHide), 40.0), Some(CMD_AUTOHIDE));
        assert_eq!(
            at(Line::Switch(Switch::TopBar), WIDTH - 30.0),
            Some(CMD_TOP_BAR)
        );
        assert_eq!(at(Line::Exit, 60.0), Some(CMD_EXIT));
        assert_eq!(at(Line::Update, WIDTH - PAD - 10.0), Some(CMD_UPDATE_CHECK));
        assert_eq!(hit(Line::Update, 40.0), None);
        assert_eq!(
            hit(Line::Section("Según lo que hay en pantalla"), 40.0),
            None
        );
    }

    #[test]
    fn every_row_is_on_one_page_and_the_tabs_switch_them() {
        let all: Vec<Line> = Page::ALL.into_iter().flat_map(lines).collect();
        for line in &all {
            assert_eq!(all.iter().filter(|l| *l == line).count(), 1);
        }
        // Debajo de la última fila de la página no hay nada que pulsar.
        let y = row_center(Line::Opacity);
        assert_eq!(state().hit_test(60.0, y), None);
        let settings = state();
        let tab_y = TABS_TOP + TABS_H / 2.0;
        for (i, page) in Page::ALL.into_iter().enumerate() {
            let x = PAD + tab_width() * (i as f32 + 0.5);
            assert_eq!(settings.hit_test(x, tab_y), Some(Hit::Page(page)));
        }
        assert_eq!(settings.command(Hit::Page(Page::GlassBar)), None);
        assert!(height() < 500.0);
    }

    #[test]
    fn muted_apps_can_be_unmuted_only_when_there_are_some() {
        let mut settings = on(Line::Muted);
        let x = WIDTH - PAD - 10.0;
        let y = row_center(Line::Muted);
        assert_eq!(settings.hit_test(x, y), None);
        settings.styles.mute_app("Chat", false);
        assert_eq!(settings.hit_test(x, y), Some(Hit::Unmute));
        assert_eq!(
            settings.command(Hit::Unmute),
            Some(crate::appmenu::CMD_UNMUTE_APPS)
        );
    }

    #[test]
    fn the_picker_cycles_through_the_appearances() {
        let settings = on(Line::Look(State::Maximized));
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
    fn the_start_color_picker_steps_both_ways() {
        use crate::appmenu::{CMD_START_COLOR_NEXT, CMD_START_COLOR_PREV};
        let settings = on(Line::StartColor);
        let y = row_center(Line::StartColor);
        let right = right_edge();
        let next = settings.hit_test(right - 10.0, y);
        let previous = settings.hit_test(right - PICKER_W + 10.0, y);
        assert_eq!(
            next.and_then(|h| settings.command(h)),
            Some(CMD_START_COLOR_NEXT)
        );
        assert_eq!(
            previous.and_then(|h| settings.command(h)),
            Some(CMD_START_COLOR_PREV)
        );
        assert_eq!(settings.hit_test(60.0, y), None);
        let swatch = settings.hit_test(swatch_left() + SWATCH / 2.0, y);
        assert_eq!(swatch, Some(Hit::PickColor));
        assert_eq!(settings.command(Hit::PickColor), None);
    }

    #[test]
    fn the_header_moves_the_window_but_not_its_buttons() {
        assert!(SettingsState::in_title(40.0, 20.0));
        assert!(SettingsState::in_title(MINIMIZE_LEFT - 1.0, 50.0));
        assert!(!SettingsState::in_title(MINIMIZE_LEFT + 10.0, 25.0));
        assert!(!SettingsState::in_title(WIDTH - PAD - 10.0, 25.0));
        assert!(!SettingsState::in_title(40.0, TITLE_H + 1.0));
    }

    #[test]
    fn minimize_close_and_opacity() {
        let settings = on(Line::Opacity);
        assert_eq!(
            settings.hit_test(WIDTH - PAD - 10.0, 25.0),
            Some(Hit::Close)
        );
        assert_eq!(
            settings.hit_test(MINIMIZE_LEFT + 10.0, 25.0),
            Some(Hit::Minimize)
        );
        assert_eq!(settings.command(Hit::Minimize), None);
        let y = row_center(Line::Opacity);
        assert_eq!(
            settings.hit_test(right_edge() - 60.0, y),
            Some(Hit::Opacity)
        );
        assert_eq!(SettingsState::opacity_at(-100.0), 0.0);
        assert_eq!(SettingsState::opacity_at(1000.0), 1.0);
    }
}
