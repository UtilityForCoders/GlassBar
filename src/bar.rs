//! Dock compacto: Inicio, programas anclados, estado del sistema y hora.
use crate::apps::{default_apps, draw_app_icon, draw_windows_start_icon, AppItem};
use crate::clock::{draw_icon_aligned, ClockRenderer};
use crate::glass::{draw_hover, draw_running_dot, draw_transparent_dock};
use crate::spring::Spring;
use crate::status::{Item, Status};
use tiny_skia::Pixmap;

pub const DOCK_H: f32 = 52.0;
pub const SHADOW_PAD_X: f32 = 2.0;
pub const SHADOW_PAD_Y: f32 = 2.0;
pub const BOTTOM_MARGIN: f32 = 8.0;
const APP_START: f32 = 48.0;
const APP_STEP: f32 = 46.0;
const CLOCK_W: f32 = 78.0;
const ICON_SIZE: f32 = 28.0;
const OVERFLOW_W: f32 = 30.0;
/// Grupo de red, volumen y batería: margen a cada lado y ancho de cada icono.
const STATUS_PAD: f32 = 6.0;
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
    /// Icono del grupo de estado (red, volumen o batería), por su posición.
    Status(usize),
    Clock,
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
    clock: ClockRenderer,
}

impl BarState {
    pub fn new() -> Self {
        Self {
            primary_apps: crate::pins::load().unwrap_or_else(default_apps),
            open_apps: Vec::new(),
            drag: None,
            launcher_hover: Spring::new(1.0, 300.0, 32.0),
            launcher_press: Spring::new(1.0, 340.0, 32.0),
            hovered: None,
            pressed: None,
            status: Status::default(),
            clock: ClockRenderer::new(),
        }
    }
    pub fn dock_width(&self) -> f32 {
        APP_START + self.item_count() as f32 * APP_STEP + OVERFLOW_W + self.status_width() + CLOCK_W
    }
    fn status_width(&self) -> f32 {
        STATUS_PAD * 2.0 + self.status.items().len() as f32 * STATUS_STEP
    }
    /// Borde izquierdo del grupo de estado, en unidades del dock.
    fn status_left(&self) -> f32 {
        APP_START + self.item_count() as f32 * APP_STEP + OVERFLOW_W
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
        (SHADOW_PAD_X + APP_START + self.item_count() as f32 * APP_STEP + OVERFLOW_W / 2.0) * scale
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
        if let HoverTarget::Status(index) = target {
            let item = self.status_item(index)?;
            return Some((self.status.label(item), self.status_center(index, scale)));
        }
        let (i, (_, app)) = self.items().enumerate().find(|(_, (t, _))| *t == target)?;
        let cx = (SHADOW_PAD_X + APP_START + (i as f32 + 0.5) * APP_STEP) * scale;
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
        let (offset, group, from) = match drag.target {
            HoverTarget::PrimaryApp(i) => (0, &mut self.primary_apps, i),
            HoverTarget::OpenApp(i) => (self.primary_apps.len(), &mut self.open_apps, i),
            _ => return false,
        };
        let center = x - drag.grab;
        let slot = ((center / scale - SHADOW_PAD_X - APP_START) / APP_STEP).floor() as isize;
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
    pub fn update_hover(&mut self, target: Option<HoverTarget>) {
        self.hovered = target;
        self.launcher_hover
            .set_target(if target == Some(HoverTarget::Launcher) {
                1.04
            } else {
                1.0
            });
        for (i, app) in self.primary_apps.iter_mut().enumerate() {
            app.hover_spring
                .set_target(if target == Some(HoverTarget::PrimaryApp(i)) {
                    1.06
                } else {
                    1.0
                });
        }
        for (i, app) in self.open_apps.iter_mut().enumerate() {
            app.hover_spring
                .set_target(if target == Some(HoverTarget::OpenApp(i)) {
                    1.06
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
        if x < APP_START {
            return Some(HoverTarget::Launcher);
        }
        let index = ((x - APP_START) / APP_STEP) as usize;
        if index < self.primary_apps.len() {
            Some(HoverTarget::PrimaryApp(index))
        } else if index < self.item_count() {
            Some(HoverTarget::OpenApp(index - self.primary_apps.len()))
        } else if x < self.status_left() {
            Some(HoverTarget::Overflow)
        } else if x < self.status_left() + self.status_width() {
            let index = ((x - self.status_left() - STATUS_PAD) / STATUS_STEP).floor();
            let last = self.status.items().len() - 1;
            Some(HoverTarget::Status((index.max(0.0) as usize).min(last)))
        } else {
            Some(HoverTarget::Clock)
        }
    }
    pub fn draw(&self, pixmap: &mut Pixmap, scale: f32) {
        pixmap.fill(tiny_skia::Color::TRANSPARENT);
        let x = SHADOW_PAD_X * scale;
        let y = SHADOW_PAD_Y * scale;
        let cy = y + DOCK_H * scale / 2.0;
        draw_transparent_dock(
            pixmap,
            x,
            y,
            self.dock_width() * scale,
            DOCK_H * scale,
            scale,
        );
        let launcher_x = x + APP_START * scale / 2.0;
        if self.hovered == Some(HoverTarget::Launcher) {
            draw_hover(pixmap, launcher_x, cy, scale);
        }
        draw_windows_start_icon(
            pixmap,
            launcher_x,
            cy,
            18.0 * self.launcher_hover.value * self.launcher_press.value,
            scale,
        );
        let dragged = self.drag.filter(|d| d.active);
        let mut lifted = None;
        for (i, (target, app)) in self.items().enumerate() {
            let slot = x + (APP_START + (i as f32 + 0.5) * APP_STEP) * scale;
            let mut cx = slot + app.slide.value * scale;
            if let Some(drag) = dragged.filter(|d| d.target == target) {
                // El icono arrastrado sigue al cursor y se dibuja al final, encima.
                let (first, last) = self.group_span(target);
                let center = |n: usize| x + (APP_START + (n as f32 + 0.5) * APP_STEP) * scale;
                cx = (drag.x - drag.grab).clamp(center(first), center(last));
                lifted = Some((app, cx));
                continue;
            }
            if self.hovered == Some(target) && dragged.is_none() {
                draw_hover(pixmap, cx, cy, scale);
            }
            draw_app_icon(
                pixmap,
                app,
                cx,
                cy,
                ICON_SIZE,
                ICON_SIZE * app.hover_spring.value * app.press_spring.value,
                scale,
            );
            if app.is_running {
                draw_running_dot(pixmap, cx, y + (DOCK_H - 5.0) * scale, scale);
            }
        }
        if let Some((app, cx)) = lifted {
            draw_hover(pixmap, cx, cy, scale);
            draw_app_icon(pixmap, app, cx, cy, ICON_SIZE, ICON_SIZE * 1.1, scale);
        }
        let arrow_x = self.overflow_center(scale);
        if self.hovered == Some(HoverTarget::Overflow) {
            draw_hover(pixmap, arrow_x, cy, scale);
        }
        draw_chevron(pixmap, arrow_x, cy, scale);
        // Como en Windows, el resaltado cubre el grupo entero: todo abre la configuración rápida.
        if matches!(self.hovered, Some(HoverTarget::Status(_))) {
            crate::apps::fill_rounded(
                pixmap,
                [255, 255, 255, 17],
                x + (self.status_left() + 2.0) * scale,
                cy - 19.0 * scale,
                (self.status_width() - 4.0) * scale,
                38.0 * scale,
                10.0 * scale,
            );
        }
        for (index, item) in self.status.items().into_iter().enumerate() {
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
        self.clock.draw(
            pixmap,
            x + (self.dock_width() - CLOCK_W / 2.0) * scale,
            cy,
            scale,
        );
    }
}

/// Flecha hacia arriba ("^") de la bandeja, con trazo redondeado.
fn draw_chevron(pixmap: &mut Pixmap, cx: f32, cy: f32, scale: f32) {
    let (half, rise) = (5.0 * scale, 2.75 * scale);
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

    fn item(command: &str) -> AppItem {
        AppItem::new(crate::apps::AppEntry {
            name: command.into(),
            command: command.into(),
        })
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
        // Con batería el grupo de estado tiene tres iconos.
        bar.status.battery = Some(crate::status::Battery {
            percent: 80,
            plugged: false,
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
                    bar.hit_test(
                        (SHADOW_PAD_X + bar.dock_width() - CLOCK_W / 2.0) * scale,
                        cy,
                        scale
                    ),
                    Some(HoverTarget::Clock)
                );
                assert_eq!(
                    bar.hit_test(bar.overflow_center(scale), cy, scale),
                    Some(HoverTarget::Overflow)
                );
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
