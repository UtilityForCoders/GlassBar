//! Aspecto del dock según lo que pasa en pantalla, como TranslucentTB: cada
//! situación (escritorio, ventana visible, maximizada, Inicio, búsqueda, vista de
//! tareas) tiene su apariencia, y gana la situación activada de mayor prioridad.
use std::path::PathBuf;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, IsZoomed,
    GWL_EXSTYLE, GW_OWNER, WS_EX_TOOLWINDOW,
};

/// Situaciones, de menor a mayor prioridad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum State {
    Desktop,
    VisibleWindow,
    Maximized,
    Start,
    Search,
    TaskView,
}

impl State {
    pub const ALL: [State; 6] = [
        State::Desktop,
        State::VisibleWindow,
        State::Maximized,
        State::Start,
        State::Search,
        State::TaskView,
    ];
    pub fn name(self) -> &'static str {
        match self {
            State::Desktop => "Escritorio",
            State::VisibleWindow => "Ventana visible",
            State::Maximized => "Ventana maximizada",
            State::Start => "Inicio abierto",
            State::Search => "Búsqueda abierta",
            State::TaskView => "Vista de tareas abierta",
        }
    }
    /// Icono de Segoe Fluent Icons para la configuración.
    pub fn glyph(self) -> char {
        match self {
            State::Desktop => '\u{E7F4}',
            State::VisibleWindow => '\u{E8A7}',
            State::Maximized => '\u{E740}',
            State::Start => '\u{E80F}',
            State::Search => '\u{E721}',
            State::TaskView => '\u{E7C4}',
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Appearance {
    /// El cristal de siempre de GlassBar.
    #[default]
    Normal,
    Opaque,
    Clear,
    Blur,
    Acrylic,
}

impl Appearance {
    pub const ALL: [Appearance; 5] = [
        Appearance::Normal,
        Appearance::Opaque,
        Appearance::Clear,
        Appearance::Blur,
        Appearance::Acrylic,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Appearance::Normal => "Normal",
            Appearance::Opaque => "Opaca",
            Appearance::Clear => "Transparente",
            Appearance::Blur => "Desenfoque",
            Appearance::Acrylic => "Acrílico",
        }
    }
    /// Relleno y borde del dock. El relleno es un degradado horizontal de cristal
    /// gris: más claro en los extremos y más oscuro en el centro, todo con la misma
    /// opacidad. Nunca del todo transparente: con alfa 0 los clics atravesarían la
    /// ventana.
    pub fn colors(self) -> ([[u8; 4]; 3], [u8; 4]) {
        const RIM: [u8; 4] = [255, 255, 255, 60];
        let glass = |a: u8| [[58, 60, 66, a], [30, 32, 37, a], [52, 54, 60, a]];
        match self {
            Appearance::Normal => (glass(130), RIM),
            Appearance::Opaque => (
                [[46, 46, 50, 255], [32, 32, 35, 255], [42, 42, 46, 255]],
                [255, 255, 255, 36],
            ),
            Appearance::Clear => ([[0, 0, 0, 1]; 3], [0, 0, 0, 0]),
            // El desenfoque lo pone Windows debajo; encima, un velo para el contraste.
            Appearance::Blur => (glass(70), RIM),
            Appearance::Acrylic => ([[0, 0, 0, 1]; 3], RIM),
        }
    }
    /// Necesita que Windows desenfoque lo que hay detrás.
    fn backdrop(self) -> bool {
        matches!(self, Appearance::Blur | Appearance::Acrylic)
    }
}

/// Apariencia de una situación y si cuenta.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Look {
    pub enabled: bool,
    pub appearance: Appearance,
}

/// Ajustes de las seis situaciones, en el orden de `State::ALL`, y la opacidad de
/// los paneles y menús.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Styles {
    looks: Vec<Look>,
    /// En tanto por ciento. Los ajustes guardados antes de existir no la tienen.
    #[serde(default = "default_menu_opacity")]
    menu_opacity: u8,
    /// El dock no se esconde y Windows le reserva su sitio (ver `appbar`).
    #[serde(default)]
    always_visible: bool,
    /// Controles arriba y el dock solo con apps, como macOS (ver `topbar`).
    #[serde(default)]
    top_bar: bool,
}

