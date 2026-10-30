//! Ajustes del sistema para el panel rápido: Wi-Fi y Bluetooth (encender/apagar),
//! redes Wi-Fi, dispositivos Bluetooth, salidas de audio, brillo de la pantalla del dock y nombre de la red. Leerlos y
//! cambiarlos puede tardar (WMI, radios, búsquedas), así que se hace en un hilo
//! aparte que avisa al panel con un mensaje al terminar.
use crate::bluetooth::Device;
use crate::wifi::{Network, Security, Wlan};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{BSTR, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

/// Lo último leído del sistema. `None`: no disponible en este equipo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub wifi: Option<bool>,
    pub bluetooth: Option<bool>,
    /// De 0 a 100.
    pub brightness: Option<u8>,
    /// Nombre de la red conectada, como lo muestra Windows.
    pub network: Option<String>,
    /// Redes Wi-Fi cercanas; `None` si el equipo no tiene adaptador Wi-Fi.
    pub networks: Option<Vec<Network>>,
    /// Dispositivos Bluetooth emparejados.
    pub devices: Vec<Device>,
    /// Red nueva a la que no se pudo conectar (contraseña incorrecta o sin señal);
    /// su perfil ya se borró.
    pub join_failed: Option<String>,
    /// Salidas de audio y aplicaciones que lo usan.
    pub audio: crate::audio::Mixer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Radio {
    Wifi,
    Bluetooth,
}

enum Command {
    Refresh,
    SetRadio(Radio, bool),
    SetBrightness(u8),
    /// Buscar redes Wi-Fi (al abrir la lista).
    Scan,
    /// Conectarse con un perfil guardado.
    Connect(String),
    Disconnect,
    /// Conectarse a una red nueva creando su perfil.
    Join {
        ssid: String,
        security: Security,
        password: String,
    },
    /// Conectar o desconectar el audio de un dispositivo Bluetooth.
    BluetoothAudio(u64, bool),
    /// Salida de audio predeterminada.
    DefaultOutput(String),
    /// Salida de una aplicación (sus procesos); `None` vuelve a la predeterminada.
    AppOutput(Vec<u32>, Option<String>),
    /// Dispositivos Bluetooth recién leídos por su hilo (ver `bluetooth_reader`).
    Devices(Vec<Device>),
}

/// Tiempo para que Windows confirme la conexión a una red nueva; si no lo hace, la
/// contraseña no era la buena (o no hay señal) y su perfil se borra.
const JOIN_FOR: Duration = Duration::from_secs(20);

/// Tras buscar redes o conectarse, Windows tarda unos segundos en tener el resultado:
/// se vuelve a leer pasado este tiempo, y una vez más si sigue cambiando.
const SETTLE: Duration = Duration::from_secs(3);

pub struct Backend {
    sender: Sender<Command>,
    snapshot: Arc<Mutex<Snapshot>>,
}
impl Backend {
    /// Arranca el hilo; cada vez que termina una lectura publica `message` en `notify`.
    pub fn start(notify: HWND, message: u32) -> Self {
        let (sender, receiver) = channel();
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let shared = snapshot.clone();
        let target = notify.0 as isize;
        let devices = bluetooth_reader(sender.clone());
        std::thread::spawn(move || run(receiver, devices, shared, target, message));
        Self { sender, snapshot }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.lock().map(|s| s.clone()).unwrap_or_default()
    }
    pub fn refresh(&self) {
        let _ = self.sender.send(Command::Refresh);
    }
    pub fn set_radio(&self, radio: Radio, on: bool) {
        let _ = self.sender.send(Command::SetRadio(radio, on));
    }
    pub fn set_brightness(&self, percent: u8) {
        let _ = self.sender.send(Command::SetBrightness(percent.min(100)));
    }
    pub fn scan(&self) {
        let _ = self.sender.send(Command::Scan);
    }
    pub fn connect(&self, profile: String) {
        let _ = self.sender.send(Command::Connect(profile));
    }
    pub fn disconnect(&self) {
        let _ = self.sender.send(Command::Disconnect);
    }
    pub fn join(&self, ssid: String, security: Security, password: String) {
        let _ = self.sender.send(Command::Join {
            ssid,
            security,
            password,
        });
    }
    pub fn set_bluetooth_audio(&self, address: u64, connected: bool) {
        let _ = self
            .sender
            .send(Command::BluetoothAudio(address, connected));
    }
    pub fn set_default_output(&self, id: String) {
        let _ = self.sender.send(Command::DefaultOutput(id));
    }
    pub fn set_app_output(&self, pids: Vec<u32>, id: Option<String>) {
        let _ = self.sender.send(Command::AppOutput(pids, id));
    }
}

/// Hilo que lee los dispositivos Bluetooth y los devuelve como `Command::Devices`.
/// Windows a veces tarda decenas de segundos en contestar; aparte, el panel no
/// espera: muestra la última lista y se pone al día cuando llega la nueva.
fn bluetooth_reader(results: Sender<Command>) -> Sender<()> {
    let (requests, queue) = channel::<()>();
    std::thread::spawn(move || {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        crate::profile::name_thread("Bluetooth");
        while queue.recv().is_ok() {
            // Las peticiones que llegaron mientras tanto quedan cubiertas por esta.
            queue.try_iter().for_each(drop);
            if results
                .send(Command::Devices(crate::bluetooth::devices()))
                .is_err()
            {
                break;
            }
        }
    });
    requests
}

fn run(
    receiver: Receiver<Command>,
    bluetooth_reader: Sender<()>,
    shared: Arc<Mutex<Snapshot>>,
    target: isize,
    message: u32,
) {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    crate::profile::name_thread("sistema (Wi-Fi, BT, audio)");
    let wmi = crate::profile::time("system.wmi_connect", Brightness::connect);
    // Qué pantalla y por qué vía se controla; se recalcula en cada lectura por si
    // cambió el monitor principal o se conectó otro.
    let mut display = Display::None;
    let wlan = Wlan::open();
    // Red nueva a la espera de que Windows confirme la conexión, y hasta cuándo.
    let mut joining: Option<(String, Instant)> = None;
    let mut join_failed: Option<String> = None;
    // Iconos de las aplicaciones con audio, cargados una vez.
    let mut icons = std::collections::HashMap::new();
    // Última lista de dispositivos Bluetooth; si hay una lectura en curso, y si hace
    // falta otra al terminar (algo cambió mientras tanto).
    let mut devices: Vec<Device> = Vec::new();
    let (mut reading_devices, mut devices_again) = (false, false);
    // Cuántas lecturas quedan pendientes tras una búsqueda o conexión, y cuándo.
    let (mut rereads, mut reread_at) = (0, Instant::now());
    loop {
        let first = if rereads > 0 {
            match receiver.recv_timeout(reread_at.saturating_duration_since(Instant::now())) {
                Ok(command) => Some(command),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match receiver.recv() {
                Ok(command) => Some(command),
                Err(_) => break,
            }
        };
        // Al arrastrar el brillo llegan muchas órdenes seguidas: solo cuenta la última.
        let mut commands: Vec<Command> = first.into_iter().collect();
        commands.extend(receiver.try_iter());
        let last_brightness = commands.iter().rev().find_map(|c| match c {
            Command::SetBrightness(value) => Some(*value),
            _ => None,
        });
        // Sin orden nueva: venció la espera de una relectura.
        let mut reread = commands.is_empty();
        if reread {
            rereads -= 1;
            reread_at = Instant::now() + SETTLE;
        }
        for command in &commands {
            match command {
                Command::SetRadio(radio, on) => {
                    set_radio(*radio, *on);
                    reread = true;
                }
                Command::Join {
                    ssid,
                    security,
                    password,
                } => {
                    join_failed = None;
                    match wlan
                        .as_ref()
                        .and_then(|w| w.join(ssid, *security, password))
                    {
                        Some(profile) => {
                            joining = Some((profile, Instant::now() + JOIN_FOR));
                            // Se relee cada 3 s hasta confirmarla o agotar el plazo.
                            let reads = JOIN_FOR.as_secs() / SETTLE.as_secs() + 1;
                            (rereads, reread_at) = (reads as i32, Instant::now() + SETTLE);
                        }
                        None => join_failed = Some(ssid.clone()),
                    }
                    reread = true;
                }
                Command::DefaultOutput(id) => {
                    crate::audio::set_default(id);
                    reread = true;
                }
                Command::AppOutput(pids, id) => {
                    crate::audio::set_app_output(pids, id.as_deref());
                    reread = true;
                }
                Command::BluetoothAudio(address, connected) => {
                    crate::bluetooth::set_audio_connected(*address, *connected);
                    (rereads, reread_at) = (2, Instant::now() + SETTLE);
                    reread = true;
                }
                Command::Scan | Command::Connect(_) | Command::Disconnect => {
                    if let Some(wlan) = &wlan {
                        match command {
                            Command::Scan => wlan.scan(),
                            Command::Connect(profile) => wlan.connect(profile),
                            _ => wlan.disconnect(),
                        }
                    }
                    (rereads, reread_at) = (2, Instant::now() + SETTLE);
                    reread = true;
                }
                _ => {}
            }
        }
        // Al abrir el panel se vuelve a mirar qué pantalla controla el brillo.
        let opened = commands.iter().any(|c| matches!(c, Command::Refresh));
        if opened {
            display = crate::profile::time("system.display_find", || Display::find(wmi.as_ref()));
        }
        let _read = crate::profile::scope("system.read_all");
        if let Some(value) = last_brightness {
            display.set(wmi.as_ref(), value);
        }
        // Tras mover el brillo no hace falta releerlo todo: el panel ya muestra el valor.
        // Llegaron los dispositivos: solo cambia esa parte de lo publicado.
        let arrived = commands.iter_mut().rev().find_map(|c| match c {
            Command::Devices(list) => Some(std::mem::take(list)),
            _ => None,
        });
        let fresh = arrived.is_some();
        if let Some(list) = arrived {
            devices = list;
            reading_devices = false;
            if std::mem::take(&mut devices_again) {
                reading_devices = bluetooth_reader.send(()).is_ok();
            }
            if !opened && !reread {
                if let Ok(mut shared) = shared.lock() {
                    shared.devices = devices.clone();
                }
                unsafe {
                    let _ =
                        PostMessageW(Some(HWND(target as *mut _)), message, WPARAM(0), LPARAM(0));
                }
                continue;
            }
        }
        if !opened && !reread {
            continue;
        }
        let (wifi, bluetooth) = crate::profile::time("system.radios", radios);
        let networks = crate::profile::time("system.wifi_networks", || {
            wlan.as_ref().and_then(Wlan::networks)
        });
        if let Some((ssid, deadline)) = &joining {
            let connected = networks
                .iter()
                .flatten()
                .any(|n| n.connected && n.profile.as_deref() == Some(ssid.as_str()));
            if connected {
                joining = None;
            } else if Instant::now() >= *deadline {
                if let Some(wlan) = &wlan {
                    wlan.forget(ssid);
                }
                join_failed = joining.take().map(|(ssid, _)| ssid);
            }
        }
        let snapshot = Snapshot {
            wifi,
            bluetooth,
            brightness: crate::profile::time("system.brightness_get", || display.get(wmi.as_ref())),
            network: crate::profile::time("system.network_name", || unsafe { network_name() }),
            networks,
            devices: if bluetooth == Some(true) {
                if fresh {
                    // Recién llegados: no hace falta pedirlos otra vez.
                } else if reading_devices {
                    devices_again = true;
                } else {
                    reading_devices = bluetooth_reader.send(()).is_ok();
                }
                devices.clone()
            } else {
                Vec::new()
            },
            join_failed: join_failed.clone(),
            audio: crate::audio::read(&mut icons),
        };
        drop(_read);
        if let Ok(mut shared) = shared.lock() {
            *shared = snapshot;
        }
        unsafe {
            let _ = PostMessageW(Some(HWND(target as *mut _)), message, WPARAM(0), LPARAM(0));
        }
    }
}

/// Estado de las radios Wi-Fi y Bluetooth (encendidas o no); `None` si no hay.
fn radios() -> (Option<bool>, Option<bool>) {
    use windows::Devices::Radios::{Radio as WinRadio, RadioKind, RadioState};
    let Ok(list) = WinRadio::GetRadiosAsync().and_then(|op| op.join()) else {
        return (None, None);
    };
    let (mut wifi, mut bluetooth) = (None, None);
    for radio in list {
        let (Ok(kind), Ok(state)) = (radio.Kind(), radio.State()) else {
            continue;
        };
        let on = state == RadioState::On;
        match kind {
            RadioKind::WiFi => wifi = Some(wifi.unwrap_or(false) || on),
            RadioKind::Bluetooth => bluetooth = Some(bluetooth.unwrap_or(false) || on),
            _ => {}
        }
    }
    (wifi, bluetooth)
}

fn set_radio(which: Radio, on: bool) {
    use windows::Devices::Radios::{Radio as WinRadio, RadioKind, RadioState};
    let kind = match which {
        Radio::Wifi => RadioKind::WiFi,
        Radio::Bluetooth => RadioKind::Bluetooth,
    };
    let state = if on { RadioState::On } else { RadioState::Off };
    // Las apps de escritorio tienen permiso, pero Windows pide preguntarlo una vez.
    let _ = WinRadio::RequestAccessAsync().and_then(|op| op.join());
    let Ok(list) = WinRadio::GetRadiosAsync().and_then(|op| op.join()) else {
        return;
    };
    for radio in list {
        if radio.Kind().is_ok_and(|k| k == kind) {
            let _ = radio.SetStateAsync(state).and_then(|op| op.join());
        }
    }
}

/// Nombre de la red conectada (el de la Wi-Fi, o "Red 2" en un cable), como en Windows.
unsafe fn network_name() -> Option<String> {
    use windows::Win32::Networking::NetworkListManager::{
        INetwork, INetworkListManager, NetworkListManager, NLM_ENUM_NETWORK_CONNECTED,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    let manager: INetworkListManager =
        CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL).ok()?;
    let networks = manager.GetNetworks(NLM_ENUM_NETWORK_CONNECTED).ok()?;
    let mut found: [Option<INetwork>; 1] = [None];
    let mut fetched = 0;
    networks.Next(&mut found, Some(&mut fetched)).ok()?;
    let network = found[0].take().filter(|_| fetched == 1)?;
    let name = network.GetName().ok()?.to_string();
    (!name.trim().is_empty()).then_some(name)
}

/// Pantalla cuyo brillo controla el panel: la del monitor principal, donde está el
/// dock. La integrada del portátil se controla con WMI; un monitor externo, por
/// DDC/CI (el canal que usa su propio menú), si lo admite y lo tiene activado.
enum Display {
    Internal,
    External(PhysicalMonitors),
    None,
}
impl Display {
    fn find(wmi: Option<&Brightness>) -> Self {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTOPRIMARY};
        let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        let internal = unsafe { is_internal(monitor) };
        if internal == Some(true) {
            return if wmi.is_some() {
                Self::Internal
            } else {
                Self::None
            };
        }
        if let Some(monitors) = PhysicalMonitors::open(monitor) {
            return Self::External(monitors);
        }
        // Si Windows no dice qué pantalla es, WMI solo es seguro con una sola pantalla:
        // con varias podría cambiar el brillo de otra.
        let single = unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                windows::Win32::UI::WindowsAndMessaging::SM_CMONITORS,
            ) == 1
        };
        if internal.is_none() && single && wmi.is_some() {
            Self::Internal
        } else {
            Self::None
        }
    }
    fn get(&self, wmi: Option<&Brightness>) -> Option<u8> {
        match self {
            Self::Internal => wmi?.get(),
            Self::External(monitors) => monitors.get(),
            Self::None => None,
        }
    }
    fn set(&self, wmi: Option<&Brightness>, percent: u8) {
        match self {
            Self::Internal => {
                if let Some(wmi) = wmi {
                    wmi.set(percent);
                }
            }
            Self::External(monitors) => monitors.set(percent),
            Self::None => {}
        }
    }
}

