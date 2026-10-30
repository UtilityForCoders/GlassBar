//! Estado del sistema junto al reloj, como en la barra de Windows 11: red, volumen
//! y batería (esta solo si el equipo tiene). Se lee una vez por segundo y el dock
//! solo se redibuja cuando algo cambia.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    Wifi,
    Ethernet,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Volume {
    /// De 0 a 1, como el control de volumen de Windows.
    pub level: f32,
    pub muted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    /// Enchufado a la corriente, esté cargando o no.
    pub plugged: bool,
    /// La batería está cargando ahora mismo (enchufada no basta: llena, o con el
    /// límite de carga de algunos portátiles, no carga).
    pub charging: bool,
    /// Minutos que quedan con batería, si Windows los calcula.
    pub remaining: Option<u32>,
}

impl Battery {
    /// Estado en pocas palabras: "Cargando", "Cargada", "Quedan 3 h 20 min"…
    pub fn state(&self) -> String {
        match (self.plugged, self.charging, self.remaining) {
            (_, true, _) => "Cargando".into(),
            (true, false, _) if self.percent >= 100 => "Cargada".into(),
            (true, false, _) => "Enchufada, sin cargar".into(),
            (false, _, Some(minutes)) if minutes >= 60 => {
                format!("Quedan {} h {} min", minutes / 60, minutes % 60)
            }
            (false, _, Some(minutes)) => format!("Quedan {minutes} min"),
            (false, _, None) => "Con batería".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Status {
    pub link: Link,
    pub internet: bool,
    /// `None` si no hay ninguna salida de audio.
    pub volume: Option<Volume>,
    /// `None` en equipos sin batería.
    pub battery: Option<Battery>,
}

/// Icono del grupo, en el orden en que se muestran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Network,
    Volume,
    Battery,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            link: Link::None,
            internet: false,
            volume: None,
            battery: None,
        }
    }
}

impl Status {
    pub fn items(&self) -> Vec<Item> {
        let mut items = vec![Item::Network, Item::Volume];
        if self.battery.is_some() {
            items.push(Item::Battery);
        }
        items
    }

    /// Glifo de Segoe Fluent Icons y opacidad con que se dibuja.
    pub fn glyph(&self, item: Item) -> (char, f32) {
        match item {
            Item::Network => match self.link {
                // Sin Internet, el icono de la conexión se atenúa.
                Link::Wifi => ('\u{E701}', if self.internet { 1.0 } else { 0.55 }),
                Link::Ethernet => ('\u{E839}', if self.internet { 1.0 } else { 0.55 }),
                Link::None => ('\u{F384}', 0.8),
            },
            Item::Volume => match self.volume {
                None => ('\u{E74F}', 0.55),
                Some(v) if v.muted => ('\u{E74F}', 1.0),
                Some(v) => (volume_glyph(v.level), 1.0),
            },
            Item::Battery => match self.battery {
                Some(b) => (battery_glyph(b), 1.0),
                None => (' ', 0.0),
            },
        }
    }

    /// Texto del globo al pasar el cursor.
    pub fn label(&self, item: Item) -> String {
        match item {
            Item::Network => match (self.link, self.internet) {
                (Link::Wifi, true) => "Wi-Fi: con acceso a Internet".into(),
                (Link::Wifi, false) => "Wi-Fi: sin acceso a Internet".into(),
                (Link::Ethernet, true) => "Ethernet: con acceso a Internet".into(),
                (Link::Ethernet, false) => "Ethernet: sin acceso a Internet".into(),
                (Link::None, _) => "Sin conexión a Internet".into(),
            },
            Item::Volume => match self.volume {
                None => "Sin salida de audio".into(),
                Some(v) if v.muted => "Volumen: silenciado".into(),
                Some(v) => format!("Volumen: {} %", (v.level * 100.0).round() as u32),
            },
            Item::Battery => match self.battery {
                Some(b) => format!("Batería: {} % ({})", b.percent, b.state().to_lowercase()),
                None => String::new(),
            },
        }
    }
}

/// Volumen0 a Volumen3, como Windows: sin ondas, una, dos o tres.
pub fn volume_glyph(level: f32) -> char {
    match level {
        l if l <= 0.0 => '\u{E992}',
        l if l < 0.34 => '\u{E993}',
        l if l < 0.67 => '\u{E994}',
        _ => '\u{E995}',
    }
}