fn default_menu_opacity() -> u8 {
    crate::glass::DEFAULT_MENU_OPACITY
}

impl Default for Styles {
    /// Como hasta ahora: el cristal normal y ninguna situación más activada.
    fn default() -> Self {
        let mut looks = vec![Look::default(); State::ALL.len()];
        looks[0].enabled = true;
        Self {
            looks,
            menu_opacity: default_menu_opacity(),
            always_visible: false,
            top_bar: false,
        }
    }
}

impl Styles {
    /// Opacidad de los paneles y menús propios, en tanto por ciento.
    pub fn menu_opacity(&self) -> u8 {
        self.menu_opacity.clamp(crate::glass::MIN_MENU_OPACITY, 100)
    }
    pub fn set_menu_opacity(&mut self, percent: u8) {
        self.menu_opacity = percent.clamp(crate::glass::MIN_MENU_OPACITY, 100);
    }
    /// El dock se queda siempre a la vista en vez de esconderse.
    pub fn always_visible(&self) -> bool {
        self.always_visible
    }
    pub fn set_always_visible(&mut self, on: bool) {
        self.always_visible = on;
    }
    /// Barra superior con los controles; el dock, solo con las apps.
    pub fn top_bar(&self) -> bool {
        self.top_bar
    }
    pub fn set_top_bar(&mut self, on: bool) {
        self.top_bar = on;
    }
    pub fn look(&self, state: State) -> Look {
        let index = state as usize;
        let mut look = self.looks.get(index).copied().unwrap_or_default();
        // El escritorio es la base: siempre cuenta.
        look.enabled |= state == State::Desktop;
        look
    }
    fn look_mut(&mut self, state: State) -> &mut Look {
        self.looks.resize(State::ALL.len(), Look::default());
        &mut self.looks[state as usize]
    }
    /// Elegir una apariencia también activa la situación.
    pub fn set_appearance(&mut self, state: State, appearance: Appearance) {
        *self.look_mut(state) = Look {
            enabled: true,
            appearance,
        };
    }
    /// Alguna situación aparte del escritorio está activada: hay que mirar la pantalla.
    pub fn watches_screen(&self) -> bool {
        State::ALL[1..]
            .iter()
            .any(|&state| self.look(state).enabled)
    }
    pub fn toggle(&mut self, state: State) {
        if state != State::Desktop {
            let look = self.look_mut(state);
            look.enabled = !look.enabled;
        }
    }
    /// La apariencia de la situación activada de mayor prioridad de las que se dan.
    pub fn resolve(&self, scene: &Scene) -> Appearance {
        State::ALL
            .iter()
            .rev()
            .map(|&state| (state, self.look(state)))
            .find(|(state, look)| look.enabled && scene.has(*state))
            .map(|(_, look)| look.appearance)
            .unwrap_or_default()
    }

    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> std::io::Result<()> {
        let path = path().ok_or_else(|| std::io::Error::other("LOCALAPPDATA no disponible"))?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        let pending = path.with_extension("tmp");
        std::fs::write(&pending, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(pending, path)
    }
}

/// Opción del menú de estilos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Toggle(State),
    Set(State, Appearance),
}

/// Órdenes del menú: `COMMANDS + situación * 10 + apariencia`, y `+ 9` para activarla.
const COMMANDS: u32 = 200;
const TOGGLE: u32 = 9;

