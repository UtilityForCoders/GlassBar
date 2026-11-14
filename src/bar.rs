//! Dock compacto: Inicio, programas anclados, estado del sistema y hora.
use crate::apps::{default_apps, draw_app_icon, draw_windows_start_icon, AppItem};
use crate::clock::{draw_icon_aligned, ClockRenderer};
use crate::glass::{draw_attention, draw_attention_dot, draw_divider, draw_dock, draw_running_dot};
use crate::spring::Spring;
use crate::status::{Item, Status};
use tiny_skia::Pixmap;

pub const DOCK_H: f32 = 52.0;
pub const SHADOW_PAD_X: f32 = 2.0;
pub const SHADOW_PAD_Y: f32 = 2.0;
pub const BOTTOM_MARGIN: f32 = 8.0;
/// Centro y tamaño del logo de Inicio.
const LAUNCHER_CX: f32 = 30.0;
const LAUNCHER_ICON: f32 = 24.0;
/// Línea entre el botón de Inicio y los programas.
const LAUNCHER_DIVIDER: f32 = 57.0;
const APP_START: f32 = 64.0;
/// Margen a cada lado de los programas cuando el dock solo lleva apps (ver `compact`).
const COMPACT_PAD: f32 = 10.0;
const APP_STEP: f32 = 46.0;
/// Hueco con una línea fina entre el grupo de estado y el reloj.
const CLOCK_GAP: f32 = 14.0;
const CLOCK_W: f32 = 78.0;
/// Franja del extremo derecho que muestra el escritorio, como en Windows 11.
const SHOW_DESKTOP_W: f32 = 14.0;
/// Alto de las líneas que separan las partes del dock.
const DIVIDER_H: f32 = 26.0;
const ICON_SIZE: f32 = 28.0;
/// Al pasar el cursor, el icono crece hasta este factor y sube `HOVER_LIFT`
/// unidades, como en el Dock de macOS; sin fondo detrás.
const HOVER_GROW: f32 = 1.1;
const HOVER_LIFT: f32 = 6.0;
const OVERFLOW_W: f32 = 30.0;
/// Hueco entre el último programa y la flecha, con una línea fina en medio.
const TRAY_GAP: f32 = 24.0;
/// Indicador de idioma del teclado ("ESP" / "INTL"), entre la flecha y el estado.
const LANGUAGE_W: f32 = 44.0;
/// Grupo de red, volumen y batería, dentro de una cápsula: margen a cada lado y
/// ancho de cada icono.
const STATUS_PAD: f32 = 10.0;
const STATUS_STEP: f32 = 24.0;
const STATUS_ICON: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverTarget {
    Launcher,
    PrimaryApp(usize),
    /// Programa abierto que no está anclado.
    OpenApp(usize),
    /// Flecha que abre los iconos ocultos de la bandeja.
    Overflow,
    /// Indicador de idioma del teclado.
    Language,
    /// Icono del grupo de estado (red, volumen o batería), por su posición.
    Status(usize),
    Clock,
    /// Franja del extremo derecho: muestra el escritorio (Win+D).
    ShowDesktop,
}

/// Icono pulsado que puede arrastrarse para cambiarlo de posición.
#[derive(Clone, Copy)]
pub struct Drag {
    pub target: HoverTarget,
    start_x: f32,
    x: f32,
    /// Distancia del cursor al centro del icono al pulsarlo.
    grab: f32,
    pub active: bool,
    moved: bool,
}

pub struct BarState {
    pub primary_apps: Vec<AppItem>,
    pub open_apps: Vec<AppItem>,
    pub drag: Option<Drag>,
    pub launcher_hover: Spring,
    pub launcher_press: Spring,
    pub hovered: Option<HoverTarget>,
    pub pressed: Option<HoverTarget>,
    pub status: Status,
    /// Teclado de la ventana activa; `None` si solo hay uno instalado.
    pub keyboard: Option<crate::keyboard::Layout>,
    /// Apariencia del fondo según lo que pasa en pantalla (ver `style`).
    pub appearance: crate::style::Appearance,
    /// Solo los programas: Inicio, la bandeja, el estado y la hora van en la barra
    /// superior (ver `topbar`).
    pub compact: bool,
    clock: ClockRenderer,
    /// Lo que no se mueve al pasar el cursor por los iconos (fondo, flecha, idioma,
    /// estado y reloj), ya dibujado. Cada fotograma lo copia en vez de repintarlo.
    fixed: std::cell::RefCell<Option<(FixedKey, Pixmap)>>,
}