/// Batería en once pasos (0 a 10). Si está enchufada, `draw_battery` le pone el rayo.
pub fn battery_glyph(battery: Battery) -> char {
    let step = ((battery.percent.min(100) as u32 + 5) / 10).min(10);
    if step == 10 {
        '\u{E83F}'
    } else {
        char::from_u32(0xE850 + step).unwrap_or('\u{E850}')
    }
}

/// Icono de batería centrado en (`cx`, `cy`), con un rayo encima si está enchufada.
/// El rayo se dibuja aparte, con un borde oscuro para que se vea también sobre la
/// parte llena, en lugar de depender de los iconos de carga de cada versión de Windows.
pub fn draw_battery(
    pixmap: &mut tiny_skia::Pixmap,
    battery: Battery,
    cx: f32,
    cy: f32,
    size: f32,
    opacity: f32,
) {
    crate::clock::draw_icon_aligned(pixmap, battery_glyph(battery), cx, cy, size, opacity);
    if !battery.plugged {
        return;
    }
    // Rayo en coordenadas de 0 a 1, de arriba abajo.
    const BOLT: [(f32, f32); 7] = [
        (0.62, 0.0),
        (0.14, 0.56),
        (0.46, 0.56),
        (0.36, 1.0),
        (0.86, 0.42),
        (0.54, 0.42),
        (0.68, 0.0),
    ];
    let height = size * 0.72;
    let (left, top) = (cx - size * 0.06 - height * 0.5, cy - height / 2.0);
    let mut pb = tiny_skia::PathBuilder::new();
    for (i, (x, y)) in BOLT.iter().enumerate() {
        let (px, py) = (left + x * height, top + y * height);
        if i == 0 {
            pb.move_to(px, py);
        } else {
            pb.line_to(px, py);
        }
    }
    pb.close();
    let Some(path) = pb.finish() else {
        return;
    };
    let mut paint = tiny_skia::Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color_rgba8(28, 28, 30, (230.0 * opacity) as u8);
    let outline = tiny_skia::Stroke {
        width: size * 0.16,
        line_join: tiny_skia::LineJoin::Round,
        ..Default::default()
    };
    let identity = tiny_skia::Transform::identity();
    pixmap.stroke_path(&path, &paint, &outline, identity, None);
    paint.set_color_rgba8(255, 255, 255, (245.0 * opacity) as u8);
    pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, identity, None);
}

/// Lee el estado una vez por segundo reutilizando los objetos de Windows: crear el
/// de audio y el de red en cada lectura costaba unos 11 ms por segundo. La red,
/// que es lo más lento de mirar, solo se vuelve a leer cuando Windows avisa de que
/// cambió alguna dirección IP, o cada 10 s por si cambió otra cosa (Internet).
pub struct Reader {
    devices: Option<windows::Win32::Media::Audio::IMMDeviceEnumerator>,
    /// Salida predeterminada (su id) y su control de volumen.
    endpoint: Option<(
        String,
        windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume,
    )>,
    network: Option<windows::Win32::Networking::NetworkListManager::INetworkListManager>,
    changes: Option<AddressChanges>,
    /// Última lectura de la red y cuándo se hizo.
    last_network: Option<(std::time::Instant, Link, bool)>,
}

const NETWORK_EVERY: std::time::Duration = std::time::Duration::from_secs(10);