impl Choice {
    pub fn command(self) -> u32 {
        match self {
            Choice::Toggle(state) => COMMANDS + state as u32 * 10 + TOGGLE,
            Choice::Set(state, appearance) => COMMANDS + state as u32 * 10 + appearance as u32,
        }
    }
    pub fn from_command(command: u32) -> Option<Self> {
        let offset = command.checked_sub(COMMANDS)?;
        let state = *State::ALL.get((offset / 10) as usize)?;
        match offset % 10 {
            TOGGLE => Some(Choice::Toggle(state)),
            index => Appearance::ALL
                .get(index as usize)
                .map(|&appearance| Choice::Set(state, appearance)),
        }
    }
    pub fn apply(self, styles: &mut Styles) {
        match self {
            Choice::Toggle(state) => styles.toggle(state),
            Choice::Set(state, appearance) => styles.set_appearance(state, appearance),
        }
    }
}

fn path() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("GlassBar")
            .join("styles.json"),
    )
}

/// Lo que está pasando en pantalla.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scene {
    pub visible_window: bool,
    pub maximized: bool,
    pub start: bool,
    pub search: bool,
    pub task_view: bool,
}

impl Scene {
    fn has(&self, state: State) -> bool {
        match state {
            State::Desktop => true,
            State::VisibleWindow => self.visible_window,
            State::Maximized => self.maximized,
            State::Start => self.start,
            State::Search => self.search,
            State::TaskView => self.task_view,
        }
    }

    /// Mira las ventanas del monitor `monitor`. `start` y `typing` son el lanzador
    /// de GlassBar abierto y con algo escrito en su buscador.
    pub fn read(monitor: RECT, start: bool, typing: bool) -> Self {
        let mut windows = Windows {
            monitor,
            visible: false,
            maximized: false,
        };
        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM(&mut windows as *mut Windows as isize));
        }
        let foreground = unsafe { GetForegroundWindow() };
        let class = class_of(foreground);
        let process = unsafe { crate::apps::window_process(foreground) }
            .map(|(_, exe, _)| file_name(&exe).to_ascii_lowercase())
            .unwrap_or_default();
        Self {
            visible_window: windows.visible,
            maximized: windows.maximized,
            start,
            search: typing || is_search(&process),
            task_view: is_task_view(&class, &process),
        }
    }
}

/// La búsqueda de Windows (Win+S), según la versión.
fn is_search(process: &str) -> bool {
    matches!(process, "searchhost.exe" | "searchapp.exe" | "searchui.exe")
}

/// La vista de tareas (Win+Tab): en Windows 11 es una isla XAML de Explorer.
fn is_task_view(class: &str, process: &str) -> bool {
    class == "MultitaskingViewFrame"
        || (class == "XamlExplorerHostIslandWindow" && process == "explorer.exe")
}

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

fn class_of(hwnd: HWND) -> String {
    let mut name = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut name) }.max(0) as usize;
    String::from_utf16_lossy(&name[..len])
}

struct Windows {
    monitor: RECT,
    visible: bool,
    maximized: bool,
}

/// Las ventanas llegan de delante hacia atrás: basta con la primera que cuenta.
/// "Maximizada" es que esa, la de delante, lo esté; una maximizada detrás de otra
/// ventana no se ve y no cuenta, y entonces vale "Ventana visible".
unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let windows = &mut *(lparam.0 as *mut Windows);
    if !counts(hwnd, windows.monitor) {
        return true.into();
    }
    windows.visible = true;
    windows.maximized = IsZoomed(hwnd).as_bool();
    false.into()
}

/// Ventana de una app, a la vista en el monitor: ni minimizada, ni oculta en otro
/// escritorio, ni de GlassBar.
unsafe fn counts(hwnd: HWND, monitor: RECT) -> bool {
    if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return false;
    }
    if GetWindowTextLengthW(hwnd) == 0
        || GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid())
        || GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0
    {
        return false;
    }
    let mut process = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut process));
    if process == std::process::id() {
        return false;
    }
    let class = class_of(hwnd);
    if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd") {
        return false;
    }
    let mut cloaked = 0u32;
    if DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        &mut cloaked as *mut u32 as *mut _,
        std::mem::size_of::<u32>() as u32,
    )
    .is_ok()
        && cloaked != 0
    {
        return false;
    }
    let mut rect = RECT::default();
    GetWindowRect(hwnd, &mut rect).is_ok() && overlaps(rect, monitor)
}