/// Todo lo que cambia la capa fija del dock: si es igual, la guardada sirve.
#[derive(Clone, PartialEq)]
struct FixedKey {
    size: (u32, u32),
    scale: u32,
    appearance: crate::style::Appearance,
    status: Status,
    keyboard: Option<crate::keyboard::Layout>,
    items: usize,
    compact: bool,
    /// Resaltado de la flecha, el idioma o el estado: se dibuja debajo de su icono.
    hovered: Option<HoverTarget>,
    minute: u64,
}

impl BarState {
    pub fn new() -> Self {
        Self {
            primary_apps: crate::pins::load().unwrap_or_else(default_apps),
            open_apps: Vec::new(),
            drag: None,
            // El mismo muelle que los iconos de las apps: sube y rebota igual.
            launcher_hover: Spring::new(1.0, 260.0, 18.0),
            launcher_press: Spring::new(1.0, 340.0, 32.0),
            hovered: None,
            pressed: None,
            status: Status::default(),
            keyboard: None,
            appearance: Default::default(),
            compact: false,
            clock: ClockRenderer::new(),
            fixed: std::cell::RefCell::new(None),
        }
    }
    pub fn dock_width(&self) -> f32 {
        if self.compact {
            return self.apps_right() + COMPACT_PAD;
        }
        self.show_desktop_left() + SHOW_DESKTOP_W
    }
    /// Borde izquierdo del primer programa, en unidades del dock.
    fn apps_start(&self) -> f32 {
        if self.compact {
            COMPACT_PAD
        } else {
            APP_START
        }
    }
    /// Borde izquierdo del reloj, en unidades del dock.
    fn clock_left(&self) -> f32 {
        self.status_left() + self.status_width() + CLOCK_GAP
    }
    /// Borde izquierdo de la franja de mostrar el escritorio, en unidades del dock.
    fn show_desktop_left(&self) -> f32 {
        self.clock_left() + CLOCK_W
    }
    /// Centro horizontal de la franja de mostrar el escritorio, en píxeles de la ventana.
    pub fn show_desktop_center(&self, scale: f32) -> f32 {
        (SHADOW_PAD_X + self.show_desktop_left() + SHOW_DESKTOP_W / 2.0) * scale
    }
    /// Lo que se recorre con el teclado (Win+T, Win+B), de izquierda a derecha.
    pub fn focus_order(&self) -> Vec<HoverTarget> {
        if self.compact {
            return self.items().map(|(target, _)| target).collect();
        }
        let mut order = vec![HoverTarget::Launcher];
        order.extend(self.items().map(|(target, _)| target));
        order.push(HoverTarget::Overflow);
        if self.keyboard.is_some() {
            order.push(HoverTarget::Language);
        }
        // Todo el grupo de estado abre el mismo panel: cuenta como uno.
        order.push(HoverTarget::Status(0));
        order.push(HoverTarget::Clock);
        order.push(HoverTarget::ShowDesktop);
        order
    }
    /// Lo que hay `step` posiciones a la derecha (o a la izquierda si es negativo)
    /// de `from` en el recorrido con el teclado, sin dar la vuelta.
    pub fn focus_step(&self, from: Option<HoverTarget>, step: isize) -> HoverTarget {
        let order = self.focus_order();
        if order.is_empty() {
            return HoverTarget::Launcher;
        }
        let from = from.map(|t| match t {
            HoverTarget::Status(_) => HoverTarget::Status(0),
            t => t,
        });
        let at = from
            .and_then(|t| order.iter().position(|o| *o == t))
            .unwrap_or(0) as isize;
        let to = (at + step).clamp(0, order.len() as isize - 1);
        order[to as usize]
    }
    fn language_width(&self) -> f32 {
        if self.keyboard.is_some() {
            LANGUAGE_W
        } else {
            0.0
        }
    }
    /// Borde izquierdo del indicador de idioma, en unidades del dock.
    fn language_left(&self) -> f32 {
        self.overflow_left() + OVERFLOW_W
    }
    /// Fin del último programa, en unidades del dock: ahí empieza el hueco.
    fn apps_right(&self) -> f32 {
        self.apps_start() + self.item_count() as f32 * APP_STEP
    }
    /// Borde izquierdo de la flecha, en unidades del dock.
    fn overflow_left(&self) -> f32 {
        self.apps_right() + TRAY_GAP
    }
    /// Centro horizontal del reloj, en píxeles de la ventana: el calendario se abre
    /// sobre él.
    pub fn clock_center(&self, scale: f32) -> f32 {
        (SHADOW_PAD_X + self.clock_left() + CLOCK_W / 2.0) * scale
    }
    /// Centro horizontal del indicador de idioma, en píxeles de la ventana.
    pub fn language_center(&self, scale: f32) -> f32 {
        (SHADOW_PAD_X + self.language_left() + LANGUAGE_W / 2.0) * scale
    }
    fn status_width(&self) -> f32 {
        STATUS_PAD * 2.0 + self.status.items().len() as f32 * STATUS_STEP
    }
    /// Borde izquierdo del grupo de estado, en unidades del dock.
    fn status_left(&self) -> f32 {
        self.language_left() + self.language_width()
    }
    /// Centro horizontal del icono de estado `index`, en píxeles de la ventana.
    fn status_center(&self, index: usize, scale: f32) -> f32 {
        (SHADOW_PAD_X + self.status_left() + STATUS_PAD + (index as f32 + 0.5) * STATUS_STEP)
            * scale
    }
    /// Centro horizontal del grupo de estado, en píxeles de la ventana: el panel
    /// rápido se abre sobre él.
    pub fn status_group_center(&self, scale: f32) -> f32 {
        (SHADOW_PAD_X + self.status_left() + self.status_width() / 2.0) * scale
    }
    /// Qué muestra el icono de estado `index` (red, volumen o batería).
    pub fn status_item(&self, index: usize) -> Option<Item> {
        self.status.items().get(index).copied()
    }
    /// Centro horizontal de la flecha, en píxeles de la ventana.
    pub fn overflow_center(&self, scale: f32) -> f32 {
        (SHADOW_PAD_X + self.overflow_left() + OVERFLOW_W / 2.0) * scale
    }
    fn item_count(&self) -> usize {
        self.primary_apps.len() + self.open_apps.len()
    }
    /// Anclados y luego abiertos, con su destino de hover.
    fn items(&self) -> impl Iterator<Item = (HoverTarget, &AppItem)> {
        let pinned = self.primary_apps.iter().enumerate();
        let open = self.open_apps.iter().enumerate();
        pinned
            .map(|(i, a)| (HoverTarget::PrimaryApp(i), a))
            .chain(open.map(|(i, a)| (HoverTarget::OpenApp(i), a)))
    }
    /// Nombre y centro horizontal (en píxeles de la ventana) del icono indicado.
    pub fn item_label(&self, target: HoverTarget, scale: f32) -> Option<(String, f32)> {
        if target == HoverTarget::Overflow {
            return Some(("Mostrar iconos ocultos".into(), self.overflow_center(scale)));
        }
        if target == HoverTarget::Language {
            let name = self.keyboard.as_ref()?.name.clone();
            return Some((name, self.language_center(scale)));
        }
        if target == HoverTarget::ShowDesktop {
            return Some(("Mostrar escritorio".into(), self.show_desktop_center(scale)));
        }
        if target == HoverTarget::Clock {
            // Como en Windows: la fecha completa al pasar el cursor por el reloj.
            return Some((crate::clock::today(true), self.clock_center(scale)));
        }
        if let HoverTarget::Status(index) = target {
            let item = self.status_item(index)?;
            return Some((self.status.label(item), self.status_center(index, scale)));
        }
        let (i, (_, app)) = self.items().enumerate().find(|(_, (t, _))| *t == target)?;
        let cx = (SHADOW_PAD_X + self.apps_start() + (i as f32 + 0.5) * APP_STEP) * scale;
        Some((app.entry.name.clone(), cx))
    }
    /// Empieza un posible arrastre; `x` en píxeles de la ventana.
    pub fn begin_drag(&mut self, target: HoverTarget, x: f32, scale: f32) {
        let movable = matches!(target, HoverTarget::PrimaryApp(_) | HoverTarget::OpenApp(_));
        self.drag = self
            .item_label(target, scale)
            .filter(|_| movable)
            .map(|(_, center)| Drag {
                target,
                start_x: x,
                x,
                grab: x - center,
                active: false,
                moved: false,
            });
    }

