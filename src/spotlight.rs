//! Buscador rápido al estilo de Spotlight de macOS: Alt+Espacio abre una barra
//! centrada en la parte alta de la pantalla; al escribir aparecen las apps que
//! coinciden (y el resultado de una operación, la Configuración, archivos y la
//! búsqueda web) y Enter abre la primera. Las apps que más se usan van delante.
use crate::apps::{fill_rounded, stroke_rounded, AppEntry};
use crate::catalog::search_key;
use crate::clock::{draw_icon, draw_text_on_baseline, fit_text, line_metrics, text_width};
use crate::extras::{Extra, Kind};
use crate::launcher::IconLoader;
use crate::textfield::TextField;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

pub const WIDTH: f32 = 680.0;
/// Alto del campo de búsqueda; sin resultados, el panel es solo esto.
pub const FIELD_H: f32 = 60.0;
const ROW_H: f32 = 44.0;
const LIST_PAD: f32 = 6.0;
const MAX_ROWS: usize = 8;
const MAX_APPS: usize = 6;
const RADIUS: f32 = 14.0;
const TEXT_X: f32 = 52.0;
const TEXT_W: f32 = WIDTH - TEXT_X - 20.0;
const FONT: f32 = 20.0;
const ROW_FONT: f32 = 14.0;
const DETAIL_FONT: f32 = 12.0;
const ICON: f32 = 28.0;
/// El catálogo (lento de leer) se reutiliza entre aperturas durante este tiempo.
const CATALOG_TTL: Duration = Duration::from_secs(120);
/// Lo que pesa haber abierto una app desde GlassBar hace poco frente a cada
/// apertura que cuenta Windows.
const HISTORY_BONUS: u64 = 10;