fn overlaps(a: RECT, b: RECT) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// Desenfoque de Windows detrás del dock y de los paneles y menús propios. Son
/// ventanas con transparencia por píxel, y en ellas Windows desenfoca el rectángulo
/// entero sin respetar su forma. Por eso el desenfoque va en una ventana aparte,
/// justo debajo de la que lo lleva y con su misma forma.
///
/// La forma se recorta con Windows.UI.Composition: un pincel con el fondo
/// desenfocado del sistema (`HostBackdropBrush`) dentro de uno o varios rectángulos
/// redondeados con los bordes suavizados (un menú con su submenú abierto son dos).
/// El desenfoque clásico (`SetWindowCompositionAttribute`) no respeta la región de
/// la ventana en Windows 11 y asomaban las esquinas cuadradas; solo se usa si la
/// composición no está disponible.
pub struct Backdrop {
    hwnd: HWND,
    /// Ventana recortada por composición; `None` si se usa el desenfoque clásico.
    composition: Option<Composition>,
    /// Lo aplicado la última vez, para no repetir el trabajo en cada fotograma.
    applied: std::cell::RefCell<Option<Applied>>,
}

/// Rectángulo redondeado que se desenfoca, en píxeles y relativo a la ventana.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
}

impl Shape {
    /// El panel de una ventana de `width` x `height` píxeles, separado `inset`
    /// píxeles de cada borde.
    pub fn inset(width: u32, height: u32, inset: f32, radius: f32) -> Self {
        Self {
            x: inset,
            y: inset,
            width: (width as f32 - inset * 2.0).max(0.0),
            height: (height as f32 - inset * 2.0).max(0.0),
            radius,
        }
    }
}

#[derive(Clone, PartialEq)]
struct Applied {
    appearance: Appearance,
    width: i32,
    height: i32,
    shapes: Vec<Shape>,
    opacity: f32,
}

/// Lo que la composición necesita mantener vivo, y lo que se cambia después.
struct Composition {
    _target: windows::UI::Composition::Desktop::DesktopWindowTarget,
    compositor: windows::UI::Composition::Compositor,
    root: windows::UI::Composition::ContainerVisual,
    /// Fondo desenfocado del sistema y tinte del acrílico, compartidos por las formas.
    backdrop: windows::UI::Composition::CompositionBackdropBrush,
    tint: windows::UI::Composition::CompositionColorBrush,
    /// Una por rectángulo; las que sobran se ocultan.
    shapes: std::cell::RefCell<Vec<ShapeVisual>>,
}

/// Un rectángulo recortado: el desenfoque y, encima, el tinte (solo en Acrílico).
struct ShapeVisual {
    visual: windows::UI::Composition::ContainerVisual,
    geometry: windows::UI::Composition::CompositionRoundedRectangleGeometry,
    tint: windows::UI::Composition::SpriteVisual,
}