    /// Primera y última posición global del grupo al que pertenece `target`.
    fn group_span(&self, target: HoverTarget) -> (usize, usize) {
        let pinned = self.primary_apps.len();
        match target {
            HoverTarget::OpenApp(_) => (pinned, self.item_count().saturating_sub(1)),
            _ => (0, pinned.saturating_sub(1)),
        }
    }

    /// Sigue el cursor y reordena dentro del grupo del icono (anclados o abiertos).
    /// Devuelve `true` si el arrastre está activo.
    pub fn drag_to(&mut self, x: f32, scale: f32) -> bool {
        let Some(mut drag) = self.drag else {
            return false;
        };
        drag.x = x;
        if !drag.active && (x - drag.start_x).abs() < 6.0 * scale {
            self.drag = Some(drag);
            return false;
        }
        drag.active = true;
        let start = self.apps_start();
        let (offset, group, from) = match drag.target {
            HoverTarget::PrimaryApp(i) => (0, &mut self.primary_apps, i),
            HoverTarget::OpenApp(i) => (self.primary_apps.len(), &mut self.open_apps, i),
            _ => return false,
        };
        let center = x - drag.grab;
        let slot = ((center / scale - SHADOW_PAD_X - start) / APP_STEP).floor() as isize;
        let to = (slot - offset as isize).clamp(0, group.len() as isize - 1) as usize;
        if to != from {
            let item = group.remove(from);
            group.insert(to, item);
            // Los iconos que ceden su sitio parten de donde estaban y se deslizan.
            let shift = if to > from { APP_STEP } else { -APP_STEP };
            let range = if to > from {
                from..to
            } else {
                to + 1..from + 1
            };
            for app in &mut group[range] {
                app.slide.value += shift;
                app.slide.set_target(0.0);
            }
            drag.moved = true;
            drag.target = match drag.target {
                HoverTarget::PrimaryApp(_) => HoverTarget::PrimaryApp(to),
                _ => HoverTarget::OpenApp(to),
            };
        }
        self.hovered = Some(drag.target);
        self.drag = Some(drag);
        true
    }

