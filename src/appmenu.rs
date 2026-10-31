//! Menús propios con el estilo de los de Windows 11: panel oscuro redondeado,
//! títulos de sección en gris, un icono por opción, resaltado al pasar el cursor y
//! submenús a un lado. Los usan el menú de cada app del dock y el de GlassBar. Aquí
//! está solo el contenido y su dibujo; la ventana que lo muestra está en `main`.
use crate::apps::{fill_rounded, stroke_rounded};
use crate::clock::{draw_icon, draw_text, fit_text, text_width};
use crate::style::{Appearance, Choice, State, Styles};
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
/// Marca de la opción elegida.
pub const GLYPH_CHECK: char = '\u{E73E}';
const GLYPH_CHEVRON: char = '\u{E76C}';
const GLYPH_EXIT: char = '\u{E7E8}';

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
/// Hueco a la derecha para la flecha de los submenús.
const CHEVRON_W: f32 = 28.0;
const RADIUS: f32 = 8.0;
/// Separación entre un menú y su submenú.
const GAP: f32 = 2.0;

/// Icono de una opción: una imagen ya reducida a `ICON` por la escala, o un glifo.
pub enum Icon {
    Image(Option<Pixmap>),
    Glyph(char),
}

impl Icon {
    /// La marca de las opciones elegidas; sin elegir, el hueco vacío.
    pub fn check(on: bool) -> Self {
        if on {
            Icon::Glyph(GLYPH_CHECK)
        } else {
            Icon::Image(None)
        }
    }
}

pub enum Entry {
    /// Título de sección ("Más visitado", "Tareas"…).
    Header(String),
    Item {
        icon: Icon,
        label: String,
        command: u32,
    },
    /// Opción que abre otro menú a su lado.
    Submenu {
        icon: Icon,
        label: String,
        menu: Box<AppMenu>,
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
                Entry::Submenu { label, .. } => text_width(label, TEXT) + TEXT_X + CHEVRON_W,
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
            Entry::Item { .. } | Entry::Submenu { .. } => ITEM_H,
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
            .position(|(top, entry)| Self::selectable(entry) && y >= top && y < top + ITEM_H)
    }

    fn selectable(entry: &Entry) -> bool {
        matches!(entry, Entry::Item { .. } | Entry::Submenu { .. })
    }

    /// Orden de la opción `index`, si es una opción.
    pub fn command(&self, index: usize) -> Option<u32> {
        match self.entries.get(index)? {
            Entry::Item { command, .. } => Some(*command),
            _ => None,
        }
    }

    /// El submenú que abre la opción `index`, si abre uno.
    pub fn submenu(&self, index: usize) -> Option<&AppMenu> {
        match self.entries.get(index)? {
            Entry::Submenu { menu, .. } => Some(menu),
            _ => None,
        }
    }
    fn submenu_mut(&mut self, index: usize) -> Option<&mut AppMenu> {
        match self.entries.get_mut(index)? {
            Entry::Submenu { menu, .. } => Some(menu),
            _ => None,
        }
    }

