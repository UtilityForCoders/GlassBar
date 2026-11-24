//! Notificaciones al estilo de macOS: tarjetas de cristal arriba a la derecha en
//! lugar de los avisos de Windows abajo a la derecha.
//!
//! Windows no deja cambiar el aspecto de sus avisos: los dibuja `ShellExperienceHost`
//! en una ventana propia ("Nueva notificación"), que no se muestra ni se oculta sino
//! que DWM la tapa (cloak) y destapa. Un hilo aparte (`Watcher`) escucha esos cambios
//! en ese proceso, lee con UI Automation lo que dice cada aviso (app, título, texto y
//! botones) y se lo pasa a la ventana de GlassBar, que dibuja las tarjetas (`Stack`).
//! La ventana de Windows se deja transparente, pero sigue viva: sus avisos duran lo
//! mismo de siempre y pulsar una tarjeta pulsa, por UI Automation, el aviso original
//! (abre la app, ejecuta un botón o lo descarta).
//!
//! Si GlassBar se cierra, la ventana de Windows vuelve a ser opaca (`restore`); si se
//! cierra a la fuerza, lo hace el vigilante (`restore_native`, ver `guard`).
use crate::apps::{fill_rounded, stroke_rounded};
use crate::clock::{
    draw_icon, draw_text_on_baseline, draw_text_semibold_on_baseline, fit_text, line_metrics,
    text_width,
};
use crate::spring::Spring;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;
use windows::Win32::Foundation::{CloseHandle, COLORREF, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Proceso que dibuja los avisos de Windows.
const HOST: &str = "ShellExperienceHost.exe";
/// Clase de su ventana (la comparte con otras ventanas del sistema).
const CORE_WINDOW: &str = "Windows.UI.Core.CoreWindow";
/// Títulos que tiene en reposo, para dejarla transparente antes del primer aviso.
/// Si el idioma de Windows es otro, se reconoce al llegar el primer aviso.
const IDLE_TITLES: [&str; 2] = ["Nueva notificación", "New notification"];

/// Espera tras un cambio antes de leerla: el contenido llega justo después.
const READ_DELAY: Duration = Duration::from_millis(40);
/// Mientras hay avisos se vuelve a leer cada tanto, por si cambian sin avisar.
const REREAD: Duration = Duration::from_millis(500);
/// Sin `ShellExperienceHost` (se reinicia con Explorer), cada cuánto se busca.
const RETRY_MS: u32 = 5000;

/// Un aviso de Windows, tal como lo enseña su ventana.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Toast {
    /// Nombre de la app que lo envía.
    pub sender: String,
    pub title: String,
    pub body: String,
    /// Texto de sus botones, en orden.
    pub actions: Vec<String>,
    /// De dónde viene, si Windows lo indica: "a través de Enlace Móvil" en los
    /// avisos de las apps del móvil.
    pub attribution: String,
}

/// Cómo se llama Enlace Móvil (antes "Tu teléfono") en el texto de origen de sus
/// avisos, en minúsculas.
const PHONE_LINK_NAMES: [&str; 5] = [
    "enlace móvil",
    "phone link",
    "tu teléfono",
    "your phone",
    "vínculo móvil",
];

/// Clave del icono de Enlace Móvil entre los de las apps (ver `Stack::icons`).
pub const PHONE_LINK: &str = "\u{0}Enlace Móvil";
/// Enlace Móvil en el menú Inicio, igual en todos los idiomas.
const PHONE_LINK_APP: &str = "shell:AppsFolder\\Microsoft.YourPhone_8wekyb3d8bbwe!App";

impl Toast {
    /// Llega del móvil a través de Enlace Móvil (no de la app del PC).
    pub fn is_from_phone(&self) -> bool {
        let text = self.attribution.to_lowercase();
        PHONE_LINK_NAMES.iter().any(|name| text.contains(name))
    }
}

/// Con qué se reconoce un aviso entre dos lecturas: su contenido y, si hay varios
/// iguales, cuál de ellos es.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    sender: String,
    title: String,
    body: String,
    phone: bool,
    nth: usize,
}

/// La clave de cada aviso de la lista.
pub fn keys(toasts: &[Toast]) -> Vec<Key> {
    let mut seen: HashMap<(&str, &str, &str, bool), usize> = HashMap::new();
    toasts
        .iter()
        .map(|t| {
            let phone = t.is_from_phone();
            let count = seen
                .entry((&t.sender, &t.title, &t.body, phone))
                .or_default();
            let key = Key {
                sender: t.sender.clone(),
                title: t.title.clone(),
                body: t.body.clone(),
                phone,
                nth: *count,
            };
            *count += 1;
            key
        })
        .collect()
}

/// Lo que el hilo cuenta a la ventana.
pub enum Event {
    /// Los avisos que hay ahora en pantalla (vacío: ninguno).
    Toasts(Vec<Toast>),
    /// El icono de la app `sender` (`None` si no se encontró).
    Icon(String, Option<Pixmap>),
}

/// Lo que la ventana pide al hilo.
pub enum Command {
    /// Pulsar el aviso: abre la app como al pulsarlo en Windows.
    Open(Key),
    /// Pulsar su botón número `n`.
    Action(Key, usize),
    /// Descartarlo (pasa al centro de notificaciones).
    Dismiss(Key),
}

/// El hilo que vigila los avisos de Windows. Al soltarlo, el hilo termina y la
/// ventana de Windows vuelve a verse.
pub struct Watcher {
    thread: u32,
    commands: Sender<Command>,
    events: Receiver<Event>,
    /// Para los hilos que buscan iconos.
    sender: Sender<Event>,
    target: isize,
    message: u32,
}

