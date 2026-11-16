//! Lanzador propio al estilo del Inicio de Windows 11: búsqueda, anclados,
//! recientes (apps y archivos) y todas las apps por categorías, en cuadrícula o
//! en lista, en una vista con desplazamiento.
use crate::apps::{fill_rounded, stroke_rounded, AppEntry};
use crate::catalog::Launch;
use crate::categories::Category;
use crate::clock::{
    draw_icon, draw_text, draw_text_on_baseline, draw_text_semibold, fit_text, line_metrics,
    text_width,
};
use crate::textfield::TextField;
use crate::usage::Usage;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

pub const WIDTH: f32 = 640.0;
pub const HEIGHT: f32 = 700.0;
const PAD: f32 = 32.0;
const SEARCH_Y: f32 = 24.0;
const SEARCH_H: f32 = 38.0;
/// Zona del texto dentro del campo, entre la lupa y el botón de limpiar.
const SEARCH_TEXT_X: f32 = PAD + 44.0;
const SEARCH_TEXT_W: f32 = CONTENT_W - 88.0;
const SEARCH_FONT: f32 = 14.0;
/// Zona desplazable entre la búsqueda y el pie.
const VIEW_TOP: f32 = 82.0;
const FOOTER_Y: f32 = HEIGHT - 64.0;
const VIEW_H: f32 = FOOTER_Y - 8.0 - VIEW_TOP;
const CONTENT_W: f32 = WIDTH - PAD * 2.0;
const COLS: usize = 6;
const TILE_W: f32 = CONTENT_W / COLS as f32;
const TILE_H: f32 = 88.0;
const HEADER_H: f32 = 32.0;
const SECTION_GAP: f32 = 14.0;
/// Margen al final de la vista desplazable.
const BOTTOM_PAD: f32 = 16.0;
const ROW_H: f32 = 52.0;
const ICON: f32 = 32.0;
const MAX_PINNED: usize = COLS * 3;
/// Recientes: dos filas de tres, apps y archivos mezclados, como en Windows 11.
const RECENT_COLS: usize = 3;
const MAX_RECENT: usize = RECENT_COLS * 2;
/// Carpetas de la vista por categorías: cuatro por fila, con una rejilla de 2x2
/// iconos y el nombre debajo.
const FOLDER_COLS: usize = 4;
const FOLDER_W: f32 = CONTENT_W / FOLDER_COLS as f32;
const FOLDER_BOX: f32 = 112.0;
const FOLDER_H: f32 = FOLDER_BOX + 44.0;
/// Iconos pequeños del cuarto hueco de una carpeta con más de cuatro apps.
const MINI_ICON: f32 = 14.0;
/// Filas de la vista de lista.
const LINE_H: f32 = 40.0;
const LINE_ICON: f32 = 24.0;
/// Texto de los enlaces de las cabeceras ("Mostrar todo", "Visualización").
const LINK_FONT: f32 = 12.5;
/// El catálogo completo (lento de leer) se reutiliza entre aperturas durante este tiempo.
const CATALOG_TTL: Duration = Duration::from_secs(120);
/// Fondo casi opaco, como el Inicio de Windows en modo oscuro.
fn background() -> [u8; 4] {
    crate::glass::panel()
}
const MENU_W: f32 = 196.0;
const MENU_ROW: f32 = 36.0;
const MENU_PAD: f32 = 6.0;
/// Entrada del menú de energía: duración y desplazamiento inicial hacia abajo.
const MENU_ANIM: Duration = Duration::from_millis(200);
const MENU_RISE: f32 = 12.0;
/// Al buscar, los resultados ocupan la columna izquierda y el detalle del elegido
/// la derecha, como la búsqueda de Windows 11.
const LIST_W: f32 = 260.0;
const DETAIL_X: f32 = PAD + LIST_W + 12.0;
const DETAIL_W: f32 = WIDTH - PAD - DETAIL_X;
const DETAIL_ICON: f32 = 56.0;
const DETAIL_BUTTON_Y: f32 = VIEW_TOP + 196.0;
const DETAIL_BUTTON_H: f32 = 32.0;
/// Comienzo de la lista de recientes de la app (su jump list) dentro del detalle.
const DETAIL_JUMPS_Y: f32 = DETAIL_BUTTON_Y + DETAIL_BUTTON_H + 18.0;
const JUMP_HEADER_H: f32 = 28.0;
const JUMP_ROW_H: f32 = 30.0;
/// Opciones del menú contextual de una app (clic derecho u "Opciones").
const CMD_OPEN: u32 = 1;
const CMD_ADMIN: u32 = 2;
const CMD_LOCATION: u32 = 3;
const CMD_PIN: u32 = 4;
const CMD_UNPIN: u32 = 5;
const CMD_UNINSTALL: u32 = 6;
/// Glifos de Segoe Fluent Icons de esas opciones, los mismos que usa Windows.
const GLYPH_OPEN: char = '\u{E8A7}';
const GLYPH_ADMIN: char = '\u{E7EF}';
const GLYPH_FOLDER: char = '\u{E838}';
const GLYPH_UNINSTALL: char = '\u{E74D}';
const GLYPH_CHEVRON_DOWN: char = '\u{E70D}';
const GLYPH_CHEVRON_RIGHT: char = '\u{E76C}';
const GLYPH_BACK: char = '\u{E72B}';
const GLYPH_CHECK: char = '\u{E73E}';
/// Opciones del menú de "Visualización": `CMD_VIEW` más la posición en `View::ALL`.
const CMD_VIEW: u32 = 100;
/// Apertura y cierre del panel: el cierre es algo más corto, como en Windows 11.
const OPEN_ANIM: Duration = Duration::from_millis(240);
const CLOSE_ANIM: Duration = Duration::from_millis(160);
/// Píxeles lógicos que el panel sube al abrirse (y baja al cerrarse).
pub const PANEL_RISE: f32 = 36.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Pinned(usize),
    Recent(usize),
    App(usize),
    /// "Mostrar todo" de Recientes: abre la carpeta de recientes de Windows.
    ShowAllRecent,
    /// "Visualización: …" de Todo: el menú para elegir la vista.
    ViewPicker,
    /// Carpeta `n` de la vista por categorías.
    Category(usize),
    /// Vuelta de una categoría abierta a "Todo".
    CategoryBack,
    Power,
    PowerOption(Power),
    /// Clic fuera del menú abierto (energía o contextual): solo lo cierra.
    Outside,
    ClearSearch,
    /// Opción `n` del menú contextual abierto.
    Context(usize),
    /// Botones del detalle del resultado elegido y elementos de su jump list.
    DetailOpen,
    DetailOptions,
    DetailJump(usize),
    /// Resultado `n` que no es un programa (ver `crate::extras`).
    Extra(usize),
}
pub enum Action {
    Launch(AppEntry),
    /// Un archivo reciente (o la carpeta de recientes): se abre sin pasar al
    /// historial de apps de GlassBar.
    OpenFile(AppEntry),
    /// Copia un texto (el resultado de una operación).
    Copy(String),
    Power(Power),
    RunAsAdmin(AppEntry),
    OpenLocation(AppEntry),
    Pin(AppEntry),
    Unpin(AppEntry),
    /// Abre "Aplicaciones instaladas" de Configuración.
    Uninstall,
    /// Un elemento de la jump list de la app.
    Jump(AppEntry, crate::jumplist::JumpItem),
}
impl Action {
    /// Anclar y desanclar dejan el lanzador abierto, como el Inicio de Windows.
    pub fn closes_launcher(&self) -> bool {
        !matches!(self, Action::Pin(_) | Action::Unpin(_))
    }
}

/// Menú flotante dibujado dentro del panel: el contextual de una app (`entry`) o,
/// sin app, el de "Visualización". `x` e `y` son su esquina superior izquierda en
/// coordenadas del panel.
struct Context {
    menu: crate::appmenu::AppMenu,
    x: f32,
    y: f32,
    entry: Option<AppEntry>,
}

/// Cómo se muestra la sección "Todo", como el selector del Inicio de Windows 11.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Category,
    Grid,
    List,
}
impl View {
    const ALL: [View; 3] = [View::Category, View::Grid, View::List];
    fn label(self) -> &'static str {
        match self {
            View::Category => "Categoría",
            View::Grid => "Cuadrícula",
            View::List => "Lista",
        }
    }
    fn key(self) -> &'static str {
        match self {
            View::Category => "categoria",
            View::Grid => "cuadricula",
            View::List => "lista",
        }
    }
}

/// Elemento de Recientes: una app o un archivo, con cuándo se abrió (segundos
/// Unix; 0 si no se sabe).
#[derive(Clone)]
pub struct RecentItem {
    pub entry: AppEntry,
    pub at: u64,
    pub file: bool,
}

/// Opciones del menú de `entry`, como el clic derecho del Inicio de Windows 11.
fn context_entries(entry: &AppEntry, pinned: bool, admin: bool) -> Vec<crate::appmenu::Entry> {
    use crate::appmenu::{Entry, Icon, GLYPH_PIN, GLYPH_UNPIN};
    let item = |glyph: char, label: &str, command: u32| Entry::Item {
        icon: Icon::Glyph(glyph),
        label: label.into(),
        command,
    };
    let mut entries = vec![item(GLYPH_OPEN, "Abrir", CMD_OPEN)];
    if admin {
        entries.push(item(GLYPH_ADMIN, "Ejecutar como administrador", CMD_ADMIN));
    }
    if file_path(&entry.command) {
        entries.push(item(
            GLYPH_FOLDER,
            "Abrir ubicación del archivo",
            CMD_LOCATION,
        ));
    }
    entries.push(if pinned {
        item(GLYPH_UNPIN, "Desanclar de la barra de tareas", CMD_UNPIN)
    } else {
        item(GLYPH_PIN, "Anclar a la barra de tareas", CMD_PIN)
    });
    entries.push(Entry::Separator);
    entries.push(item(GLYPH_UNINSTALL, "Desinstalar", CMD_UNINSTALL));
    entries
}

/// Windows ofrece ejecutar la app como administrador (ver `apps::can_run_as_admin`).
#[cfg(not(test))]
fn admin_allowed(command: &str) -> bool {
    crate::apps::can_run_as_admin(command)
}
/// En los tests no se consulta el Shell: solo las apps de la Store ("familia!app")
/// quedan sin la opción.
#[cfg(test)]
fn admin_allowed(command: &str) -> bool {
    !command
        .strip_prefix("shell:AppsFolder\\")
        .is_some_and(|id| id.contains('!'))
}

/// Lo que hace la opción `command` del menú contextual de `entry`.
fn context_action(command: u32, entry: AppEntry) -> Option<Action> {
    Some(match command {
        CMD_OPEN => Action::Launch(entry),
        CMD_ADMIN => Action::RunAsAdmin(entry),
        CMD_LOCATION => Action::OpenLocation(entry),
        CMD_PIN => Action::Pin(entry),
        CMD_UNPIN => Action::Unpin(entry),
        CMD_UNINSTALL => Action::Uninstall,
        _ => return None,
    })
}

/// La orden es un archivo del disco (programa o acceso directo), con carpeta propia.
fn file_path(command: &str) -> bool {
    let bytes = command.as_bytes();
    (bytes.len() > 2 && bytes[1] == b':' && bytes[2] == b'\\') || command.starts_with("\\\\")
}

/// Opciones del menú de energía, en el orden en que se muestran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Power {
    Lock,
    Sleep,
    Shutdown,
    Restart,
}
impl Power {
    pub const ALL: [Power; 4] = [Power::Lock, Power::Sleep, Power::Shutdown, Power::Restart];
    fn label(self) -> &'static str {
        match self {
            Power::Lock => "Bloquear",
            Power::Sleep => "Suspender",
            Power::Shutdown => "Apagar",
            Power::Restart => "Reiniciar",
        }
    }
    /// Glifo de Segoe Fluent Icons / MDL2 Assets que usa Windows para cada opción.
    fn glyph(self) -> char {
        match self {
            Power::Lock => '\u{E72E}',
            Power::Sleep => '\u{E708}',
            Power::Shutdown => '\u{E7E8}',
            Power::Restart => '\u{E777}',
        }
    }
}

/// Forma con que se dibuja un elemento de la vista.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// Icono con el nombre debajo (Anclado, cuadrícula).
    Tile,
    /// Icono, nombre y una línea de detalle (Recientes, resultados).
    Row,
    /// Icono pequeño y nombre en una línea (vista de lista).
    Line,
    /// Carpeta de una categoría.
    Folder,
    /// Enlace de texto de una cabecera.
    Link,
}

/// Elemento pulsable; coordenadas de contenido (x desde `PAD`, y sin desplazamiento).
#[derive(Clone, Copy, Debug)]
struct Slot {
    hit: Hit,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    shape: Shape,
}
impl Slot {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
    fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}
struct Layout {
    headers: Vec<(String, f32)>,
    slots: Vec<Slot>,
    height: f32,
}

