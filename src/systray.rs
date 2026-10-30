//! Iconos de la bandeja de los demás programas (las apps en segundo plano), para
//! mostrarlos en un panel propio en vez del panel nativo de iconos ocultos.
//!
//! Windows no deja leer los iconos de la bandeja de Explorer. Los programas los
//! registran con `Shell_NotifyIcon`, que los manda (`WM_COPYDATA`) a la primera
//! ventana `Shell_TrayWnd` que encuentra. GlassBar crea la suya y la mantiene
//! delante de la de Explorer, como hacen RetroBar o ManagedShell: así recibe cada
//! icono, se lo guarda y reenvía el mensaje a Explorer, cuya bandeja sigue igual.
use tiny_skia::Pixmap;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Mensajes de `Shell_NotifyIcon` (NIM_*).
const NIM_ADD: u32 = 0;
const NIM_MODIFY: u32 = 1;
const NIM_DELETE: u32 = 2;
const NIM_SETVERSION: u32 = 4;
/// Campos presentes (NIF_*).
const NIF_MESSAGE: u32 = 0x01;
const NIF_ICON: u32 = 0x02;
const NIF_TIP: u32 = 0x04;
const NIF_STATE: u32 = 0x08;
const NIF_GUID: u32 = 0x20;
const NIS_HIDDEN: u32 = 0x01;
/// Firma de los datos que manda `Shell_NotifyIcon`.
const SIGNATURE: u32 = 0x3475_3423;
/// `NIN_SELECT`: clic con la versión 4 de los iconos.
const NIN_SELECT: u32 = WM_USER;

/// Un icono tal como lo manda un programa. Los identificadores van en 32 bits para
/// que valgan entre procesos de 32 y 64 bits.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notify {
    pub flags: u32,
    pub owner: u32,
    pub id: u32,
    pub callback: u32,
    pub icon: u32,
    pub tip: String,
    pub state: u32,
    pub state_mask: u32,
    pub version: u32,
    pub guid: Option<u128>,
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn text_at(bytes: &[u8], at: usize, chars: usize) -> String {
    let Some(raw) = bytes.get(at..at + chars * 2) else {
        return String::new();
    };
    let wide: Vec<u16> = raw
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&c| c != 0)
        .collect();
    String::from_utf16_lossy(&wide)
}

/// Lee los datos de `Shell_NotifyIcon`: el mensaje (NIM_*) y el icono. Es la
/// estructura `NOTIFYICONDATAW` de 32 bits detrás de una firma y el mensaje; los
/// programas antiguos mandan una versión más corta (`cbSize`), sin los campos del
/// final.
pub fn parse(bytes: &[u8]) -> Option<(u32, Notify)> {
    if u32_at(bytes, 0)? != SIGNATURE {
        return None;
    }
    let message = u32_at(bytes, 4)?;
    let data = bytes.get(8..)?;
    let size = (u32_at(data, 0)? as usize).min(data.len());
    let data = &data[..size];
    let flags = u32_at(data, 12).unwrap_or(0);
    let guid = (flags & NIF_GUID != 0)
        .then(|| data.get(936..952))
        .flatten()
        .map(|g| u128::from_le_bytes(g.try_into().unwrap_or([0; 16])))
        .filter(|&g| g != 0);
    Some((
        message,
        Notify {
            flags,
            owner: u32_at(data, 4)?,
            id: u32_at(data, 8).unwrap_or(0),
            callback: u32_at(data, 16).unwrap_or(0),
            icon: u32_at(data, 20).unwrap_or(0),
            tip: text_at(data, 24, 128),
            state: u32_at(data, 280).unwrap_or(0),
            state_mask: u32_at(data, 284).unwrap_or(0),
            version: u32_at(data, 800).unwrap_or(0),
            guid,
        },
    ))
}

/// Un icono guardado.
pub struct Icon {
    pub owner: u32,
    pub id: u32,
    pub guid: Option<u128>,
    pub callback: u32,
    pub version: u32,
    pub tip: String,
    pub hidden: bool,
    pub image: Option<Pixmap>,
}

impl Icon {
    fn matches(&self, other: &Notify) -> bool {
        match (self.guid, other.guid) {
            (Some(a), Some(b)) => a == b,
            _ => self.owner == other.owner && self.id == other.id,
        }
    }
    fn window(&self) -> HWND {
        // Los identificadores de ventana de 32 bits se extienden con signo.
        HWND(self.owner as i32 as isize as *mut _)
    }
}