    /// Termina el arrastre; `Some(true)` si cambió el orden de los anclados.
    pub fn end_drag(&mut self) -> Option<bool> {
        let drag = self.drag.take().filter(|d| d.active)?;
        Some(drag.moved && matches!(drag.target, HoverTarget::PrimaryApp(_)))
    }

    pub fn item_mut(&mut self, target: HoverTarget) -> Option<&mut AppItem> {
        match target {
            HoverTarget::PrimaryApp(i) => self.primary_apps.get_mut(i),
            HoverTarget::OpenApp(i) => self.open_apps.get_mut(i),
            _ => None,
        }
    }
    pub fn window_size(&self, scale: f32) -> (u32, u32) {
        (
            ((self.dock_width() + SHADOW_PAD_X * 2.0) * scale).ceil() as u32,
            ((DOCK_H + SHADOW_PAD_Y * 2.0) * scale).ceil() as u32,
        )
    }
    pub fn step_animations(&mut self, dt: f32) -> bool {
        let mut moving = self.launcher_hover.step(dt) | self.launcher_press.step(dt);
        for app in self.primary_apps.iter_mut().chain(&mut self.open_apps) {
            moving |= app.hover_spring.step(dt) | app.press_spring.step(dt) | app.slide.step(dt);
        }
        moving
    }
    /// El reloj muestra un minuto que ya pasó.
    pub fn clock_outdated(&self) -> bool {
        self.clock.outdated()
    }
    pub fn update_hover(&mut self, target: Option<HoverTarget>) {
        self.hovered = target;
        self.launcher_hover
            .set_target(if target == Some(HoverTarget::Launcher) {
                HOVER_GROW
            } else {
                1.0
            });
        for (i, app) in self.primary_apps.iter_mut().enumerate() {
            app.hover_spring
                .set_target(if target == Some(HoverTarget::PrimaryApp(i)) {
                    HOVER_GROW
                } else {
                    1.0
                });
        }
        for (i, app) in self.open_apps.iter_mut().enumerate() {
            app.hover_spring
                .set_target(if target == Some(HoverTarget::OpenApp(i)) {
                    HOVER_GROW
                } else {
                    1.0
                });
        }
    }
    pub fn hit_test(&self, mx: f32, my: f32, scale: f32) -> Option<HoverTarget> {
        let x = mx / scale - SHADOW_PAD_X;
        let y = my / scale - SHADOW_PAD_Y;
        if !(0.0..DOCK_H).contains(&y) || !(0.0..self.dock_width()).contains(&x) {
            return None;
        }
        if x < self.apps_start() {
            return (!self.compact).then_some(HoverTarget::Launcher);
        }
        let index = ((x - self.apps_start()) / APP_STEP) as usize;
        if index < self.primary_apps.len() {
            Some(HoverTarget::PrimaryApp(index))
        } else if index < self.item_count() {
            Some(HoverTarget::OpenApp(index - self.primary_apps.len()))
        } else if self.compact || x < self.overflow_left() {
            // El hueco entre los programas y la flecha no es de nadie.
            None
        } else if x < self.language_left() {
            Some(HoverTarget::Overflow)
        } else if x < self.status_left() {
            Some(HoverTarget::Language)
        } else if x < self.status_left() + self.status_width() {
            let index = ((x - self.status_left() - STATUS_PAD) / STATUS_STEP).floor();
            let last = self.status.items().len() - 1;
            Some(HoverTarget::Status((index.max(0.0) as usize).min(last)))
        } else if x < self.show_desktop_left() {
            Some(HoverTarget::Clock)
        } else {
            Some(HoverTarget::ShowDesktop)
        }
    }
    pub fn draw(&self, pixmap: &mut Pixmap, scale: f32) {
        let x = SHADOW_PAD_X * scale;
        let y = SHADOW_PAD_Y * scale;
        let cy = y + DOCK_H * scale / 2.0;
        self.draw_fixed(pixmap, scale);
        if !self.compact {
            let launcher_x = x + LAUNCHER_CX * scale;
            // Como las apps: sin fondo, sube y crece al pasar el cursor.
            let lift = hover_lift(self.launcher_hover.value);
            draw_windows_start_icon(
                pixmap,
                launcher_x,
                cy - lift * scale,
                LAUNCHER_ICON * self.launcher_hover.value * self.launcher_press.value,
                scale,
            );
        }
        let dragged = self.drag.filter(|d| d.active);
        let mut lifted = None;
        for (i, (target, app)) in self.items().enumerate() {
            let slot = x + (self.apps_start() + (i as f32 + 0.5) * APP_STEP) * scale;
            let mut cx = slot + app.slide.value * scale;
            if let Some(drag) = dragged.filter(|d| d.target == target) {
                // El icono arrastrado sigue al cursor y se dibuja al final, encima.
                let (first, last) = self.group_span(target);
                let start = self.apps_start();
                let center = |n: usize| x + (start + (n as f32 + 0.5) * APP_STEP) * scale;
                cx = (drag.x - drag.grab).clamp(center(first), center(last));
                lifted = Some((app, cx));
                continue;
            }
            if app.attention {
                draw_attention(pixmap, cx, cy, scale);
            }
            let lift = hover_lift(app.hover_spring.value);
            draw_app_icon(
                pixmap,
                app,
                cx,
                cy - lift * scale,
                ICON_SIZE,
                ICON_SIZE * app.hover_spring.value * app.press_spring.value,
                scale,
            );
            if app.attention {
                draw_attention_dot(pixmap, cx, y + (DOCK_H - 5.0) * scale, scale);
            } else if app.is_running {
                draw_running_dot(pixmap, cx, y + (DOCK_H - 5.0) * scale, scale);
            }
        }
        if let Some((app, cx)) = lifted {
            let cy = cy - HOVER_LIFT * scale;
            draw_app_icon(
                pixmap,
                app,
                cx,
                cy,
                ICON_SIZE,
                ICON_SIZE * HOVER_GROW,
                scale,
            );
        }
    }