/// La pantalla de `monitor` es la integrada del portátil (según la conexión que
/// informa Windows); `None` si no se puede saber.
unsafe fn is_internal(monitor: windows::Win32::Graphics::Gdi::HMONITOR) -> Option<bool> {
    use windows::Win32::Devices::Display::{
        DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
        DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER,
        DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED,
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED,
        DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
    };
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO, MONITORINFOEXW};
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if !GetMonitorInfoW(
        monitor,
        &mut info as *mut MONITORINFOEXW as *mut MONITORINFO,
    )
    .as_bool()
    {
        return None;
    }
    let (mut path_count, mut mode_count) = (0, 0);
    if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count).0 != 0 {
        return None;
    }
    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
    let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
    let result = QueryDisplayConfig(
        QDC_ONLY_ACTIVE_PATHS,
        &mut path_count,
        paths.as_mut_ptr(),
        &mut mode_count,
        modes.as_mut_ptr(),
        None,
    );
    if result.0 != 0 {
        return None;
    }
    // En modo "duplicar" una misma salida de Windows llega a varias pantallas: basta
    // con que una sea la integrada.
    let mut found = None;
    for path in &paths[..path_count as usize] {
        let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                size: std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                adapterId: path.sourceInfo.adapterId,
                id: path.sourceInfo.id,
            },
            ..Default::default()
        };
        if DisplayConfigGetDeviceInfo(&mut source.header) != 0
            || source.viewGdiDeviceName != info.szDevice
        {
            continue;
        }
        let technology = path.targetInfo.outputTechnology;
        let internal = technology == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL
            || technology == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED
            || technology == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED;
        found = Some(found.unwrap_or(false) || internal);
    }
    found
}