/// Aplica un mensaje de `Shell_NotifyIcon` a la lista. `image` convierte el icono
/// (su identificador de Windows) en imagen; solo se llama si cambió. Devuelve si
/// la lista cambió.
pub fn apply(
    icons: &mut Vec<Icon>,
    message: u32,
    notify: &Notify,
    image: impl Fn(u32) -> Option<Pixmap>,
) -> bool {
    let index = icons.iter().position(|i| i.matches(notify));
    match message {
        NIM_DELETE => match index {
            Some(index) => {
                icons.remove(index);
                true
            }
            None => false,
        },
        NIM_SETVERSION => {
            if let Some(index) = index {
                icons[index].version = notify.version;
            }
            false
        }
        NIM_ADD | NIM_MODIFY => {
            let index = match index {
                Some(index) => index,
                // Un cambio de un icono desconocido (registrado antes de abrir
                // GlassBar) cuenta como nuevo.
                None => {
                    icons.push(Icon {
                        owner: notify.owner,
                        id: notify.id,
                        guid: notify.guid,
                        callback: 0,
                        version: 0,
                        tip: String::new(),
                        hidden: false,
                        image: None,
                    });
                    icons.len() - 1
                }
            };
            let icon = &mut icons[index];
            if notify.flags & NIF_MESSAGE != 0 {
                icon.callback = notify.callback;
            }
            if notify.flags & NIF_ICON != 0 {
                icon.image = image(notify.icon);
            }
            if notify.flags & NIF_TIP != 0 {
                icon.tip = notify.tip.clone();
            }
            if notify.flags & NIF_STATE != 0 && notify.state_mask & NIS_HIDDEN != 0 {
                icon.hidden = notify.state & NIS_HIDDEN != 0;
            }
            true
        }
        _ => false,
    }
}

/// Botón con el que se pulsó un icono en el panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Left,
    Double,
    Right,
}

/// Mensajes que recibe el programa por cada pulsación, como los de la bandeja de
/// Windows: botón abajo, arriba y, con la versión 4, el aviso de selección o de
/// menú contextual.
pub fn click_messages(click: Click, version: u32) -> Vec<u32> {
    let mut list = match click {
        Click::Left => vec![WM_LBUTTONDOWN, WM_LBUTTONUP],
        Click::Double => vec![WM_LBUTTONDBLCLK, WM_LBUTTONUP],
        Click::Right => vec![WM_RBUTTONDOWN, WM_RBUTTONUP],
    };
    if version >= 4 {
        list.push(match click {
            Click::Right => WM_CONTEXTMENU,
            _ => NIN_SELECT,
        });
    }
    list
}

/// Lo que hay que mandar al programa: con la versión 4, la posición en `wparam` y
/// el evento y el icono en `lparam`; antes, el icono en `wparam` y el evento en
/// `lparam`.
pub fn callback_params(version: u32, id: u32, event: u32, at: POINT) -> (usize, isize) {
    if version >= 4 {
        let position = (at.x as u16 as usize) | ((at.y as u16 as usize) << 16);
        let lparam = (event & 0xffff) | ((id & 0xffff) << 16);
        (position, lparam as i32 as isize)
    } else {
        (id as usize, event as isize)
    }
}

// ---------------------------------------------------------------------------
// La ventana `Shell_TrayWnd` propia.
// ---------------------------------------------------------------------------

/// Registro de lo que pasa en la bandeja propia (`%TEMP%\glassbar-tray.log`), para
/// saber qué manda cada programa y qué clics recibe. Se empieza de cero al abrir.
fn log(line: impl AsRef<str>) {
    use std::io::Write;
    let path = std::env::temp_dir().join("glassbar-tray.log");
    // Sin crecer sin límite si GlassBar pasa días abierto.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 2_000_000) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{}", line.as_ref());
    }
}

fn process_name(window: HWND) -> String {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(window, Some(&mut pid));
    }
    crate::apps::process_info(pid)
        .map(|(exe, _)| exe.rsplit('\\').next().unwrap_or_default().to_string())
        .unwrap_or_else(|| format!("pid {pid}"))
}

thread_local! {
    /// Reenviando un mensaje a Explorer: si este volviera a llamar a la bandeja
    /// propia mientras tanto, no se reenvía otra vez (sería un bucle sin fin).
    static FORWARDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ICONS: std::cell::RefCell<Vec<Icon>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Ventana a la que se avisa cuando cambian los iconos, y con qué mensaje.
    static NOTIFY: std::cell::Cell<(isize, u32)> = const { std::cell::Cell::new((0, 0)) };
}