    /// Mueve el resaltado con las flechas a la opción siguiente o anterior,
    /// saltando títulos y separadores. Sin resaltado, empieza por un extremo.
    pub fn move_hover(&mut self, forward: bool) {
        let items: Vec<usize> = (0..self.entries.len())
            .filter(|&i| Self::selectable(&self.entries[i]))
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

    /// Dibuja el menú con su esquina superior izquierda en (`ox`, `oy`), en
    /// unidades lógicas, sin borrar lo que ya hay.
    fn draw_at(&self, image: &mut Pixmap, scale: f32, ox: f32, oy: f32) {
        let (w, h) = self.size();
        let s = |v: f32| v * scale;
        // Panel oscuro como los menús de Windows 11.
        fill_rounded(
            image,
            [44, 44, 44, 250],
            s(ox + 0.5),
            s(oy + 0.5),
            s(w - 1.0),
            s(h - 1.0),
            s(RADIUS),
        );
        stroke_rounded(
            image,
            [255, 255, 255, 26],
            s(ox + 0.5),
            s(oy + 0.5),
            s(w - 1.0),
            s(h - 1.0),
            s(RADIUS),
            scale * 0.75,
        );
        for ((top, entry), index) in self.tops().into_iter().zip(&self.entries).zip(0..) {
            let top = oy + top;
            let (icon, label, submenu) = match entry {
                Entry::Header(title) => {
                    let title = fit_text(title, s(HEADER_TEXT), s(w - PAD * 2.0 - ICON_X * 2.0));
                    draw_text(
                        image,
                        &title,
                        s(ox + PAD + ICON_X),
                        s(top + 9.0),
                        s(HEADER_TEXT),
                        0.6,
                    );
                    continue;
                }
                Entry::Separator => {
                    fill_rounded(
                        image,
                        [255, 255, 255, 22],
                        s(ox + PAD),
                        s(top + SEPARATOR_H / 2.0).round(),
                        s(w - PAD * 2.0),
                        scale.max(1.0),
                        0.0,
                    );
                    continue;
                }
                Entry::Item { icon, label, .. } => (icon, label, false),
                Entry::Submenu { icon, label, .. } => (icon, label, true),
            };
            if self.hovered == Some(index) {
                fill_rounded(
                    image,
                    [255, 255, 255, 16],
                    s(ox + PAD),
                    s(top + 2.0),
                    s(w - PAD * 2.0),
                    s(ITEM_H - 4.0),
                    s(4.0),
                );
            }
            let (icon_x, center_y) = (s(ox + PAD + ICON_X), s(top + ITEM_H / 2.0));
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
                        Transform::from_scale(factor, factor)
                            .post_translate(icon_x.round(), (center_y - side / 2.0).round()),
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
            let right = if submenu { CHEVRON_W } else { 12.0 };
            let label = fit_text(label, s(TEXT), s(w - PAD * 2.0 - TEXT_X - right));
            draw_text(
                image,
                &label,
                s(ox + PAD + TEXT_X),
                center_y - s(TEXT * 0.62),
                s(TEXT),
                0.95,
            );
            if submenu {
                draw_icon(
                    image,
                    GLYPH_CHEVRON,
                    s(ox + w - PAD - 16.0),
                    center_y,
                    s(11.0),
                    0.8,
                );
            }
        }
    }
}

/// Opciones del menú de GlassBar.
pub const CMD_AUTOSTART: u32 = 2;
pub const CMD_EXIT: u32 = 3;

/// El menú de GlassBar: un submenú por situación con su apariencia (como en
/// TranslucentTB), iniciar con Windows y salir.
pub fn glassbar(styles: &Styles, autostart: bool) -> AppMenu {
    let mut entries = vec![
        Entry::Header("GlassBar v0.1.0 — Liquid Glass".into()),
        Entry::Separator,
    ];
    for state in State::ALL {
        let look = styles.look(state);
        let mut items = Vec::new();
        if state != State::Desktop {
            items.push(Entry::Item {
                icon: Icon::check(look.enabled),
                label: "Activado".into(),
                command: Choice::Toggle(state).command(),
            });
            items.push(Entry::Separator);
        }
        for appearance in Appearance::ALL {
            // Desactivada, sin marca: elegir una apariencia la activa.
            items.push(Entry::Item {
                icon: Icon::check(look.enabled && look.appearance == appearance),
                label: appearance.name().into(),
                command: Choice::Set(state, appearance).command(),
            });
        }
        entries.push(Entry::Submenu {
            icon: Icon::Glyph(state.glyph()),
            label: state.name().into(),
            menu: Box::new(AppMenu::new(items)),
        });
    }
    entries.push(Entry::Separator);
    entries.push(Entry::Item {
        icon: Icon::check(autostart),
        label: "Iniciar con Windows".into(),
        command: CMD_AUTOSTART,
    });
    entries.push(Entry::Separator);
    entries.push(Entry::Item {
        icon: Icon::Glyph(GLYPH_EXIT),
        label: "Salir de GlassBar".into(),
        command: CMD_EXIT,
    });
    AppMenu::new(entries)
}

/// Lo que hay bajo el cursor en un menú con submenú.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Root(usize),
    Sub(usize),
}

/// Respuesta del menú a una tecla.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// Solo cambió el resaltado (o nada).
    Moved,
    Close,
    Choose(u32),
}

/// Un menú y, a su lado, el submenú de la opción abierta. Todo en una sola
/// ventana: tan grande como el menú más el submenú más grande, con lo que sobra
/// transparente (los clics lo atraviesan).
#[derive(Default)]
pub struct Cascade {
    pub root: AppMenu,
    /// Opción de `root` cuyo submenú está abierto.
    pub open: Option<usize>,
    /// Las flechas mueven el resaltado del submenú abierto.
    pub in_sub: bool,
    /// Los submenús salen a la izquierda (no caben a la derecha).
    left: bool,
    size: (f32, f32),
}

impl Cascade {
    pub fn new(root: AppMenu, left: bool) -> Self {
        let (root_w, root_h) = root.size();
        let (sub_w, sub_h) = (0..root.entries.len())
            .filter_map(|i| root.submenu(i).map(AppMenu::size))
            .fold((0.0f32, 0.0f32), |(w, h), (sw, sh)| (w.max(sw), h.max(sh)));
        let width = if sub_w > 0.0 {
            root_w + GAP + sub_w
        } else {
            root_w
        };
        Self {
            root,
            open: None,
            in_sub: false,
            left: left && sub_w > 0.0,
            size: (width, root_h.max(sub_h)),
        }
    }

