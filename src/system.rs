//! Ajustes del sistema para el panel rápido: Wi-Fi y Bluetooth (encender/apagar),
//! brillo y nombre de la red. Leerlos y cambiarlos puede tardar (WMI, radios), así
//! que se hace en un hilo aparte que avisa al panel con un mensaje al terminar.
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
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
}

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
        std::thread::spawn(move || run(receiver, shared, target, message));
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
}

fn run(receiver: Receiver<Command>, shared: Arc<Mutex<Snapshot>>, target: isize, message: u32) {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let brightness = Brightness::connect();
    while let Ok(first) = receiver.recv() {
        // Al arrastrar el brillo llegan muchas órdenes seguidas: solo cuenta la última.
        let mut commands = vec![first];
        commands.extend(receiver.try_iter());
        let last_brightness = commands.iter().rev().find_map(|c| match c {
            Command::SetBrightness(value) => Some(*value),
            _ => None,
        });
        let mut changed_radio = false;
        for command in &commands {
            if let Command::SetRadio(radio, on) = command {
                set_radio(*radio, *on);
                changed_radio = true;
            }
        }
        if let (Some(value), Some(wmi)) = (last_brightness, &brightness) {
            wmi.set(value);
        }
        let refresh = changed_radio || commands.iter().any(|c| matches!(c, Command::Refresh));
        // Tras mover el brillo no hace falta releerlo todo: el panel ya muestra el valor.
        if !refresh {
            continue;
        }
        let (wifi, bluetooth) = radios();
        let snapshot = Snapshot {
            wifi,
            bluetooth,
            brightness: brightness.as_ref().and_then(|wmi| wmi.get()),
            network: unsafe { network_name() },
        };
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

/// Brillo de la pantalla del portátil mediante WMI (`root\WMI`). Los monitores
/// externos no lo admiten por esta vía: en ellos no hay control de brillo.
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
