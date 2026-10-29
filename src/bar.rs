//! Dock compacto: Inicio, programas anclados y hora.
use crate::apps::{default_apps, draw_app_icon, draw_windows_start_icon, AppItem};
use crate::clock::ClockRenderer;
use crate::glass::{draw_hover, draw_running_dot, draw_transparent_dock};
use crate::spring::Spring;
use tiny_skia::Pixmap;

pub const DOCK_H: f32 = 52.0;
pub const SHADOW_PAD_X: f32 = 2.0;
pub const SHADOW_PAD_Y: f32 = 2.0;
pub const BOTTOM_MARGIN: f32 = 8.0;
const APP_START: f32 = 48.0;
const APP_STEP: f32 = 46.0;
const CLOCK_W: f32 = 78.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverTarget {
    Launcher,
    PrimaryApp(usize),
    Clock,
}

pub struct BarState {
    pub primary_apps: Vec<AppItem>,
    pub launcher_hover: Spring,
    pub launcher_press: Spring,
    pub hovered: Option<HoverTarget>,
    pub pressed: Option<HoverTarget>,
    clock: ClockRenderer,
}

impl BarState {
    pub fn new() -> Self {
        Self {
            primary_apps: crate::pins::load().unwrap_or_else(default_apps),
            launcher_hover: Spring::new(1.0, 300.0, 32.0),
            launcher_press: Spring::new(1.0, 340.0, 32.0),
            hovered: None,
            pressed: None,
            clock: ClockRenderer::new(),
        }
    }
    pub fn dock_width(&self) -> f32 {
        APP_START + self.primary_apps.len() as f32 * APP_STEP + CLOCK_W
    }
    pub fn window_size(&self, scale: f32) -> (u32, u32) {
        (
            ((self.dock_width() + SHADOW_PAD_X * 2.0) * scale).ceil() as u32,
            ((DOCK_H + SHADOW_PAD_Y * 2.0) * scale).ceil() as u32,
        )
    }
    pub fn step_animations(&mut self, dt: f32) -> bool {
        let mut moving = self.launcher_hover.step(dt) | self.launcher_press.step(dt);
        for app in &mut self.primary_apps {
            moving |= app.hover_spring.step(dt) | app.press_spring.step(dt);
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
        for (i, app) in self.primary_apps.iter().enumerate() {
            let cx = x + (APP_START + (i as f32 + 0.5) * APP_STEP) * scale;
            if self.hovered == Some(HoverTarget::PrimaryApp(i)) {
                draw_hover(pixmap, cx, cy, scale);
            }
            draw_app_icon(
                pixmap,
                app,
                cx,
                cy,
                28.0 * app.hover_spring.value * app.press_spring.value,
                scale,
            );
            if app.is_running {
                draw_running_dot(pixmap, cx, y + (DOCK_H - 5.0) * scale, scale);
            }
        }
        self.clock.draw(
            pixmap,
            x + (self.dock_width() - CLOCK_W / 2.0) * scale,
            cy,
            scale,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn icons_and_clock_remain_clickable_without_overlap() {
        let mut bar = BarState::new();
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
                assert_eq!(bar.hit_test(-1.0, cy, scale), None);
            }
        }
    }
}