/// Monitores físicos de una pantalla de Windows, con su brillo por DDC/CI.
struct PhysicalMonitors {
    monitors: Vec<windows::Win32::Devices::Display::PHYSICAL_MONITOR>,
    /// Rango de brillo de cada monitor que respondió; DDC/CI es lento, así que se
    /// pregunta una vez al abrir y no en cada movimiento de la barra.
    ranges: Vec<Option<(u32, u32)>>,
    /// Brillo al abrir, de 0 a 100.
    current: Option<u8>,
}
impl PhysicalMonitors {
    /// Solo si al menos uno responde a DDC/CI con su brillo.
    fn open(monitor: windows::Win32::Graphics::Gdi::HMONITOR) -> Option<Self> {
        use windows::Win32::Devices::Display::{
            GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
            GetPhysicalMonitorsFromHMONITOR, PHYSICAL_MONITOR,
        };
        unsafe {
            let mut count = 0;
            GetNumberOfPhysicalMonitorsFromHMONITOR(monitor, &mut count).ok()?;
            let mut monitors = vec![PHYSICAL_MONITOR::default(); count as usize];
            GetPhysicalMonitorsFromHMONITOR(monitor, &mut monitors).ok()?;
            let mut current = None;
            let ranges = monitors
                .iter()
                .map(|m| {
                    let (mut min, mut now, mut max) = (0, 0, 0);
                    if GetMonitorBrightness(m.hPhysicalMonitor, &mut min, &mut now, &mut max) == 0
                        || max <= min
                    {
                        return None;
                    }
                    current.get_or_insert(((now.clamp(min, max) - min) * 100 / (max - min)) as u8);
                    Some((min, max))
                })
                .collect();
            // Se crea antes de comprobar para que, si nadie responde, Drop los libere.
            let opened = Self {
                monitors,
                ranges,
                current,
            };
            opened.current?;
            Some(opened)
        }
    }
    fn get(&self) -> Option<u8> {
        self.current
    }
    fn set(&self, percent: u8) {
        use windows::Win32::Devices::Display::SetMonitorBrightness;
        for (m, range) in self.monitors.iter().zip(&self.ranges) {
            if let Some((min, max)) = range {
                let value = min + (max - min) * percent.min(100) as u32 / 100;
                unsafe {
                    let _ = SetMonitorBrightness(m.hPhysicalMonitor, value);
                }
            }
        }
    }
}
impl Drop for PhysicalMonitors {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Devices::Display::DestroyPhysicalMonitors(&self.monitors);
        }
    }
}