impl Reader {
    /// Se crea en el hilo de la interfaz, que queda con COM iniciado para siempre.
    pub fn new() -> Self {
        use windows::Win32::Media::Audio::MMDeviceEnumerator;
        use windows::Win32::Networking::NetworkListManager::NetworkListManager;
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
        };
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            Self {
                devices: CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok(),
                endpoint: None,
                network: CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL).ok(),
                changes: AddressChanges::new(),
                last_network: None,
            }
        }
    }

    pub fn read(&mut self) -> Status {
        let _p = crate::profile::scope("status.read");
        let now = std::time::Instant::now();
        // Se mira el aviso siempre, para volver a vigilar aunque toque leer igual.
        let changed = self.changes.as_mut().is_some_and(AddressChanges::changed);
        let (link, internet) = match self.last_network {
            Some((at, link, internet)) if !changed && now.duration_since(at) < NETWORK_EVERY => {
                (link, internet)
            }
            _ => {
                let link = crate::profile::time("status.read.link", || unsafe { link() });
                let internet = crate::profile::time("status.read.internet", || self.internet());
                self.last_network = Some((now, link, internet));
                (link, internet)
            }
        };
        Status {
            link,
            internet,
            volume: crate::profile::time("status.read.volume", || self.volume()),
            battery: crate::profile::time("status.read.battery", || unsafe { battery() }),
        }
    }

    fn internet(&self) -> bool {
        use windows::Win32::Networking::NetworkListManager::{
            NLM_CONNECTIVITY_IPV4_INTERNET, NLM_CONNECTIVITY_IPV6_INTERNET,
        };
        let Some(manager) = &self.network else {
            return unsafe { internet() };
        };
        unsafe { manager.GetConnectivity() }.is_ok_and(|c| {
            c.0 & (NLM_CONNECTIVITY_IPV4_INTERNET.0 | NLM_CONNECTIVITY_IPV6_INTERNET.0) != 0
        })
    }

    /// Volumen de la salida predeterminada. Su control solo se vuelve a pedir si la
    /// salida cambió (otro dispositivo, o se desconectó).
    fn volume(&mut self) -> Option<Volume> {
        use windows::Win32::Media::Audio::{eConsole, eRender};
        use windows::Win32::System::Com::{CoTaskMemFree, CLSCTX_ALL};
        let devices = self.devices.as_ref()?;
        unsafe {
            let Ok(device) = devices.GetDefaultAudioEndpoint(eRender, eConsole) else {
                self.endpoint = None;
                return None;
            };
            let raw = device.GetId().ok()?;
            let id = raw.to_string().unwrap_or_default();
            CoTaskMemFree(Some(raw.0 as *const _));
            if self.endpoint.as_ref().is_none_or(|(known, _)| *known != id) {
                self.endpoint = device.Activate(CLSCTX_ALL, None).ok().map(|v| (id, v));
            }
            let (_, control) = self.endpoint.as_ref()?;
            Some(Volume {
                level: control.GetMasterVolumeLevelScalar().ok()?,
                muted: control.GetMute().ok()?.as_bool(),
            })
        }
    }
}

/// Aviso de Windows cuando cambia alguna dirección IP (conectar o desconectar la
/// red), sin tener que preguntar cada segundo.
struct AddressChanges {
    overlapped: Box<windows::Win32::System::IO::OVERLAPPED>,
    handle: windows::Win32::Foundation::HANDLE,
}

impl AddressChanges {
    fn new() -> Option<Self> {
        use windows::Win32::Foundation::{CloseHandle, ERROR_IO_PENDING};
        use windows::Win32::NetworkManagement::IpHelper::NotifyAddrChange;
        use windows::Win32::System::Threading::CreateEventW;
        unsafe {
            let event = CreateEventW(None, false, false, None).ok()?;
            let overlapped = Box::new(windows::Win32::System::IO::OVERLAPPED {
                hEvent: event,
                ..Default::default()
            });
            let mut handle = Default::default();
            if NotifyAddrChange(&mut handle, &*overlapped) != ERROR_IO_PENDING.0 {
                let _ = CloseHandle(event);
                return None;
            }
            Some(Self { overlapped, handle })
        }
    }

    /// Si cambió alguna dirección desde la última vez; en ese caso vuelve a vigilar.
    fn changed(&mut self) -> bool {
        use windows::Win32::Foundation::WAIT_OBJECT_0;
        use windows::Win32::NetworkManagement::IpHelper::NotifyAddrChange;
        use windows::Win32::System::Threading::WaitForSingleObject;
        unsafe {
            if WaitForSingleObject(self.overlapped.hEvent, 0) != WAIT_OBJECT_0 {
                return false;
            }
            let _ = NotifyAddrChange(&mut self.handle, &*self.overlapped);
        }
        true
    }
}

impl Drop for AddressChanges {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::NetworkManagement::IpHelper::CancelIPChangeNotify;
        unsafe {
            let _ = CancelIPChangeNotify(&*self.overlapped);
            let _ = CloseHandle(self.overlapped.hEvent);
        }
    }
}