/// La bandeja propia mientras existe; al soltarla, los programas vuelven a hablar
/// solo con Explorer.
pub struct Host {
    hwnd: HWND,
}

impl Host {
    /// Crea la ventana y pide a los programas que vuelvan a registrar sus iconos.
    /// Avisa a `notify` con `message` cada vez que cambian. `None` si no se pudo
    /// poner delante de la de Explorer.
    pub fn install(notify: HWND, message: u32) -> Option<Self> {
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        NOTIFY.with(|n| n.set((notify.0 as isize, message)));
        let _ = std::fs::remove_file(std::env::temp_dir().join("glassbar-tray.log"));
        unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(host_wndproc),
                hInstance: instance.into(),
                lpszClassName: w!("Shell_TrayWnd"),
                ..Default::default()
            };
            if RegisterClassExW(&class) == 0 {
                return None;
            }
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                w!("Shell_TrayWnd"),
                None,
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .ok()?;
            let host = Self { hwnd };
            host.keep_in_front();
            if FindWindowW(w!("Shell_TrayWnd"), None).ok() != Some(hwnd) {
                log("No se pudo poner la bandeja propia delante de la de Explorer");
                return None;
            }
            log("Bandeja propia lista; se pide a los programas sus iconos");
            announce();
            Some(host)
        }
    }

    /// Vuelve a ponerse delante si Explorer pasó la suya por encima.
    pub fn keep_in_front(&self) {
        unsafe {
            if FindWindowW(w!("Shell_TrayWnd"), None).ok() != Some(self.hwnd) {
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                );
            }
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            if let Ok(instance) = GetModuleHandleW(None) {
                let _ = UnregisterClassW(w!("Shell_TrayWnd"), Some(instance.into()));
            }
            // Que vuelvan a registrarlos con Explorer, por si alguno no le llegó.
            announce();
        }
        ICONS.with(|icons| icons.borrow_mut().clear());
    }
}

/// Avisa a todos los programas de que la barra se creó: vuelven a registrar sus
/// iconos (como cuando Explorer se reinicia).
fn announce() {
    unsafe {
        let message = RegisterWindowMessageW(w!("TaskbarCreated"));
        if message != 0 {
            let _ = SendNotifyMessageW(HWND_BROADCAST, message, WPARAM(0), LPARAM(0));
        }
    }
}

/// Imagen de un icono que manda otro programa. Los iconos son objetos compartidos
/// de Windows: el identificador vale aquí mientras el programa espera respuesta.
fn image_of(handle: u32) -> Option<Pixmap> {
    let icon = HICON(handle as i32 as isize as *mut _);
    if icon.is_invalid() {
        return None;
    }
    // El tamaño de los iconos pequeños de Windows, que es el que dan los programas.
    let side = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
    crate::icons::from_hicon(icon, side)
}