/// Brillo de la pantalla integrada del portátil mediante WMI (`root\WMI`); los
/// monitores externos no lo admiten por esta vía.
struct Brightness {
    services: windows::Win32::System::Wmi::IWbemServices,
}
impl Brightness {
    fn connect() -> Option<Self> {
        use windows::Win32::System::Com::{
            CoCreateInstance, CoSetProxyBlanket, CLSCTX_INPROC_SERVER, EOAC_NONE,
            RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
        };
        use windows::Win32::System::Wmi::{IWbemLocator, WbemLocator};
        unsafe {
            let locator: IWbemLocator =
                CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).ok()?;
            let services = locator
                .ConnectServer(
                    &BSTR::from("ROOT\\WMI"),
                    &BSTR::new(),
                    &BSTR::new(),
                    &BSTR::new(),
                    0,
                    &BSTR::new(),
                    None,
                )
                .ok()?;
            // 10 = RPC_C_AUTHN_WINNT; 0 = RPC_C_AUTHZ_NONE.
            CoSetProxyBlanket(
                &services,
                10,
                0,
                PCWSTR::null(),
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
            )
            .ok()?;
            let wmi = Self { services };
            // Sin pantalla compatible (sobremesa) no se ofrece el control.
            wmi.get()?;
            Some(wmi)
        }
    }

    /// Primer objeto activo de `class`.
    unsafe fn first(&self, class: &str) -> Option<windows::Win32::System::Wmi::IWbemClassObject> {
        use windows::Win32::System::Wmi::{
            IWbemClassObject, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE,
        };
        let query = format!("SELECT * FROM {class} WHERE Active=TRUE");
        let objects = self
            .services
            .ExecQuery(
                &BSTR::from("WQL"),
                &BSTR::from(query),
                WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                None,
            )
            .ok()?;
        let mut found: [Option<IWbemClassObject>; 1] = [None];
        let mut returned = 0;
        objects
            .Next(WBEM_INFINITE, &mut found, &mut returned)
            .ok()
            .ok()?;
        found[0].take().filter(|_| returned == 1)
    }

    fn get(&self) -> Option<u8> {
        use windows::Win32::System::Variant::VARIANT;
        unsafe {
            let monitor = self.first("WmiMonitorBrightness")?;
            let mut value = VARIANT::default();
            monitor
                .Get(
                    windows::core::w!("CurrentBrightness"),
                    0,
                    &mut value,
                    None,
                    None,
                )
                .ok()?;
            u32::try_from(&value).ok().map(|v| v.min(100) as u8)
        }
    }

    fn set(&self, percent: u8) {
        use windows::Win32::System::Variant::VARIANT;
        use windows::Win32::System::Wmi::{IWbemClassObject, WBEM_FLAG_RETURN_WBEM_COMPLETE};
        unsafe {
            let Some(methods) = self.first("WmiMonitorBrightnessMethods") else {
                return;
            };
            let mut path = VARIANT::default();
            if methods
                .Get(windows::core::w!("__PATH"), 0, &mut path, None, None)
                .is_err()
            {
                return;
            }
            let Ok(path) = BSTR::try_from(&path) else {
                return;
            };
            let mut class = None;
            if self
                .services
                .GetObject(
                    &BSTR::from("WmiMonitorBrightnessMethods"),
                    WBEM_FLAG_RETURN_WBEM_COMPLETE,
                    None,
                    Some(&mut class),
                    None,
                )
                .is_err()
            {
                return;
            }
            let Some(class) = class else {
                return;
            };
            let mut signature: Option<IWbemClassObject> = None;
            if class
                .GetMethod(
                    windows::core::w!("WmiSetBrightness"),
                    0,
                    &mut signature,
                    std::ptr::null_mut(),
                )
                .is_err()
            {
                return;
            }
            let Some(params) = signature.and_then(|s| s.SpawnInstance(0).ok()) else {
                return;
            };
            // Timeout es uint32 (WMI lo pasa como VT_I4) y Brightness, uint8.
            let _ = params.Put(windows::core::w!("Timeout"), 0, &VARIANT::from(0i32), 0);
            let _ = params.Put(
                windows::core::w!("Brightness"),
                0,
                &VARIANT::from(percent),
                0,
            );
            let _ = self.services.ExecMethod(
                &path,
                &BSTR::from("WmiSetBrightness"),
                WBEM_FLAG_RETURN_WBEM_COMPLETE,
                None,
                &params,
                None,
                None,
            );
        }
    }
}