pub struct LauncherState {
    pub apps: Vec<AppEntry>,
    pub recent: Vec<RecentItem>,
    pub pinned: Vec<AppEntry>,
    /// Catálogo por categorías y en orden alfabético, para la sección "Todo".
    categories: Vec<Category>,
    alphabetical: Vec<usize>,
    pub view: View,
    /// Categoría abierta y el desplazamiento que había antes, para volver a él.
    open_category: Option<(usize, f32)>,
    pub query: TextField,
    pub scroll: f32,
    pub hovered: Option<Hit>,
    pub power_menu: bool,
    /// Cuándo se abrió el menú de energía o el contextual, para su animación de entrada.
    menu_opened: Option<Instant>,
    /// Menú contextual de una app abierto (clic derecho u "Opciones").
    context: Option<Context>,
    pub selection: Option<Hit>,
    /// Desplazamiento horizontal del texto de búsqueda cuando no cabe, para que el
    /// cursor siempre se vea.
    search_offset: f32,
    /// El cursor del texto se ve en esta fase del parpadeo.
    caret_on: bool,
    /// Última pulsación en el buscador (momento, x y cuántas seguidas), para el
    /// doble y el triple clic.
    last_press: Option<(Instant, f32, u8)>,
    user: String,
    icons: HashMap<String, Option<Pixmap>>,
    /// Con ventana, los iconos que faltan se cargan en otro hilo (ver `IconLoader`);
    /// `requested` son los ya pedidos.
    loader: Option<IconLoader>,
    requested: std::collections::HashSet<String>,
    /// Capa de la zona desplazable, reutilizada entre fotogramas.
    view_layer: Option<Pixmap>,
    loaded: Instant,
    /// Última búsqueda y sus resultados: la vista se recalcula en cada movimiento del ratón.
    search_cache: RefCell<Option<(String, Vec<usize>)>>,
    /// Iconos grandes del detalle, sin reducir, por orden de la app.
    detail_icons: HashMap<String, Option<Pixmap>>,
    /// Jump list de cada app mostrada en el detalle; se vuelve a leer en cada apertura.
    jumps: HashMap<String, Vec<crate::jumplist::Section>>,
    /// Si cada app admite "Ejecutar como administrador", por orden de la app.
    admin: HashMap<String, bool>,
    /// Archivos en los que se busca; se leen en cada apertura.
    files: Vec<AppEntry>,
    /// Resultados de la búsqueda actual que no son programas.
    extras: Vec<crate::extras::Extra>,
}
impl LauncherState {
    pub fn new(pinned: &[AppEntry]) -> Self {
        let mut state = Self {
            apps: crate::catalog::installed(pinned),
            recent: Vec::new(),
            pinned: Vec::new(),
            categories: Vec::new(),
            alphabetical: Vec::new(),
            view: load_view(),
            open_category: None,
            query: TextField::default(),
            scroll: 0.0,
            hovered: None,
            power_menu: false,
            menu_opened: None,
            context: None,
            selection: None,
            search_offset: 0.0,
            caret_on: true,
            last_press: None,
            user: user_display_name(),
            icons: HashMap::new(),
            loader: None,
            requested: Default::default(),
            view_layer: None,
            loaded: Instant::now(),
            search_cache: RefCell::new(None),
            detail_icons: HashMap::new(),
            jumps: HashMap::new(),
            admin: HashMap::new(),
            files: Vec::new(),
            extras: Vec::new(),
        };
        state.refresh(pinned);
        state
    }
    /// Prepara una nueva apertura: anclados, recientes y categorías al día, sin
    /// volver a leer el catálogo ni los iconos si es reciente.
    pub fn refresh(&mut self, pinned: &[AppEntry]) {
        if self.loaded.elapsed() > CATALOG_TTL {
            self.apps = crate::catalog::installed(pinned);
            self.loaded = Instant::now();
            self.search_cache.replace(None);
        }
        self.pinned = pinned.to_vec();
        let usage = crate::usage::load();
        self.recent = rank(
            &self.apps,
            &usage,
            &crate::catalog::recent(),
            crate::catalog::recent_files(),
        );
        self.regroup(&usage);
        self.files = crate::catalog::searchable_files();
        self.open_category = None;
        self.query.clear();
        self.caret_on = true;
        self.power_menu = false;
        self.context = None;
        self.jumps.clear();
        self.reset_results();
    }
    /// Agrupa el catálogo para "Todo": por categorías (las más usadas delante) y
    /// por orden alfabético.
    fn regroup(&mut self, usage: &HashMap<String, Usage>) {
        self.categories = crate::categories::group(&self.apps, |entry| {
            usage_of(usage, entry).map_or(0, |u| ((u.runs as u64) << 32) | u.focus_ms as u64)
        });
        self.alphabetical = crate::categories::alphabetical(&self.apps);
    }
    /// Pone al día los anclados (tras anclar o desanclar desde el propio lanzador).
    pub fn set_pinned(&mut self, pinned: &[AppEntry]) {
        self.pinned = pinned.to_vec();
        self.hovered = None;
        self.selection = self.selection.filter(|hit| !matches!(hit, Hit::Pinned(_)));
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
    }
    fn is_pinned(&self, entry: &AppEntry) -> bool {
        self.pinned
            .iter()
            .any(|p| p.command.eq_ignore_ascii_case(&entry.command))
    }
    /// Siguiente fase del parpadeo del cursor del texto. Devuelve si cambia algo
    /// visible (con texto seleccionado o un menú abierto, el cursor no se ve).
    pub fn blink(&mut self) -> bool {
        self.caret_on = !self.caret_on;
        self.query.selection().is_none() && !self.overlay_open()
    }
    /// El cursor del ratón está sobre el texto del buscador y escribir ahí es posible.
    pub fn over_text(&self, x: f32, y: f32) -> bool {
        !self.overlay_open() && self.in_search(x, y)
    }
    /// Hay un menú flotante abierto (energía o contextual): el resto no responde.
    pub fn overlay_open(&self) -> bool {
        self.power_menu || self.context.is_some()
    }
    /// Cierra el menú flotante abierto, si lo hay.
    pub fn close_overlay(&mut self) -> bool {
        let open = self.overlay_open();
        self.power_menu = false;
        self.context = None;
        if open {
            self.selection = self
                .selection
                .filter(|hit| !matches!(hit, Hit::PowerOption(_) | Hit::Context(_)));
            self.hovered = None;
        }
        open
    }
    /// Suelta las capas que se reutilizan entre fotogramas; con el lanzador oculto
    /// solo ocupan memoria, y el siguiente dibujo las vuelve a crear.
    pub fn release_layers(&mut self) {
        self.view_layer = None;
    }
    /// Desde ahora los iconos que falten se cargan en segundo plano.
    pub fn load_icons_with(&mut self, loader: IconLoader) {
        self.loader = Some(loader);
    }
    /// El lanzador pasa a otra escala (el dock se fue a un monitor con otro DPI):
    /// se olvidan los iconos, que estaban reducidos a la anterior, y se vuelven a
    /// cargar con `loader` a medida que se dibujan.
    pub fn reload_icons_with(&mut self, loader: IconLoader) {
        self.icons.clear();
        self.requested.clear();
        self.detail_icons.clear();
        self.loader = Some(loader);
    }
    /// Guarda los iconos que ya llegaron del hilo de carga.
    fn take_loaded_icons(&mut self) {
        if let Some(loader) = &self.loader {
            loader
                .notified
                .store(false, std::sync::atomic::Ordering::Relaxed);
            for (command, icon) in loader.loaded.try_iter() {
                self.requested.remove(&command);
                self.icons.insert(command, icon);
            }
        }
    }
    /// Carga de antemano los iconos de la vista inicial (anclados y recientes), que
    /// es lo que más tarda la primera vez que se abre el lanzador.
    /// Se guardan ya reducidos al tamaño en que se dibujan a `scale`.
    pub fn preload_icons(&mut self, scale: f32) {
        let side = (ICON * scale).round().max(1.0) as u32;
        let entries: Vec<String> = self
            .pinned
            .iter()
            .take(MAX_PINNED)
            .chain(self.recent.iter().take(MAX_RECENT).map(|r| &r.entry))
            .map(|e| e.command.clone())
            .collect();
        for command in entries {
            self.icons.entry(command.clone()).or_insert_with(|| {
                crate::icons::load(&command).map(|icon| crate::apps::downscale(&icon, side))
            });
        }
    }
    fn filtered(&self) -> Vec<usize> {
        let mut cache = self.search_cache.borrow_mut();
        if let Some((query, hits)) = cache.as_ref() {
            if query == self.query.text() {
                return hits.clone();
            }
        }
        let hits = crate::catalog::search(&self.apps, self.query.text());
        *cache = Some((self.query.text().to_string(), hits.clone()));
        hits
    }
    fn searching(&self) -> bool {
        !self.query.text().trim().is_empty()
    }