impl Backdrop {
    pub fn new() -> Option<Self> {
        use windows::core::w;
        use windows::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            RegisterClassExW, WNDCLASSEXW, WS_EX_NOREDIRECTIONBITMAP,
        };
        unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            // Negro: con el desenfoque clásico, lo negro deja ver el desenfoque.
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(backdrop_wndproc),
                hInstance: instance.into(),
                hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
                lpszClassName: w!("GlassBarBackdrop"),
                ..Default::default()
            };
            RegisterClassExW(&class);
            // Primero con composición: la ventana no tiene imagen propia, solo la
            // que le pone la composición.
            if let Some(hwnd) = backdrop_window(WS_EX_NOREDIRECTIONBITMAP) {
                match compose(hwnd) {
                    Ok(composition) => {
                        return Some(Self {
                            hwnd,
                            composition: Some(composition),
                            applied: Default::default(),
                        })
                    }
                    Err(_) => {
                        let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
                    }
                }
            }
            let hwnd = backdrop_window(Default::default())?;
            Some(Self {
                hwnd,
                composition: None,
                applied: Default::default(),
            })
        }
    }

    /// Pone el desenfoque de `appearance` bajo la ventana `dock`, en `rect` (en
    /// pantalla) con esquinas de radio `radius`; lo quita si no lo necesita.
    pub fn update(&self, dock: HWND, appearance: Appearance, rect: RECT, radius: i32) {
        if !appearance.backdrop() {
            self.hide();
            return;
        }
        let shape = Shape {
            x: 0.0,
            y: 0.0,
            width: (rect.right - rect.left) as f32,
            height: (rect.bottom - rect.top) as f32,
            radius: radius as f32,
        };
        self.place(dock, appearance, rect, &[shape], 1.0);
    }

    /// Desenfoque bajo un panel o menú propio, como el del dock en la apariencia
    /// Desenfoque: en los rectángulos `shapes` (relativos a `rect`, la ventana en
    /// pantalla) y con la opacidad del panel mientras aparece o se desvanece.
    pub fn under(&self, window: HWND, rect: RECT, shapes: &[Shape], opacity: f32) {
        if opacity <= 0.0 || shapes.is_empty() {
            self.hide();
            return;
        }
        self.place(window, Appearance::Blur, rect, shapes, opacity.min(1.0));
    }

    /// Quita el desenfoque.
    pub fn hide(&self) {
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
        if self.applied.take().is_some() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }

    /// Lo pone justo debajo de `above`, en `rect`, recortado a `shapes`.
    fn place(
        &self,
        above: HWND,
        appearance: Appearance,
        rect: RECT,
        shapes: &[Shape],
        opacity: f32,
    ) {
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, SWP_NOACTIVATE, SWP_SHOWWINDOW,
        };
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        if width <= 0 || height <= 0 {
            self.hide();
            return;
        }
        let applied = Applied {
            appearance,
            width,
            height,
            shapes: shapes.to_vec(),
            opacity,
        };
        let before = self.applied.borrow_mut().take();
        let acrylic = appearance == Appearance::Acrylic;
        unsafe {
            match &self.composition {
                Some(composition) => {
                    if before.as_ref() != Some(&applied) {
                        let _ = composition.set(shapes, acrylic, opacity);
                    }
                }
                // Sin composición no se puede desvanecer: se ve mientras se ve el panel.
                None => {
                    if before.as_ref().map(|a| a.appearance) != Some(appearance) {
                        // ACCENT_ENABLE_ACRYLICBLURBEHIND y ACCENT_ENABLE_BLURBEHIND.
                        set_accent(self.hwnd, if acrylic { 4 } else { 3 }, ACRYLIC_TINT);
                    }
                    if before.as_ref().map(|a| (a.width, a.height, &a.shapes))
                        != Some((width, height, &applied.shapes))
                    {
                        // Windows se queda con la región.
                        let _ = SetWindowRgn(self.hwnd, Some(region(shapes)), true);
                    }
                }
            }
            // Justo debajo de su ventana, aunque otra "siempre encima" se haya
            // puesto entre medias.
            let _ = SetWindowPos(
                self.hwnd,
                Some(above),
                rect.left,
                rect.top,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        *self.applied.borrow_mut() = Some(applied);
    }
}

/// Unión de los rectángulos redondeados, para el desenfoque clásico.
unsafe fn region(shapes: &[Shape]) -> windows::Win32::Graphics::Gdi::HRGN {
    use windows::Win32::Graphics::Gdi::{CombineRgn, DeleteObject, RGN_OR};
    let round = |s: &Shape| {
        let d = (s.radius * 2.0).round() as i32;
        CreateRoundRectRgn(
            s.x.round() as i32,
            s.y.round() as i32,
            (s.x + s.width).round() as i32 + 1,
            (s.y + s.height).round() as i32 + 1,
            d,
            d,
        )
    };
    let Some((first, rest)) = shapes.split_first() else {
        return CreateRoundRectRgn(0, 0, 0, 0, 0, 0);
    };
    let all = round(first);
    for shape in rest {
        let one = round(shape);
        CombineRgn(Some(all), Some(all), Some(one), RGN_OR);
        let _ = DeleteObject(one.into());
    }
    all
}