impl Watcher {
    /// Arranca el hilo. Cada vez que tiene algo que contar, envía `message` a
    /// `target` y lo deja en `events`.
    pub fn start(target: HWND, message: u32) -> Option<Self> {
        let (event_tx, events) = std::sync::mpsc::channel();
        let (command_tx, commands) = std::sync::mpsc::channel();
        let (id_tx, id_rx) = std::sync::mpsc::channel();
        let target = target.0 as isize;
        let sender = event_tx.clone();
        std::thread::Builder::new()
            .name("notificaciones".into())
            .spawn(move || {
                unsafe {
                    // Crea la cola de mensajes del hilo antes de dar su id.
                    let mut msg = MSG::default();
                    let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                    let _ = id_tx.send(windows::Win32::System::Threading::GetCurrentThreadId());
                }
                run(target, message, event_tx, commands);
            })
            .ok()?;
        let thread = id_rx.recv_timeout(Duration::from_secs(2)).ok()?;
        Some(Self {
            thread,
            commands: command_tx,
            events,
            sender,
            target,
            message,
        })
    }

    /// Busca en otro hilo el icono de la app `sender` y lo cuenta como `Event::Icon`.
    pub fn find_icon(&self, sender: String) {
        let events = self.sender.clone();
        let (target, message) = (self.target, self.message);
        let _ = std::thread::Builder::new()
            .name("icono de aviso".into())
            .spawn(move || {
                let icon = find_icon(&sender);
                if events.send(Event::Icon(sender, icon)).is_ok() {
                    post(target, message);
                }
            });
    }

    pub fn send(&self, command: Command) {
        if self.commands.send(command).is_ok() {
            self.wake(WM_NULL);
        }
    }

    /// Lo que el hilo ha contado desde la última vez.
    pub fn events(&self) -> impl Iterator<Item = Event> + '_ {
        self.events.try_iter()
    }

    fn wake(&self, message: u32) {
        unsafe {
            let _ = PostThreadMessageW(self.thread, message, WPARAM(0), LPARAM(0));
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.wake(WM_QUIT);
        // El hilo también la restaura al salir; esto la deja bien aunque esté
        // ocupado en una lectura.
        restore();
    }
}

thread_local! {
    /// Ventanas de `ShellExperienceHost` que cambiaron desde la última lectura.
    static CHANGED: RefCell<HashSet<isize>> = RefCell::new(HashSet::new());
}

unsafe extern "system" fn on_event(
    _: HWINEVENTHOOK,
    _: u32,
    hwnd: HWND,
    object: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    if object != OBJID_WINDOW.0 || hwnd.is_invalid() || class_of(hwnd) != CORE_WINDOW {
        return;
    }
    CHANGED.with(|c| c.borrow_mut().insert(hwnd.0 as isize));
}

/// `ShellExperienceHost` en marcha, con los ganchos que escuchan sus ventanas.
struct Host {
    pid: u32,
    process: HANDLE,
    hooks: [HWINEVENTHOOK; 2],
}

impl Host {
    fn attach() -> Option<Self> {
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
        let pid = host_pid()?;
        let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.ok()?;
        let hook = |from, to| unsafe {
            SetWinEventHook(
                from,
                to,
                None,
                Some(on_event),
                pid,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        let hooks = [
            hook(EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
            hook(EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE),
        ];
        Some(Self {
            pid,
            process,
            hooks,
        })
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        unsafe {
            for hook in self.hooks {
                if !hook.is_invalid() {
                    let _ = UnhookWinEvent(hook);
                }
            }
            let _ = CloseHandle(self.process);
        }
    }
}

fn host_pid() -> Option<u32> {
    crate::audio::processes()
        .into_iter()
        .find(|(_, (_, name))| name.eq_ignore_ascii_case(HOST))
        .map(|(pid, _)| pid)
}

/// Las ventanas `CoreWindow` del proceso `pid`.
fn core_windows(pid: u32) -> Vec<HWND> {
    let class: Vec<u16> = CORE_WINDOW.encode_utf16().chain(Some(0)).collect();
    let mut result = Vec::new();
    let mut previous = None;
    unsafe {
        while let Ok(hwnd) =
            FindWindowExW(None, previous, windows::core::PCWSTR(class.as_ptr()), None)
        {
            let mut owner = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut owner));
            if owner == pid {
                result.push(hwnd);
            }
            previous = Some(hwnd);
        }
    }
    result
}

fn run(target: isize, message: u32, events: Sender<Event>, commands: Receiver<Command>) {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let automation: Option<IUIAutomation> =
        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.ok();
    if let Some(automation) = automation {
        let mut watch = Watch {
            automation,
            host: None,
            window: None,
            shown: Vec::new(),
            target,
            message,
            events,
        };
        watch.run(&commands);
    }
    restore();
    if initialized {
        unsafe { CoUninitialize() };
    }
}

struct Watch {
    automation: windows::Win32::UI::Accessibility::IUIAutomation,
    host: Option<Host>,
    /// La ventana de los avisos, cuando ya se sabe cuál es.
    window: Option<HWND>,
    shown: Vec<Toast>,
    target: isize,
    message: u32,
    events: Sender<Event>,
}

impl Watch {
    fn run(&mut self, commands: &Receiver<Command>) {
        use windows::Win32::Foundation::WAIT_OBJECT_0;
        use windows::Win32::System::Threading::INFINITE;
        let mut due: Option<Instant> = None;
        loop {
            if self.host.is_none() {
                self.host = Host::attach();
                if let Some(host) = &self.host {
                    self.window = find_idle(host.pid);
                    if let Some(window) = self.window {
                        hide(window);
                    }
                }
            }
            let timeout = match (due, &self.host) {
                (Some(at), _) => {
                    let left = at.saturating_duration_since(Instant::now()).as_millis();
                    left.min(u32::MAX as u128) as u32
                }
                (None, None) => RETRY_MS,
                (None, Some(_)) => INFINITE,
            };
            let handles = self.host.as_ref().map(|h| [h.process]);
            let woke = unsafe {
                MsgWaitForMultipleObjects(
                    handles.as_ref().map(|h| &h[..]),
                    false,
                    timeout,
                    QS_ALLINPUT,
                )
            };
            // `ShellExperienceHost` se cerró: se vuelve a buscar.
            if self.host.is_some() && woke == WAIT_OBJECT_0 {
                self.host = None;
                self.window = None;
                CHANGED.with(|c| c.borrow_mut().clear());
                self.publish(Vec::new());
                continue;
            }
            let mut msg = MSG::default();
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        return;
                    }
                    DispatchMessageW(&msg);
                }
            }
            let now = Instant::now();
            if CHANGED.with(|c| !c.borrow().is_empty()) {
                let at = now + READ_DELAY;
                due = Some(due.map_or(at, |d| d.min(at)));
            }
            for command in commands.try_iter() {
                self.act(command);
                due = Some(now + READ_DELAY);
            }
            if due.is_some_and(|at| at <= Instant::now()) {
                let toasts = self.read();
                due = (!toasts.is_empty()).then(|| Instant::now() + REREAD);
                self.publish(toasts);
            }
        }
    }

    fn publish(&mut self, toasts: Vec<Toast>) {
        if toasts == self.shown {
            return;
        }
        self.shown = toasts.clone();
        if self.events.send(Event::Toasts(toasts)).is_ok() {
            self.post();
        }
    }

    fn post(&self) {
        post(self.target, self.message);
    }

    /// Los avisos que se ven ahora. De paso reconoce la ventana de los avisos si
    /// aún no se sabía cuál era, y la deja transparente.
    fn read(&mut self) -> Vec<Toast> {
        let mut candidates: Vec<isize> = CHANGED.with(|c| c.borrow_mut().drain().collect());
        if let Some(window) = self.window {
            candidates.retain(|&h| h != window.0 as isize);
            candidates.insert(0, window.0 as isize);
        }
        for raw in candidates {
            let hwnd = HWND(raw as *mut _);
            let known = self.window == Some(hwnd);
            if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
                if known {
                    self.window = None;
                }
                continue;
            }
            if is_cloaked(hwnd) {
                if known {
                    return Vec::new();
                }
                continue;
            }
            let entries = entries(&self.automation, hwnd);
            if !entries.is_empty() {
                if !known {
                    self.window = Some(hwnd);
                }
                hide(hwnd);
                return entries.into_iter().map(|e| e.toast).collect();
            }
            if known {
                return Vec::new();
            }
        }
        Vec::new()
    }

    fn act(&mut self, command: Command) {
        let Some(window) = self.window else {
            return;
        };
        let entries = entries(&self.automation, window);
        let toasts: Vec<Toast> = entries.iter().map(|e| e.toast.clone()).collect();
        let keys = keys(&toasts);
        let find = |key: &Key| keys.iter().position(|k| k == key).map(|i| &entries[i]);
        let element = match &command {
            Command::Open(key) => find(key).map(|e| e.view.clone()),
            Command::Action(key, n) => find(key).and_then(|e| e.actions.get(*n).cloned()),
            Command::Dismiss(key) => find(key).and_then(|e| e.dismiss.clone()),
        };
        if let Some(element) = element {
            invoke(&element);
        }
    }
}