/// Sube o baja el volumen en pasos de 2 %, como la rueda sobre el icono de Windows.
/// Al subirlo, quita el silencio.
pub fn change_volume(steps: f32) {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        if let Some(volume) = endpoint() {
            if let Ok(level) = volume.GetMasterVolumeLevelScalar() {
                let level = ((level * 50.0).round() + steps).clamp(0.0, 50.0) / 50.0;
                let _ = volume.SetMasterVolumeLevelScalar(level, std::ptr::null());
                if steps > 0.0 {
                    let _ = volume.SetMute(false, std::ptr::null());
                }
            }
        }
        if initialized {
            CoUninitialize();
        }
    }
}

/// Pone el volumen en `level` (de 0 a 1) desde el panel rápido; quita el silencio
/// si se sube por encima de cero, como el control de Windows.
pub fn set_volume(level: f32) {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        if let Some(volume) = endpoint() {
            let level = level.clamp(0.0, 1.0);
            let _ = volume.SetMasterVolumeLevelScalar(level, std::ptr::null());
            if level > 0.0 {
                let _ = volume.SetMute(false, std::ptr::null());
            }
        }
        if initialized {
            CoUninitialize();
        }
    }
}

pub fn set_mute(muted: bool) {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        if let Some(volume) = endpoint() {
            let _ = volume.SetMute(muted, std::ptr::null());
        }
        if initialized {
            CoUninitialize();
        }
    }
}

/// Abre la configuración rápida de Windows (Wi-Fi, volumen, batería…) con Win+A.
pub fn open_quick_settings() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_LWIN,
    };
    let key = |vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    let a = VIRTUAL_KEY(0x41);
    let inputs = [
        key(VK_LWIN, KEYEVENTF_EXTENDEDKEY),
        key(a, KEYBD_EVENT_FLAGS(0)),
        key(a, KEYEVENTF_KEYUP),
        key(VK_LWIN, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP),
    ];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Control de volumen de la salida de audio predeterminada.
unsafe fn endpoint() -> Option<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume> {
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    let enumerator: IMMDeviceEnumerator =
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
    let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
    device.Activate(CLSCTX_ALL, None).ok()
}