    /// Copia en `pixmap` la capa fija; solo la vuelve a dibujar si cambió algo de ella.
    fn draw_fixed(&self, pixmap: &mut Pixmap, scale: f32) {
        let key = FixedKey {
            size: (pixmap.width(), pixmap.height()),
            scale: scale.to_bits(),
            appearance: self.appearance,
            status: self.status,
            keyboard: self.keyboard.clone(),
            items: self.item_count(),
            compact: self.compact,
            hovered: self.hovered.filter(|h| {
                matches!(
                    h,
                    HoverTarget::Overflow
                        | HoverTarget::Language
                        | HoverTarget::Status(_)
                        | HoverTarget::ShowDesktop
                )
            }),
            minute: crate::clock::minute(),
        };
        let mut fixed = self.fixed.borrow_mut();
        if fixed.as_ref().is_none_or(|(k, _)| *k != key) {
            let Some(mut layer) = Pixmap::new(pixmap.width(), pixmap.height()) else {
                self.paint_fixed(pixmap, scale);
                return;
            };
            self.paint_fixed(&mut layer, scale);
            *fixed = Some((key, layer));
        }
        if let Some((_, layer)) = fixed.as_ref() {
            pixmap.data_mut().copy_from_slice(layer.data());
        }
    }

    fn paint_fixed(&self, pixmap: &mut Pixmap, scale: f32) {
        pixmap.fill(tiny_skia::Color::TRANSPARENT);
        let x = SHADOW_PAD_X * scale;
        let y = SHADOW_PAD_Y * scale;
        let cy = y + DOCK_H * scale / 2.0;
        draw_dock(
            pixmap,
            self.appearance,
            x,
            y,
            self.dock_width() * scale,
            DOCK_H * scale,
            scale,
        );
        if self.compact {
            return;
        }
        // Transparente: solo los iconos flotando, sin líneas ni cápsula (los
        // resaltados al pasar el cursor sí se ven).
        let bare = self.appearance == crate::style::Appearance::Clear;
        if !bare {
            draw_divider(pixmap, x + LAUNCHER_DIVIDER * scale, cy, DIVIDER_H, scale);
        }
        // La línea entre los programas y la flecha solo se mueve con el número de apps.
        let divider_x = x + (self.apps_right() + TRAY_GAP / 2.0) * scale;
        if !bare {
            draw_divider(pixmap, divider_x, cy, DIVIDER_H, scale);
        }
        let clock_divider = self.status_left() + self.status_width() + CLOCK_GAP / 2.0;
        if !bare {
            draw_divider(pixmap, x + clock_divider * scale, cy, DIVIDER_H, scale);
        }
        let arrow_x = self.overflow_center(scale);
        // Resaltado estrecho, del ancho de la flecha y sin borde, como el del idioma.
        if self.hovered == Some(HoverTarget::Overflow) {
            crate::apps::fill_rounded(
                pixmap,
                [255, 255, 255, 17],
                arrow_x - (OVERFLOW_W / 2.0 - 3.0) * scale,
                cy - 19.0 * scale,
                (OVERFLOW_W - 6.0) * scale,
                38.0 * scale,
                11.0 * scale,
            );
        }
        draw_chevron(pixmap, arrow_x, cy, scale, false);
        if let Some(keyboard) = &self.keyboard {
            let lx = self.language_center(scale);
            if self.hovered == Some(HoverTarget::Language) {
                crate::apps::fill_rounded(
                    pixmap,
                    [255, 255, 255, 17],
                    lx - (LANGUAGE_W / 2.0 - 2.0) * scale,
                    cy - 19.0 * scale,
                    (LANGUAGE_W - 4.0) * scale,
                    38.0 * scale,
                    11.0 * scale,
                );
            }
            draw_language(pixmap, keyboard, lx, cy, scale);
        }
        // El grupo de estado va en una cápsula; como en Windows, el resaltado la
        // cubre entera: todo abre la configuración rápida.
        let hovered = matches!(self.hovered, Some(HoverTarget::Status(_)));
        let alpha = if hovered { 26 } else { 12 };
        let (left, width) = (
            x + (self.status_left() + 2.0) * scale,
            (self.status_width() - 4.0) * scale,
        );
        // Sin cápsula en Transparente; al pasar el cursor, su resaltado.
        if !bare || hovered {
            crate::apps::fill_rounded(
                pixmap,
                [255, 255, 255, alpha],
                left,
                cy - 19.0 * scale,
                width,
                38.0 * scale,
                19.0 * scale,
            );
            crate::apps::stroke_rounded(
                pixmap,
                [255, 255, 255, alpha + 6],
                left + 0.5 * scale,
                cy - 18.5 * scale,
                width - scale,
                37.0 * scale,
                18.5 * scale,
                scale,
            );
        }
        for (index, item) in self.status.items().into_iter().enumerate() {
            if let (Item::Battery, Some(battery)) = (item, self.status.battery) {
                crate::status::draw_battery(
                    pixmap,
                    battery,
                    self.status_center(index, scale),
                    cy,
                    STATUS_ICON * scale,
                    0.95,
                );
                continue;
            }
            let (glyph, opacity) = self.status.glyph(item);
            draw_icon_aligned(
                pixmap,
                glyph,
                self.status_center(index, scale),
                cy,
                STATUS_ICON * scale,
                opacity * 0.95,
            );
        }
        self.clock.draw(pixmap, self.clock_center(scale), cy, scale);
        // Mostrar escritorio: una línea fina en el extremo, resaltada al pasar por ella.
        let desktop_x = self.show_desktop_center(scale);
        if self.hovered == Some(HoverTarget::ShowDesktop) {
            crate::apps::fill_rounded(
                pixmap,
                [255, 255, 255, 17],
                desktop_x - (SHOW_DESKTOP_W / 2.0 - 2.0) * scale,
                cy - 19.0 * scale,
                (SHOW_DESKTOP_W - 4.0) * scale,
                38.0 * scale,
                5.0 * scale,
            );
        }
        if !bare {
            draw_divider(pixmap, desktop_x, cy, 16.0, scale);
        }
    }
}