unsafe extern "system" fn host_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;
    let forward = || {
        let mut result = 0usize;
        if FORWARDING.with(|f| f.replace(true)) {
            log(format!(
                "Mensaje 0x{msg:X} de vuelta mientras se reenviaba: no se reenvía"
            ));
            return LRESULT(0);
        }
        if let Some(explorer) = crate::shell::explorer_tray() {
            let _ = SendMessageTimeoutW(
                explorer,
                msg,
                wparam,
                lparam,
                SMTO_ABORTIFHUNG,
                2000,
                Some(&mut result),
            );
        }
        FORWARDING.with(|f| f.set(false));
        LRESULT(result as isize)
    };
    if msg == WM_COPYDATA {
        let data = &*(lparam.0 as *const COPYDATASTRUCT);
        // 1: iconos. Los demás (barras de aplicación, posición de iconos) son de Explorer.
        if data.dwData == 1 && !data.lpData.is_null() {
            let bytes = std::slice::from_raw_parts(data.lpData as *const u8, data.cbData as usize);
            if let Some((message, notify)) = parse(bytes) {
                log(format!(
                    "NIM {message} de {} (ventana 0x{:X}, id {}): flags 0x{:X}, mensaje 0x{:X}, versión {}, nombre {:?}",
                    process_name(HWND(notify.owner as i32 as isize as *mut _)),
                    notify.owner,
                    notify.id,
                    notify.flags,
                    notify.callback,
                    notify.version,
                    notify.tip,
                ));
                let changed =
                    ICONS.with(|icons| apply(&mut icons.borrow_mut(), message, &notify, image_of));
                if changed {
                    let (target, message) = NOTIFY.with(|n| n.get());
                    let _ =
                        PostMessageW(Some(HWND(target as *mut _)), message, WPARAM(0), LPARAM(0));
                }
            }
            forward();
            return LRESULT(1);
        }
        return forward();
    }
    // Mensajes que otros programas mandan a la barra: los atiende Explorer. Los
    // registrados (como "TaskbarCreated", que se anuncia a todos) no.
    if (WM_USER..0xC000).contains(&msg) || msg == WM_COMMAND {
        return forward();
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Icono que se muestra en el panel: su imagen y su nombre.
pub struct Shown {
    pub image: Option<Pixmap>,
    pub tip: String,
}

/// Iconos visibles, en el orden en que se registraron. Quita los de programas que
/// ya terminaron sin borrarlos.
pub fn shown() -> Vec<Shown> {
    ICONS.with(|icons| {
        let mut icons = icons.borrow_mut();
        icons.retain(|i| unsafe { IsWindow(Some(i.window())).as_bool() });
        icons
            .iter()
            .filter(|i| !i.hidden)
            .map(|i| Shown {
                image: i.image.clone(),
                tip: i.tip.clone(),
            })
            .collect()
    })
}

/// Pulsa el icono `index` de los visibles, como en la bandeja de Windows: el
/// programa recibe el clic y puede abrir su ventana o su menú. `at` es el punto de
/// la pantalla donde se pulsó.
pub fn click(index: usize, click: Click, at: POINT) {
    let target = ICONS.with(|icons| {
        icons
            .borrow()
            .iter()
            .filter(|i| !i.hidden)
            .nth(index)
            .map(|i| (i.window(), i.callback, i.version, i.id))
    });
    let Some((window, callback, version, id)) = target else {
        return;
    };
    let name = process_name(window);
    if callback == 0 {
        log(format!(
            "Clic {click:?} en {name}: no dio mensaje de aviso (NIF_MESSAGE), no se puede avisar"
        ));
        return;
    }
    unsafe {
        // Para que el programa pueda traer su ventana o su menú al frente.
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        let allowed = AllowSetForegroundWindow(if pid != 0 { pid } else { ASFW_ANY });
        let mut sent = Vec::new();
        for event in click_messages(click, version) {
            let (w, l) = callback_params(version, id, event, at);
            let result = SendNotifyMessageW(window, callback, WPARAM(w), LPARAM(l));
            sent.push(format!(
                "0x{event:X}:{}",
                if result.is_ok() { "ok" } else { "error" }
            ));
        }
        log(format!(
            "Clic {click:?} en {name} (ventana 0x{:X}, id {id}, versión {version}, mensaje 0x{callback:X}): primer plano {}, enviados {}",
            window.0 as usize,
            if allowed.is_ok() { "permitido" } else { "no permitido" },
            sent.join(" "),
        ));
    }
}

/// Avisa al programa de que el cursor pasa por su icono (algunos ponen al día su
/// nombre entonces).
pub fn hover(index: usize, at: POINT) {
    let target = ICONS.with(|icons| {
        icons
            .borrow()
            .iter()
            .filter(|i| !i.hidden)
            .nth(index)
            .map(|i| (i.window(), i.callback, i.version, i.id))
    });
    if let Some((window, callback, version, id)) = target.filter(|t| t.1 != 0) {
        let (w, l) = callback_params(version, id, WM_MOUSEMOVE, at);
        unsafe {
            let _ = PostMessageW(Some(window), callback, WPARAM(w), LPARAM(l));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Datos como los manda `Shell_NotifyIcon` (NOTIFYICONDATAW de 32 bits).
    fn message(nim: u32, owner: u32, id: u32, flags: u32, tip: &str, guid: u128) -> Vec<u8> {
        let mut data = vec![0u8; 956];
        let mut put =
            |at: usize, value: u32| data[at..at + 4].copy_from_slice(&value.to_le_bytes());
        put(0, 956);
        put(4, owner);
        put(8, id);
        put(12, flags);
        put(16, 0x8001);
        put(20, 0x1234);
        put(280, NIS_HIDDEN);
        put(284, NIS_HIDDEN);
        put(800, 4);
        for (i, c) in tip.encode_utf16().enumerate() {
            data[24 + i * 2..26 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
        data[936..952].copy_from_slice(&guid.to_le_bytes());
        let mut bytes = SIGNATURE.to_le_bytes().to_vec();
        bytes.extend(nim.to_le_bytes());
        bytes.extend(data);
        bytes
    }

    #[test]
    fn reads_what_shell_notify_icon_sends() {
        let bytes = message(
            NIM_ADD,
            0x10_0042,
            7,
            NIF_MESSAGE | NIF_ICON | NIF_TIP,
            "Spotify",
            0,
        );
        let (nim, notify) = parse(&bytes).unwrap();
        assert_eq!(nim, NIM_ADD);
        assert_eq!(
            (notify.owner, notify.id, notify.callback),
            (0x10_0042, 7, 0x8001)
        );
        assert_eq!(notify.icon, 0x1234);
        assert_eq!(notify.tip, "Spotify");
        assert_eq!(notify.version, 4);
        assert_eq!(notify.guid, None);
        // Con NIF_GUID se identifica por el GUID.
        let bytes = message(NIM_ADD, 1, 1, NIF_GUID, "", 0xABCD);
        assert_eq!(parse(&bytes).unwrap().1.guid, Some(0xABCD));
        // Otra firma o datos cortados: no son iconos.
        assert_eq!(parse(&[0u8; 8]), None);
        assert_eq!(parse(&bytes[..10]), None);
    }

    #[test]
    fn icons_are_added_changed_hidden_and_removed() {
        let mut icons = Vec::new();
        let image = |_| None;
        let add = |owner, id, tip: &str| {
            parse(&message(NIM_ADD, owner, id, NIF_MESSAGE | NIF_TIP, tip, 0)).unwrap()
        };
        let (nim, a) = add(1, 1, "Discord");
        assert!(apply(&mut icons, nim, &a, image));
        let (nim, b) = add(2, 1, "Steam");
        assert!(apply(&mut icons, nim, &b, image));
        assert_eq!(icons.len(), 2);
        // Cambiar el nombre no crea otro.
        let (_, renamed) = parse(&message(NIM_MODIFY, 1, 1, NIF_TIP, "Discord (3)", 0)).unwrap();
        assert!(apply(&mut icons, NIM_MODIFY, &renamed, image));
        assert_eq!(icons.len(), 2);
        assert_eq!(icons[0].tip, "Discord (3)");
        // Ocultar con NIF_STATE.
        let (_, hidden) = parse(&message(NIM_MODIFY, 2, 1, NIF_STATE, "", 0)).unwrap();
        apply(&mut icons, NIM_MODIFY, &hidden, image);
        assert!(icons[1].hidden);
        // La versión se guarda para mandar bien los clics.
        let (_, version) = parse(&message(NIM_SETVERSION, 1, 1, 0, "", 0)).unwrap();
        apply(&mut icons, NIM_SETVERSION, &version, image);
        assert_eq!(icons[0].version, 4);
        // Borrar.
        let (_, gone) = parse(&message(NIM_DELETE, 1, 1, 0, "", 0)).unwrap();
        assert!(apply(&mut icons, NIM_DELETE, &gone, image));
        assert_eq!(icons.len(), 1);
        assert!(!apply(&mut icons, NIM_DELETE, &gone, image));
    }

    #[test]
    fn clicks_reach_the_program_like_in_the_windows_tray() {
        assert_eq!(
            click_messages(Click::Left, 4),
            [WM_LBUTTONDOWN, WM_LBUTTONUP, NIN_SELECT]
        );
        assert_eq!(
            click_messages(Click::Right, 4),
            [WM_RBUTTONDOWN, WM_RBUTTONUP, WM_CONTEXTMENU]
        );
        assert_eq!(
            click_messages(Click::Right, 0),
            [WM_RBUTTONDOWN, WM_RBUTTONUP]
        );
        // Versión 4: posición en wparam; evento e icono en lparam.
        let at = POINT { x: 1200, y: 1040 };
        assert_eq!(
            callback_params(4, 7, WM_LBUTTONUP, at),
            (1200 | (1040 << 16), (WM_LBUTTONUP | (7 << 16)) as isize)
        );
        // Antes: icono en wparam y evento en lparam.
        assert_eq!(
            callback_params(0, 7, WM_LBUTTONUP, at),
            (7, WM_LBUTTONUP as isize)
        );
    }
}