    fn layout(&self) -> Layout {
        let mut headers = Vec::new();
        let mut slots = Vec::new();
        let mut y = 0.0;
        let tiles = |slots: &mut Vec<Slot>, y: f32, hits: &mut dyn Iterator<Item = Hit>| {
            let mut count: usize = 0;
            for (i, hit) in hits.enumerate() {
                slots.push(Slot {
                    hit,
                    x: (i % COLS) as f32 * TILE_W,
                    y: y + (i / COLS) as f32 * TILE_H,
                    w: TILE_W,
                    h: TILE_H,
                    shape: Shape::Tile,
                });
                count += 1;
            }
            count.div_ceil(COLS) as f32 * TILE_H
        };
        if self.searching() {
            // Como en Windows: la mejor coincidencia aparte y debajo las demás apps;
            // el detalle del elegido ocupa la columna derecha.
            let hits = self.filtered();
            // Los que no son programas, por secciones: el resultado de una operación
            // delante de todo y el resto detrás de las apps.
            let section = |kind: crate::extras::Kind,
                           headers: &mut Vec<(String, f32)>,
                           slots: &mut Vec<Slot>,
                           y: &mut f32| {
                let mut first = true;
                for (index, extra) in self.extras.iter().enumerate() {
                    if extra.kind != kind {
                        continue;
                    }
                    if first {
                        if *y > 0.0 {
                            *y += SECTION_GAP;
                        }
                        headers.push((kind.header().to_string(), *y));
                        *y += HEADER_H;
                        first = false;
                    }
                    slots.push(Slot {
                        hit: Hit::Extra(index),
                        x: 0.0,
                        y: *y,
                        w: LIST_W,
                        h: ROW_H - 4.0,
                        shape: Shape::Row,
                    });
                    *y += ROW_H;
                }
            };
            use crate::extras::Kind;
            section(Kind::Calc, &mut headers, &mut slots, &mut y);
            if hits.is_empty() && self.extras.iter().all(|e| e.kind == Kind::Web) {
                headers.push(("Sin resultados".to_string(), y));
                y += HEADER_H;
            } else if !hits.is_empty() && y > 0.0 {
                y += SECTION_GAP;
            }
            for (n, index) in hits.into_iter().enumerate() {
                if n < 2 {
                    let title = if n == 0 {
                        "Mejor coincidencia"
                    } else {
                        "Aplicaciones"
                    };
                    if n == 1 {
                        y += SECTION_GAP;
                    }
                    headers.push((title.to_string(), y));
                    y += HEADER_H;
                }
                slots.push(Slot {
                    hit: Hit::App(index),
                    x: 0.0,
                    y,
                    w: LIST_W,
                    h: ROW_H - 4.0,
                    shape: Shape::Row,
                });
                y += ROW_H;
            }
            for kind in [Kind::Setting, Kind::File, Kind::Web] {
                section(kind, &mut headers, &mut slots, &mut y);
            }
            return Layout {
                headers,
                slots,
                height: y,
            };
        }
        // Enlace de texto alineado a la derecha de la cabecera que empieza en `y`.
        let link = |slots: &mut Vec<Slot>, hit: Hit, y: f32| {
            let w = text_width(&self.link_label(hit), LINK_FONT) + 40.0;
            slots.push(Slot {
                hit,
                x: CONTENT_W - w,
                y: y + 2.0,
                w,
                h: HEADER_H - 6.0,
                shape: Shape::Link,
            });
        };
        if let Some(category) = self
            .open_category
            .and_then(|(index, _)| self.categories.get(index))
        {
            // Categoría abierta: su nombre con la flecha para volver, y sus apps.
            let w = text_width(category.kind.label(), 14.0) + 52.0;
            slots.push(Slot {
                hit: Hit::CategoryBack,
                x: 0.0,
                y,
                w,
                h: HEADER_H,
                shape: Shape::Link,
            });
            y += HEADER_H + 8.0;
            y += tiles(
                &mut slots,
                y,
                &mut category.apps.iter().map(|&i| Hit::App(i)),
            );
            return Layout {
                headers,
                slots,
                height: y + BOTTOM_PAD,
            };
        }
        if !self.pinned.is_empty() {
            headers.push(("Anclado".to_string(), y));
            y += HEADER_H;
            let count = self.pinned.len().min(MAX_PINNED);
            y += tiles(&mut slots, y, &mut (0..count).map(Hit::Pinned)) + SECTION_GAP;
        }
        if !self.recent.is_empty() {
            headers.push(("Recientes".to_string(), y));
            link(&mut slots, Hit::ShowAllRecent, y);
            y += HEADER_H;
            let count = self.recent.len().min(MAX_RECENT);
            let w = CONTENT_W / RECENT_COLS as f32;
            for i in 0..count {
                slots.push(Slot {
                    hit: Hit::Recent(i),
                    x: (i % RECENT_COLS) as f32 * w,
                    y: y + (i / RECENT_COLS) as f32 * ROW_H,
                    w: w - 4.0,
                    h: ROW_H - 4.0,
                    shape: Shape::Row,
                });
            }
            y += count.div_ceil(RECENT_COLS) as f32 * ROW_H + SECTION_GAP;
        }
        headers.push(("Todo".to_string(), y));
        link(&mut slots, Hit::ViewPicker, y);
        y += HEADER_H + 4.0;
        match self.view {
            View::Category => {
                for (i, _) in self.categories.iter().enumerate() {
                    slots.push(Slot {
                        hit: Hit::Category(i),
                        x: (i % FOLDER_COLS) as f32 * FOLDER_W,
                        y: y + (i / FOLDER_COLS) as f32 * FOLDER_H,
                        w: FOLDER_W,
                        h: FOLDER_H,
                        shape: Shape::Folder,
                    });
                }
                y += self.categories.len().div_ceil(FOLDER_COLS) as f32 * FOLDER_H;
            }
            View::Grid => {
                y += tiles(
                    &mut slots,
                    y,
                    &mut self.alphabetical.iter().map(|&i| Hit::App(i)),
                );
            }
            View::List => {
                // Como la lista de Windows: una letra encima de las apps que empiezan por ella.
                let mut letter = String::new();
                for &index in &self.alphabetical {
                    let initial = crate::categories::initial(&self.apps[index].name);
                    if initial != letter {
                        if !letter.is_empty() {
                            y += 6.0;
                        }
                        headers.push((initial.clone(), y));
                        y += HEADER_H;
                        letter = initial;
                    }
                    slots.push(Slot {
                        hit: Hit::App(index),
                        x: 0.0,
                        y,
                        w: CONTENT_W,
                        h: LINE_H - 2.0,
                        shape: Shape::Line,
                    });
                    y += LINE_H;
                }
            }
        }
        Layout {
            headers,
            slots,
            height: y + BOTTOM_PAD,
        }
    }
    /// Texto de un enlace de cabecera.
    fn link_label(&self, hit: Hit) -> String {
        match hit {
            Hit::ShowAllRecent => "Mostrar todo".into(),
            Hit::ViewPicker => format!("Visualización: {}", self.view.label()),
            Hit::CategoryBack => self
                .open_category
                .and_then(|(index, _)| self.categories.get(index))
                .map(|c| c.kind.label().to_string())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }
    fn max_scroll(&self) -> f32 {
        (self.layout().height - VIEW_H).max(0.0)
    }
    pub fn scroll_by(&mut self, delta: f32) {
        self.scroll = (self.scroll + delta).clamp(0.0, self.max_scroll());
        self.hovered = None;
    }
    pub fn scroll_page(&mut self, forward: bool) {
        self.scroll_by(if forward { VIEW_H } else { -VIEW_H } * 0.9);
    }
    fn ensure_visible(&mut self, hit: Hit) {
        if let Some(slot) = self.layout().slots.into_iter().find(|s| s.hit == hit) {
            // Al principio de la vista también se ve el encabezado de su sección.
            if slot.y - HEADER_H < self.scroll {
                self.scroll = (slot.y - HEADER_H).max(0.0);
            } else if slot.y + slot.h > self.scroll + VIEW_H {
                self.scroll = slot.y + slot.h - VIEW_H;
            }
            self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        }
    }

    /// Aplica una edición al campo de búsqueda; si cambia el texto, la vista vuelve
    /// al principio de los resultados. Con un menú flotante abierto no se edita.
    pub fn edit<R>(&mut self, change: impl FnOnce(&mut TextField) -> R) -> Option<R> {
        if self.overlay_open() {
            return None;
        }
        // Al escribir o mover el cursor, se ve fijo hasta el siguiente parpadeo.
        self.caret_on = true;
        let before = self.query.text().to_string();
        let result = change(&mut self.query);
        if self.query.text() != before {
            self.reset_results();
        }
        Some(result)
    }
    pub fn insert_text(&mut self, text: &str) {
        self.edit(|field| field.insert(text));
    }
    fn reset_results(&mut self) {
        self.extras = crate::extras::search(self.query.text(), &self.files);
        self.scroll = 0.0;
        self.hovered = None;
        self.selection = None;
    }
    /// Clic en el campo de búsqueda: pone el cursor en la letra más cercana; con
    /// Mayús amplía la selección hasta ahí.
    pub fn click_search(&mut self, x: f32, extend: bool) {
        let index = self.char_at(x);
        self.edit(|field| field.move_to(index, extend));
    }
    /// Pulsación en el buscador en el momento `now`: un clic pone el cursor, dos
    /// seguidos en el mismo sitio (dentro de `double` del anterior, como pide
    /// Windows) seleccionan la palabra, y tres todo el texto.
    pub fn press_search(&mut self, x: f32, extend: bool, now: Instant, double: Duration) {
        let count = match self.last_press {
            Some((at, px, count))
                if !extend
                    && now.saturating_duration_since(at) <= double
                    && (x - px).abs() <= 4.0 =>
            {
                count % 3 + 1
            }
            _ => 1,
        };
        self.last_press = Some((now, x, count));
        match count {
            2 => {
                let index = self.char_at(x);
                self.edit(|field| field.select_word(index));
            }
            3 => {
                self.edit(|field| field.select_all());
            }
            _ => self.click_search(x, extend),
        }
    }
    /// Carácter del buscador más cercano a `x` (antes del que queda a la derecha).
    fn char_at(&self, x: f32) -> usize {
        let target = x - SEARCH_TEXT_X - 1.0 + self.search_offset;
        let mut left = 0.0;
        let mut index = 0;
        for c in self.query.text().chars() {
            let width = text_width(&c.to_string(), SEARCH_FONT);
            if left + width / 2.0 > target {
                break;
            }
            left += width;
            index += 1;
        }
        index
    }
    /// Mueve la selección con las flechas: izquierda/derecha siguen el orden de lectura;
    /// arriba/abajo saltan al elemento más cercano en esa dirección, entre secciones.
    pub fn move_selection(&mut self, dx: i32, dy: i32) {
        if let Some(context) = &mut self.context {
            // En el menú contextual las flechas recorren sus opciones.
            context.menu.move_hover(dy > 0 || (dy == 0 && dx > 0));
            self.hovered = None;
            return;
        }
        if self.power_menu {
            // En el menú de energía las flechas recorren sus opciones.
            let current = match self.selection {
                Some(Hit::PowerOption(p)) => Power::ALL.iter().position(|&o| o == p),
                _ => None,
            };
            let step = if dy != 0 { dy } else { dx } as isize;
            let next = match current {
                None if step < 0 => Power::ALL.len() - 1,
                None => 0,
                Some(i) => (i as isize + step).clamp(0, Power::ALL.len() as isize - 1) as usize,
            };
            self.selection = Some(Hit::PowerOption(Power::ALL[next]));
            return;
        }
        let slots = self.layout().slots;
        let Some(first) = slots.first() else {
            return;
        };
        let current = self
            .selection
            .and_then(|hit| slots.iter().position(|s| s.hit == hit));
        let next = match current {
            None => first.hit,
            Some(i) if dy == 0 => {
                let j = (i as isize + dx as isize).clamp(0, slots.len() as isize - 1);
                slots[j as usize].hit
            }
            Some(i) => {
                let (cx, cy) = slots[i].center();
                slots
                    .iter()
                    .filter(|s| {
                        let (_, sy) = s.center();
                        if dy > 0 {
                            sy > cy + 1.0
                        } else {
                            sy < cy - 1.0
                        }
                    })
                    .min_by(|a, b| {
                        let key = |s: &Slot| {
                            let (sx, sy) = s.center();
                            ((sy - cy).abs(), (sx - cx).abs())
                        };
                        key(a).partial_cmp(&key(b)).unwrap()
                    })
                    .map(|s| s.hit)
                    .unwrap_or(slots[i].hit)
            }
        };
        self.selection = Some(next);
        self.ensure_visible(next);
    }
    fn entry(&self, hit: Hit) -> Option<&AppEntry> {
        match hit {
            Hit::Pinned(i) => self.pinned.get(i),
            Hit::Recent(i) => self.recent.get(i).map(|r| &r.entry),
            Hit::App(i) => self.apps.get(i),
            Hit::Extra(i) => self.extras.get(i).map(|e| &e.entry),
            _ => None,
        }
    }
    /// El resultado que no es un programa bajo `hit`, o el del detalle si `hit` es
    /// uno de sus botones.
    fn extra(&self, hit: Hit) -> Option<&crate::extras::Extra> {
        match hit {
            Hit::Extra(i) => self.extras.get(i),
            Hit::DetailOpen | Hit::DetailOptions => self.extra(self.detail_hit()?),
            _ => None,
        }
    }
    /// Lo que hace elegir un resultado que no es un programa: copiar el de una
    /// operación o abrir los demás.
    fn extra_action(extra: &crate::extras::Extra) -> Action {
        match extra.kind {
            crate::extras::Kind::Calc => Action::Copy(extra.entry.command.clone()),
            _ => Action::OpenFile(extra.entry.clone()),
        }
    }
    pub fn activate(&mut self, hit: Hit) -> Option<Action> {
        if let Some(context) = self.context.take() {
            // Igual que el de energía: cualquier clic lo cierra; solo sus opciones actúan.
            self.hovered = None;
            let Hit::Context(index) = hit else {
                return None;
            };
            let command = context.menu.command(index)?;
            return match context.entry {
                Some(entry) => context_action(command, entry),
                None => {
                    if let Some(&view) = View::ALL.get(command.wrapping_sub(CMD_VIEW) as usize) {
                        self.set_view(view);
                    }
                    None
                }
            };
        }
        if self.power_menu {
            // Con el menú abierto, cualquier clic lo cierra; solo sus opciones actúan.
            self.power_menu = false;
            self.selection = None;
            self.hovered = None;
            if let Hit::PowerOption(option) = hit {
                return Some(Action::Power(option));
            }
            return None;
        }
        match hit {
            Hit::Recent(i) => {
                let item = self.recent.get(i)?;
                let entry = item.entry.clone();
                return Some(if item.file {
                    Action::OpenFile(entry)
                } else {
                    Action::Launch(entry)
                });
            }
            Hit::Pinned(_) | Hit::App(_) => return self.entry(hit).cloned().map(Action::Launch),
            Hit::Extra(_) => return self.extra(hit).map(Self::extra_action),
            Hit::ShowAllRecent => {
                let folder = crate::catalog::recent_folder()?;
                return Some(Action::OpenFile(AppEntry {
                    name: "Recientes".into(),
                    command: folder.to_string_lossy().into_owned(),
                }));
            }
            Hit::ViewPicker => self.open_view_menu(),
            Hit::Category(index) => {
                if index < self.categories.len() {
                    self.open_category = Some((index, self.scroll));
                    self.scroll = 0.0;
                    self.selection = None;
                    self.hovered = None;
                }
            }
            Hit::CategoryBack => {
                self.close_category();
            }
            Hit::ClearSearch => {
                self.query.clear();
                self.reset_results();
            }
            Hit::Power => {
                self.power_menu = true;
                self.menu_opened = animations_enabled().then(Instant::now);
                self.selection = None;
                self.hovered = None;
            }
            Hit::DetailOpen => {
                if let Some(extra) = self.extra(hit) {
                    return Some(Self::extra_action(extra));
                }
                return self.detail_entry().cloned().map(Action::Launch);
            }
            Hit::DetailOptions => {
                let (x, y, _, h) = Self::detail_options_rect();
                self.open_context(Hit::DetailOptions, x, y + h + 4.0);
            }
            Hit::DetailJump(index) => {
                let entry = self.detail_entry()?.clone();
                let item = self.jump_item(&entry, index)?.clone();
                return Some(Action::Jump(entry, item));
            }
            _ => {}
        }
        None
    }
    /// Lo que hay bajo el cursor pasa a resaltarse; en el menú contextual, su opción.
    pub fn set_hovered(&mut self, hit: Option<Hit>) {
        self.hovered = hit;
        if let Some(context) = &mut self.context {
            context.menu.hovered = match hit {
                Some(Hit::Context(index)) => Some(index),
                _ => None,
            };
        }
    }
    /// Abre el menú contextual de la app de `hit` con su esquina en (`x`, `y`), sin
    /// salirse del panel. Devuelve si había una app ahí (los archivos recientes no
    /// tienen menú de app).
    pub fn open_context(&mut self, hit: Hit, x: f32, y: f32) -> bool {
        // Los resultados que no son programas no tienen menú.
        if self.extra(hit).is_some() {
            return false;
        }
        if let Hit::Recent(i) = hit {
            if self.recent.get(i).is_some_and(|r| r.file) {
                return false;
            }
        }
        let entry = match hit {
            Hit::DetailOpen | Hit::DetailOptions => self.detail_entry(),
            _ => self.entry(hit),
        };
        let Some(entry) = entry.cloned() else {
            return false;
        };
        // Preguntarlo al Shell tarda; se recuerda por app mientras GlassBar siga abierto.
        let admin = *self
            .admin
            .entry(entry.command.clone())
            .or_insert_with(|| admin_allowed(&entry.command));
        let menu =
            crate::appmenu::AppMenu::new(context_entries(&entry, self.is_pinned(&entry), admin));
        self.show_menu(menu, x, y, Some(entry));
        true
    }
    /// Muestra `menu` con su esquina en (`x`, `y`), sin salirse del panel.
    fn show_menu(
        &mut self,
        menu: crate::appmenu::AppMenu,
        x: f32,
        y: f32,
        entry: Option<AppEntry>,
    ) {
        let (w, h) = menu.size();
        let x = x.min(WIDTH - 8.0 - w).max(8.0);
        // Debajo del cursor si cabe; si no, encima, como los menús de Windows.
        let y = if y + h > HEIGHT - 8.0 {
            (y - h).max(8.0)
        } else {
            y
        };
        self.power_menu = false;
        self.context = Some(Context { menu, x, y, entry });
        self.menu_opened = animations_enabled().then(Instant::now);
        self.hovered = None;
    }
    /// Menú de "Visualización", bajo su enlace y alineado con su borde derecho, con
    /// la vista actual marcada.
    fn open_view_menu(&mut self) {
        use crate::appmenu::{Entry, Icon};
        let entries = View::ALL
            .iter()
            .enumerate()
            .map(|(i, view)| Entry::Item {
                icon: Icon::Glyph(if *view == self.view { GLYPH_CHECK } else { ' ' }),
                label: view.label().into(),
                command: CMD_VIEW + i as u32,
            })
            .collect();
        let mut menu = crate::appmenu::AppMenu::new(entries);
        menu.hovered = View::ALL.iter().position(|v| *v == self.view);
        let Some(slot) = self
            .layout()
            .slots
            .into_iter()
            .find(|s| s.hit == Hit::ViewPicker)
        else {
            return;
        };
        let (w, _) = menu.size();
        let x = PAD + slot.x + slot.w - w;
        let y = VIEW_TOP + slot.y - self.scroll + slot.h + 4.0;
        self.show_menu(menu, x, y, None);
    }
    /// Cambia la vista de "Todo" y la recuerda para la próxima vez.
    pub fn set_view(&mut self, view: View) {
        if self.view != view {
            self.view = view;
            save_view(view);
        }
        self.open_category = None;
        self.selection = Some(Hit::ViewPicker);
        self.hovered = None;
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
    }
    /// Vuelve de una categoría abierta a "Todo", donde estaba, con su carpeta elegida.
    fn close_category(&mut self) -> bool {
        let Some((index, scroll)) = self.open_category.take() else {
            return false;
        };
        self.scroll = scroll.clamp(0.0, self.max_scroll());
        self.selection = Some(Hit::Category(index));
        self.hovered = None;
        true
    }
    /// Escape: cierra el menú flotante o la categoría abierta. Devuelve si había
    /// algo que cerrar (si no, Escape cierra el lanzador).
    pub fn back(&mut self) -> bool {
        self.close_overlay() || (!self.searching() && self.close_category())
    }
    /// Tecla de menú (o Mayús+F10): el menú de la app elegida, con su primera
    /// opción resaltada. Al buscar, el del resultado del detalle bajo "Opciones".
    pub fn open_context_from_keyboard(&mut self) -> bool {
        if self.overlay_open() {
            return false;
        }
        let opened = if self.searching() {
            let (x, y, _, h) = Self::detail_options_rect();
            self.open_context(Hit::DetailOptions, x, y + h + 4.0)
        } else {
            let slot = self
                .selection
                .and_then(|hit| self.layout().slots.into_iter().find(|s| s.hit == hit));
            match slot {
                Some(slot) => {
                    let (cx, cy) = slot.center();
                    self.open_context(slot.hit, PAD + cx, VIEW_TOP + cy - self.scroll)
                }
                None => false,
            }
        };
        if let Some(context) = &mut self.context {
            context.menu.move_hover(true);
        }
        opened
    }
    /// Resultado cuyo detalle se muestra al buscar: el elegido con las flechas o,
    /// si no hay, la mejor coincidencia.
    fn detail_hit(&self) -> Option<Hit> {
        if !self.searching() {
            return None;
        }
        let slots = self.layout().slots;
        self.selection
            .filter(|hit| slots.iter().any(|s| s.hit == *hit))
            .or_else(|| slots.first().map(|s| s.hit))
    }
    fn detail_entry(&self) -> Option<&AppEntry> {
        self.entry(self.detail_hit()?)
    }
    /// Elemento `index` de la jump list de `entry`, contando sección tras sección.
    fn jump_item(&self, entry: &AppEntry, index: usize) -> Option<&crate::jumplist::JumpItem> {
        self.jumps
            .get(&entry.command)?
            .iter()
            .flat_map(|s| &s.items)
            .nth(index)
    }
    /// Filas de la jump list del detalle que caben: (arriba, elemento, texto). Los
    /// títulos de sección no llevan elemento.
    fn jump_rows(&self) -> Vec<(f32, Option<usize>, String)> {
        let Some(sections) = self
            .detail_entry()
            .and_then(|entry| self.jumps.get(&entry.command))
        else {
            return Vec::new();
        };
        let bottom = VIEW_TOP + VIEW_H - 8.0;
        let mut rows = Vec::new();
        let (mut y, mut index) = (DETAIL_JUMPS_Y, 0);
        for section in sections {
            if !section.items.is_empty() && y + JUMP_HEADER_H + JUMP_ROW_H <= bottom {
                rows.push((y, None, section.title.clone()));
                y += JUMP_HEADER_H;
                for item in &section.items {
                    if y + JUMP_ROW_H <= bottom {
                        rows.push((y, Some(index), item.name.clone()));
                        y += JUMP_ROW_H;
                    }
                    index += 1;
                }
            } else {
                index += section.items.len();
            }
        }
        rows
    }
    fn detail_open_rect() -> (f32, f32, f32, f32) {
        let w = (DETAIL_W - 48.0) / 2.0;
        (DETAIL_X + 20.0, DETAIL_BUTTON_Y, w, DETAIL_BUTTON_H)
    }
    fn detail_options_rect() -> (f32, f32, f32, f32) {
        let (x, y, w, h) = Self::detail_open_rect();
        (x + w + 8.0, y, w, h)
    }
    /// Elemento que abre Enter: el seleccionado o, al buscar, el primer resultado.
    pub fn first_match(&self) -> Option<Hit> {
        if let Some(context) = &self.context {
            return context.menu.hovered.map(Hit::Context);
        }
        if self.power_menu {
            return self
                .selection
                .filter(|hit| matches!(hit, Hit::PowerOption(_)));
        }
        let slots = self.layout().slots;
        self.selection
            .filter(|hit| slots.iter().any(|s| s.hit == *hit))
            .or_else(|| {
                self.searching()
                    .then(|| slots.first().map(|s| s.hit))
                    .flatten()
            })
    }
    /// El clic cae sobre el campo de búsqueda (fuera del botón de limpiar).
    pub fn in_search(&self, x: f32, y: f32) -> bool {
        (SEARCH_Y..SEARCH_Y + SEARCH_H).contains(&y) && (PAD..WIDTH - PAD - 40.0).contains(&x)
    }
    fn power_button() -> (f32, f32, f32) {
        (WIDTH - PAD - 36.0, FOOTER_Y + 14.0, 36.0)
    }
    /// Menú de energía: encima del botón y alineado con su borde derecho.
    fn power_menu_rect() -> (f32, f32, f32, f32) {
        let (px, py, ps) = Self::power_button();
        let h = MENU_PAD * 2.0 + Power::ALL.len() as f32 * MENU_ROW;
        (px + ps - MENU_W, py - 8.0 - h, MENU_W, h)
    }
    fn power_option_rect(index: usize) -> (f32, f32, f32, f32) {
        let (x, y, w, _) = Self::power_menu_rect();
        (
            x + MENU_PAD,
            y + MENU_PAD + index as f32 * MENU_ROW,
            w - MENU_PAD * 2.0,
            MENU_ROW,
        )
    }
    pub fn hit_test(&self, x: f32, y: f32) -> Option<Hit> {
        let within = |left: f32, top: f32, width: f32, height: f32| {
            x >= left && x < left + width && y >= top && y < top + height
        };
        if let Some(context) = &self.context {
            let (w, h) = context.menu.size();
            if !within(context.x, context.y, w, h) {
                return Some(Hit::Outside);
            }
            // Títulos y separadores no responden, pero tampoco cierran el menú.
            return context
                .menu
                .hit_test(x - context.x, y - context.y)
                .map(Hit::Context);
        }
        if self.power_menu {
            let (px, py, ps) = Self::power_button();
            if within(px, py, ps, ps) {
                return Some(Hit::Power);
            }
            for (i, option) in Power::ALL.into_iter().enumerate() {
                let (ox, oy, ow, oh) = Self::power_option_rect(i);
                if within(ox, oy, ow, oh) {
                    return Some(Hit::PowerOption(option));
                }
            }
            let (mx, my, mw, mh) = Self::power_menu_rect();
            return (!within(mx, my, mw, mh)).then_some(Hit::Outside);
        }
        if !self.query.is_empty() && within(WIDTH - PAD - 40.0, SEARCH_Y, 40.0, SEARCH_H) {
            return Some(Hit::ClearSearch);
        }
        let (px, py, ps) = Self::power_button();
        if within(px, py, ps, ps) {
            return Some(Hit::Power);
        }
        if !(VIEW_TOP..VIEW_TOP + VIEW_H).contains(&y) {
            return None;
        }
        if self.searching() && x >= DETAIL_X - 6.0 {
            self.detail_entry()?;
            let (ox, oy, ow, oh) = Self::detail_open_rect();
            if within(ox, oy, ow, oh) {
                return Some(Hit::DetailOpen);
            }
            let (ox, oy, ow, oh) = Self::detail_options_rect();
            if within(ox, oy, ow, oh) && self.extra(Hit::DetailOptions).is_none() {
                return Some(Hit::DetailOptions);
            }
            return self
                .jump_rows()
                .into_iter()
                .find(|(top, item, _)| {
                    item.is_some() && within(DETAIL_X + 12.0, *top, DETAIL_W - 24.0, JUMP_ROW_H)
                })
                .and_then(|(_, item, _)| item.map(Hit::DetailJump));
        }
        let (cx, cy) = (x - PAD, y - VIEW_TOP + self.scroll);
        self.layout()
            .slots
            .into_iter()
            .find(|s| s.contains(cx, cy))
            .map(|s| s.hit)
    }

    pub fn draw(&mut self, image: &mut Pixmap, scale: f32) {
        self.take_loaded_icons();
        image.fill(tiny_skia::Color::TRANSPARENT);
        fill_rounded(
            image,
            background(),
            scale,
            scale,
            (WIDTH - 2.0) * scale,
            (HEIGHT - 2.0) * scale,
            12.0 * scale,
        );
        self.draw_footer(image, scale);
        stroke_rounded(
            image,
            [255, 255, 255, 34],
            scale,
            scale,
            (WIDTH - 2.0) * scale,
            (HEIGHT - 2.0) * scale,
            12.0 * scale,
            scale * 0.75,
        );
        self.draw_search(image, scale);
        self.draw_view(image, scale);
        if self.searching() {
            self.draw_detail(image, scale);
        }
        if self.power_menu {
            self.draw_power_menu(image, scale, self.menu_progress());
        }
        self.draw_context(image, scale, self.menu_progress());
    }

    fn draw_search(&mut self, image: &mut Pixmap, scale: f32) {
        let (x, y, w, h) = (PAD, SEARCH_Y, CONTENT_W, SEARCH_H);
        fill_rounded(
            image,
            [255, 255, 255, 14],
            x * scale,
            y * scale,
            w * scale,
            h * scale,
            h / 2.0 * scale,
        );
        stroke_rounded(
            image,
            [255, 255, 255, 30],
            x * scale,
            y * scale,
            w * scale,
            h * scale,
            h / 2.0 * scale,
            scale * 0.75,
        );
        draw_magnifier(image, (x + 22.0) * scale, (y + h / 2.0) * scale, scale);

        // El texto va en una capa del ancho de su zona: si no cabe, se desplaza para
        // seguir al cursor y se recorta en los bordes en lugar de cortarse con "…".
        let size = SEARCH_FONT * scale;
        let (ascent, descent) = line_metrics(size);
        let line_top = ((h * scale - (ascent + descent)) / 2.0).round();
        let baseline = line_top + ascent.round();
        let Some(mut layer) = Pixmap::new(
            (SEARCH_TEXT_W * scale).ceil() as u32,
            (h * scale).ceil() as u32,
        ) else {
            return;
        };
        let text = self.query.text();
        let width_to = |chars: usize| {
            let prefix: String = text.chars().take(chars).collect();
            text_width(&prefix, size)
        };
        let caret = width_to(self.query.caret());
        let room = layer.width() as f32 - 2.0 * scale;
        let mut offset = self.search_offset * scale;
        if caret - offset > room {
            offset = caret - room;
        }
        offset = offset
            .min(caret)
            .min((text_width(text, size) - room).max(0.0))
            .max(0.0);
        self.search_offset = offset / scale;

        if text.is_empty() {
            draw_text_on_baseline(&mut layer, "Buscar aplicaciones", 0.0, baseline, size, 0.55);
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
        if self.query.selection().is_none() && !self.overlay_open() && self.caret_on {
            fill_rounded(
                &mut layer,
                [235, 240, 250, 220],
                (caret - offset + scale).round(),
                line_top,
                scale.max(1.0),
                ascent + descent,
                0.0,
            );
        }
        image.draw_pixmap(
            (SEARCH_TEXT_X * scale).round() as i32,
            (y * scale).round() as i32,
            layer.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );

        if !self.query.is_empty() {
            let (bx, cy) = (WIDTH - PAD - 20.0, y + h / 2.0);
            if self.hovered == Some(Hit::ClearSearch) {
                fill_rounded(
                    image,
                    [255, 255, 255, 20],
                    (bx - 14.0) * scale,
                    (cy - 14.0) * scale,
                    28.0 * scale,
                    28.0 * scale,
                    14.0 * scale,
                );
            }
            // "Cancelar" de Segoe Fluent Icons, la misma cruz que usa Windows.
            draw_icon(image, '\u{E711}', bx * scale, cy * scale, 11.0 * scale, 0.8);
        }
    }

    /// Secciones desplazables, dibujadas aparte para recortarlas a su zona.
    fn draw_view(&mut self, image: &mut Pixmap, scale: f32) {
        let width = (WIDTH * scale).round() as u32;
        let height = (VIEW_H * scale).round() as u32;
        let reused = self
            .view_layer
            .take()
            .filter(|v| v.width() == width.max(1) && v.height() == height.max(1));
        let mut view = match reused {
            Some(mut view) => {
                view.fill(tiny_skia::Color::TRANSPARENT);
                view
            }
            None => match Pixmap::new(width.max(1), height.max(1)) {
                Some(view) => view,
                None => return,
            },
        };
        let layout = self.layout();
        let scroll = self.scroll;
        let top = move |y: f32| y - scroll;
        let visible = move |y: f32, h: f32| top(y) + h > 0.0 && top(y) < VIEW_H;
        for (title, y) in &layout.headers {
            if visible(*y, HEADER_H) {
                draw_text_semibold(
                    &mut view,
                    title,
                    (PAD + 8.0) * scale,
                    (top(*y) + 6.0) * scale,
                    14.0 * scale,
                    0.95,
                );
            }
        }
        if layout.slots.is_empty() && self.searching() {
            self.text(
                &mut view,
                "Prueba con otro nombre.",
                PAD + 8.0,
                HEADER_H + 4.0,
                13.0,
                scale,
                0.55,
            );
        }
        // Al buscar, el resultado del detalle queda marcado, como en Windows.
        let detail = self.detail_hit();
        let now = unix_now();
        for slot in layout.slots.iter().filter(|s| visible(s.y, s.h)) {
            let (x, y) = (PAD + slot.x, top(slot.y));
            let chosen = self.selection == Some(slot.hit) || detail == Some(slot.hit);
            let lit = chosen || self.hovered == Some(slot.hit);
            match slot.shape {
                Shape::Link => {
                    self.draw_link(&mut view, slot, x, y, lit, scale);
                    continue;
                }
                Shape::Folder => {
                    if let Hit::Category(index) = slot.hit {
                        self.draw_folder(&mut view, index, slot, x, y, lit, chosen, scale);
                    }
                    continue;
                }
                _ => {}
            }
            let Some(entry) = self.entry(slot.hit).cloned() else {
                continue;
            };
            if lit {
                fill_rounded(
                    &mut view,
                    [255, 255, 255, if chosen { 26 } else { 18 }],
                    (x + 2.0) * scale,
                    y * scale,
                    (slot.w - 4.0) * scale,
                    slot.h * scale,
                    6.0 * scale,
                );
            }
            if detail == Some(slot.hit) {
                fill_rounded(
                    &mut view,
                    crate::accent::with_alpha(crate::accent::current().light, 255),
                    (x + 2.0) * scale,
                    (y + slot.h / 2.0 - 8.0) * scale,
                    3.0 * scale,
                    16.0 * scale,
                    1.5 * scale,
                );
            }
            let extra = self
                .extra(slot.hit)
                .map(|e| (e.kind.glyph(), e.detail.clone()));
            match slot.shape {
                Shape::Row => {
                    match extra.as_ref().and_then(|(glyph, _)| *glyph) {
                        Some(glyph) => draw_icon(
                            &mut view,
                            glyph,
                            (x + 12.0 + ICON / 2.0) * scale,
                            (y + slot.h / 2.0) * scale,
                            20.0 * scale,
                            0.9,
                        ),
                        None => self.icon(
                            &mut view,
                            &entry,
                            x + 12.0,
                            y + (slot.h - ICON) / 2.0,
                            ICON,
                            scale,
                        ),
                    }
                    let name = fit_text(&entry.name, 13.0 * scale, (slot.w - 64.0) * scale);
                    let subtitle = match slot.hit {
                        Hit::Recent(i) => match self.recent.get(i) {
                            Some(item) if item.at > 0 => describe_time(now, item.at, local_moment),
                            _ => "Abierta recientemente".into(),
                        },
                        _ => match &extra {
                            Some((_, detail)) => detail.clone(),
                            None => "Aplicación".into(),
                        },
                    };
                    let subtitle = fit_text(&subtitle, 11.5 * scale, (slot.w - 64.0) * scale);
                    self.text(&mut view, &name, x + 56.0, y + 8.0, 13.0, scale, 0.95);
                    self.text(&mut view, &subtitle, x + 56.0, y + 27.0, 11.5, scale, 0.55);
                }
                Shape::Line => {
                    self.icon(
                        &mut view,
                        &entry,
                        x + 14.0,
                        y + (slot.h - LINE_ICON) / 2.0,
                        LINE_ICON,
                        scale,
                    );
                    let name = fit_text(&entry.name, 13.0 * scale, (slot.w - 64.0) * scale);
                    self.text(
                        &mut view,
                        &name,
                        x + 50.0,
                        y + slot.h / 2.0 - 9.0,
                        13.0,
                        scale,
                        0.95,
                    );
                }
                _ => {
                    self.icon(
                        &mut view,
                        &entry,
                        x + (slot.w - ICON) / 2.0,
                        y + 14.0,
                        ICON,
                        scale,
                    );
                    let label = fit_text(&entry.name, 12.0 * scale, (slot.w - 8.0) * scale);
                    let label_x = x + (slot.w - text_width(&label, 12.0 * scale) / scale) / 2.0;
                    self.text(&mut view, &label, label_x, y + 56.0, 12.0, scale, 0.9);
                }
            }
        }
        let max = (layout.height - VIEW_H).max(0.0);
        if max > 0.0 {
            // Barra de desplazamiento fina, como la del Inicio.
            let track = VIEW_H - 8.0;
            let thumb = (track * VIEW_H / layout.height).max(32.0);
            let thumb_y = 4.0 + (track - thumb) * self.scroll / max;
            // Al buscar, junto a la lista de resultados, a la izquierda del detalle.
            let bar_x = if self.searching() {
                PAD + LIST_W + 3.0
            } else {
                WIDTH - 12.0
            };
            fill_rounded(
                &mut view,
                [255, 255, 255, 70],
                bar_x * scale,
                thumb_y * scale,
                3.0 * scale,
                thumb * scale,
                1.5 * scale,
            );
        }
        image.draw_pixmap(
            0,
            (VIEW_TOP * scale).round() as i32,
            view.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        self.view_layer = Some(view);
    }

    /// Enlace de una cabecera: "Mostrar todo ›" y "Visualización: … ⌄" a la derecha;
    /// la vuelta de una categoría abierta, con la flecha y su nombre, a la izquierda.
    fn draw_link(&self, image: &mut Pixmap, slot: &Slot, x: f32, y: f32, lit: bool, scale: f32) {
        let s = |v: f32| v * scale;
        if lit {
            fill_rounded(
                image,
                [
                    255,
                    255,
                    255,
                    if self.selection == Some(slot.hit) {
                        26
                    } else {
                        18
                    },
                ],
                s(x),
                s(y),
                s(slot.w),
                s(slot.h),
                s(5.0),
            );
        }
        let label = self.link_label(slot.hit);
        let cy = y + slot.h / 2.0;
        if slot.hit == Hit::CategoryBack {
            draw_icon(image, GLYPH_BACK, s(x + 18.0), s(cy), s(13.0), 0.9);
            draw_text_semibold(image, &label, s(x + 38.0), s(cy - 10.0), s(14.0), 0.95);
            return;
        }
        let glyph = if slot.hit == Hit::ShowAllRecent {
            GLYPH_CHEVRON_RIGHT
        } else {
            GLYPH_CHEVRON_DOWN
        };
        self.text(image, &label, x + 12.0, cy - 9.0, LINK_FONT, scale, 0.85);
        draw_icon(image, glyph, s(x + slot.w - 16.0), s(cy), s(9.0), 0.8);
    }

    /// Carpeta de una categoría, como en el Inicio de Windows 11: un recuadro con
    /// sus apps más usadas en una rejilla de 2x2 (si hay más de cuatro, el último
    /// hueco lleva otras cuatro en pequeño) y el nombre debajo.
    #[allow(clippy::too_many_arguments)]
    fn draw_folder(
        &mut self,
        image: &mut Pixmap,
        index: usize,
        slot: &Slot,
        x: f32,
        y: f32,
        lit: bool,
        chosen: bool,
        scale: f32,
    ) {
        let Some(category) = self.categories.get(index).cloned() else {
            return;
        };
        let s = |v: f32| v * scale;
        let (bx, by) = (x + (slot.w - FOLDER_BOX) / 2.0, y + 4.0);
        let fill = match (lit, chosen) {
            (_, true) => 30,
            (true, _) => 22,
            _ => 12,
        };
        fill_rounded(
            image,
            [255, 255, 255, fill],
            s(bx),
            s(by),
            s(FOLDER_BOX),
            s(FOLDER_BOX),
            s(8.0),
        );
        stroke_rounded(
            image,
            [255, 255, 255, 20],
            s(bx),
            s(by),
            s(FOLDER_BOX),
            s(FOLDER_BOX),
            s(8.0),
            scale * 0.75,
        );
        let cell = FOLDER_BOX / 2.0;
        let apps: Vec<AppEntry> = category
            .apps
            .iter()
            .filter_map(|&i| self.apps.get(i).cloned())
            .collect();
        let big = if apps.len() > 4 { 3 } else { apps.len() };
        for (n, entry) in apps.iter().take(big).enumerate() {
            let (cx, cy) = (bx + (n % 2) as f32 * cell, by + (n / 2) as f32 * cell);
            self.icon(
                image,
                entry,
                cx + (cell - ICON) / 2.0,
                cy + (cell - ICON) / 2.0,
                ICON,
                scale,
            );
        }
        if apps.len() > 4 {
            // Cuarto hueco: hasta cuatro apps más en una rejilla pequeña.
            let (cx, cy) = (bx + cell, by + cell);
            let gap = 4.0;
            let span = MINI_ICON * 2.0 + gap;
            let (ox, oy) = (cx + (cell - span) / 2.0, cy + (cell - span) / 2.0);
            for (n, entry) in apps.iter().skip(3).take(4).enumerate() {
                self.icon(
                    image,
                    entry,
                    ox + (n % 2) as f32 * (MINI_ICON + gap),
                    oy + (n / 2) as f32 * (MINI_ICON + gap),
                    MINI_ICON,
                    scale,
                );
            }
        }
        let label = fit_text(category.kind.label(), s(12.0), s(slot.w - 8.0));
        let label_x = x + (slot.w - text_width(&label, s(12.0)) / scale) / 2.0;
        self.text(
            image,
            &label,
            label_x,
            by + FOLDER_BOX + 10.0,
            12.0,
            scale,
            0.9,
        );
    }

    fn draw_footer(&self, image: &mut Pixmap, scale: f32) {
        // Pie algo más oscuro: solo redondeado abajo, siguiendo las esquinas del panel.
        let (left, right) = (scale, (WIDTH - 1.0) * scale);
        let (top, bottom, r) = (FOOTER_Y * scale, (HEIGHT - 1.0) * scale, 12.0 * scale);
        let mut pb = tiny_skia::PathBuilder::new();
        pb.move_to(left, top);
        pb.line_to(right, top);
        pb.line_to(right, bottom - r);
        pb.quad_to(right, bottom, right - r, bottom);
        pb.line_to(left + r, bottom);
        pb.quad_to(left, bottom, left, bottom - r);
        pb.close();
        if let Some(path) = pb.finish() {
            let mut paint = tiny_skia::Paint {
                anti_alias: true,
                ..Default::default()
            };
            paint.set_color_rgba8(0, 0, 0, 40);
            image.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        fill_rounded(
            image,
            [255, 255, 255, 22],
            scale,
            FOOTER_Y * scale,
            (WIDTH - 2.0) * scale,
            scale.max(1.0),
            0.0,
        );
        let (cx, cy) = (PAD + 30.0, FOOTER_Y + 32.0);
        fill_rounded(
            image,
            [255, 255, 255, 34],
            (cx - 16.0) * scale,
            (cy - 16.0) * scale,
            32.0 * scale,
            32.0 * scale,
            16.0 * scale,
        );
        let initial: String = self
            .user
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        let w = text_width(&initial, 14.0 * scale) / scale;
        draw_text_semibold(
            image,
            &initial,
            (cx - w / 2.0) * scale,
            (cy - 6.0) * scale,
            14.0 * scale,
            0.9,
        );
        let name = fit_text(&self.user, 13.0 * scale, 360.0 * scale);
        self.text(image, &name, cx + 28.0, cy - 9.0, 13.0, scale, 0.9);
        let (px, py, ps) = Self::power_button();
        if self.hovered == Some(Hit::Power) || self.power_menu {
            fill_rounded(
                image,
                [255, 255, 255, if self.power_menu { 30 } else { 22 }],
                px * scale,
                py * scale,
                ps * scale,
                ps * scale,
                6.0 * scale,
            );
        }
        draw_power_glyph(
            image,
            (px + ps / 2.0) * scale,
            (py + ps / 2.0) * scale,
            scale,
        );
    }

    /// Avance de la entrada del menú de energía, de 0 a 1 con frenado al final
    /// (curva cúbica), como los menús flotantes de Windows 11.
    fn menu_progress(&self) -> f32 {
        ease_out(
            self.menu_opened
                .map(|t| t.elapsed().as_secs_f32() / MENU_ANIM.as_secs_f32())
                .unwrap_or(1.0),
        )
    }
    /// El menú sigue entrando y hace falta redibujar el siguiente fotograma.
    pub fn animating(&self) -> bool {
        self.overlay_open() && self.menu_progress() < 1.0
    }

    /// Menú flotante de energía, como el del Inicio de Windows 11. Se dibuja en una
    /// capa aparte para entrar desvaneciéndose y subiendo unos píxeles.
    fn draw_power_menu(&self, image: &mut Pixmap, scale: f32, progress: f32) {
        let (x, y, w, h) = Self::power_menu_rect();
        let margin = 4.0;
        let (gx, gy) = (x - margin, y - margin);
        let Some(mut layer) = Pixmap::new(
            ((w + margin * 2.0) * scale).ceil() as u32,
            ((h + margin * 2.0) * scale).ceil() as u32,
        ) else {
            return;
        };
        // Coordenadas del panel pasadas a la capa, ya en píxeles.
        let lx = |v: f32| (v - gx) * scale;
        let ly = |v: f32| (v - gy) * scale;
        // Sombra suave para separarlo del panel.
        fill_rounded(
            &mut layer,
            [0, 0, 0, 60],
            lx(x - 1.0),
            ly(y + 2.0),
            (w + 2.0) * scale,
            (h + 2.0) * scale,
            9.0 * scale,
        );
        fill_rounded(
            &mut layer,
            [44, 44, 46, 255],
            lx(x),
            ly(y),
            w * scale,
            h * scale,
            8.0 * scale,
        );
        stroke_rounded(
            &mut layer,
            [255, 255, 255, 34],
            lx(x),
            ly(y),
            w * scale,
            h * scale,
            8.0 * scale,
            scale * 0.75,
        );
        for (i, option) in Power::ALL.into_iter().enumerate() {
            let (ox, oy, ow, oh) = Self::power_option_rect(i);
            let hit = Some(Hit::PowerOption(option));
            if self.hovered == hit || self.selection == hit {
                fill_rounded(
                    &mut layer,
                    [255, 255, 255, 22],
                    lx(ox),
                    ly(oy + 2.0),
                    ow * scale,
                    (oh - 4.0) * scale,
                    5.0 * scale,
                );
            }
            draw_icon(
                &mut layer,
                option.glyph(),
                lx(ox + 22.0),
                ly(oy + oh / 2.0),
                16.0 * scale,
                0.9,
            );
            draw_text(
                &mut layer,
                option.label(),
                lx(ox + 46.0),
                ly(oy + 10.0),
                13.0 * scale,
                0.95,
            );
        }
        image.draw_pixmap(
            (gx * scale).round() as i32,
            ((gy + (1.0 - progress) * MENU_RISE) * scale).round() as i32,
            layer.as_ref(),
            &PixmapPaint {
                opacity: progress,
                ..Default::default()
            },
            Transform::identity(),
            None,
        );
    }

    /// Detalle del resultado elegido, como la búsqueda de Windows 11: nombre, icono
    /// grande, "Abrir" y "Opciones", y debajo la jump list de la app.
    fn draw_detail(&mut self, image: &mut Pixmap, scale: f32) {
        let Some(entry) = self.detail_entry().cloned() else {
            return;
        };
        let command = entry.command.clone();
        let big = (DETAIL_ICON * scale).round().max(1.0) as u32;
        // Un resultado que no es un programa: su icono de la fuente (los archivos, el
        // suyo), qué es en lugar de "Aplicación", y ni opciones ni jump list.
        let extra = self
            .extra(Hit::DetailOpen)
            .map(|e| (e.kind, e.detail.clone()));
        let glyph = extra.as_ref().and_then(|(kind, _)| kind.glyph());
        let icon = if glyph.is_some() {
            None
        } else {
            self.detail_icons
                .entry(command.clone())
                .or_insert_with(|| {
                    crate::icons::load(&command).map(|icon| {
                        if icon.width() > big {
                            crate::apps::downscale(&icon, big)
                        } else {
                            icon
                        }
                    })
                })
                .clone()
        };
        self.jumps.entry(command.clone()).or_insert_with(|| {
            if extra.is_some() {
                return Vec::new();
            }
            crate::jumplist::app_id(&command, &[])
                .map(|id| crate::jumplist::sections(&id))
                .unwrap_or_default()
        });

        let s = |v: f32| v * scale;
        let (x, y, w, h) = (DETAIL_X, VIEW_TOP, DETAIL_W, VIEW_H);
        fill_rounded(image, [255, 255, 255, 10], s(x), s(y), s(w), s(h), s(8.0));
        stroke_rounded(
            image,
            [255, 255, 255, 18],
            s(x),
            s(y),
            s(w),
            s(h),
            s(8.0),
            scale * 0.75,
        );
        let name = fit_text(&entry.name, s(16.0), s(w - 40.0));
        draw_text_semibold(image, &name, s(x + 20.0), s(y + 18.0), s(16.0), 0.95);
        let what = match &extra {
            Some((_, detail)) => detail.as_str(),
            None => "Aplicación",
        };
        self.text(image, what, x + 20.0, y + 42.0, 12.0, scale, 0.55);

        // Tarjeta con el icono grande.
        let (cx, cy, cw, ch) = (x + 20.0, y + 70.0, w - 40.0, 112.0);
        fill_rounded(
            image,
            [255, 255, 255, 8],
            s(cx),
            s(cy),
            s(cw),
            s(ch),
            s(6.0),
        );
        stroke_rounded(
            image,
            [255, 255, 255, 20],
            s(cx),
            s(cy),
            s(cw),
            s(ch),
            s(6.0),
            scale * 0.75,
        );
        if let Some(icon) = &icon {
            blit(
                image,
                icon,
                s(cx + (cw - DETAIL_ICON) / 2.0),
                s(cy + (ch - DETAIL_ICON) / 2.0),
                s(DETAIL_ICON),
            );
        }
        if let Some(glyph) = glyph {
            draw_icon(
                image,
                glyph,
                s(cx + cw / 2.0),
                s(cy + ch / 2.0),
                s(DETAIL_ICON * 0.7),
                0.9,
            );
        }

        // "Abrir" y "Opciones", con el resaltado al pasar el cursor.
        for (hit, (bx, by, bw, bh)) in [
            (Hit::DetailOpen, Self::detail_open_rect()),
            (Hit::DetailOptions, Self::detail_options_rect()),
        ] {
            if hit == Hit::DetailOptions && extra.is_some() {
                continue;
            }
            let active =
                self.hovered == Some(hit) || (hit == Hit::DetailOptions && self.context.is_some());
            fill_rounded(
                image,
                [255, 255, 255, if active { 30 } else { 16 }],
                s(bx),
                s(by),
                s(bw),
                s(bh),
                s(4.0),
            );
            let label = match (hit, &extra) {
                (Hit::DetailOpen, Some((crate::extras::Kind::Calc, _))) => "Copiar",
                (Hit::DetailOpen, _) => "Abrir",
                _ => "Opciones",
            };
            let size = 13.0;
            let label_w = text_width(label, s(size)) / scale;
            let extra = if hit == Hit::DetailOptions { 18.0 } else { 0.0 };
            let lx = bx + (bw - label_w - extra) / 2.0;
            self.text(image, label, lx, by + 8.0, size, scale, 0.95);
            if hit == Hit::DetailOptions {
                draw_icon(
                    image,
                    GLYPH_CHEVRON_DOWN,
                    s(lx + label_w + 12.0),
                    s(by + bh / 2.0),
                    s(10.0),
                    0.8,
                );
            }
        }

        // Jump list: recientes, frecuentes y tareas de la app, hasta donde quepan.
        for (top, item, label) in self.jump_rows() {
            match item {
                None => draw_text_semibold(image, &label, s(x + 20.0), s(top + 7.0), s(13.0), 0.9),
                Some(index) => {
                    if self.hovered == Some(Hit::DetailJump(index)) {
                        fill_rounded(
                            image,
                            [255, 255, 255, 18],
                            s(x + 12.0),
                            s(top),
                            s(w - 24.0),
                            s(JUMP_ROW_H - 2.0),
                            s(4.0),
                        );
                    }
                    if let Some(icon) = &icon {
                        blit(image, icon, s(x + 20.0), s(top + 6.0), s(16.0));
                    }
                    let label = fit_text(&label, s(13.0), s(w - 72.0));
                    self.text(image, &label, x + 46.0, top + 6.0, 13.0, scale, 0.9);
                }
            }
        }
    }

    /// Menú contextual de una app, dentro del panel. Entra como el de energía.
    fn draw_context(&self, image: &mut Pixmap, scale: f32, progress: f32) {
        let Some(context) = &self.context else {
            return;
        };
        let (w, h) = context.menu.size();
        let margin = 4.0;
        let Some(mut layer) = Pixmap::new(
            ((w + margin * 2.0) * scale).ceil() as u32,
            ((h + margin * 2.0) * scale).ceil() as u32,
        ) else {
            return;
        };
        // Sombra y una base opaca: el fondo del menú es translúcido y aquí no hay
        // desenfoque debajo, solo el contenido del panel.
        fill_rounded(
            &mut layer,
            [0, 0, 0, 60],
            (margin - 1.0) * scale,
            (margin + 2.0) * scale,
            (w + 2.0) * scale,
            (h + 2.0) * scale,
            9.0 * scale,
        );
        fill_rounded(
            &mut layer,
            [36, 36, 38, 255],
            (margin + 0.5) * scale,
            (margin + 0.5) * scale,
            (w - 1.0) * scale,
            (h - 1.0) * scale,
            8.0 * scale,
        );
        context.menu.draw_at(&mut layer, scale, margin, margin);
        image.draw_pixmap(
            ((context.x - margin) * scale).round() as i32,
            ((context.y - margin + (1.0 - progress) * MENU_RISE * 0.5) * scale).round() as i32,
            layer.as_ref(),
            &PixmapPaint {
                opacity: progress,
                ..Default::default()
            },
            Transform::identity(),
            None,
        );
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

    /// Icono de `entry` con su esquina en (`x`, `y`) y `side` píxeles lógicos de lado.
    /// Se guarda reducido al tamaño `ICON`; los más pequeños se reducen al dibujar.
    fn icon(
        &mut self,
        image: &mut Pixmap,
        entry: &AppEntry,
        x: f32,
        y: f32,
        side: f32,
        scale: f32,
    ) {
        let size = (ICON * scale).round().max(1.0) as u32;
        if !self.icons.contains_key(&entry.command) {
            // Cargar un icono tarda decenas de ms: con hilo de carga se pide y se
            // dibuja cuando llegue, sin detener el panel.
            if let Some(loader) = &self.loader {
                if self.requested.insert(entry.command.clone()) {
                    let _ = loader.requests.send(entry.command.clone());
                }
                return;
            }
        }
        let icon = self
            .icons
            .entry(entry.command.clone())
            .or_insert_with(|| crate::icons::load(&entry.command));
        // Se guarda ya reducido: la escala del lanzador es fija durante su vida.
        if icon.as_ref().is_some_and(|i| i.width() > size) {
            *icon = icon.as_ref().map(|i| crate::apps::downscale(i, size));
        }
        if let Some(icon) = icon {
            let side = (side * scale).round().max(1.0);
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
}

/// Hilo que carga los iconos del lanzador. Al terminar cada uno avisa a la ventana
/// con `message`, una sola vez hasta que el lanzador recoja los que llegaron.
pub struct IconLoader {
    requests: std::sync::mpsc::Sender<String>,
    loaded: std::sync::mpsc::Receiver<(String, Option<Pixmap>)>,
    notified: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl IconLoader {
    /// Los iconos se guardan ya reducidos al tamaño en que se dibujan a `scale`.
    pub fn start(scale: f32, window: isize, message: u32) -> Self {
        use std::sync::atomic::Ordering::Relaxed;
        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
        let side = (ICON * scale).round().max(1.0) as u32;
        let (requests, queue) = std::sync::mpsc::channel::<String>();
        let (done, loaded) = std::sync::mpsc::channel();
        let notified = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = notified.clone();
        std::thread::spawn(move || {
            for command in queue {
                let icon = crate::icons::load(&command).map(|icon| {
                    if icon.width() > side {
                        crate::apps::downscale(&icon, side)
                    } else {
                        icon
                    }
                });
                if done.send((command, icon)).is_err() {
                    return;
                }
                if !flag.swap(true, Relaxed) {
                    unsafe {
                        let _ = PostMessageW(
                            Some(HWND(window as *mut _)),
                            message,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                }
            }
        });
        Self {
            requests,
            loaded,
            notified,
        }
    }
}

/// Uso que registra Windows de una app del catálogo, si lo hay.
fn usage_of(usage: &HashMap<String, Usage>, entry: &AppEntry) -> Option<Usage> {
    let name = entry.command.strip_prefix("shell:AppsFolder\\")?;
    usage.get(&name.to_lowercase()).copied()
}

/// Recientes a partir del uso que registra Windows, del historial de GlassBar y de
/// los archivos recientes de Windows (`files`, con su hora). Solo entran apps del
/// catálogo (accesos directos o apps instaladas), así no aparecen procesos sueltos
/// ni desinstaladores.
fn rank(
    apps: &[AppEntry],
    usage: &HashMap<String, Usage>,
    history: &[Launch],
    files: Vec<(AppEntry, u64)>,
) -> Vec<RecentItem> {
    // La apertura más reciente según Windows o GlassBar. Sin tiempo en primer
    // plano son aperturas del sistema (p. ej. la barra de juegos), no del usuario.
    let mut recent: Vec<RecentItem> = apps
        .iter()
        .filter_map(|e| Some((e, usage_of(usage, e)?)))
        .filter(|(_, u)| u.last > 0 && u.focus_ms > 0)
        .map(|(e, u)| RecentItem {
            entry: e.clone(),
            at: u.last,
            file: false,
        })
        .collect();
    for launch in history {
        match recent
            .iter_mut()
            .find(|r| r.entry.command.eq_ignore_ascii_case(&launch.entry.command))
        {
            Some(item) => item.at = item.at.max(launch.at),
            None => recent.push(RecentItem {
                entry: launch.entry.clone(),
                at: launch.at,
                file: false,
            }),
        }
    }
    recent.extend(files.into_iter().map(|(entry, at)| RecentItem {
        entry,
        at,
        file: true,
    }));
    // Los historiales antiguos (sin hora) conservan su orden, detrás de los fechados.
    recent.sort_by_key(|item| std::cmp::Reverse(item.at));
    recent.truncate(MAX_RECENT);
    recent
}

/// Segundos Unix de ahora.
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Fecha y hora locales de un instante.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Moment {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
}

/// Fecha civil (año, mes, día) del día `days` contado desde el 1-1-1970.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Algoritmo de Howard Hinnant para el calendario gregoriano.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// `Moment` de `unix` desplazado `offset` segundos (la zona horaria local).
fn moment(unix: i64, offset: i64) -> Moment {
    let local = unix + offset;
    let (year, month, day) = civil_from_days(local.div_euclid(86_400));
    let seconds = local.rem_euclid(86_400);
    Moment {
        year,
        month,
        day,
        hour: (seconds / 3600) as u32,
        minute: (seconds % 3600 / 60) as u32,
    }
}

/// Hora local de `unix`, con la diferencia actual entre la hora local y la UTC.
fn local_moment(unix: u64) -> Moment {
    use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
    let seconds = |t: windows::Win32::Foundation::SYSTEMTIME| {
        let (y, m, d) = (t.wYear as i64, t.wMonth as i64, t.wDay as i64);
        // Días desde el 1-1-1970 de esa fecha (inverso de `civil_from_days`).
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        days * 86_400 + t.wHour as i64 * 3600 + t.wMinute as i64 * 60
    };
    let (local, utc) = unsafe { (GetLocalTime(), GetSystemTime()) };
    // Redondeada al cuarto de hora: las zonas horarias van de 15 en 15 minutos.
    let offset = ((seconds(local) - seconds(utc)) as f64 / 900.0).round() as i64 * 900;
    moment(unix as i64, offset)
}

/// Cuándo se abrió algo, como lo dice el Inicio de Windows 11: "Hace 5 min",
/// "Hace 13 h", "Ayer a las 11:58 a. m.", "3 de nov." o, de otro año, "3/11/2025".
fn describe_time(now: u64, at: u64, local: impl Fn(u64) -> Moment) -> String {
    let ago = now.saturating_sub(at);
    if ago < 60 {
        return "Hace un momento".into();
    }
    if ago < 3600 {
        return format!("Hace {} min", ago / 60);
    }
    if ago < 86_400 {
        return format!("Hace {} h", ago / 3600);
    }
    let then = local(at);
    let today = local(now);
    let yesterday = local(now.saturating_sub(86_400));
    if (then.year, then.month, then.day) == (yesterday.year, yesterday.month, yesterday.day) {
        let hour = match then.hour % 12 {
            0 => 12,
            h => h,
        };
        let half = if then.hour < 12 { "a. m." } else { "p. m." };
        return format!("Ayer a las {hour}:{:02} {half}", then.minute);
    }
    const MONTHS: [&str; 12] = [
        "ene.", "feb.", "mar.", "abr.", "may.", "jun.", "jul.", "ago.", "sept.", "oct.", "nov.",
        "dic.",
    ];
    if then.year == today.year {
        format!(
            "{} de {}",
            then.day,
            MONTHS[(then.month as usize).clamp(1, 12) - 1]
        )
    } else {
        format!("{}/{}/{}", then.day, then.month, then.year)
    }
}

/// Archivo donde se recuerda la vista elegida para "Todo".
fn view_path() -> Option<std::path::PathBuf> {
    Some(
        std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("GlassBar")
            .join("start-view.txt"),
    )
}
/// Vista de "Todo" guardada; por categorías si no hay ninguna, como Windows.
fn load_view() -> View {
    if cfg!(test) {
        return View::Category;
    }
    let saved = view_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    View::ALL
        .into_iter()
        .find(|v| v.key() == saved.trim())
        .unwrap_or(View::Category)
}
fn save_view(view: View) {
    if cfg!(test) {
        return;
    }
    if let Some(path) = view_path() {
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let _ = std::fs::write(path, view.key());
    }
}

/// Dibuja `icon` con su esquina en (`x`, `y`) y `side` píxeles de lado.
fn blit(image: &mut Pixmap, icon: &Pixmap, x: f32, y: f32, side: f32) {
    let transform = Transform::from_scale(side / icon.width() as f32, side / icon.height() as f32)
        .post_translate(x.round(), y.round());
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

fn stroke(image: &mut Pixmap, path: tiny_skia::Path, scale: f32) {
    let mut paint = tiny_skia::Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color_rgba8(255, 255, 255, 200);
    let stroke = tiny_skia::Stroke {
        width: 1.5 * scale,
        line_cap: tiny_skia::LineCap::Round,
        ..Default::default()
    };
    image.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

/// Lupa: círculo con mango hacia abajo a la derecha.
fn draw_magnifier(image: &mut Pixmap, cx: f32, cy: f32, scale: f32) {
    let r = 5.5 * scale;
    let (ox, oy) = (cx - 1.5 * scale, cy - 1.5 * scale);
    let mut pb = tiny_skia::PathBuilder::new();
    pb.push_circle(ox, oy, r);
    let d = r * std::f32::consts::FRAC_1_SQRT_2;
    pb.move_to(ox + d, oy + d);
    pb.line_to(ox + d + 4.5 * scale, oy + d + 4.5 * scale);
    if let Some(path) = pb.finish() {
        stroke(image, path, scale);
    }
}

/// Símbolo de encendido: arco abierto arriba y una línea vertical.
fn draw_power_glyph(image: &mut Pixmap, cx: f32, cy: f32, scale: f32) {
    let r = 7.0 * scale;
    let mut pb = tiny_skia::PathBuilder::new();
    let (start, end) = (-60f32.to_radians(), 240f32.to_radians());
    let steps = 28;
    for i in 0..=steps {
        // Ángulos medidos desde arriba, en sentido horario.
        let a = start + (end - start) * i as f32 / steps as f32;
        let (x, y) = (cx + r * a.sin(), cy - r * a.cos());
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    pb.move_to(cx, cy - r - 1.5 * scale);
    pb.line_to(cx, cy - 1.0 * scale);
    if let Some(path) = pb.finish() {
        stroke(image, path, scale);
    }
}

/// Apertura y cierre del panel completo: entra subiendo y apareciendo, y sale
/// bajando y desvaneciéndose. Si se invierte a medias, sigue desde donde estaba.
pub struct Transition {
    opening: bool,
    /// Avance lineal (0 oculto, 1 abierto) al empezar el tramo actual.
    from: f32,
    since: Instant,
    animate: bool,
}
impl Transition {
    pub fn hidden() -> Self {
        Self {
            opening: false,
            from: 0.0,
            since: Instant::now(),
            animate: true,
        }
    }
    fn level(&self, now: Instant) -> f32 {
        if !self.animate {
            return if self.opening { 1.0 } else { 0.0 };
        }
        let elapsed = now.saturating_duration_since(self.since).as_secs_f32();
        if self.opening {
            (self.from + elapsed / OPEN_ANIM.as_secs_f32()).min(1.0)
        } else {
            (self.from - elapsed / CLOSE_ANIM.as_secs_f32()).max(0.0)
        }
    }
    fn turn(&mut self, opening: bool, now: Instant, animate: bool) {
        self.from = self.level(now);
        self.opening = opening;
        self.since = now;
        self.animate = animate;
    }
    pub fn open(&mut self, now: Instant) {
        self.turn(true, now, animations_enabled());
    }
    pub fn close(&mut self, now: Instant) {
        self.turn(false, now, animations_enabled());
    }
    /// Cuánto se ve el panel, de 0 a 1. Con la misma curva en ambos sentidos, al
    /// abrir frena al llegar y al cerrar arranca suave y acelera al irse.
    pub fn shown(&self, now: Instant) -> f32 {
        ease_out(self.level(now))
    }
    /// Desplazamiento hacia abajo, en píxeles lógicos, para ese momento.
    pub fn offset(&self, now: Instant) -> f32 {
        (1.0 - self.shown(now)) * PANEL_RISE
    }
    pub fn running(&self, now: Instant) -> bool {
        let level = self.level(now);
        if self.opening {
            level < 1.0
        } else {
            level > 0.0
        }
    }
}

/// Curva de salida cúbica: arranca rápido y frena al llegar, como Fluent.
fn ease_out(linear: f32) -> f32 {
    1.0 - (1.0 - linear.clamp(0.0, 1.0)).powi(3)
}

/// Respeta "Efectos de animación" de Windows (Accesibilidad): si están desactivados,
/// el panel y el menú aparecen sin transición.
fn animations_enabled() -> bool {
    use windows::core::BOOL;
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };
    let mut enabled = BOOL(1);
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut enabled as *mut BOOL as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    enabled.as_bool()
}

/// Nombre completo de la cuenta (p. ej. "Gerson Portal Linares"); si Windows no
/// lo da, el nombre de usuario.
fn user_display_name() -> String {
    use windows::core::PWSTR;
    use windows::Win32::Security::Authentication::Identity::{GetUserNameExW, NameDisplay};
    let mut buf = vec![0u16; 256];
    let mut len = buf.len() as u32;
    let name = unsafe { GetUserNameExW(NameDisplay, Some(PWSTR(buf.as_mut_ptr())), &mut len) }
        .then(|| String::from_utf16_lossy(&buf[..len as usize]))
        .filter(|n| !n.trim().is_empty());
    name.or_else(|| std::env::var("USERNAME").ok())
        .unwrap_or_else(|| "Usuario".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(name: &str, command: &str) -> AppEntry {
        AppEntry {
            name: name.into(),
            command: command.into(),
        }
    }
    fn state() -> LauncherState {
        let mut state = LauncherState {
            apps: (0..25)
                .map(|i| entry(&format!("Editor {i:02}"), &format!("{i}.exe")))
                .collect(),
            recent: vec![],
            pinned: vec![],
            categories: vec![],
            alphabetical: vec![],
            view: View::Category,
            open_category: None,
            query: TextField::default(),
            scroll: 0.0,
            hovered: None,
            power_menu: false,
            menu_opened: None,
            context: None,
            selection: None,
            search_offset: 0.0,
            caret_on: true,
            last_press: None,
            user: "Usuario de prueba".into(),
            icons: HashMap::new(),
            loader: None,
            requested: Default::default(),
            view_layer: None,
            loaded: Instant::now(),
            search_cache: RefCell::new(None),
            detail_icons: HashMap::new(),
            jumps: HashMap::new(),
            admin: HashMap::new(),
            files: Vec::new(),
            extras: Vec::new(),
        };
        state.regroup(&HashMap::new());
        state
    }
    fn recent_app(name: &str, command: &str) -> RecentItem {
        RecentItem {
            entry: entry(name, command),
            at: 0,
            file: false,
        }
    }
    fn many(prefix: &str, count: usize) -> Vec<AppEntry> {
        (0..count)
            .map(|i| entry(&format!("{prefix} {i:02}"), &format!("{prefix}{i}.exe")))
            .collect()
    }
    /// Punto de pantalla en el centro del elemento, con el desplazamiento actual.
    fn point_of(menu: &LauncherState, hit: Hit) -> (f32, f32) {
        let slot = menu
            .layout()
            .slots
            .into_iter()
            .find(|s| s.hit == hit)
            .unwrap();
        let (x, y) = slot.center();
        (x + PAD, y + VIEW_TOP - menu.scroll)
    }

    #[test]
    fn browse_view_shows_pinned_recent_and_everything_by_category() {
        let mut menu = state();
        menu.pinned = vec![entry("Explorador", "explorer.exe")];
        menu.recent = vec![recent_app("Spotify", "spotify.exe")];
        let layout = menu.layout();
        let titles: Vec<_> = layout.headers.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(titles, ["Anclado", "Recientes", "Todo"]);
        let hits: Vec<_> = layout.slots.iter().map(|s| s.hit).collect();
        assert_eq!(
            hits,
            [
                Hit::Pinned(0),
                Hit::ShowAllRecent,
                Hit::Recent(0),
                Hit::ViewPicker,
                Hit::Category(0)
            ]
        );
        let (x, y) = point_of(&menu, Hit::Recent(0));
        assert_eq!(menu.hit_test(x, y), Some(Hit::Recent(0)));
        // Al buscar solo quedan los resultados: las apps y, al final, la web.
        menu.insert_text("editor 24");
        assert!(menu
            .layout()
            .slots
            .iter()
            .all(|s| matches!(s.hit, Hit::App(_) | Hit::Extra(_))));
        assert_eq!(menu.layout().slots.len(), 2);
    }

    #[test]
    fn rank_uses_windows_usage_and_glassbar_history() {
        let apps = vec![
            entry("Google Chrome", "shell:AppsFolder\\Chrome"),
            entry("Warp", "shell:AppsFolder\\dev.warp.Warp"),
            entry("Barra de juegos", "shell:AppsFolder\\Xbox!App"),
            entry("Desinstalar", "shell:AppsFolder\\uninstall.exe"),
            entry("Notas", "shell:AppsFolder\\Notes!App"),
        ];
        let at = |runs, focus_ms, last| Usage {
            runs,
            focus_ms,
            last,
        };
        let usage = HashMap::from([
            ("chrome".to_string(), at(15, 1141, 300)),
            ("dev.warp.warp".to_string(), at(6, 2_751_768, 200)),
            // Abierta por el sistema, nunca en primer plano.
            ("xbox!app".to_string(), at(50, 0, 100)),
            ("notes!app".to_string(), at(3, 500, 50)),
            // Usada pero fuera del catálogo: se ignora.
            ("c:\\tools\\helper.exe".to_string(), at(99, 99, 999)),
        ]);
        let names = |list: &[RecentItem]| {
            list.iter()
                .map(|r| (r.entry.name.clone(), r.file))
                .collect::<Vec<_>>()
        };

        // Por hora, apps y archivos mezclados; la apertura desde GlassBar (400)
        // adelanta a Notas y la barra de juegos (sin primer plano) no entra.
        let history = vec![Launch {
            entry: entry("Notas", "shell:AppsFolder\\Notes!App"),
            at: 400,
        }];
        let files = vec![(entry("foto.jpg", "C:\\Recent\\foto.jpg.lnk"), 250)];
        let recent = rank(&apps, &usage, &history, files);
        assert_eq!(
            names(&recent),
            [
                ("Notas".to_string(), false),
                ("Google Chrome".to_string(), false),
                ("foto.jpg".to_string(), true),
                ("Warp".to_string(), false)
            ]
        );
        assert_eq!(recent[0].at, 400);

        // Sin fechas de Windows no hay recientes de apps.
        let undated: HashMap<_, _> = usage
            .into_iter()
            .map(|(k, u)| (k, Usage { last: 0, ..u }))
            .collect();
        assert!(rank(&apps, &undated, &[], vec![]).is_empty());
    }

    #[test]
    fn recent_files_open_without_the_app_menu() {
        let mut menu = state();
        menu.recent = vec![RecentItem {
            entry: entry("foto.jpg", "C:\\Recent\\foto.jpg.lnk"),
            at: 100,
            file: true,
        }];
        assert!(matches!(
            menu.activate(Hit::Recent(0)),
            Some(Action::OpenFile(e)) if e.name == "foto.jpg"
        ));
        let (x, y) = point_of(&menu, Hit::Recent(0));
        assert!(!menu.open_context(Hit::Recent(0), x, y));
        assert!(!menu.overlay_open());
    }

    #[test]
    fn a_category_opens_with_its_apps_and_escape_goes_back() {
        let mut menu = state();
        menu.recent = many("Reciente", 6)
            .into_iter()
            .map(|e| RecentItem {
                entry: e,
                at: 0,
                file: false,
            })
            .collect();
        menu.pinned = many("Anclada", 18);
        menu.scroll_by(10_000.0);
        let before = menu.scroll;
        assert!(before > 0.0);
        let (x, y) = point_of(&menu, Hit::Category(0));
        let hit = menu.hit_test(x, y).unwrap();
        assert_eq!(hit, Hit::Category(0));
        assert!(menu.activate(hit).is_none());
        let hits: Vec<_> = menu.layout().slots.iter().map(|s| s.hit).collect();
        assert_eq!(hits[0], Hit::CategoryBack);
        assert_eq!(hits.len(), 1 + 25);
        assert!(hits[1..].iter().all(|h| matches!(h, Hit::App(_))));
        assert_eq!(menu.scroll, 0.0);
        assert_eq!(menu.link_label(Hit::CategoryBack), "Otros");
        // Escape vuelve a "Todo" donde estaba; otra vez ya no hay nada que cerrar.
        assert!(menu.back());
        assert_eq!(menu.scroll, before);
        assert_eq!(menu.selection, Some(Hit::Category(0)));
        assert!(!menu.back());
    }

    #[test]
    fn the_view_picker_switches_to_grid_and_list() {
        let mut menu = state();
        assert_eq!(menu.link_label(Hit::ViewPicker), "Visualización: Categoría");
        assert!(menu.activate(Hit::ViewPicker).is_none());
        assert!(menu.overlay_open());
        let context = menu.context.as_ref().unwrap();
        assert!(context.entry.is_none());
        // La vista actual sale resaltada.
        assert_eq!(context.menu.hovered, Some(0));
        let (cx, cy) = (context.x, context.y);
        let (_, h) = context.menu.size();
        // Tercera opción: Lista.
        let hit = menu.hit_test(cx + 20.0, cy + h - 20.0).unwrap();
        assert!(menu.activate(hit).is_none());
        assert_eq!(menu.view, View::List);
        assert!(!menu.overlay_open());
        let layout = menu.layout();
        let titles: Vec<_> = layout.headers.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(titles, ["Todo", "E"]);
        let lines: Vec<_> = layout
            .slots
            .iter()
            .filter(|s| s.shape == Shape::Line)
            .map(|s| s.hit)
            .collect();
        assert_eq!(lines.len(), 25);
        assert_eq!(lines[0], Hit::App(0));

        menu.set_view(View::Grid);
        let tiles = menu
            .layout()
            .slots
            .iter()
            .filter(|s| s.shape == Shape::Tile)
            .count();
        assert_eq!(tiles, 25);
    }

    #[test]
    fn recent_times_read_like_windows() {
        let utc = |unix: u64| moment(unix as i64, 0);
        // 15-11-2026 12:00 UTC.
        let now = 1_794_744_000;
        assert_eq!(
            utc(now),
            Moment {
                year: 2026,
                month: 11,
                day: 15,
                hour: 12,
                minute: 0
            }
        );
        assert_eq!(describe_time(now, now - 30, utc), "Hace un momento");
        assert_eq!(describe_time(now, now - 5 * 60, utc), "Hace 5 min");
        assert_eq!(describe_time(now, now - 13 * 3600, utc), "Hace 13 h");
        // Ayer a las 11:58, hace más de un día.
        assert_eq!(
            describe_time(now, now - 86_400 - 120, utc),
            "Ayer a las 11:58 a. m."
        );
        assert_eq!(describe_time(now, now - 12 * 86_400, utc), "3 de nov.");
        assert_eq!(describe_time(now, now - 400 * 86_400, utc), "11/10/2025");
        // Con zona horaria: 12:00 UTC son las 7:00 en UTC-5.
        assert_eq!(moment(now as i64, -5 * 3600).hour, 7);
    }

    #[test]
    fn hit_test_and_scroll_target_the_displayed_results() {
        let mut menu = state();
        menu.apps = many("App", 60);
        menu.insert_text("app");
        let (x, y) = point_of(&menu, Hit::App(7));
        assert_eq!(menu.hit_test(x, y), Some(Hit::App(7)));
        // Lo que queda fuera de la vista no se puede pulsar hasta desplazarse.
        let (x, y) = point_of(&menu, Hit::App(59));
        assert!(y > VIEW_TOP + VIEW_H);
        assert_eq!(menu.hit_test(x, y), None);
        menu.scroll_by(10_000.0);
        assert_eq!(menu.scroll, menu.max_scroll());
        let (x, y) = point_of(&menu, Hit::App(59));
        assert_eq!(menu.hit_test(x, y), Some(Hit::App(59)));
    }

    #[test]
    fn arrows_move_through_sections_and_scroll_into_view() {
        let mut menu = state();
        menu.pinned = many("Anclada", 8);
        menu.view = View::Grid;
        menu.move_selection(0, 1);
        assert_eq!(menu.selection, Some(Hit::Pinned(0)));
        menu.move_selection(0, 1);
        assert_eq!(menu.selection, Some(Hit::Pinned(COLS)));
        menu.move_selection(1, 0);
        assert_eq!(menu.selection, Some(Hit::Pinned(COLS + 1)));
        // Bajar desde la última fila de anclados pasa por "Visualización" y luego a
        // la cuadrícula de "Todo", hasta su última app.
        menu.move_selection(0, 1);
        assert_eq!(menu.selection, Some(Hit::ViewPicker));
        menu.move_selection(0, 1);
        assert_eq!(menu.selection, Some(Hit::App(COLS - 1)));
        for _ in 0..10 {
            menu.move_selection(0, 1);
        }
        assert_eq!(menu.selection, Some(Hit::App(24)));
        let (_, y) = point_of(&menu, menu.selection.unwrap());
        assert!((VIEW_TOP..VIEW_TOP + VIEW_H).contains(&y));
        assert_eq!(menu.first_match(), menu.selection);
    }

    #[test]
    fn search_edits_reset_the_results_but_moving_the_caret_does_not() {
        let mut menu = state();
        menu.apps = many("App", 60);
        menu.insert_text("app 1");
        menu.scroll_by(50.0);
        menu.selection = menu.first_match();
        // Mover el cursor no cambia la búsqueda: se conservan desplazamiento y selección.
        menu.edit(|field| field.left(true, false));
        assert!(menu.selection.is_some());
        // Ctrl+Retroceso cambia el texto y vuelve al principio de los resultados.
        menu.edit(|field| field.end(false));
        menu.edit(|field| field.backspace(true));
        assert_eq!(menu.query.text(), "app ");
        assert_eq!((menu.scroll, menu.selection), (0.0, None));
        // Con el menú de energía abierto no se edita.
        menu.power_menu = true;
        assert!(menu.edit(|field| field.insert("x")).is_none());
        assert_eq!(menu.query.text(), "app ");
    }

    #[test]
    fn caret_blinks_but_stays_on_while_typing_and_hides_with_a_menu() {
        let mut menu = state();
        assert!(menu.caret_on);
        assert!(menu.blink());
        assert!(!menu.caret_on);
        // Escribir o mover el cursor lo vuelve a mostrar.
        menu.insert_text("a");
        assert!(menu.caret_on);
        menu.blink();
        menu.edit(|field| field.home(false));
        assert!(menu.caret_on);
        // Sobre el texto del buscador hay cursor de texto; con un menú abierto, no.
        let y = SEARCH_Y + SEARCH_H / 2.0;
        assert!(menu.over_text(SEARCH_TEXT_X + 10.0, y));
        assert!(!menu.over_text(SEARCH_TEXT_X + 10.0, VIEW_TOP + 20.0));
        menu.activate(Hit::Power);
        assert!(!menu.over_text(SEARCH_TEXT_X + 10.0, y));
        assert!(!menu.blink());
    }

    #[test]
    fn double_click_selects_a_word_and_triple_click_everything() {
        let mut menu = state();
        menu.insert_text("visual studio");
        let double = Duration::from_millis(500);
        let start = Instant::now();
        let x = SEARCH_TEXT_X + 1.0 + text_width("visual stu", SEARCH_FONT);
        menu.press_search(x, false, start, double);
        assert_eq!(menu.query.selection(), None);
        menu.press_search(x + 2.0, false, start + Duration::from_millis(200), double);
        assert_eq!(menu.query.selected_text().as_deref(), Some("studio"));
        menu.press_search(x, false, start + Duration::from_millis(400), double);
        assert_eq!(menu.query.selected_text().as_deref(), Some("visual studio"));
        // Pasado el tiempo de doble clic, vuelve a ser un clic que solo pone el cursor.
        menu.press_search(x, false, start + Duration::from_secs(2), double);
        assert_eq!(menu.query.selection(), None);
        // Lejos del clic anterior tampoco cuenta como doble.
        let far = SEARCH_TEXT_X + 5.0;
        menu.press_search(far, false, start + Duration::from_millis(2100), double);
        assert_eq!(menu.query.selection(), None);
    }

    #[test]
    fn clicking_the_search_places_the_caret_and_shift_click_selects() {
        let mut menu = state();
        menu.insert_text("spotify");
        let y = SEARCH_Y + SEARCH_H / 2.0;
        assert!(menu.in_search(SEARCH_TEXT_X, y));
        menu.click_search(SEARCH_TEXT_X, false);
        assert_eq!(menu.query.caret(), 0);
        let after_spot = SEARCH_TEXT_X + 1.0 + text_width("spot", SEARCH_FONT);
        menu.click_search(after_spot, false);
        assert_eq!(menu.query.caret(), 4);
        menu.click_search(SEARCH_TEXT_X + SEARCH_TEXT_W, true);
        assert_eq!(menu.query.selected_text().as_deref(), Some("ify"));
    }

    #[test]
    fn search_finds_and_launches_store_entry_without_a_shortcut() {
        let mut menu = state();
        menu.apps
            .push(entry("Spotify", "shell:AppsFolder\\SpotifyAB.Test!Spotify"));
        menu.insert_text("  SPOTIFY ");
        let hit = menu.first_match().unwrap();
        let (x, y) = point_of(&menu, hit);
        assert_eq!(menu.hit_test(x, y), Some(hit));
        assert!(
            matches!(menu.activate(hit), Some(Action::Launch(entry)) if entry.command.starts_with("shell:AppsFolder\\"))
        );
        menu.activate(Hit::ClearSearch);
        assert!(menu.query.is_empty());
        assert_eq!(menu.first_match(), None);
    }

    #[test]
    fn launcher_background_is_nearly_opaque() {
        let mut menu = state();
        menu.apps.clear();
        let mut image = Pixmap::new(WIDTH as u32, HEIGHT as u32).unwrap();
        menu.draw(&mut image, 1.0);
        assert_eq!(image.pixel(320, 500).unwrap().alpha(), background()[3]);
        assert_eq!(image.pixel(0, 0).unwrap().alpha(), 0);
    }

    fn center_of_option(index: usize) -> (f32, f32) {
        let (x, y, w, h) = LauncherState::power_option_rect(index);
        (x + w / 2.0, y + h / 2.0)
    }

    #[test]
    fn power_button_opens_a_menu_with_the_four_options() {
        let mut menu = state();
        let (px, py, ps) = LauncherState::power_button();
        let button = (px + ps / 2.0, py + ps / 2.0);
        // Cerrado: las opciones no existen y el botón solo abre el menú.
        let (lx, ly) = center_of_option(0);
        assert_ne!(menu.hit_test(lx, ly), Some(Hit::PowerOption(Power::Lock)));
        assert_eq!(menu.hit_test(button.0, button.1), Some(Hit::Power));
        assert!(menu.activate(Hit::Power).is_none());
        assert!(menu.power_menu);

        for (i, option) in Power::ALL.into_iter().enumerate() {
            let (x, y) = center_of_option(i);
            assert_eq!(menu.hit_test(x, y), Some(Hit::PowerOption(option)));
        }
        // El menú queda dentro del panel, encima del botón.
        let (mx, my, mw, mh) = LauncherState::power_menu_rect();
        assert!(mx >= PAD && mx + mw <= WIDTH && my + mh < py && my > VIEW_TOP);

        // Clic fuera o en el botón: se cierra sin hacer nada.
        assert_eq!(
            menu.hit_test(PAD + 10.0, VIEW_TOP + 10.0),
            Some(Hit::Outside)
        );
        assert!(menu.activate(Hit::Outside).is_none());
        assert!(!menu.power_menu);
        menu.activate(Hit::Power);
        assert!(menu.activate(Hit::Power).is_none());
        assert!(!menu.power_menu);

        // Una opción actúa y cierra el menú.
        menu.activate(Hit::Power);
        let (x, y) = center_of_option(3);
        let hit = menu.hit_test(x, y).unwrap();
        assert!(matches!(
            menu.activate(hit),
            Some(Action::Power(Power::Restart))
        ));
        assert!(!menu.power_menu);
    }

    #[test]
    fn power_menu_fades_and_rises_into_place() {
        let mut menu = state();
        menu.apps.clear();
        let mut panel = Pixmap::new(WIDTH as u32, HEIGHT as u32).unwrap();
        menu.draw(&mut panel, 1.0);
        let (x, y, w, h) = LauncherState::power_menu_rect();
        // Puntos del menú cerca de su borde superior y del inferior, sin texto ni iconos.
        let (px, top, bottom) = (
            (x + w - 12.0) as u32,
            (y + 3.0) as u32,
            (y + h - 3.0) as u32,
        );
        let frame = |progress: f32| {
            let mut image = panel.clone();
            menu.draw_power_menu(&mut image, 1.0, progress);
            image
        };
        let red = |image: &Pixmap, y: u32| image.pixel(px, y).unwrap().demultiply().red();

        let start = frame(0.0);
        let halfway = frame(0.5);
        let done = frame(1.0);
        // Al empezar no se ve: el panel queda igual.
        assert_eq!(red(&start, bottom), red(&panel, bottom));
        // A mitad, semitransparente y aún más abajo: su borde superior no ha llegado.
        assert!(red(&panel, bottom) < red(&halfway, bottom));
        assert!(red(&halfway, bottom) < red(&done, bottom));
        assert_eq!(red(&halfway, top), red(&panel, top));
        assert_ne!(red(&done, top), red(&panel, top));

        // La curva frena al final y la animación termina sola.
        assert_eq!(ease_out(0.0), 0.0);
        assert!(ease_out(0.5) > 0.5);
        assert_eq!(ease_out(2.0), 1.0);
        menu.power_menu = true;
        menu.menu_opened = None;
        assert!(!menu.animating());
        menu.menu_opened = Some(Instant::now());
        assert!(menu.animating());
        menu.menu_opened = Instant::now().checked_sub(MENU_ANIM * 2);
        assert!(!menu.animating());
    }

    #[test]
    fn panel_rises_in_and_sinks_out_reversing_midway() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut panel = Transition::hidden();
        assert_eq!(panel.shown(start), 0.0);
        assert!(!panel.running(start));

        panel.turn(true, start, true);
        assert_eq!(panel.shown(start), 0.0);
        assert_eq!(panel.offset(start), PANEL_RISE);
        let half = at(OPEN_ANIM.as_millis() as u64 / 2);
        assert!(panel.shown(half) > 0.5 && panel.shown(half) < 1.0);
        assert!(panel.offset(half) > 0.0 && panel.offset(half) < PANEL_RISE / 2.0);
        assert!(panel.running(half));
        let open = at(OPEN_ANIM.as_millis() as u64);
        assert_eq!(panel.shown(open), 1.0);
        assert_eq!(panel.offset(open), 0.0);
        assert!(!panel.running(open));

        // Cerrar a mitad de la apertura sigue desde lo que ya se veía, sin saltos.
        let mut reversed = Transition::hidden();
        reversed.turn(true, start, true);
        let seen = reversed.shown(half);
        reversed.turn(false, half, true);
        assert_eq!(reversed.shown(half), seen);
        assert!(reversed.shown(half + Duration::from_millis(40)) < seen);
        assert!(reversed.running(half));

        // El cierre es más corto que la apertura y termina oculto.
        panel.turn(false, open, true);
        assert!(panel.running(open));
        let closed = open + CLOSE_ANIM;
        assert_eq!(panel.shown(closed), 0.0);
        assert!(!panel.running(closed));
        assert!(CLOSE_ANIM < OPEN_ANIM);

        // Sin efectos de animación, abre y cierra al instante.
        panel.turn(true, closed, false);
        assert_eq!(panel.shown(closed), 1.0);
        assert!(!panel.running(closed));
        panel.turn(false, closed, false);
        assert_eq!(panel.shown(closed), 0.0);
        assert!(!panel.running(closed));
    }

    #[test]
    fn arrows_and_enter_choose_a_power_option() {
        let mut menu = state();
        menu.activate(Hit::Power);
        assert_eq!(menu.first_match(), None);
        menu.move_selection(0, 1);
        assert_eq!(menu.first_match(), Some(Hit::PowerOption(Power::Lock)));
        menu.move_selection(0, 1);
        menu.move_selection(0, 1);
        assert_eq!(menu.first_match(), Some(Hit::PowerOption(Power::Shutdown)));
        menu.move_selection(0, 5);
        assert_eq!(menu.first_match(), Some(Hit::PowerOption(Power::Restart)));
        menu.move_selection(0, -1);
        assert_eq!(menu.first_match(), Some(Hit::PowerOption(Power::Shutdown)));
    }

    /// Opciones del menú contextual abierto, en orden.
    fn context_labels(menu: &LauncherState) -> Vec<String> {
        menu.context
            .as_ref()
            .unwrap()
            .menu
            .entries
            .iter()
            .filter_map(|e| match e {
                crate::appmenu::Entry::Item { label, .. } => Some(label.clone()),
                _ => None,
            })
            .collect()
    }

    /// Centro de la opción con ese texto en el menú contextual, en el panel.
    fn context_point(menu: &LauncherState, label: &str) -> (f32, f32) {
        let context = menu.context.as_ref().unwrap();
        let (w, h) = context.menu.size();
        (0..h as i32)
            .map(|y| (context.x + w / 2.0, context.y + y as f32))
            .find(|&(x, y)| {
                matches!(menu.hit_test(x, y), Some(Hit::Context(i))
                    if matches!(&context.menu.entries[i],
                        crate::appmenu::Entry::Item { label: l, .. } if l == label))
            })
            .unwrap()
    }

    #[test]
    fn right_click_opens_the_app_menu_and_its_options_act() {
        let mut menu = state();
        menu.pinned = vec![entry("Explorador", "C:\\Windows\\explorer.exe")];
        let (x, y) = point_of(&menu, Hit::Pinned(0));
        assert!(menu.open_context(Hit::Pinned(0), x, y));
        assert!(menu.overlay_open());
        assert_eq!(
            context_labels(&menu),
            [
                "Abrir",
                "Ejecutar como administrador",
                "Abrir ubicación del archivo",
                "Desanclar de la barra de tareas",
                "Desinstalar",
            ]
        );
        // El menú queda dentro del panel; con él abierto no se escribe.
        let context = menu.context.as_ref().unwrap();
        let (w, h) = context.menu.size();
        assert!(context.x >= 0.0 && context.x + w <= WIDTH);
        assert!(context.y >= 0.0 && context.y + h <= HEIGHT);
        assert!(menu.edit(|field| field.insert("x")).is_none());

        let (ux, uy) = context_point(&menu, "Desanclar de la barra de tareas");
        let hit = menu.hit_test(ux, uy).unwrap();
        assert!(matches!(menu.activate(hit), Some(Action::Unpin(e)) if e.name == "Explorador"));
        assert!(!menu.overlay_open());

        // Una app de la Store: sin administrador ni ubicación; sin anclar, "Anclar".
        menu.apps
            .push(entry("Spotify", "shell:AppsFolder\\SpotifyAB.Test!Spotify"));
        menu.insert_text("spotify");
        let hit = menu.first_match().unwrap();
        let (x, y) = point_of(&menu, hit);
        assert!(menu.open_context(hit, x, y));
        assert_eq!(
            context_labels(&menu),
            ["Abrir", "Anclar a la barra de tareas", "Desinstalar"]
        );
        // Clic fuera: se cierra sin hacer nada.
        assert_eq!(
            menu.hit_test(WIDTH - 10.0, HEIGHT - 10.0),
            Some(Hit::Outside)
        );
        assert!(menu.activate(Hit::Outside).is_none());
        assert!(menu.context.is_none());
    }

    #[test]
    fn search_shows_the_best_match_in_the_detail_with_its_actions() {
        let mut menu = state();
        menu.insert_text("editor");
        let layout = menu.layout();
        let titles: Vec<_> = layout.headers.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(titles, ["Mejor coincidencia", "Aplicaciones", "Web"]);
        // Los resultados dejan sitio al detalle, a su derecha.
        assert!(layout.slots.iter().all(|s| PAD + s.x + s.w < DETAIL_X));
        let best = menu.first_match().unwrap();
        assert_eq!(menu.detail_hit(), Some(best));

        let (x, y, w, h) = LauncherState::detail_open_rect();
        let open = (x + w / 2.0, y + h / 2.0);
        assert_eq!(menu.hit_test(open.0, open.1), Some(Hit::DetailOpen));
        let launched = |menu: &mut LauncherState| match menu.activate(Hit::DetailOpen) {
            Some(Action::Launch(e)) => e.name,
            _ => panic!("debía abrir la app del detalle"),
        };
        let first = launched(&mut menu);
        // Con las flechas el detalle pasa al resultado elegido.
        menu.move_selection(0, 1);
        menu.move_selection(0, 1);
        assert_ne!(menu.detail_hit(), Some(best));
        assert_ne!(launched(&mut menu), first);

        // "Opciones" abre el menú de esa app debajo del botón; Esc (cerrar) lo quita.
        let (x, y, w, h) = LauncherState::detail_options_rect();
        assert_eq!(
            menu.hit_test(x + w / 2.0, y + h / 2.0),
            Some(Hit::DetailOptions)
        );
        assert!(menu.activate(Hit::DetailOptions).is_none());
        assert!(menu.context.as_ref().unwrap().y >= y + h);
        assert!(menu.close_overlay());
        // La tecla de menú lo abre con la primera opción resaltada; Enter la elige.
        assert!(menu.open_context_from_keyboard());
        assert!(matches!(
            menu.first_match().and_then(|hit| menu.activate(hit)),
            Some(Action::Launch(_))
        ));
    }

    #[test]
    fn search_also_offers_results_that_are_not_apps() {
        let mut menu = state();
        menu.files = vec![entry("Editor de notas.txt", r"C:\notas.txt")];
        let titles = |menu: &LauncherState| -> Vec<String> {
            menu.layout().headers.into_iter().map(|(t, _)| t).collect()
        };

        // Una operación: su resultado es lo primero, y Enter lo copia.
        menu.insert_text("12*12");
        assert_eq!(titles(&menu), ["Calculadora", "Web"]);
        let first = menu.first_match().unwrap();
        assert!(matches!(first, Hit::Extra(_)));
        assert_eq!(menu.entry(first).unwrap().name, "= 144");
        assert!(matches!(menu.activate(first), Some(Action::Copy(text)) if text == "144"));
        // En el detalle, "Copiar" hace lo mismo y no hay "Opciones" ni menú.
        assert!(matches!(
            menu.activate(Hit::DetailOpen),
            Some(Action::Copy(_))
        ));
        let (x, y, w, h) = LauncherState::detail_options_rect();
        assert_eq!(menu.hit_test(x + w / 2.0, y + h / 2.0), None);
        assert!(!menu.open_context_from_keyboard());
        assert!(menu.context.is_none());

        // Una página de Configuración se abre por su dirección.
        menu.activate(Hit::ClearSearch);
        assert!(menu.extras.is_empty());
        menu.insert_text("bluetooth");
        assert_eq!(titles(&menu), ["Configuración", "Web"]);
        let setting = menu.first_match().unwrap();
        assert!(matches!(
            menu.activate(setting),
            Some(Action::OpenFile(e)) if e.command == "ms-settings:bluetooth"
        ));

        // Con apps que coinciden, estas van delante y los archivos y la web detrás.
        menu.activate(Hit::ClearSearch);
        menu.insert_text("editor");
        assert_eq!(
            titles(&menu),
            ["Mejor coincidencia", "Aplicaciones", "Archivos", "Web"]
        );
        assert!(matches!(menu.first_match(), Some(Hit::App(_))));
        let layout = menu.layout();
        let file = layout
            .slots
            .iter()
            .find(|s| {
                menu.extra(s.hit)
                    .is_some_and(|e| e.kind == crate::extras::Kind::File)
            })
            .unwrap();
        assert!(matches!(
            menu.activate(file.hit),
            Some(Action::OpenFile(e)) if e.command == r"C:\notas.txt"
        ));
        // La web es siempre lo último.
        let last = layout.slots.last().unwrap().hit;
        assert!(matches!(
            menu.activate(last),
            Some(Action::OpenFile(e)) if e.command.starts_with("https://")
        ));

        // Sin nada que coincida queda el aviso, y la web.
        menu.activate(Hit::ClearSearch);
        menu.insert_text("zzzz");
        assert_eq!(titles(&menu), ["Sin resultados", "Web"]);
    }
}