/// Cuánto sube un icono que ha crecido hasta `grow`: sale del mismo muelle que lo
/// hace crecer, así sube y rebota a la vez.
fn hover_lift(grow: f32) -> f32 {
    (grow - 1.0) / (HOVER_GROW - 1.0) * HOVER_LIFT
}

/// Idioma en una línea o, si la distribución no es la normal, idioma arriba y
/// distribución debajo, como la barra de Windows.
pub fn draw_language(
    pixmap: &mut Pixmap,
    keyboard: &crate::keyboard::Layout,
    cx: f32,
    cy: f32,
    scale: f32,
) {
    use crate::clock::{draw_text_on_baseline, line_metrics, text_width};
    let size = 11.0 * scale;
    let (ascent, descent) = line_metrics(size);
    let line = ascent + descent;
    let lines: Vec<&str> = std::iter::once(keyboard.language.as_str())
        .chain(keyboard.variant.as_deref())
        .collect();
    let top = cy - line * lines.len() as f32 / 2.0;
    for (index, text) in lines.iter().enumerate() {
        draw_text_on_baseline(
            pixmap,
            text,
            cx - text_width(text, size) / 2.0,
            top + line * index as f32 + ascent,
            size,
            0.95,
        );
    }
}

/// Flecha de la bandeja ("^"), con trazo redondeado.
/// Con `down`, hacia abajo ("v"): en la barra superior el panel se abre debajo.
pub fn draw_chevron(pixmap: &mut Pixmap, cx: f32, cy: f32, scale: f32, down: bool) {
    let half = 5.0 * scale;
    let rise = if down { -2.75 * scale } else { 2.75 * scale };
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(cx - half, cy + rise);
    pb.line_to(cx, cy - rise);
    pb.line_to(cx + half, cy + rise);
    let Some(path) = pb.finish() else {
        return;
    };
    let mut paint = tiny_skia::Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color_rgba8(255, 255, 255, 225);
    let stroke = tiny_skia::Stroke {
        width: 1.5 * scale,
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..Default::default()
    };
    pixmap.stroke_path(
        &path,
        &paint,
        &stroke,
        tiny_skia::Transform::identity(),
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compact_dock_only_has_the_apps() {
        let mut bar = BarState::new();
        bar.compact = true;
        let scale = 1.0;
        let cy = (SHADOW_PAD_Y + DOCK_H / 2.0) * scale;
        let count = bar.item_count();
        assert_eq!(
            bar.dock_width(),
            COMPACT_PAD * 2.0 + count as f32 * APP_STEP
        );
        assert_eq!(bar.hit_test((SHADOW_PAD_X + 2.0) * scale, cy, scale), None);
        if count > 0 {
            let first = (SHADOW_PAD_X + COMPACT_PAD + APP_STEP / 2.0) * scale;
            assert_eq!(
                bar.hit_test(first, cy, scale),
                Some(HoverTarget::PrimaryApp(0))
            );
            assert!(bar
                .focus_order()
                .iter()
                .all(|t| matches!(t, HoverTarget::PrimaryApp(_) | HoverTarget::OpenApp(_))));
        }
        let end = (SHADOW_PAD_X + bar.dock_width() - 1.0) * scale;
        assert_eq!(bar.hit_test(end, cy, scale), None);
    }

    fn item(command: &str) -> AppItem {
        AppItem::new(crate::apps::AppEntry {
            name: command.into(),
            command: command.into(),
        })
    }

    #[test]
    fn fixed_layer_is_reused_until_something_in_it_changes() {
        let mut bar = BarState::new();
        bar.primary_apps = vec![item("a.exe")];
        let (w, h) = bar.window_size(1.0);
        let draw = |bar: &BarState| {
            let mut image = Pixmap::new(w, h).unwrap();
            bar.draw(&mut image, 1.0);
            image
        };
        let first = draw(&bar);
        assert_eq!(first.data(), draw(&bar).data());
        // El resaltado del estado va en la capa fija: tiene que verse.
        bar.hovered = Some(HoverTarget::Status(0));
        let hovered = draw(&bar);
        assert_ne!(first.data(), hovered.data());
        bar.hovered = None;
        assert_eq!(first.data(), draw(&bar).data());
    }

    #[test]
    fn an_app_asking_for_attention_is_drawn_differently() {
        let mut bar = BarState::new();
        bar.primary_apps = vec![item("a.exe")];
        let (w, h) = bar.window_size(1.0);
        let draw = |bar: &BarState| {
            let mut image = Pixmap::new(w, h).unwrap();
            bar.draw(&mut image, 1.0);
            image
        };
        let calm = draw(&bar);
        bar.primary_apps[0].attention = true;
        assert_ne!(calm.data(), draw(&bar).data());
    }

    #[test]
    fn dragging_reorders_within_its_group_only() {
        let mut bar = BarState::new();
        bar.primary_apps = vec![item("a.exe"), item("b.exe"), item("c.exe")];
        bar.open_apps = vec![item("x.exe")];
        let scale = 1.25;
        let center = |i: f32| (SHADOW_PAD_X + APP_START + (i + 0.5) * APP_STEP) * scale;

        // Un movimiento corto no es un arrastre: el clic sigue abriendo el programa.
        bar.begin_drag(HoverTarget::PrimaryApp(0), center(0.0), scale);
        assert!(!bar.drag_to(center(0.0) + 3.0, scale));
        assert_eq!(bar.end_drag(), None);

        // Llevar el primero hasta más allá del grupo lo deja al final de los anclados.
        bar.begin_drag(HoverTarget::PrimaryApp(0), center(0.0), scale);
        assert!(bar.drag_to(center(5.0), scale));
        assert_eq!(bar.end_drag(), Some(true));
        let order: Vec<_> = bar
            .primary_apps
            .iter()
            .map(|a| a.entry.command.as_str())
            .collect();
        assert_eq!(order, ["b.exe", "c.exe", "a.exe"]);
        assert_eq!(bar.open_apps[0].entry.command, "x.exe");

        // Los abiertos no invaden la zona de anclados ni se guardan.
        bar.begin_drag(HoverTarget::OpenApp(0), center(3.0), scale);
        assert!(bar.drag_to(center(0.0), scale));
        assert_eq!(bar.end_drag(), Some(false));
        assert_eq!(bar.primary_apps.len(), 3);
    }
    #[test]
    fn icons_and_clock_remain_clickable_without_overlap() {
        let mut bar = BarState::new();
        // Con varios teclados aparece el indicador de idioma.
        bar.keyboard = Some(crate::keyboard::Layout {
            id: 0x0409,
            language: "ESP".into(),
            variant: Some("INTL".into()),
            name: "Español (España) · Estados Unidos-Internacional".into(),
        });
        // Con batería el grupo de estado tiene tres iconos.
        bar.status.battery = Some(crate::status::Battery {
            percent: 80,
            plugged: false,
            charging: false,
            remaining: None,
        });
        for count in [0, 1, 5, 24] {
            bar.primary_apps = (0..count)
                .map(|_| {
                    AppItem::new(crate::apps::AppEntry {
                        name: "Test".into(),
                        command: "missing.exe".into(),
                    })
                })
                .collect();
            for scale in [1.0, 1.5, 2.0] {
                let cy = (SHADOW_PAD_Y + DOCK_H / 2.0) * scale;
                for i in 0..count {
                    assert_eq!(
                        bar.hit_test(
                            (SHADOW_PAD_X + APP_START + (i as f32 + 0.5) * APP_STEP) * scale,
                            cy,
                            scale
                        ),
                        Some(HoverTarget::PrimaryApp(i))
                    );
                }
                assert_eq!(
                    bar.hit_test(bar.clock_center(scale), cy, scale),
                    Some(HoverTarget::Clock)
                );
                assert_eq!(
                    bar.hit_test(bar.overflow_center(scale), cy, scale),
                    Some(HoverTarget::Overflow)
                );
                if bar.keyboard.is_some() {
                    assert_eq!(
                        bar.hit_test(bar.language_center(scale), cy, scale),
                        Some(HoverTarget::Language)
                    );
                }
                for index in 0..bar.status.items().len() {
                    assert_eq!(
                        bar.hit_test(bar.status_center(index, scale), cy, scale),
                        Some(HoverTarget::Status(index))
                    );
                }
                assert_eq!(bar.hit_test(-1.0, cy, scale), None);
            }
        }
    }
}