fn post(target: isize, message: u32) {
    unsafe {
        let _ = PostMessageW(Some(HWND(target as *mut _)), message, WPARAM(0), LPARAM(0));
    }
}

/// La ventana de los avisos en reposo, por su título (si Windows está en un idioma
/// conocido).
fn find_idle(pid: u32) -> Option<HWND> {
    core_windows(pid).into_iter().find(|&hwnd| {
        let mut text = [0u16; 128];
        let len = unsafe { GetWindowTextW(hwnd, &mut text) };
        let title = String::from_utf16_lossy(&text[..len.max(0) as usize]);
        IDLE_TITLES.contains(&title.as_str())
    })
}

fn is_cloaked(hwnd: HWND) -> bool {
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    let mut cloaked = 0u32;
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut _,
            std::mem::size_of::<u32>() as u32,
        )
        .is_ok()
            && cloaked != 0
    }
}

/// Un aviso leído con UI Automation, con lo que se puede pulsar de él.
struct Entry {
    toast: Toast,
    view: windows::Win32::UI::Accessibility::IUIAutomationElement,
    actions: Vec<windows::Win32::UI::Accessibility::IUIAutomationElement>,
    dismiss: Option<windows::Win32::UI::Accessibility::IUIAutomationElement>,
}

/// Los avisos de la ventana `hwnd`. Cada aviso es un elemento ventana
/// (`NormalToastView`, `FlexibleToastView`) seguido de sus partes, en orden.
fn entries(
    automation: &windows::Win32::UI::Accessibility::IUIAutomation,
    hwnd: HWND,
) -> Vec<Entry> {
    use windows::Win32::UI::Accessibility::{TreeScope_Descendants, UIA_WindowControlTypeId};
    let mut result: Vec<Entry> = Vec::new();
    unsafe {
        let Ok(root) = automation.ElementFromHandle(hwnd) else {
            return result;
        };
        let Ok(condition) = automation.CreateTrueCondition() else {
            return result;
        };
        let Ok(all) = root.FindAll(TreeScope_Descendants, &condition) else {
            return result;
        };
        let count = all.Length().unwrap_or(0);
        for i in 0..count {
            let Ok(element) = all.GetElement(i) else {
                continue;
            };
            let id = element
                .CurrentAutomationId()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let text = || {
                element
                    .CurrentName()
                    .map(|s| s.to_string())
                    .unwrap_or_default()
            };
            let is_view = id.ends_with("ToastView")
                || (element.CurrentControlType().ok() == Some(UIA_WindowControlTypeId)
                    && element
                        .CurrentClassName()
                        .is_ok_and(|c| c.to_string().ends_with("ToastView")));
            if is_view {
                result.push(Entry {
                    toast: Toast::default(),
                    view: element.clone(),
                    actions: Vec::new(),
                    dismiss: None,
                });
                continue;
            }
            let Some(entry) = result.last_mut() else {
                continue;
            };
            match id.as_str() {
                "SenderName" => entry.toast.sender = clean(&text()),
                "Title" => entry.toast.title = clean(&text()),
                "MessageText" => {
                    let line = clean(&text());
                    if !line.is_empty() {
                        if !entry.toast.body.is_empty() {
                            entry.toast.body.push(' ');
                        }
                        entry.toast.body.push_str(&line);
                    }
                }
                "VerbButton" => {
                    entry.toast.actions.push(clean(&text()));
                    entry.actions.push(element.clone());
                }
                "DismissButton" => entry.dismiss = Some(element.clone()),
                "Attribution" => entry.toast.attribution = clean(&text()),
                _ => {}
            }
        }
    }
    // Una vista sin nada legible no es un aviso (o aún se está llenando).
    result.retain(|e| !e.toast.sender.is_empty() || !e.toast.title.is_empty());
    result
}