/// Con `--bench`: mide cada lectura del hilo de sistema por separado, en un hilo
/// con COM como el suyo.
#[cfg(feature = "profile")]
pub fn bench() {
    use crate::profile::{repeat, time};
    let _ = std::thread::spawn(|| {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        crate::profile::name_thread("bench sistema");
        let wmi = time("bench.system.wmi_connect", Brightness::connect);
        let mut display = Display::None;
        repeat("bench.system.display_find", 5, || {
            display = Display::find(wmi.as_ref())
        });
        repeat("bench.system.brightness_get", 10, || {
            display.get(wmi.as_ref());
        });
        repeat("bench.system.radios", 10, || {
            radios();
        });
        let wlan = time("bench.system.wlan_open", Wlan::open);
        repeat("bench.system.wifi_networks", 10, || {
            wlan.as_ref().and_then(Wlan::networks);
        });
        repeat("bench.system.network_name", 20, || unsafe {
            network_name();
        });
        repeat("bench.system.bluetooth_devices", 3, || {
            crate::bluetooth::devices();
        });
        let mut icons = std::collections::HashMap::new();
        time("bench.system.audio_read (primera, sin iconos)", || {
            crate::audio::read(&mut icons)
        });
        repeat("bench.system.audio_read", 10, || {
            crate::audio::read(&mut icons);
        });
    })
    .join();
}