/// Un resultado: una app del catálogo u otro (operación, ajuste, archivo, web).
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    App(AppEntry),
    Extra(Extra),
}
impl Item {
    fn entry(&self) -> &AppEntry {
        match self {
            Item::App(entry) => entry,
            Item::Extra(extra) => &extra.entry,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Action {
    /// Abre una app y la apunta en el historial (sube en las próximas búsquedas).
    Launch(AppEntry),
    /// Abre un archivo, un ajuste o la búsqueda web.
    Open(AppEntry),
    /// Copia el resultado de una operación.
    Copy(String),
    /// Ctrl+Enter: muestra la app o el archivo en el Explorador.
    Reveal(AppEntry),
}

/// Cómo coincide un nombre con lo escrito; cuanto menor, mejor.
fn tier(name: &str, query: &str) -> Option<u8> {
    let name = search_key(name);
    let words: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || matches!(c, '-' | '_' | '.' | '(' | ')'))
        .filter(|w| !w.is_empty())
        .collect();
    if name == query {
        return Some(0);
    }
    if name.starts_with(query) {
        return Some(1);
    }
    if words.iter().any(|w| w.starts_with(query)) {
        return Some(2);
    }
    // Iniciales: "vsc" encuentra "Visual Studio Code".
    if !query.contains(' ') && query.chars().count() >= 2 {
        let initials: String = words.iter().filter_map(|w| w.chars().next()).collect();
        if initials.starts_with(query) {
            return Some(3);
        }
    }
    // Todas las palabras, en cualquier orden y en cualquier sitio del nombre.
    if query.split_whitespace().all(|q| name.contains(q)) {
        return Some(4);
    }
    None
}

/// Índices de `apps` que coinciden con `query`, de mejor a peor: primero cómo
/// coinciden y, a igualdad, las más usadas (`score`).
pub fn rank(apps: &[AppEntry], query: &str, score: impl Fn(&AppEntry) -> u64) -> Vec<usize> {
    let query = search_key(query.trim());
    if query.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<_> = apps
        .iter()
        .enumerate()
        .filter_map(|(i, entry)| {
            let tier = tier(&entry.name, &query)?;
            Some((
                tier,
                std::cmp::Reverse(score(entry)),
                search_key(&entry.name),
                i,
            ))
        })
        .collect();
    hits.sort();
    hits.into_iter().map(|(.., i)| i).collect()
}

/// Lista que se muestra: la operación, las apps, los ajustes y archivos, y la
/// búsqueda web siempre al final.
fn compose(apps: Vec<AppEntry>, extras: Vec<Extra>) -> Vec<Item> {
    let (web, extras): (Vec<_>, Vec<_>) = extras.into_iter().partition(|e| e.kind == Kind::Web);
    let (calc, others): (Vec<_>, Vec<_>) = extras.into_iter().partition(|e| e.kind == Kind::Calc);
    let mut items: Vec<Item> = calc.into_iter().map(Item::Extra).collect();
    items.extend(apps.into_iter().take(MAX_APPS).map(Item::App));
    items.extend(others.into_iter().map(Item::Extra));
    items.truncate(MAX_ROWS - web.len().min(1));
    items.extend(web.into_iter().take(1).map(Item::Extra));
    items
}

pub struct SpotlightState {
    apps: Vec<AppEntry>,
    files: Vec<AppEntry>,
    /// Uso de cada app por su orden en minúsculas, para ordenar los resultados.
    scores: HashMap<String, u64>,
    pub query: TextField,
    results: Vec<Item>,
    selected: usize,
    caret_on: bool,
    /// Desplazamiento del texto cuando no cabe, para que el cursor siempre se vea.
    offset: f32,
    icons: HashMap<String, Option<Pixmap>>,
    requested: std::collections::HashSet<String>,
    loader: Option<IconLoader>,
    loaded: Instant,
}

impl SpotlightState {
    pub fn new(pinned: &[AppEntry]) -> Self {
        Self::with_apps(crate::catalog::installed(pinned))
    }
    fn with_apps(apps: Vec<AppEntry>) -> Self {
        let mut state = Self {
            apps,
            files: Vec::new(),
            scores: HashMap::new(),
            query: TextField::default(),
            results: Vec::new(),
            selected: 0,
            caret_on: true,
            offset: 0.0,
            icons: HashMap::new(),
            requested: Default::default(),
            loader: None,
            loaded: Instant::now(),
        };
        state.scores = state.read_scores();
        state
    }
    /// Prepara una apertura: el campo vacío, el uso al día y, si es viejo, el
    /// catálogo otra vez.
    pub fn refresh(&mut self, pinned: &[AppEntry]) {
        if self.loaded.elapsed() > CATALOG_TTL {
            self.apps = crate::catalog::installed(pinned);
            self.loaded = Instant::now();
        }
        self.scores = self.read_scores();
        self.files = crate::catalog::searchable_files();
        self.query.clear();
        self.offset = 0.0;
        self.caret_on = true;
        self.update_results();
    }
    /// Veces que Windows ha visto abrir cada app, más un extra si se abrió hace
    /// poco desde GlassBar.
    fn read_scores(&self) -> HashMap<String, u64> {
        let usage = crate::usage::load();
        let history = crate::catalog::recent();
        self.apps
            .iter()
            .map(|entry| {
                let runs = entry
                    .command
                    .strip_prefix("shell:AppsFolder\\")
                    .and_then(|name| usage.get(&name.to_lowercase()))
                    .map_or(0, |u| u.runs as u64);
                let recent = history
                    .iter()
                    .any(|l| l.entry.command.eq_ignore_ascii_case(&entry.command));
                let bonus = if recent { HISTORY_BONUS } else { 0 };
                (entry.command.to_lowercase(), runs + bonus)
            })
            .collect()
    }
    pub fn load_icons_with(&mut self, loader: IconLoader) {
        self.icons.clear();
        self.requested.clear();
        self.loader = Some(loader);
    }
    fn update_results(&mut self) {
        let text = self.query.text().to_string();
        let scores = &self.scores;
        let apps = rank(&self.apps, &text, |e| {
            scores.get(&e.command.to_lowercase()).copied().unwrap_or(0)
        })
        .into_iter()
        .map(|i| self.apps[i].clone())
        .collect();
        self.results = compose(apps, crate::extras::search(&text, &self.files));
        self.selected = 0;
    }
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Cambia el texto con `change`; si cambia, se buscan otra vez los resultados.
    pub fn edit<R>(&mut self, change: impl FnOnce(&mut TextField) -> R) -> R {
        self.caret_on = true;
        let before = self.query.text().to_string();
        let result = change(&mut self.query);
        if self.query.text() != before {
            self.update_results();
        }
        result
    }
    pub fn insert_text(&mut self, text: &str) {
        self.edit(|field| field.insert(text));
    }
    pub fn blink(&mut self) -> bool {
        self.caret_on = !self.caret_on;
        self.query.selection().is_none()
    }
    pub fn move_selection(&mut self, delta: i32) {
        if self.results.is_empty() {
            return;
        }
        let last = self.results.len() as i32 - 1;
        self.selected = (self.selected as i32 + delta).clamp(0, last) as usize;
    }
    pub fn select(&mut self, index: usize) -> bool {
        let changed = index < self.results.len() && index != self.selected;
        if changed {
            self.selected = index;
        }
        changed
    }
    /// Lo que hace elegir el resultado `index`; con `reveal`, mostrarlo en su carpeta.
    pub fn activate(&self, index: usize, reveal: bool) -> Option<Action> {
        let item = self.results.get(index)?;
        Some(match item {
            Item::App(entry) if reveal => Action::Reveal(entry.clone()),
            Item::App(entry) => Action::Launch(entry.clone()),
            Item::Extra(extra) => match extra.kind {
                Kind::Calc => Action::Copy(extra.entry.command.clone()),
                Kind::File if reveal => Action::Reveal(extra.entry.clone()),
                _ => Action::Open(extra.entry.clone()),
            },
        })
    }

    /// Alto lógico del panel: el campo y, si hay resultados, su lista.
    pub fn height(&self) -> f32 {
        if self.results.is_empty() {
            FIELD_H
        } else {
            FIELD_H + LIST_PAD * 2.0 + self.results.len() as f32 * ROW_H
        }
    }
    /// Resultado bajo el punto (`x`, `y`), en coordenadas lógicas del panel.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<usize> {
        let top = FIELD_H + LIST_PAD;
        if !(0.0..WIDTH).contains(&x) || y < top {
            return None;
        }
        let index = ((y - top) / ROW_H) as usize;
        (index < self.results.len()).then_some(index)
    }
    pub fn over_text(&self, x: f32, y: f32) -> bool {
        (TEXT_X..TEXT_X + TEXT_W).contains(&x) && (0.0..FIELD_H).contains(&y)
    }

    pub fn draw(&mut self, image: &mut Pixmap, scale: f32) {
        if let Some(loader) = &self.loader {
            for (command, icon) in loader.drain() {
                self.requested.remove(&command);
                self.icons.insert(command, icon);
            }
        }
        image.fill(tiny_skia::Color::TRANSPARENT);
        let height = self.height();
        fill_rounded(
            image,
            crate::glass::panel(),
            scale,
            scale,
            (WIDTH - 2.0) * scale,
            (height - 2.0) * scale,
            RADIUS * scale,
        );
        // Borde blanco que separa el panel de lo que haya detrás.
        stroke_rounded(
            image,
            [255, 255, 255, 150],
            scale,
            scale,
            (WIDTH - 2.0) * scale,
            (height - 2.0) * scale,
            RADIUS * scale,
            scale * 1.25,
        );
        crate::launcher::draw_magnifier(image, 28.0 * scale, FIELD_H / 2.0 * scale, scale * 1.3);
        self.draw_field(image, scale);
        if self.results.is_empty() {
            return;
        }
        fill_rounded(
            image,
            [255, 255, 255, 26],
            scale,
            FIELD_H * scale,
            (WIDTH - 2.0) * scale,
            scale.max(1.0),
            0.0,
        );
        for index in 0..self.results.len() {
            self.draw_row(image, index, scale);
        }
    }

    fn draw_field(&mut self, image: &mut Pixmap, scale: f32) {
        let size = FONT * scale;
        let (ascent, descent) = line_metrics(size);
        let line_top = ((FIELD_H * scale - (ascent + descent)) / 2.0).round();
        let baseline = line_top + ascent.round();
        let Some(mut layer) = Pixmap::new(
            (TEXT_W * scale).ceil() as u32,
            (FIELD_H * scale).ceil() as u32,
        ) else {
            return;
        };
        let text = self.query.text();
        let width_to = |chars: usize| {
            let prefix: String = text.chars().take(chars).collect();
            text_width(&prefix, size)
        };
        // Si el texto no cabe, se desplaza para seguir al cursor.
        let caret = width_to(self.query.caret());
        let room = layer.width() as f32 - 2.0 * scale;
        let mut offset = self.offset * scale;
        if caret - offset > room {
            offset = caret - room;
        }
        offset = offset
            .min(caret)
            .min((text_width(text, size) - room).max(0.0))
            .max(0.0);
        self.offset = offset / scale;
        if text.is_empty() {
            draw_text_on_baseline(
                &mut layer,
                "Buscar apps, archivos y más",
                0.0,
                baseline,
                size,
                0.45,
            );
        } else {
            if let Some((start, end)) = self.query.selection() {
                let left = width_to(start) - offset + scale;
                fill_rounded(
                    &mut layer,
                    crate::accent::with_alpha(crate::accent::current().light, 110),
                    left,
                    line_top,
                    width_to(end) - offset + scale - left,
                    ascent + descent,
                    2.0 * scale,
                );
            }
            draw_text_on_baseline(&mut layer, text, -offset, baseline, size, 1.0);
        }
        if self.query.selection().is_none() && self.caret_on {
            fill_rounded(
                &mut layer,
                [235, 240, 250, 220],
                (caret - offset + scale).round(),
                line_top,
                (1.5 * scale).max(1.0),
                ascent + descent,
                0.0,
            );
        }
        image.draw_pixmap(
            (TEXT_X * scale).round() as i32,
            0,
            layer.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }

    fn draw_row(&mut self, image: &mut Pixmap, index: usize, scale: f32) {
        let y = FIELD_H + LIST_PAD + index as f32 * ROW_H;
        if index == self.selected {
            fill_rounded(
                image,
                crate::accent::with_alpha(crate::accent::current().light, 90),
                LIST_PAD * scale,
                y * scale,
                (WIDTH - LIST_PAD * 2.0) * scale,
                ROW_H * scale,
                8.0 * scale,
            );
        }
        let item = self.results[index].clone();
        let icon_x = 18.0;
        let cy = y + ROW_H / 2.0;
        let glyph = match &item {
            Item::Extra(extra) => extra.kind.glyph(),
            Item::App(_) => None,
        };
        match glyph {
            Some(glyph) => draw_icon(
                image,
                glyph,
                (icon_x + ICON / 2.0) * scale,
                cy * scale,
                18.0 * scale,
                0.9,
            ),
            None => self.draw_entry_icon(image, item.entry(), icon_x, cy - ICON / 2.0, scale),
        }
        let detail = match &item {
            Item::App(_) => "Aplicación",
            Item::Extra(extra) if index == self.selected && extra.kind == Kind::Calc => {
                "Enter copia el resultado"
            }
            Item::Extra(extra) => extra.kind.header(),
        };
        let detail_size = DETAIL_FONT * scale;
        let detail_w = text_width(detail, detail_size);
        let right = (WIDTH - 22.0) * scale;
        let (ascent, descent) = line_metrics(detail_size);
        let baseline = (cy * scale - (ascent + descent) / 2.0 + ascent).round();
        draw_text_on_baseline(image, detail, right - detail_w, baseline, detail_size, 0.55);

        let size = ROW_FONT * scale;
        let x = (icon_x + ICON + 14.0) * scale;
        let room = right - detail_w - 16.0 * scale - x;
        let name = fit_text(&item.entry().name, size, room);
        let (ascent, descent) = line_metrics(size);
        let baseline = (cy * scale - (ascent + descent) / 2.0 + ascent).round();
        draw_text_on_baseline(image, &name, x, baseline, size, 1.0);
    }

    /// Icono de una app o un archivo; si aún no está, se pide al hilo de carga y
    /// se dibuja cuando llegue.
    fn draw_entry_icon(
        &mut self,
        image: &mut Pixmap,
        entry: &AppEntry,
        x: f32,
        y: f32,
        scale: f32,
    ) {
        if !self.icons.contains_key(&entry.command) {
            if let Some(loader) = &self.loader {
                if self.requested.insert(entry.command.clone()) {
                    loader.request(&entry.command);
                }
                return;
            }
            let icon = crate::icons::load(&entry.command);
            self.icons.insert(entry.command.clone(), icon);
        }
        let Some(Some(icon)) = self.icons.get(&entry.command) else {
            return;
        };
        let side = (ICON * scale).round().max(1.0);
        let transform =
            Transform::from_scale(side / icon.width() as f32, side / icon.height() as f32)
                .post_translate((x * scale).round(), (y * scale).round());
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> AppEntry {
        AppEntry {
            name: name.into(),
            command: format!("C:\\{name}.lnk"),
        }
    }
    fn names(apps: &[AppEntry], query: &str, score: impl Fn(&AppEntry) -> u64) -> Vec<String> {
        rank(apps, query, score)
            .into_iter()
            .map(|i| apps[i].name.clone())
            .collect()
    }

    #[test]
    fn prefix_beats_word_prefix_and_contains() {
        let apps = [
            app("Notas rápidas"),
            app("Bloc de notas"),
            app("Notepad++"),
            app("Anotaciones"),
        ];
        assert_eq!(
            names(&apps, "not", |_| 0),
            ["Notas rápidas", "Notepad++", "Bloc de notas", "Anotaciones"]
        );
    }

    #[test]
    fn initials_find_multi_word_apps() {
        let apps = [app("Visual Studio Code"), app("Visor de fotos")];
        assert_eq!(names(&apps, "vsc", |_| 0), ["Visual Studio Code"]);
    }

    #[test]
    fn ignores_case_and_accents() {
        let apps = [app("Configuración"), app("Calculadora")];
        assert_eq!(names(&apps, "CONFIGURACION", |_| 0), ["Configuración"]);
    }

    #[test]
    fn usage_breaks_ties_within_the_same_tier() {
        let apps = [app("TextEdit"), app("Terminal")];
        let score = |e: &AppEntry| if e.name == "Terminal" { 50 } else { 1 };
        assert_eq!(names(&apps, "te", score), ["Terminal", "TextEdit"]);
    }

    #[test]
    fn empty_query_has_no_results() {
        assert!(rank(&[app("Terminal")], "   ", |_| 0).is_empty());
    }

    #[test]
    fn web_search_stays_last_when_the_list_is_full() {
        let apps: Vec<_> = (0..10).map(|i| app(&format!("App {i}"))).collect();
        let web = Extra {
            kind: Kind::Web,
            entry: app("web"),
            detail: String::new(),
        };
        let calc = Extra {
            kind: Kind::Calc,
            entry: app("= 4"),
            detail: String::new(),
        };
        let items = compose(apps, vec![calc.clone(), web.clone()]);
        assert_eq!(items.len(), MAX_APPS + 2);
        assert_eq!(items.first(), Some(&Item::Extra(calc)));
        assert_eq!(items.last(), Some(&Item::Extra(web)));
    }

    #[test]
    fn typing_selects_the_top_hit_and_enter_launches_it() {
        let mut state = SpotlightState::with_apps(vec![app("Safari"), app("Sistema")]);
        state.insert_text("saf");
        assert_eq!(state.selected(), 0);
        assert_eq!(
            state.activate(state.selected(), false),
            Some(Action::Launch(app("Safari")))
        );
        assert_eq!(
            state.activate(state.selected(), true),
            Some(Action::Reveal(app("Safari")))
        );
    }

    #[test]
    fn panel_grows_with_results_and_rows_are_hit_tested() {
        let mut state = SpotlightState::with_apps(vec![app("Safari")]);
        assert_eq!(state.height(), FIELD_H);
        state.insert_text("saf");
        assert!(state.height() > FIELD_H);
        assert_eq!(state.hit_test(100.0, FIELD_H + LIST_PAD + 1.0), Some(0));
        assert_eq!(state.hit_test(100.0, 10.0), None);
    }
}