/// Texto en una línea, sin espacios de sobra ni caracteres de formato invisibles.
fn clean(text: &str) -> String {
    text.chars()
        .filter(|&c| !is_invisible_format(c))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Marcas de dirección y espacios de ancho cero que no se ven pero la fuente
/// pinta como un recuadro. WhatsApp, por ejemplo, rodea el nombre de quien
/// escribe en un grupo con U+2068 y U+2069. La unión de emojis (U+200D) se queda.
fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

fn invoke(element: &windows::Win32::UI::Accessibility::IUIAutomationElement) {
    use windows::Win32::UI::Accessibility::{IUIAutomationInvokePattern, UIA_InvokePatternId};
    unsafe {
        if let Ok(pattern) =
            element.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
        {
            let _ = pattern.Invoke();
        }
    }
}

/// Ventana de avisos que GlassBar dejó transparente y su estilo de antes.
static HIDDEN: AtomicIsize = AtomicIsize::new(0);
static HIDDEN_STYLE: AtomicIsize = AtomicIsize::new(0);

/// Deja transparente (y que no reciba el ratón) la ventana de avisos de Windows.
fn hide(hwnd: HWND) {
    let layered = (WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0) as isize;
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if HIDDEN.load(Ordering::Relaxed) != hwnd.0 as isize {
            restore();
            HIDDEN_STYLE.store(style & !layered, Ordering::Relaxed);
            HIDDEN.store(hwnd.0 as isize, Ordering::Relaxed);
        }
        if style & layered != layered {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | layered);
        }
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
    }
}

/// Vuelve a dejar visible la ventana de avisos de Windows.
pub fn restore() {
    let raw = HIDDEN.swap(0, Ordering::Relaxed);
    if raw == 0 {
        return;
    }
    let hwnd = HWND(raw as *mut _);
    unsafe {
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, HIDDEN_STYLE.load(Ordering::Relaxed));
        }
    }
}

/// Desde otro proceso (el vigilante): vuelve visibles las ventanas de
/// `ShellExperienceHost` que estén transparentes del todo, como las deja GlassBar.
pub fn restore_native() {
    let Some(pid) = host_pid() else {
        return;
    };
    let layered = (WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0) as isize;
    for hwnd in core_windows(pid) {
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            if style & layered != layered {
                continue;
            }
            let mut alpha = 255u8;
            let mut flags = LAYERED_WINDOW_ATTRIBUTES_FLAGS(0);
            let read = GetLayeredWindowAttributes(hwnd, None, Some(&mut alpha), Some(&mut flags));
            if read.is_ok() && flags.contains(LWA_ALPHA) && alpha == 0 {
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style & !layered);
            }
        }
    }
}

fn class_of(hwnd: HWND) -> String {
    let mut name = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut name) };
    String::from_utf16_lossy(&name[..len.max(0) as usize])
}

/// Apps de Inicio, con su nombre ya preparado para comparar, y cuándo se leyeron.
type Catalog = (Instant, Vec<(String, crate::apps::AppEntry)>);
static CATALOG: std::sync::Mutex<Option<Catalog>> = std::sync::Mutex::new(None);
/// Si una app no aparece, el catálogo se vuelve a leer pasado este tiempo (puede
/// estar recién instalada).
const CATALOG_STALE: Duration = Duration::from_secs(300);

/// Lee el catálogo de apps (tarda un poco), para que el primer aviso ya tenga icono.
pub fn warm_icons() {
    let _ = std::thread::Builder::new()
        .name("catálogo de avisos".into())
        .spawn(|| {
            catalog_entry("", false);
        });
}

/// La app de Inicio que se llama `wanted` (ya con `search_key`). Con `retry`, si no
/// está y el catálogo es viejo, lo vuelve a leer.
fn catalog_entry(wanted: &str, retry: bool) -> Option<crate::apps::AppEntry> {
    let mut catalog = CATALOG.lock().unwrap_or_else(|e| e.into_inner());
    let stale = catalog
        .as_ref()
        .is_none_or(|(read, _)| read.elapsed() > CATALOG_STALE);
    if catalog.is_none() {
        *catalog = Some(read_catalog());
    }
    let found = catalog
        .as_ref()
        .and_then(|(_, apps)| matching(apps, wanted));
    if found.is_some() || !retry || !stale {
        return found;
    }
    *catalog = Some(read_catalog());
    catalog
        .as_ref()
        .and_then(|(_, apps)| matching(apps, wanted))
}

fn read_catalog() -> Catalog {
    let apps = crate::catalog::windows_apps()
        .into_iter()
        .map(|a| (crate::catalog::search_key(&a.name), a))
        .collect();
    (Instant::now(), apps)
}

fn matching(
    apps: &[(String, crate::apps::AppEntry)],
    wanted: &str,
) -> Option<crate::apps::AppEntry> {
    if wanted.is_empty() {
        return None;
    }
    let exact = apps.iter().find(|(k, _)| k == wanted);
    // "WhatsApp" para "WhatsApp Beta", o al revés; con un mínimo para no confundir.
    let close = || {
        apps.iter()
            .filter(|(k, _)| k.len() >= 4 && wanted.len() >= 4)
            .find(|(k, _)| k.starts_with(wanted) || wanted.starts_with(k.as_str()))
    };
    exact.or_else(close).map(|(_, entry)| entry.clone())
}