    /// Tamaño lógico de la ventana.
    pub fn size(&self) -> (f32, f32) {
        self.size
    }

    /// Esquina del menú principal: abajo, y a la derecha si los submenús salen a
    /// la izquierda.
    pub fn root_origin(&self) -> (f32, f32) {
        let (root_w, root_h) = self.root.size();
        let x = if self.left { self.size.0 - root_w } else { 0.0 };
        (x, self.size.1 - root_h)
    }

    /// Esquina del submenú de la opción `index`: junto a ella, sin salirse.
    fn sub_origin(&self, index: usize) -> Option<(f32, f32)> {
        let sub = self.root.submenu(index)?;
        let (sub_w, sub_h) = sub.size();
        let (root_w, _) = self.root.size();
        let (rx, ry) = self.root_origin();
        let x = if self.left {
            rx - GAP - sub_w
        } else {
            rx + root_w + GAP
        };
        let row = ry + self.root.tops()[index] - PAD;
        Some((x, row.clamp(0.0, (self.size.1 - sub_h).max(0.0))))
    }

    fn sub(&self) -> Option<&AppMenu> {
        self.root.submenu(self.open?)
    }
    fn sub_mut(&mut self) -> Option<&mut AppMenu> {
        let open = self.open?;
        self.root.submenu_mut(open)
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<Hit> {
        if let (Some(open), Some(sub)) = (self.open, self.sub()) {
            let (sx, sy) = self.sub_origin(open)?;
            if let Some(index) = sub.hit_test(x - sx, y - sy) {
                return Some(Hit::Sub(index));
            }
        }
        let (rx, ry) = self.root_origin();
        self.root.hit_test(x - rx, y - ry).map(Hit::Root)
    }

    /// Abre el submenú de la opción `index`, o cierra el abierto si no tiene.
    fn open_at(&mut self, index: usize) {
        if self.open == Some(index) {
            return;
        }
        if let Some(sub) = self.sub_mut() {
            sub.hovered = None;
        }
        self.open = self.root.submenu(index).map(|_| index);
        self.in_sub = false;
    }

    /// El cursor se movió a (`x`, `y`): resalta lo que hay debajo y abre el
    /// submenú de la opción que lo tenga. Devuelve si hay que redibujar.
    pub fn point(&mut self, x: f32, y: f32) -> bool {
        let before = (
            self.root.hovered,
            self.open,
            self.sub().and_then(|s| s.hovered),
        );
        match self.hit_test(x, y) {
            Some(Hit::Sub(index)) => {
                self.in_sub = true;
                self.root.hovered = self.open;
                if let Some(sub) = self.sub_mut() {
                    sub.hovered = Some(index);
                }
            }
            Some(Hit::Root(index)) => {
                self.root.hovered = Some(index);
                self.open_at(index);
            }
            // Fuera de las opciones el submenú sigue abierto, como en Windows.
            None => {
                self.root.hovered = self.open;
                if let Some(sub) = self.sub_mut() {
                    sub.hovered = None;
                }
            }
        }
        before
            != (
                self.root.hovered,
                self.open,
                self.sub().and_then(|s| s.hovered),
            )
    }

    /// El cursor salió de la ventana.
    pub fn leave(&mut self) -> bool {
        let changed = self.root.hovered != self.open;
        self.root.hovered = self.open;
        if let Some(sub) = self.sub_mut() {
            sub.hovered = None;
        }
        changed
    }

    /// Clic en (`x`, `y`): la orden de la opción, o abre su submenú.
    pub fn click(&mut self, x: f32, y: f32) -> Option<u32> {
        match self.hit_test(x, y)? {
            Hit::Sub(index) => self.sub()?.command(index),
            Hit::Root(index) => {
                self.open_at(index);
                self.root.command(index)
            }
        }
    }

    /// Teclado: flechas, Enter y Escape (códigos de tecla virtuales).
    pub fn key(&mut self, key: usize) -> Key {
        const ESCAPE: usize = 0x1B;
        const ENTER: usize = 0x0D;
        const LEFT: usize = 0x25;
        const UP: usize = 0x26;
        const RIGHT: usize = 0x27;
        const DOWN: usize = 0x28;
        match key {
            UP | DOWN if self.in_sub => {
                if let Some(sub) = self.sub_mut() {
                    sub.move_hover(key == DOWN);
                }
            }
            UP | DOWN => {
                self.root.move_hover(key == DOWN);
                if let Some(sub) = self.sub_mut() {
                    sub.hovered = None;
                }
                self.open = None;
            }
            RIGHT | ENTER if !self.in_sub => {
                let Some(index) = self.root.hovered else {
                    return Key::Moved;
                };
                if self.root.submenu(index).is_some() {
                    self.open_at(index);
                    self.in_sub = true;
                    if let Some(sub) = self.sub_mut() {
                        sub.move_hover(true);
                    }
                } else if key == ENTER {
                    return self.root.command(index).map_or(Key::Moved, Key::Choose);
                }
            }
            ENTER => {
                let chosen = self.sub().and_then(|sub| sub.command(sub.hovered?));
                return chosen.map_or(Key::Moved, Key::Choose);
            }
            LEFT | ESCAPE if self.in_sub => {
                if let Some(sub) = self.sub_mut() {
                    sub.hovered = None;
                }
                self.in_sub = false;
                self.open = None;
            }
            ESCAPE => return Key::Close,
            _ => {}
        }
        Key::Moved
    }

    pub fn draw(&self, image: &mut Pixmap, scale: f32) {
        image.fill(tiny_skia::Color::TRANSPARENT);
        let (rx, ry) = self.root_origin();
        self.root.draw_at(image, scale, rx, ry);
        if let (Some(open), Some(sub)) = (self.open, self.sub()) {
            if let Some((sx, sy)) = self.sub_origin(open) {
                sub.draw_at(image, scale, sx, sy);
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

    /// Como el de GlassBar: dos opciones con submenú y una normal.
    fn settings(left: bool) -> Cascade {
        let sub = |base: u32| {
            Box::new(AppMenu::new(vec![
                item("Activado", base),
                Entry::Separator,
                item("Normal", base + 1),
                item("Opaca", base + 2),
            ]))
        };
        Cascade::new(
            AppMenu::new(vec![
                Entry::Submenu {
                    icon: Icon::Image(None),
                    label: "Escritorio".into(),
                    menu: sub(200),
                },
                Entry::Submenu {
                    icon: Icon::Image(None),
                    label: "Ventana visible".into(),
                    menu: sub(210),
                },
                item("Salir", 3),
            ]),
            left,
        )
    }

    /// Centro de la fila `row` del menú principal, en coordenadas de la ventana.
    fn root_row(menu: &Cascade, row: usize) -> (f32, f32) {
        let (rx, ry) = menu.root_origin();
        (rx + 50.0, ry + PAD + ITEM_H * row as f32 + ITEM_H / 2.0)
    }

    #[test]
    fn hovering_an_option_opens_its_submenu_beside_it() {
        let mut menu = settings(false);
        let (root_w, root_h) = menu.root.size();
        let (w, h) = menu.size();
        assert_eq!(w, root_w + GAP + MIN_W);
        assert_eq!(h, root_h.max(menu.root.submenu(0).unwrap().size().1));
        let (x, y) = root_row(&menu, 1);
        assert!(menu.point(x, y));
        assert_eq!(menu.open, Some(1));
        // El submenú sale a la derecha, a la altura de su opción.
        let (sx, sy) = menu.sub_origin(1).unwrap();
        assert_eq!(sx, root_w + GAP);
        assert!(sy <= y);
        // Su primera opción responde; el clic da su orden.
        let first = (sx + 50.0, sy + PAD + ITEM_H / 2.0);
        assert_eq!(menu.hit_test(first.0, first.1), Some(Hit::Sub(0)));
        assert_eq!(menu.click(first.0, first.1), Some(210));
        // Pasar a una opción sin submenú lo cierra; su clic da su orden.
        let (x, y) = root_row(&menu, 2);
        menu.point(x, y);
        assert_eq!(menu.open, None);
        assert_eq!(menu.click(x, y), Some(3));
    }

    #[test]
    fn submenus_flip_left_when_there_is_no_room() {
        let menu = settings(true);
        let (root_w, _) = menu.root.size();
        assert_eq!(menu.root_origin().0, menu.size().0 - root_w);
        let mut menu = menu;
        let (x, y) = root_row(&menu, 0);
        menu.point(x, y);
        assert_eq!(menu.sub_origin(0).unwrap().0, 0.0);
    }

    #[test]
    fn keyboard_walks_into_and_out_of_submenus() {
        let mut menu = settings(false);
        assert_eq!(menu.key(0x28), Key::Moved);
        assert_eq!(menu.root.hovered, Some(0));
        // Derecha entra en el submenú y resalta su primera opción.
        menu.key(0x27);
        assert!(menu.in_sub);
        assert_eq!(menu.open, Some(0));
        menu.key(0x28);
        assert_eq!(menu.key(0x0D), Key::Choose(201));
        // Izquierda vuelve al menú; Escape ya fuera lo cierra.
        menu.key(0x25);
        assert!(!menu.in_sub);
        assert_eq!(menu.open, None);
        menu.key(0x28);
        menu.key(0x28);
        assert_eq!(menu.key(0x0D), Key::Choose(3));
        assert_eq!(menu.key(0x1B), Key::Close);
    }
}