impl Composition {
    /// Ajusta los rectángulos recortados a `shapes`, con el tinte si es acrílico.
    fn set(&self, shapes: &[Shape], acrylic: bool, opacity: f32) -> windows::core::Result<()> {
        let mut visuals = self.shapes.borrow_mut();
        while visuals.len() < shapes.len() {
            visuals.push(self.shape_visual()?);
        }
        for (index, visual) in visuals.iter().enumerate() {
            let Some(shape) = shapes.get(index) else {
                visual.visual.SetIsVisible(false)?;
                continue;
            };
            let size = windows_numerics::Vector2 {
                X: shape.width,
                Y: shape.height,
            };
            visual.visual.SetOffset(windows_numerics::Vector3 {
                X: shape.x,
                Y: shape.y,
                Z: 0.0,
            })?;
            visual.visual.SetSize(size)?;
            visual.geometry.SetSize(size)?;
            visual.geometry.SetCornerRadius(windows_numerics::Vector2 {
                X: shape.radius,
                Y: shape.radius,
            })?;
            visual.tint.SetIsVisible(acrylic)?;
            visual.visual.SetIsVisible(true)?;
        }
        self.root.SetOpacity(opacity)
    }

    /// Un rectángulo más: el desenfoque y el tinte, recortados por su geometría.
    fn shape_visual(&self) -> windows::core::Result<ShapeVisual> {
        let one = windows_numerics::Vector2 { X: 1.0, Y: 1.0 };
        let visual = self.compositor.CreateContainerVisual()?;
        let blur = self.compositor.CreateSpriteVisual()?;
        blur.SetRelativeSizeAdjustment(one)?;
        blur.SetBrush(&self.backdrop)?;
        let tint = self.compositor.CreateSpriteVisual()?;
        tint.SetRelativeSizeAdjustment(one)?;
        tint.SetBrush(&self.tint)?;
        tint.SetIsVisible(false)?;
        let children = visual.Children()?;
        children.InsertAtTop(&blur)?;
        children.InsertAtTop(&tint)?;
        let geometry = self.compositor.CreateRoundedRectangleGeometry()?;
        visual.SetClip(&self.compositor.CreateGeometricClipWithGeometry(&geometry)?)?;
        self.root.Children()?.InsertAtTop(&visual)?;
        Ok(ShapeVisual {
            visual,
            geometry,
            tint,
        })
    }
}

/// Ventana del desenfoque: sin bordes, siempre encima, sin activarse ni recibir
/// clics. `extra` se suma a los estilos de siempre.
unsafe fn backdrop_window(
    extra: windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE,
) -> Option<HWND> {
    use windows::core::w;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
    };
    let instance = GetModuleHandleW(None).ok()?;
    CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TRANSPARENT | extra,
        w!("GlassBarBackdrop"),
        w!("GlassBar — Fondo"),
        WS_POPUP,
        0,
        0,
        1,
        1,
        None,
        None,
        Some(instance.into()),
        None,
    )
    .ok()
}