/// El icono de la app que se llama `sender`, buscándola entre las de Inicio. Puede
/// tardar (lee el catálogo): hay que llamarla fuera del hilo de la interfaz.
pub fn find_icon(sender: &str) -> Option<Pixmap> {
    if sender == PHONE_LINK {
        return crate::icons::load(PHONE_LINK_APP);
    }
    let entry = catalog_entry(&crate::catalog::search_key(sender), true)?;
    crate::icons::load(&entry.command)
}

// --- Tarjetas -----------------------------------------------------------------

/// Ancho de una tarjeta y margen alrededor (deja sitio al botón de cerrar).
pub const CARD_W: f32 = 344.0;
pub const MARGIN: f32 = 12.0;
pub const WIDTH: f32 = CARD_W + MARGIN * 2.0;
/// Cuántas tarjetas se ven a la vez, como mucho.
const MAX_CARDS: usize = 4;
const GAP: f32 = 10.0;
const PAD: f32 = 12.0;
const RADIUS: f32 = 16.0;
const ICON: f32 = 32.0;
const TEXT_X: f32 = PAD + ICON + 11.0;
const TEXT_W: f32 = CARD_W - TEXT_X - PAD;
const SENDER_SIZE: f32 = 11.5;
const TITLE_SIZE: f32 = 13.0;
const BODY_SIZE: f32 = 12.5;
const LINE: f32 = 17.0;
const BODY_LINES: usize = 3;
const ACTION_H: f32 = 28.0;
const ACTION_GAP: f32 = 6.0;
const ACTION_SIZE: f32 = 12.5;
/// Botón de cerrar, en la esquina de arriba a la izquierda al pasar el cursor.
const CLOSE_R: f32 = 10.0;
const CLOSE_AT: f32 = 4.0;
const GLYPH_CLOSE: char = '\u{E8BB}';
const NOW: &str = "ahora";
/// Distintivo de Enlace Móvil sobre el icono de la app.
const BADGE: f32 = 16.0;
const GLYPH_PHONE: char = '\u{E8EA}';

/// Alto más grande que puede necesitar la pila: el de la ventana.
pub fn max_height() -> f32 {
    let card = PAD + 36.0 + LINE * BODY_LINES as f32 + 8.0 + ACTION_H + PAD;
    MARGIN * 2.0 + card * MAX_CARDS as f32 + GAP * (MAX_CARDS - 1) as f32
}

/// Parte de una tarjeta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Body,
    Close,
    Action(usize),
}

struct Card {
    key: Key,
    toast: Toast,
    /// Líneas del texto ya cortadas al ancho.
    lines: Vec<String>,
    height: f32,
    /// 0 fuera (a la derecha), 1 en su sitio.
    enter: Spring,
    top: Spring,
    /// Windows ya lo quitó; se queda mientras el cursor esté encima.
    gone: bool,
    leaving: bool,
}

impl Card {
    fn new(key: Key, toast: Toast, top: f32) -> Self {
        let lines = wrap(&toast.body, BODY_SIZE, TEXT_W, BODY_LINES);
        let height = card_height(&toast, lines.len());
        Self {
            key,
            toast,
            lines,
            height,
            enter: Spring::new(0.0, 260.0, 27.0),
            top: Spring::new(top, 300.0, 34.0),
            gone: false,
            leaving: false,
        }
    }

    /// Desplazamiento a la derecha, en unidades lógicas.
    fn shift(&self) -> f32 {
        (1.0 - self.enter.value) * (CARD_W + MARGIN * 2.0)
    }

    fn left(&self) -> f32 {
        MARGIN + self.shift()
    }

    fn leave(&mut self) {
        self.leaving = true;
        self.enter.set_target(0.0);
    }

    fn actions_top(&self) -> f32 {
        self.height - PAD - ACTION_H
    }

    /// Borde izquierdo y ancho de cada botón, relativos a la tarjeta.
    fn action_slots(&self) -> Vec<(f32, f32)> {
        let n = self.toast.actions.len();
        if n == 0 {
            return Vec::new();
        }
        let width = (TEXT_W - ACTION_GAP * (n - 1) as f32) / n as f32;
        (0..n)
            .map(|i| (TEXT_X + i as f32 * (width + ACTION_GAP), width))
            .collect()
    }
}

fn card_height(toast: &Toast, body_lines: usize) -> f32 {
    let title = if toast.title.is_empty() { 0.0 } else { 18.0 };
    let text = PAD + 18.0 + title + LINE * body_lines as f32;
    let mut height = text.max(PAD + ICON) + PAD - 2.0;
    if !toast.actions.is_empty() {
        height += 8.0 + ACTION_H;
    }
    height
}

/// Corta `text` en líneas de `width` como mucho; la última, si no cabe todo, acaba
/// en "…".
fn wrap(text: &str, size: f32, width: f32, max_lines: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut lines = Vec::new();
    let mut at = 0;
    while at < words.len() && lines.len() < max_lines {
        if lines.len() + 1 == max_lines {
            lines.push(fit_text(&words[at..].join(" "), size, width));
            break;
        }
        let mut line = words[at].to_string();
        at += 1;
        while at < words.len() {
            let longer = format!("{line} {}", words[at]);
            if text_width(&longer, size) > width {
                break;
            }
            line = longer;
            at += 1;
        }
        lines.push(fit_text(&line, size, width));
    }
    lines
}

/// Las tarjetas que se ven, la más nueva arriba.
#[derive(Default)]
pub struct Stack {
    cards: Vec<Card>,
    pub hovered: Option<(Key, Part)>,
    /// Iconos originales por app y su copia reducida.
    icons: HashMap<String, Option<Pixmap>>,
    scaled: RefCell<HashMap<String, Pixmap>>,
    scaled_side: Cell<u32>,
}

