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
    /// Icono de Segoe Fluent Icons para el menú.
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
    /// Relleno y borde del dock. Nunca del todo transparente: con alfa 0 los clics
    /// atravesarían la ventana.
    pub fn colors(self) -> ([u8; 4], [u8; 4]) {
        match self {
            Appearance::Normal => ([23, 28, 36, 130], [255, 255, 255, 32]),
            Appearance::Opaque => ([32, 32, 32, 255], [255, 255, 255, 24]),
            Appearance::Clear => ([0, 0, 0, 1], [0, 0, 0, 0]),
            // El desenfoque lo pone Windows debajo; encima, un velo para el contraste.
            Appearance::Blur => ([23, 28, 36, 70], [255, 255, 255, 32]),
            Appearance::Acrylic => ([0, 0, 0, 1], [255, 255, 255, 32]),
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

/// Ajustes de las seis situaciones, en el orden de `State::ALL`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Styles {
    looks: Vec<Look>,
}

impl Default for Styles {
    /// Como hasta ahora: el cristal normal y ninguna situación más activada.
    fn default() -> Self {
        let mut looks = vec![Look::default(); State::ALL.len()];
        looks[0].enabled = true;
        Self { looks }
    }
}

impl Styles {
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

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let windows = &mut *(lparam.0 as *mut Windows);
    if counts(hwnd, windows.monitor) {
        windows.visible = true;
        windows.maximized |= IsZoomed(hwnd).as_bool();
    }
    // Basta con saber que hay una maximizada.
    (!windows.maximized).into()
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

/// Desenfoque de Windows detrás del dock (apariencias Desenfoque y Acrílico). El
/// dock es una ventana con transparencia por píxel, y en ellas Windows desenfoca el
/// rectángulo entero sin respetar su forma ni su región. Por eso el desenfoque va
/// en una ventana normal aparte, recortada a la forma redondeada del dock y justo
/// debajo de él.
pub struct Backdrop {
    hwnd: HWND,
    /// Apariencia y tamaño aplicados, para no repetir el trabajo en cada fotograma.
    applied: std::cell::Cell<Option<(Appearance, i32, i32)>>,
}

impl Backdrop {
    pub fn new() -> Option<Self> {
        use windows::core::w;
        use windows::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, RegisterClassExW, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOPMOST,
            WS_EX_TRANSPARENT, WS_POPUP,
        };
        unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            // Negro: con el acento de Windows, lo negro deja ver el desenfoque.
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(backdrop_wndproc),
                hInstance: instance.into(),
                hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
                lpszClassName: w!("GlassBarBackdrop"),
                ..Default::default()
            };
            RegisterClassExW(&class);
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TRANSPARENT,
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
            .ok()?;
            Some(Self {
                hwnd,
                applied: std::cell::Cell::new(None),
            })
        }
    }

    /// Pone el desenfoque de `appearance` bajo la ventana `dock`, en `rect` (en
    /// pantalla) con esquinas de radio `radius`; lo quita si no lo necesita.
    pub fn update(&self, dock: HWND, appearance: Appearance, rect: RECT, radius: i32) {
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, ShowWindow, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE,
        };
        if !appearance.backdrop() {
            if self.applied.take().is_some() {
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                }
            }
            return;
        }
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        let applied = self.applied.get();
        unsafe {
            if applied.map(|(a, _, _)| a) != Some(appearance) {
                // ACCENT_ENABLE_ACRYLICBLURBEHIND y ACCENT_ENABLE_BLURBEHIND.
                let state = if appearance == Appearance::Acrylic {
                    4
                } else {
                    3
                };
                set_accent(self.hwnd, state, ACRYLIC_TINT);
            }
            if applied.map(|(_, w, h)| (w, h)) != Some((width, height)) {
                let region =
                    CreateRoundRectRgn(0, 0, width + 1, height + 1, radius * 2, radius * 2);
                // Windows se queda con la región.
                let _ = SetWindowRgn(self.hwnd, Some(region), true);
            }
            // Justo debajo del dock, aunque otra ventana "siempre encima" se haya
            // puesto entre medias.
            let _ = SetWindowPos(
                self.hwnd,
                Some(dock),
                rect.left,
                rect.top,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        self.applied.set(Some((appearance, width, height)));
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

/// Tinte del acrílico (AABBGGRR): el gris oscuro del dock.
const ACRYLIC_TINT: u32 = 0xB0_24_1c_17;

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