/// Prepara en `hwnd` la composición: el fondo desenfocado del sistema y el tinte
/// del acrílico, que luego se recortan a cada rectángulo.
fn compose(hwnd: HWND) -> windows::core::Result<Composition> {
    use windows::core::Interface;
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_HOSTBACKDROPBRUSH};
    use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
    use windows::Win32::System::WinRT::{
        CreateDispatcherQueueController, DispatcherQueueOptions, DQTAT_COM_NONE,
        DQTYPE_THREAD_CURRENT,
    };
    let one = windows_numerics::Vector2 { X: 1.0, Y: 1.0 };
    unsafe {
        // Que el pincel del fondo del sistema muestre lo que hay detrás.
        let enable = BOOL(1);
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_HOSTBACKDROPBRUSH,
            &enable as *const BOOL as *const _,
            std::mem::size_of::<BOOL>() as u32,
        )?;
        // La composición necesita una cola de mensajes de WinRT en este hilo. La
        // comparten todos los desenfoques del hilo, así que vive lo que el hilo.
        if windows::System::DispatcherQueue::GetForCurrentThread().is_err() {
            std::mem::forget(CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_NONE,
            })?);
        }
        let compositor = windows::UI::Composition::Compositor::new()?;
        let target = compositor
            .cast::<ICompositorDesktopInterop>()?
            .CreateDesktopWindowTarget(hwnd, false)?;
        let root = compositor.CreateContainerVisual()?;
        root.SetRelativeSizeAdjustment(one)?;
        let backdrop = compositor.CreateHostBackdropBrush()?;
        // ACRYLIC_TINT va en AABBGGRR.
        let tint = compositor.CreateColorBrushWithColor(windows::UI::Color {
            A: (ACRYLIC_TINT >> 24) as u8,
            B: (ACRYLIC_TINT >> 16) as u8,
            G: (ACRYLIC_TINT >> 8) as u8,
            R: ACRYLIC_TINT as u8,
        })?;
        target.SetRoot(&root)?;
        Ok(Composition {
            _target: target,
            compositor,
            root,
            backdrop,
            tint,
            shapes: Default::default(),
        })
    }
}