impl Stack {
    /// Pone las tarjetas al día con los avisos de Windows. Devuelve las apps cuyo
    /// icono aún no se conoce.
    pub fn update(&mut self, toasts: Vec<Toast>) -> Vec<String> {
        let keys = keys(&toasts);
        let hovered = self.hovered.as_ref().map(|(k, _)| k.clone());
        for card in &mut self.cards {
            if !card.leaving && !keys.contains(&card.key) {
                card.gone = true;
                if hovered.as_ref() != Some(&card.key) {
                    card.leave();
                }
            }
        }
        // Las nuevas entran arriba; si hay varias, en el orden de Windows.
        let mut fresh = Vec::new();
        for (key, toast) in keys.into_iter().zip(toasts) {
            if let Some(card) = self.cards.iter_mut().find(|c| c.key == key) {
                if card.leaving || card.gone {
                    card.leaving = false;
                    card.gone = false;
                    card.enter.set_target(1.0);
                }
                continue;
            }
            fresh.push(Card::new(key, toast, MARGIN));
        }
        let mut missing = Vec::new();
        for mut card in fresh.into_iter().rev() {
            card.enter.set_target(1.0);
            let mut wanted = vec![card.toast.sender.clone()];
            if card.toast.is_from_phone() {
                wanted.push(PHONE_LINK.to_string());
            }
            for name in wanted {
                if !self.icons.contains_key(&name) && !missing.contains(&name) {
                    missing.push(name);
                }
            }
            self.cards.insert(0, card);
        }
        self.relayout();
        missing
    }

    pub fn set_icon(&mut self, sender: String, icon: Option<Pixmap>) {
        self.scaled.borrow_mut().remove(&sender);
        self.icons.insert(sender, icon);
    }

    /// Ya se pidió el icono de `sender` (aunque no haya llegado).
    pub fn mark_icon_pending(&mut self, sender: &str) {
        self.icons.entry(sender.to_string()).or_insert(None);
    }

    fn relayout(&mut self) {
        let mut top = MARGIN;
        let mut shown = 0;
        for card in &mut self.cards {
            if card.leaving {
                continue;
            }
            if shown >= MAX_CARDS {
                // Las que no caben salen por la derecha; vuelven si se libera sitio.
                card.enter.set_target(0.0);
                continue;
            }
            if !card.gone {
                card.enter.set_target(1.0);
            }
            card.top.set_target(top);
            top += card.height + GAP;
            shown += 1;
        }
    }

