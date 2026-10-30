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
    /// Enchufado a la corriente (cargando o ya cargado).
    pub plugged: bool,
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
                Some(b) if b.plugged && b.percent >= 100 => "Batería: 100 % (cargada)".into(),
                Some(b) if b.plugged => format!("Batería: {} % (cargando)", b.percent),
                Some(b) => format!("Batería: {} %", b.percent),
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

/// Batería en once pasos (0 a 10), con rayo si está enchufada.
pub fn battery_glyph(battery: Battery) -> char {
    let step = ((battery.percent.min(100) as u32 + 5) / 10).min(10);
    if battery.plugged {
        char::from_u32(0xEBA0 + step).unwrap_or('\u{EBAA}')
    } else if step == 10 {
        '\u{E83F}'
    } else {
        char::from_u32(0xE850 + step).unwrap_or('\u{E850}')
    }
}

/// Lee el estado actual de red, audio y batería.
pub fn read() -> Status {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let status = Status {
            link: link(),
            internet: internet(),
            volume: endpoint().and_then(|v| {
                Some(Volume {
                    level: v.GetMasterVolumeLevelScalar().ok()?,
                    muted: v.GetMute().ok()?.as_bool(),
                })
            }),
            battery: battery(),
        };
        if initialized {
            CoUninitialize();
        }
        status
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
    Some(Battery {
        percent: power.BatteryLifePercent,
        plugged: power.ACLineStatus == 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_only_shows_when_present_and_icons_follow_the_state() {
        let mut status = Status::default();
        assert_eq!(status.items(), [Item::Network, Item::Volume]);
        status.battery = Some(Battery {
            percent: 47,
            plugged: false,
        });
        assert_eq!(status.items(), [Item::Network, Item::Volume, Item::Battery]);

        // Batería: 47 % se redondea a 5 de 10; enchufada lleva rayo; llena, el icono completo.
        assert_eq!(status.glyph(Item::Battery).0, '\u{E855}');
        assert_eq!(status.label(Item::Battery), "Batería: 47 %");
        status.battery = Some(Battery {
            percent: 47,
            plugged: true,
        });
        assert_eq!(status.glyph(Item::Battery).0, '\u{EBA5}');
        assert_eq!(status.label(Item::Battery), "Batería: 47 % (cargando)");
        status.battery = Some(Battery {
            percent: 100,
            plugged: false,
        });
        assert_eq!(status.glyph(Item::Battery).0, '\u{E83F}');

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