impl Drop for Backdrop {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

/// La ventana del desenfoque no hace nada propio: ni recibe clics (los atraviesa)
/// ni se activa.
unsafe extern "system" fn backdrop_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, HTTRANSPARENT, WM_NCHITTEST};
    if msg == WM_NCHITTEST {
        return windows::Win32::Foundation::LRESULT(HTTRANSPARENT as isize);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Tinte del acrílico (AABBGGRR): el gris oscuro del cristal del dock.
const ACRYLIC_TINT: u32 = 0xB0_25_20_1e;

/// `SetWindowCompositionAttribute`, sin documentar pero estable desde Windows 10:
/// es lo que usan la barra de Windows y TranslucentTB. `color` es AABBGGRR.
fn set_accent(hwnd: HWND, state: u32, color: u32) {
    #[repr(C)]
    struct AccentPolicy {
        state: u32,
        flags: u32,
        color: u32,
        animation: u32,
    }
    #[repr(C)]
    struct CompositionData {
        attribute: u32,
        data: *mut std::ffi::c_void,
        size: usize,
    }
    type SetComposition = unsafe extern "system" fn(HWND, *mut CompositionData) -> BOOL;
    use windows::core::{s, w};
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    unsafe {
        let Ok(user32) = GetModuleHandleW(w!("user32.dll")) else {
            return;
        };
        let Some(function) = GetProcAddress(user32, s!("SetWindowCompositionAttribute")) else {
            return;
        };
        let function: SetComposition = std::mem::transmute(function);
        // El color solo se usa en acrílico; 2 = pintar el color de `color`.
        let mut policy = AccentPolicy {
            state,
            flags: if state == 4 { 2 } else { 0 },
            color,
            animation: 0,
        };
        let mut data = CompositionData {
            // WCA_ACCENT_POLICY
            attribute: 19,
            data: &mut policy as *mut AccentPolicy as *mut _,
            size: std::mem::size_of::<AccentPolicy>(),
        };
        let _ = function(hwnd, &mut data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn by_default_the_dock_looks_as_always() {
        let styles = Styles::default();
        let busy = Scene {
            visible_window: true,
            maximized: true,
            start: true,
            search: true,
            task_view: true,
        };
        assert_eq!(styles.resolve(&busy), Appearance::Normal);
        assert_eq!(styles.resolve(&Scene::default()), Appearance::Normal);
    }

    #[test]
    fn the_highest_enabled_situation_wins() {
        let mut styles = Styles::default();
        styles.set_appearance(State::Desktop, Appearance::Clear);
        styles.set_appearance(State::VisibleWindow, Appearance::Blur);
        styles.set_appearance(State::Maximized, Appearance::Opaque);
        styles.set_appearance(State::Start, Appearance::Acrylic);
        let mut scene = Scene::default();
        assert_eq!(styles.resolve(&scene), Appearance::Clear);
        scene.visible_window = true;
        assert_eq!(styles.resolve(&scene), Appearance::Blur);
        scene.maximized = true;
        assert_eq!(styles.resolve(&scene), Appearance::Opaque);
        scene.start = true;
        assert_eq!(styles.resolve(&scene), Appearance::Acrylic);
        // Desactivada, la situación no cuenta y gana la siguiente.
        styles.toggle(State::Start);
        assert_eq!(styles.resolve(&scene), Appearance::Opaque);
        // El escritorio no se puede desactivar.
        styles.toggle(State::Desktop);
        assert!(styles.look(State::Desktop).enabled);
    }

    #[test]
    fn settings_survive_a_round_trip_and_short_files() {
        let mut styles = Styles::default();
        styles.set_appearance(State::TaskView, Appearance::Blur);
        let json = serde_json::to_vec(&styles).unwrap();
        let back: Styles = serde_json::from_slice(&json).unwrap();
        assert_eq!(back, styles);
        // Un archivo con menos situaciones (de una versión anterior) sigue valiendo.
        let short: Styles = serde_json::from_str(r#"{"looks":[]}"#).unwrap();
        assert_eq!(short.look(State::Maximized), Look::default());
        assert!(short.look(State::Desktop).enabled);
    }

    #[test]
    fn menu_commands_round_trip() {
        for state in State::ALL {
            let toggle = Choice::Toggle(state);
            assert_eq!(Choice::from_command(toggle.command()), Some(toggle));
            for appearance in Appearance::ALL {
                let set = Choice::Set(state, appearance);
                assert_eq!(Choice::from_command(set.command()), Some(set));
            }
        }
        // Las demás órdenes del menú de GlassBar no son de estilos.
        assert_eq!(Choice::from_command(2), None);
        assert_eq!(Choice::from_command(COMMANDS + 5), None);
        assert_eq!(Choice::from_command(COMMANDS + 60), None);
        // Solo hace falta mirar la pantalla con alguna situación activada.
        let mut styles = Styles::default();
        assert!(!styles.watches_screen());
        Choice::Set(State::Maximized, Appearance::Opaque).apply(&mut styles);
        assert!(styles.watches_screen());
    }

    #[test]
    fn task_view_and_search_are_recognised() {
        assert!(is_task_view("XamlExplorerHostIslandWindow", "explorer.exe"));
        assert!(is_task_view("MultitaskingViewFrame", "explorer.exe"));
        assert!(!is_task_view("XamlExplorerHostIslandWindow", "chrome.exe"));
        assert!(is_search("searchhost.exe"));
        assert!(!is_search("explorer.exe"));
    }
}

#[cfg(test)]
mod menu_opacity_tests {
    use super::*;

    #[test]
    fn settings_saved_before_the_slider_keep_the_default_opacity() {
        let old = r#"{"looks":[{"enabled":true,"appearance":"Blur"}]}"#;
        let styles: Styles = serde_json::from_str(old).unwrap();
        assert_eq!(styles.menu_opacity(), crate::glass::DEFAULT_MENU_OPACITY);
        assert_eq!(styles.look(State::Desktop).appearance, Appearance::Blur);
    }

    #[test]
    fn the_opacity_stays_readable() {
        let mut styles = Styles::default();
        styles.set_menu_opacity(0);
        assert_eq!(styles.menu_opacity(), crate::glass::MIN_MENU_OPACITY);
        styles.set_menu_opacity(250);
        assert_eq!(styles.menu_opacity(), 100);
    }
}