    /// Avanza las animaciones. Devuelve si algo se sigue moviendo.
    pub fn step(&mut self, dt: f32) -> bool {
        let mut moving = false;
        for card in &mut self.cards {
            moving |= card.enter.step(dt);
            moving |= card.top.step(dt);
        }
        let before = self.cards.len();
        self.cards
            .retain(|c| !(c.leaving && c.enter.value <= 0.01 && c.enter.target == 0.0));
        if self.cards.len() != before {
            self.relayout();
            moving = true;
        }
        moving
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// El cursor salió: se van las tarjetas que Windows ya quitó.
    pub fn mouse_left(&mut self) -> bool {
        let changed = self.hovered.take().is_some();
        let mut gone = false;
        for card in &mut self.cards {
            if card.gone && !card.leaving {
                card.leave();
                gone = true;
            }
        }
        if gone {
            self.relayout();
        }
        changed || gone
    }

    /// Quita la tarjeta de `key` (se pulsó o se cerró). Devuelve si Windows aún
    /// tenía el aviso, para pulsarlo allí.
    pub fn take(&mut self, key: &Key) -> bool {
        let Some(card) = self.cards.iter_mut().find(|c| &c.key == key) else {
            return false;
        };
        let alive = !card.gone;
        card.leave();
        self.relayout();
        alive
    }

    /// Qué hay en `(x, y)` (unidades lógicas de la ventana).
    pub fn hit_test(&self, x: f32, y: f32) -> Option<(Key, Part)> {
        for card in self.cards.iter().filter(|c| !c.leaving) {
            let (left, top) = (card.left(), card.top.value);
            let hovered = self.hovered.as_ref().is_some_and(|(k, _)| k == &card.key);
            if hovered {
                let (cx, cy) = (left + CLOSE_AT, top + CLOSE_AT);
                if (x - cx).powi(2) + (y - cy).powi(2) <= (CLOSE_R + 2.0).powi(2) {
                    return Some((card.key.clone(), Part::Close));
                }
            }
            if x < left || x > left + CARD_W || y < top || y > top + card.height {
                continue;
            }
            let (rx, ry) = (x - left, y - top);
            let actions_top = card.actions_top();
            if (actions_top..actions_top + ACTION_H).contains(&ry) {
                for (i, (at, width)) in card.action_slots().into_iter().enumerate() {
                    if (at..at + width).contains(&rx) {
                        return Some((card.key.clone(), Part::Action(i)));
                    }
                }
            }
            return Some((card.key.clone(), Part::Body));
        }
        None
    }

    /// Rectángulos de las tarjetas en píxeles, para el desenfoque de debajo.
    pub fn shapes(&self, scale: f32) -> Vec<crate::style::Shape> {
        self.cards
            .iter()
            .filter(|c| c.enter.value > 0.001)
            .map(|c| crate::style::Shape {
                x: c.left() * scale,
                y: c.top.value * scale,
                width: CARD_W * scale,
                height: c.height * scale,
                radius: RADIUS * scale,
            })
            .collect()
    }

    pub fn draw(&self, image: &mut Pixmap, scale: f32) {
        image.fill(tiny_skia::Color::TRANSPARENT);
        if self.scaled_side.get() != (ICON * scale).round() as u32 {
            self.scaled_side.set((ICON * scale).round() as u32);
            self.scaled.borrow_mut().clear();
        }
        // De abajo arriba: la de arriba queda por encima al cruzarse.
        for card in self.cards.iter().rev() {
            if card.enter.value <= 0.001 {
                continue;
            }
            let hovered = self
                .hovered
                .as_ref()
                .filter(|(k, _)| k == &card.key)
                .map(|(_, p)| *p);
            self.draw_card(image, card, hovered, scale);
        }
    }

    fn draw_card(&self, image: &mut Pixmap, card: &Card, hovered: Option<Part>, scale: f32) {
        let s = |v: f32| v * scale;
        let (x, y) = (card.left(), card.top.value);
        let (w, h) = (CARD_W, card.height);
        fill_rounded(
            image,
            crate::glass::panel(),
            s(x),
            s(y),
            s(w),
            s(h),
            s(RADIUS),
        );
        if hovered.is_some() {
            fill_rounded(image, [255, 255, 255, 8], s(x), s(y), s(w), s(h), s(RADIUS));
        }
        stroke_rounded(
            image,
            [255, 255, 255, 34],
            s(x),
            s(y),
            s(w),
            s(h),
            s(RADIUS),
            scale * 0.75,
        );

        // Icono de la app.
        let (ix, iy) = (x + PAD, y + PAD + 2.0);
        self.draw_icon(image, &card.toast.sender, s(ix), s(iy), s(ICON));
        if card.toast.is_from_phone() {
            self.draw_phone_badge(image, s(ix + ICON), s(iy + ICON), scale);
        }

        // Remitente y "ahora", en la primera línea.
        let tx = x + TEXT_X;
        let (ascent, _) = line_metrics(s(SENDER_SIZE));
        let baseline = s(y + PAD) + ascent;
        let now_w = text_width(NOW, s(SENDER_SIZE));
        let sender = fit_text(
            &card.toast.sender,
            s(SENDER_SIZE),
            s(TEXT_W) - now_w - s(8.0),
        );
        draw_text_on_baseline(image, &sender, s(tx), baseline, s(SENDER_SIZE), 0.62);
        draw_text_on_baseline(
            image,
            NOW,
            s(x + CARD_W - PAD) - now_w,
            baseline,
            s(SENDER_SIZE),
            0.5,
        );

        // Título y texto.
        let mut line_top = y + PAD + 18.0;
        if !card.toast.title.is_empty() {
            let (ascent, _) = line_metrics(s(TITLE_SIZE));
            let title = fit_text(&card.toast.title, s(TITLE_SIZE), s(TEXT_W));
            draw_text_semibold_on_baseline(
                image,
                &title,
                s(tx),
                s(line_top) + ascent,
                s(TITLE_SIZE),
                1.0,
            );
            line_top += 18.0;
        }
        let (ascent, _) = line_metrics(s(BODY_SIZE));
        for line in &card.lines {
            draw_text_on_baseline(image, line, s(tx), s(line_top) + ascent, s(BODY_SIZE), 0.85);
            line_top += LINE;
        }

        // Botones.
        let actions_top = y + card.actions_top();
        for (i, (at, width)) in card.action_slots().into_iter().enumerate() {
            let over = hovered == Some(Part::Action(i));
            fill_rounded(
                image,
                [255, 255, 255, if over { 44 } else { 22 }],
                s(x + at),
                s(actions_top),
                s(width),
                s(ACTION_H),
                s(8.0),
            );
            let label = fit_text(&card.toast.actions[i], s(ACTION_SIZE), s(width - 12.0));
            let label_w = text_width(&label, s(ACTION_SIZE));
            let (ascent, descent) = line_metrics(s(ACTION_SIZE));
            let baseline = s(actions_top + ACTION_H / 2.0) + (ascent - descent) / 2.0;
            draw_text_on_baseline(
                image,
                &label,
                s(x + at + width / 2.0) - label_w / 2.0,
                baseline,
                s(ACTION_SIZE),
                0.95,
            );
        }

        // Cerrar, al pasar el cursor.
        if hovered.is_some() {
            let (cx, cy) = (x + CLOSE_AT, y + CLOSE_AT);
            let over = hovered == Some(Part::Close);
            let shade = if over { 92 } else { 64 };
            fill_rounded(
                image,
                [shade, shade, shade, 240],
                s(cx - CLOSE_R),
                s(cy - CLOSE_R),
                s(CLOSE_R * 2.0),
                s(CLOSE_R * 2.0),
                s(CLOSE_R),
            );
            stroke_rounded(
                image,
                [255, 255, 255, 40],
                s(cx - CLOSE_R),
                s(cy - CLOSE_R),
                s(CLOSE_R * 2.0),
                s(CLOSE_R * 2.0),
                s(CLOSE_R),
                scale * 0.75,
            );
            draw_icon(image, GLYPH_CLOSE, s(cx), s(cy), s(8.0), 0.95);
        }
    }

    /// El icono de Enlace Móvil en la esquina de abajo a la derecha del de la app
    /// (centrado cerca de `(x, y)`), con un borde del color de la tarjeta para que
    /// se separe de él. Sin su icono, un móvil de la fuente de iconos.
    fn draw_phone_badge(&self, image: &mut Pixmap, x: f32, y: f32, scale: f32) {
        let side = BADGE * scale;
        let ring = side + 4.0 * scale;
        let (cx, cy) = (x - 2.0 * scale, y - 2.0 * scale);
        fill_rounded(
            image,
            [44, 44, 46, 255],
            cx - ring / 2.0,
            cy - ring / 2.0,
            ring,
            ring,
            ring * 0.3,
        );
        if matches!(self.icons.get(PHONE_LINK), Some(Some(_))) {
            self.draw_icon(image, PHONE_LINK, cx - side / 2.0, cy - side / 2.0, side);
        } else {
            draw_icon(image, GLYPH_PHONE, cx, cy, side * 0.7, 0.95);
        }
    }

    fn draw_icon(&self, image: &mut Pixmap, sender: &str, x: f32, y: f32, side: f32) {
        let px = side.round().max(1.0) as u32;
        if let Some(Some(icon)) = self.icons.get(sender) {
            let mut cache = self.scaled.borrow_mut();
            let scaled = cache
                .entry(sender.to_string())
                .or_insert_with(|| crate::apps::downscale(icon, px));
            image.draw_pixmap(
                (x + (side - scaled.width() as f32) / 2.0).round() as i32,
                (y + (side - scaled.height() as f32) / 2.0).round() as i32,
                scaled.as_ref(),
                &tiny_skia::PixmapPaint::default(),
                tiny_skia::Transform::identity(),
                None,
            );
            return;
        }
        // Sin icono: la inicial de la app sobre un cuadrado redondeado.
        fill_rounded(image, [255, 255, 255, 40], x, y, side, side, side * 0.24);
        let initial: String = sender
            .chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        if !initial.is_empty() {
            let size = side * 0.5;
            let (ascent, descent) = line_metrics(size);
            let width = text_width(&initial, size);
            draw_text_semibold_on_baseline(
                image,
                &initial,
                x + (side - width) / 2.0,
                y + side / 2.0 + (ascent - descent) / 2.0,
                size,
                0.95,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toast(sender: &str, title: &str, body: &str) -> Toast {
        Toast {
            sender: sender.into(),
            title: title.into(),
            body: body.into(),
            actions: Vec::new(),
            attribution: String::new(),
        }
    }

    #[test]
    fn clean_drops_invisible_format_characters() {
        assert_eq!(
            clean("\u{2068}Johan\u{2069}: \u{1F4F7} Foto\u{200E}"),
            "Johan: \u{1F4F7} Foto"
        );
        // La unión de un emoji compuesto se conserva.
        assert_eq!(
            clean("\u{1F469}\u{200D}\u{1F4BB}"),
            "\u{1F469}\u{200D}\u{1F4BB}"
        );
        assert_eq!(clean("  a \n b \u{FEFF} "), "a b");
    }

    #[test]
    fn phone_link_toasts_are_told_apart_from_the_pc_ones() {
        let pc = toast("WhatsApp", "Ana", "Hola");
        let mut phone = pc.clone();
        phone.attribution = "a través de Enlace Móvil".into();
        assert!(!pc.is_from_phone());
        assert!(phone.is_from_phone());
        let mut english = pc.clone();
        english.attribution = "via Phone Link".into();
        assert!(english.is_from_phone());
        // El mismo mensaje en el PC y en el móvil son dos tarjetas distintas.
        let keys = keys(&[pc, phone.clone()]);
        assert_ne!(keys[0], keys[1]);
        assert_eq!(keys[1].nth, 0);
        // Y la del móvil pide también el icono de Enlace Móvil.
        let mut stack = Stack::default();
        let missing = stack.update(vec![phone]);
        assert_eq!(
            missing,
            vec!["WhatsApp".to_string(), PHONE_LINK.to_string()]
        );
    }

    #[test]
    fn equal_toasts_get_different_keys() {
        let list = [
            toast("Chat", "Ana", "Hola"),
            toast("Chat", "Ana", "Hola"),
            toast("Chat", "Luis", "Hola"),
        ];
        let keys = keys(&list);
        assert_ne!(keys[0], keys[1]);
        assert_eq!(keys[0].nth, 0);
        assert_eq!(keys[1].nth, 1);
        assert_eq!(keys[2].nth, 0);
    }

    #[test]
    fn new_toasts_enter_on_top_and_old_ones_leave() {
        let mut stack = Stack::default();
        let missing = stack.update(vec![toast("Chat", "Ana", "Hola")]);
        assert_eq!(missing, vec!["Chat".to_string()]);
        stack.mark_icon_pending("Chat");
        let missing = stack.update(vec![
            toast("Chat", "Ana", "Hola"),
            toast("Correo", "Factura", "Ya está"),
        ]);
        assert_eq!(missing, vec!["Correo".to_string()]);
        assert_eq!(stack.cards[0].toast.sender, "Correo");
        assert_eq!(stack.cards[1].toast.sender, "Chat");
        // La segunda baja para dejar sitio a la nueva.
        assert!(stack.cards[1].top.target > stack.cards[0].top.target);

        stack.update(vec![toast("Correo", "Factura", "Ya está")]);
        assert!(stack.cards[1].leaving);
        assert!(!stack.cards[0].leaving);
        // Al terminar de salir desaparece.
        for _ in 0..200 {
            stack.step(1.0 / 60.0);
        }
        assert_eq!(stack.cards.len(), 1);
    }

    #[test]
    fn a_hovered_card_stays_until_the_cursor_leaves() {
        let mut stack = Stack::default();
        stack.update(vec![toast("Chat", "Ana", "Hola")]);
        let key = stack.cards[0].key.clone();
        stack.hovered = Some((key.clone(), Part::Body));
        stack.update(Vec::new());
        assert!(stack.cards[0].gone);
        assert!(!stack.cards[0].leaving);
        assert!(stack.mouse_left());
        assert!(stack.cards[0].leaving);
    }

    #[test]
    fn a_card_windows_already_removed_cannot_be_pressed_there() {
        let mut stack = Stack::default();
        stack.update(vec![toast("Chat", "Ana", "Hola")]);
        let key = stack.cards[0].key.clone();
        stack.hovered = Some((key.clone(), Part::Body));
        stack.update(Vec::new());
        assert!(!stack.take(&key));
        assert!(stack.cards[0].leaving);
    }

    #[test]
    fn only_the_newest_cards_fit() {
        let mut stack = Stack::default();
        let list: Vec<Toast> = (0..6)
            .map(|i| toast("Chat", &format!("Mensaje {i}"), ""))
            .collect();
        stack.update(list);
        let shown = stack.cards.iter().filter(|c| c.enter.target == 1.0).count();
        assert_eq!(shown, MAX_CARDS);
    }

    #[test]
    fn cards_with_buttons_are_taller() {
        let plain = toast("Chat", "Ana", "Hola");
        let mut with_actions = plain.clone();
        with_actions.actions = vec!["Responder".into()];
        assert!(card_height(&with_actions, 1) > card_height(&plain, 1));
        assert!(card_height(&plain, 3) <= max_height());
    }
}
