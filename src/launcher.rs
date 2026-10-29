//! Lanzador transparente propio; ninguna interacción abre el menú Inicio nativo.
use crate::apps::{fill_rounded, stroke_rounded, AppEntry};
use crate::clock::{draw_text, fit_text};
use std::collections::HashMap;
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

pub const WIDTH: f32 = 520.0;
pub const HEIGHT: f32 = 572.0;
const PAGE_SIZE: usize = 12;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Recent(usize),
    App(usize),
    Previous,
    Next,
    Power,
    Cancel,
    ConfirmShutdown,
    ClearSearch,
}
pub enum Action {
    Launch(AppEntry),
    Shutdown,
}
pub struct LauncherState {
    pub apps: Vec<AppEntry>,
    pub recent: Vec<AppEntry>,
    pub query: String,
    pub page: usize,
    pub hovered: Option<Hit>,
    pub confirming_shutdown: bool,
    pub selection: Option<usize>,
    pub caret: usize,
    pub select_all: bool,
    icons: HashMap<String, Option<Pixmap>>,
}
impl LauncherState {
    pub fn new(pinned: &[AppEntry]) -> Self {
        Self {
            apps: crate::catalog::installed(pinned),
            recent: crate::catalog::recent(),
            query: String::new(),
            page: 0,
            hovered: None,
            confirming_shutdown: false,
            selection: None,
            caret: 0,
            select_all: false,
            icons: HashMap::new(),
        }
    }
    fn filtered(&self) -> Vec<usize> {
        crate::catalog::search(&self.apps, &self.query)
    }
    fn grid_top(&self) -> f32 {
        if self.query.trim().is_empty() {
            248.0
        } else {
            116.0
        }
    }
    fn byte_at(&self, index: usize) -> usize {
        self.query
            .char_indices()
            .nth(index)
            .map(|(i, _)| i)
            .unwrap_or(self.query.len())
    }
    pub fn insert_text(&mut self, text: &str) {
        if self.confirming_shutdown {
            return;
        }
        if self.select_all {
            self.query.clear();
            self.caret = 0;
            self.select_all = false;
        }
        self.caret = self.caret.min(self.query.chars().count());
        let text: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take(100usize.saturating_sub(self.query.chars().count()))
            .collect();
        self.query.insert_str(self.byte_at(self.caret), &text);
        self.caret += text.chars().count();
        self.reset_results();
    }
    fn reset_results(&mut self) {
        self.page = 0;
        self.hovered = None;
        self.selection = None;
    }
    pub fn move_caret(&mut self, delta: isize) {
        self.caret = self
            .caret
            .saturating_add_signed(delta)
            .min(self.query.chars().count());
        self.select_all = false;
    }
    pub fn delete_forward(&mut self) {
        if self.select_all {
            self.query.clear();
            self.caret = 0;
            self.select_all = false;
        } else if self.caret < self.query.chars().count() {
            self.query.remove(self.byte_at(self.caret));
        }
        self.reset_results();
    }
    pub fn move_selection(&mut self, delta: isize) {
        let hits = self.filtered();
        if hits.is_empty() {
            return;
        }
        let index = self
            .selection
            .and_then(|id| hits.iter().position(|&i| i == id))
            .map(|pos| pos.saturating_add_signed(delta).min(hits.len() - 1))
            .unwrap_or(0);
        self.selection = Some(hits[index]);
        self.page = index / PAGE_SIZE;
    }
    pub fn page_count(&self) -> usize {
        self.filtered().len().div_ceil(PAGE_SIZE).max(1)
    }
    pub fn change_page(&mut self, forward: bool) {
        self.page = if forward {
            (self.page + 1).min(self.page_count() - 1)
        } else {
            self.page.saturating_sub(1)
        };
        self.hovered = None;
        self.selection = None;
    }
    pub fn type_char(&mut self, ch: char) {
        if self.confirming_shutdown {
            return;
        }
        if ch == '\u{8}' {
            if self.select_all {
                self.query.clear();
                self.caret = 0;
                self.select_all = false;
            } else if self.caret > 0 {
                self.caret -= 1;
                self.query.remove(self.byte_at(self.caret));
            }
            self.reset_results();
        } else if !ch.is_control() {
            self.insert_text(&ch.to_string());
        }
    }
    pub fn activate(&mut self, hit: Hit) -> Option<Action> {
        if self.confirming_shutdown {
            match hit {
                Hit::Cancel => self.confirming_shutdown = false,
                Hit::ConfirmShutdown => {
                    self.confirming_shutdown = false;
                    return Some(Action::Shutdown);
                }
                _ => {}
            }
            return None;
        }
        match hit {
            Hit::App(i) => return self.apps.get(i).cloned().map(Action::Launch),
            Hit::Recent(i) => return self.recent.get(i).cloned().map(Action::Launch),
            Hit::ClearSearch => {
                self.query.clear();
                self.caret = 0;
                self.select_all = false;
                self.reset_results();
            }
            Hit::Previous => self.change_page(false),
            Hit::Next => self.change_page(true),
            Hit::Power => {
                self.confirming_shutdown = true;
                self.hovered = None;
            }
            _ => {}
        }
        None
    }
    pub fn first_match(&self) -> Option<Hit> {
        self.selection
            .filter(|i| self.filtered().contains(i))
            .map(Hit::App)
            .or_else(|| {
                self.filtered()
                    .get(self.page * PAGE_SIZE)
                    .copied()
                    .map(Hit::App)
            })
    }
    pub fn hit_test(&self, x: f32, y: f32) -> Option<Hit> {
        let within = |left: f32, top: f32, width: f32, height: f32| {
            x >= left && x < left + width && y >= top && y < top + height
        };
        if self.confirming_shutdown {
            if within(128.0, 316.0, 120.0, 38.0) {
                return Some(Hit::Cancel);
            }
            if within(272.0, 316.0, 120.0, 38.0) {
                return Some(Hit::ConfirmShutdown);
            }
            return None;
        }
        if !self.query.is_empty() && within(462.0, 24.0, 34.0, 44.0) {
            return Some(Hit::ClearSearch);
        }
        for i in 0..if self.query.trim().is_empty() {
            self.recent.len().min(4)
        } else {
            0
        } {
            if within(24.0 + i as f32 * 118.0, 116.0, 112.0, 78.0) {
                return Some(Hit::Recent(i));
            }
        }
        for (slot, &index) in self
            .filtered()
            .iter()
            .skip(self.page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .enumerate()
        {
            if within(
                24.0 + (slot % 4) as f32 * 118.0,
                self.grid_top() + (slot / 4) as f32 * 76.0,
                112.0,
                72.0,
            ) {
                return Some(Hit::App(index));
            }
        }
        if within(24.0, 516.0, 36.0, 36.0) {
            return Some(Hit::Previous);
        }
        if within(68.0, 516.0, 36.0, 36.0) {
            return Some(Hit::Next);
        }
        if within(400.0, 516.0, 96.0, 36.0) {
            return Some(Hit::Power);
        }
        None
    }
    pub fn draw(&mut self, image: &mut Pixmap, scale: f32) {
        image.fill(tiny_skia::Color::TRANSPARENT);
        fill_rounded(
            image,
            [23, 28, 36, 224],
            scale,
            scale,
            (WIDTH - 2.0) * scale,
            (HEIGHT - 2.0) * scale,
            22.0 * scale,
        );
        stroke_rounded(
            image,
            [255, 255, 255, 42],
            scale,
            scale,
            (WIDTH - 2.0) * scale,
            (HEIGHT - 2.0) * scale,
            22.0 * scale,
            scale * 0.75,
        );
        if self.confirming_shutdown {
            self.text(image, "¿Apagar el equipo?", 142.0, 228.0, 21.0, scale, 1.0);
            self.text(
                image,
                "Guarda tus cambios antes de continuar.",
                128.0,
                269.0,
                13.0,
                scale,
                0.7,
            );
            self.button(image, Hit::Cancel, "Cancelar", 128.0, 316.0, 120.0, scale);
            self.button(
                image,
                Hit::ConfirmShutdown,
                "Apagar",
                272.0,
                316.0,
                120.0,
                scale,
            );
            return;
        }
        fill_rounded(
            image,
            [255, 255, 255, 19],
            24.0 * scale,
            24.0 * scale,
            472.0 * scale,
            44.0 * scale,
            12.0 * scale,
        );
        let search = if self.query.is_empty() {
            "Buscar aplicaciones…".into()
        } else {
            fit_text(&self.query, 14.0 * scale, 410.0 * scale)
        };
        self.text(
            image,
            &search,
            38.0,
            38.0,
            14.0,
            scale,
            if self.query.is_empty() { 0.6 } else { 1.0 },
        );
        if !self.select_all {
            let prefix: String = self.query.chars().take(self.caret).collect();
            let cursor_x =
                39.0 * scale + crate::clock::text_width(&prefix, 14.0 * scale).min(404.0 * scale);
            fill_rounded(
                image,
                [235, 240, 250, 180],
                cursor_x,
                36.0 * scale,
                scale.max(1.0),
                19.0 * scale,
                0.0,
            );
        }
        if !self.query.is_empty() {
            self.text(image, "×", 474.0, 38.0, 16.0, scale, 0.8);
            if self.select_all {
                fill_rounded(
                    image,
                    [90, 160, 245, 45],
                    38.0 * scale,
                    32.0 * scale,
                    410.0 * scale,
                    28.0 * scale,
                    4.0 * scale,
                );
            }
        }
        if self.query.trim().is_empty() {
            self.text(image, "Recientes", 24.0, 92.0, 13.0, scale, 0.65);
            if self.recent.is_empty() {
                self.text(
                    image,
                    "Aquí aparecerán las apps que abras con GlassBar.",
                    24.0,
                    142.0,
                    13.0,
                    scale,
                    0.55,
                );
            }
            for (i, entry) in self.recent.clone().into_iter().take(4).enumerate() {
                self.item(
                    image,
                    &entry,
                    Hit::Recent(i),
                    24.0 + i as f32 * 118.0,
                    116.0,
                    scale,
                );
            }

            self.text(image, "Aplicaciones", 24.0, 219.0, 13.0, scale, 0.65);
        } else {
            self.text(
                image,
                &format!(
                    "{} {}",
                    self.filtered().len(),
                    if self.filtered().len() == 1 {
                        "resultado"
                    } else {
                        "resultados"
                    }
                ),
                24.0,
                92.0,
                13.0,
                scale,
                0.7,
            );
        }
        let indices: Vec<usize> = self
            .filtered()
            .into_iter()
            .skip(self.page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .collect();
        if indices.is_empty() {
            self.text(
                image,
                "No se encontraron aplicaciones.",
                24.0,
                self.grid_top() + 28.0,
                14.0,
                scale,
                0.6,
            );
        }
        for (slot, i) in indices.into_iter().enumerate() {
            let entry = self.apps[i].clone();
            self.item(
                image,
                &entry,
                Hit::App(i),
                24.0 + (slot % 4) as f32 * 118.0,
                self.grid_top() + (slot / 4) as f32 * 76.0,
                scale,
            );
        }
        fill_rounded(
            image,
            [255, 255, 255, 20],
            24.0 * scale,
            496.0 * scale,
            472.0 * scale,
            scale,
            0.0,
        );
        self.button(image, Hit::Previous, "‹", 24.0, 516.0, 36.0, scale);
        self.button(image, Hit::Next, "›", 68.0, 516.0, 36.0, scale);
        self.text(
            image,
            &format!("{} / {}", self.page + 1, self.page_count()),
            118.0,
            529.0,
            12.0,
            scale,
            0.6,
        );
        self.button(image, Hit::Power, "Apagar", 400.0, 516.0, 96.0, scale);
    }
    fn text(
        &self,
        image: &mut Pixmap,
        value: &str,
        x: f32,
        y: f32,
        size: f32,
        scale: f32,
        opacity: f32,
    ) {
        draw_text(image, value, x * scale, y * scale, size * scale, opacity);
    }
    fn button(
        &self,
        image: &mut Pixmap,
        hit: Hit,
        label: &str,
        x: f32,
        y: f32,
        width: f32,
        scale: f32,
    ) {
        fill_rounded(
            image,
            [
                255,
                255,
                255,
                if self.hovered == Some(hit) { 32 } else { 13 },
            ],
            x * scale,
            y * scale,
            width * scale,
            38.0 * scale,
            10.0 * scale,
        );
        self.text(
            image,
            label,
            x + if width > 40.0 { 26.0 } else { 12.0 },
            y + 12.0,
            13.0,
            scale,
            0.9,
        );
    }
    fn item(&mut self, image: &mut Pixmap, entry: &AppEntry, hit: Hit, x: f32, y: f32, scale: f32) {
        if self.hovered == Some(hit) || matches!(hit, Hit::App(i) if self.selection == Some(i)) {
            fill_rounded(
                image,
                [255, 255, 255, 20],
                x * scale,
                y * scale,
                112.0 * scale,
                72.0 * scale,
                12.0 * scale,
            );
        }
        let icon = self
            .icons
            .entry(entry.command.clone())
            .or_insert_with(|| crate::icons::load(&entry.command));
        if let Some(icon) = icon {
            let size = (32.0 * scale).round();
            let transform =
                Transform::from_scale(size / icon.width() as f32, size / icon.height() as f32)
                    .post_translate(((x + 40.0) * scale).round(), ((y + 6.0) * scale).round());
            image.draw_pixmap(
                0,
                0,
                icon.as_ref(),
                &PixmapPaint {
                    quality: FilterQuality::Bicubic,
                    ..Default::default()
                },
                transform,
                None,
            );
        }
        let label = fit_text(&entry.name, 11.5 * scale, 104.0 * scale);
        let label_x = x + (112.0 - crate::clock::text_width(&label, 11.5 * scale) / scale) / 2.0;
        self.text(image, &label, label_x, y + 49.0, 11.5, scale, 0.9);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> LauncherState {
        LauncherState {
            apps: (0..25)
                .map(|i| AppEntry {
                    name: format!("Editor {i:02}"),
                    command: format!("{i}.exe"),
                })
                .collect(),
            recent: vec![],
            query: String::new(),
            page: 0,
            hovered: None,
            confirming_shutdown: false,
            selection: None,
            caret: 0,
            select_all: false,
            icons: HashMap::new(),
        }
    }
    #[test]
    fn search_and_paging_target_the_displayed_apps() {
        let mut menu = state();
        assert_eq!(menu.page_count(), 3);
        menu.change_page(true);
        assert_eq!(menu.hit_test(60.0, 270.0), Some(Hit::App(12)));
        for ch in "Editor 24".chars() {
            menu.type_char(ch);
        }
        assert_eq!(menu.page, 0);
        assert_eq!(menu.page_count(), 1);
        assert_eq!(menu.first_match(), Some(Hit::App(24)));
    }
    #[test]
    fn search_edits_unicode_at_the_caret_and_replaces_selection() {
        let mut menu = state();
        menu.insert_text("Diseño");
        menu.move_caret(-1);
        menu.type_char('s');
        assert_eq!(menu.query, "Diseñso");
        menu.type_char('\u{8}');
        assert_eq!(menu.query, "Diseño");
        menu.delete_forward();
        assert_eq!(menu.query, "Diseñ");
        menu.select_all = true;
        menu.insert_text("Spotify");
        assert_eq!(menu.query, "Spotify");
        assert_eq!(menu.caret, 7);
        menu.select_all = true;
        menu.type_char('\u{8}');
        assert!(menu.query.is_empty());
    }
    #[test]
    fn search_finds_and_launches_store_entry_without_a_shortcut() {
        let mut menu = state();
        menu.apps.push(AppEntry {
            name: "Spotify".into(),
            command: "shell:AppsFolder\\SpotifyAB.Test!Spotify".into(),
        });
        menu.insert_text("  SPOTIFY ");
        let hit = menu.first_match().unwrap();
        assert_eq!(menu.hit_test(60.0, 140.0), Some(hit));
        assert!(
            matches!(menu.activate(hit), Some(Action::Launch(entry)) if entry.command.starts_with("shell:AppsFolder\\"))
        );
        menu.activate(Hit::ClearSearch);
        assert!(menu.query.is_empty());
    }
    #[test]
    fn changing_page_does_not_launch_a_hidden_selection() {
        let mut menu = state();
        menu.move_selection(4);
        menu.change_page(true);
        assert_eq!(menu.first_match(), Some(Hit::App(12)));
    }
    #[test]
    fn launcher_background_remains_mostly_opaque() {
        let mut menu = state();
        menu.apps.clear();
        let mut image = Pixmap::new(WIDTH as u32, HEIGHT as u32).unwrap();
        menu.draw(&mut image, 1.0);
        assert_eq!(image.pixel(260, 480).unwrap().alpha(), 224);
        assert_eq!(image.pixel(0, 0).unwrap().alpha(), 0);
    }
    #[test]
    fn shutdown_requires_the_separate_confirmation() {
        let mut menu = state();
        assert!(menu.activate(Hit::ConfirmShutdown).is_none());
        assert!(menu.activate(Hit::Power).is_none());
        assert!(menu.confirming_shutdown);
        assert!(menu.hit_test(450.0, 530.0).is_none());
        menu.activate(Hit::Cancel);
        assert!(!menu.confirming_shutdown);
        menu.activate(Hit::Power);
        assert!(matches!(
            menu.activate(Hit::ConfirmShutdown),
            Some(Action::Shutdown)
        ));
    }
}