/// Windows comprueba por su cuenta si hay Internet (el mismo dato que usa su icono).
unsafe fn internet() -> bool {
    use windows::Win32::Networking::NetworkListManager::{
        INetworkListManager, NetworkListManager, NLM_CONNECTIVITY_IPV4_INTERNET,
        NLM_CONNECTIVITY_IPV6_INTERNET,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    let Ok(manager) =
        CoCreateInstance::<_, INetworkListManager>(&NetworkListManager, None, CLSCTX_ALL)
    else {
        return false;
    };
    manager.GetConnectivity().is_ok_and(|c| {
        c.0 & (NLM_CONNECTIVITY_IPV4_INTERNET.0 | NLM_CONNECTIVITY_IPV6_INTERNET.0) != 0
    })
}

/// Tipo de la conexión en uso: un adaptador activo con puerta de enlace. Si hay
/// cable y Wi-Fi a la vez, Windows muestra el cable.
unsafe fn link() -> Link {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST,
        GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::Networking::WinSock::AF_UNSPEC;
    const ETHERNET: u32 = 6;
    const WIFI: u32 = 71;
    let flags = GAA_FLAG_INCLUDE_GATEWAYS
        | GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_DNS_SERVER;
    // u64 para que el búfer quede alineado como la estructura.
    let mut buffer = vec![0u64; 2048];
    let mut size = (buffer.len() * 8) as u32;
    let mut result = 0;
    for _ in 0..3 {
        result = GetAdaptersAddresses(
            AF_UNSPEC.0 as u32,
            flags,
            None,
            Some(buffer.as_mut_ptr().cast()),
            &mut size,
        );
        if result != 111 {
            break;
        }
        // ERROR_BUFFER_OVERFLOW: `size` trae lo que hace falta.
        buffer = vec![0u64; (size as usize).div_ceil(8)];
    }
    if result != 0 {
        return Link::None;
    }
    let (mut wifi, mut ethernet) = (false, false);
    let mut adapter = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while let Some(a) = adapter.as_ref() {
        if a.OperStatus == IfOperStatusUp && !a.FirstGatewayAddress.is_null() {
            wifi |= a.IfType == WIFI;
            ethernet |= a.IfType == ETHERNET;
        }
        adapter = a.Next;
    }
    if ethernet {
        Link::Ethernet
    } else if wifi {
        Link::Wifi
    } else {
        Link::None
    }
}

unsafe fn battery() -> Option<Battery> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut power = SYSTEM_POWER_STATUS::default();
    GetSystemPowerStatus(&mut power).ok()?;
    // 128: sin batería; 255: estado desconocido.
    if power.BatteryFlag & 128 != 0 || power.BatteryFlag == 255 || power.BatteryLifePercent > 100 {
        return None;
    }
    // BatteryLifeTime: segundos restantes, o u32::MAX si Windows no los sabe
    // (enchufada, o recién desenchufada mientras lo calcula).
    let remaining = (power.ACLineStatus != 1 && power.BatteryLifeTime != u32::MAX)
        .then(|| power.BatteryLifeTime / 60);
    Some(Battery {
        percent: power.BatteryLifePercent,
        plugged: power.ACLineStatus == 1,
        // 8: cargando.
        charging: power.BatteryFlag & 8 != 0,
        remaining,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_only_shows_when_present_and_icons_follow_the_state() {
        let battery = |percent, plugged, charging, remaining| {
            Some(Battery {
                percent,
                plugged,
                charging,
                remaining,
            })
        };
        let mut status = Status::default();
        assert_eq!(status.items(), [Item::Network, Item::Volume]);
        status.battery = battery(47, false, false, None);
        assert_eq!(status.items(), [Item::Network, Item::Volume, Item::Battery]);

        // 47 % se redondea a 5 de 10; llena, el icono completo. El rayo de la carga se
        // dibuja encima, así que el icono base es el mismo.
        assert_eq!(status.glyph(Item::Battery).0, '\u{E855}');
        assert_eq!(status.label(Item::Battery), "Batería: 47 % (con batería)");
        status.battery = battery(47, true, true, None);
        assert_eq!(status.glyph(Item::Battery).0, '\u{E855}');
        assert_eq!(status.label(Item::Battery), "Batería: 47 % (cargando)");
        status.battery = battery(100, false, false, None);
        assert_eq!(status.glyph(Item::Battery).0, '\u{E83F}');

        // Los estados que se distinguen, como en Windows.
        let state = |b: Option<Battery>| b.unwrap().state();
        assert_eq!(state(battery(100, true, false, None)), "Cargada");
        assert_eq!(
            state(battery(80, true, false, None)),
            "Enchufada, sin cargar"
        );
        assert_eq!(
            state(battery(80, false, false, Some(200))),
            "Quedan 3 h 20 min"
        );
        assert_eq!(state(battery(12, false, false, Some(25))), "Quedan 25 min");

        // Volumen: ondas según el nivel, y la cruz al silenciar.
        let volume = |level, muted| Some(Volume { level, muted });
        status.volume = volume(0.0, false);
        assert_eq!(status.glyph(Item::Volume).0, '\u{E992}');
        status.volume = volume(0.2, false);
        assert_eq!(status.glyph(Item::Volume).0, '\u{E993}');
        status.volume = volume(0.5, false);
        assert_eq!(status.glyph(Item::Volume).0, '\u{E994}');
        status.volume = volume(0.9, false);
        assert_eq!(status.glyph(Item::Volume).0, '\u{E995}');
        assert_eq!(status.label(Item::Volume), "Volumen: 90 %");
        status.volume = volume(0.9, true);
        assert_eq!(status.glyph(Item::Volume).0, '\u{E74F}');
        assert_eq!(status.label(Item::Volume), "Volumen: silenciado");

        // Red: sin Internet el icono se atenúa.
        status.link = Link::Wifi;
        status.internet = true;
        assert_eq!(status.glyph(Item::Network), ('\u{E701}', 1.0));
        status.internet = false;
        assert!(status.glyph(Item::Network).1 < 1.0);
        assert_eq!(status.label(Item::Network), "Wi-Fi: sin acceso a Internet");
        status.link = Link::None;
        assert_eq!(status.label(Item::Network), "Sin conexión a Internet");
    }
}
