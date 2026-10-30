//! Redes Wi-Fi cercanas mediante la API WLAN de Windows: buscarlas, conectarse a las
//! guardadas o a redes nuevas (creando su perfil con la contraseña) y desconectarse.
//! Las redes de empresa (con usuario) y las WEP se siguen configurando en Windows.
use windows::core::{GUID, PCWSTR};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WiFi::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Network {
    pub ssid: String,
    /// Perfil guardado con el que conectarse; `None` si nunca se ha usado.
    pub profile: Option<String>,
    /// Calidad de la señal, de 0 a 100.
    pub signal: u8,
    pub secured: bool,
    pub connected: bool,
    /// Cómo crear su perfil para conectarse por primera vez; `None` si el panel no
    /// sabe hacerlo (redes de empresa, WEP).
    pub security: Option<Security>,
}

/// Seguridad de una red, con lo que hace falta para escribir su perfil.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    Open,
    WpaPersonal { tkip: bool },
    Wpa2Personal { tkip: bool },
    Wpa3Personal,
}
impl Security {
    fn from_algorithms(auth: i32, cipher: i32) -> Option<Self> {
        // Valores de DOT11_AUTH_ALGORITHM y DOT11_CIPHER_ALGORITHM.
        let tkip = cipher == 2;
        match auth {
            1 if cipher == 0 => Some(Self::Open),
            4 => Some(Self::WpaPersonal { tkip }),
            7 => Some(Self::Wpa2Personal { tkip }),
            9 => Some(Self::Wpa3Personal),
            _ => None,
        }
    }
    pub fn needs_password(self) -> bool {
        self != Self::Open
    }
}

/// La contraseña de una red WPA: de 8 a 63 caracteres, o la clave de 64 cifras
/// hexadecimales. Devuelve el motivo si no vale.
pub fn check_password(password: &str) -> Result<(), &'static str> {
    let len = password.chars().count();
    let hex_key = len == 64 && password.chars().all(|c| c.is_ascii_hexdigit());
    if !hex_key && !(8..=63).contains(&len) {
        return Err("La contraseña debe tener entre 8 y 63 caracteres");
    }
    Ok(())
}

/// Escapa el texto para el XML del perfil (nombre de red y contraseña).
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Perfil WLAN de una red nueva, en el formato que guarda Windows. El nombre de la
/// red va también en hexadecimal para que no dependa de su codificación.
pub fn profile_xml(ssid: &str, security: Security, password: &str) -> String {
    let hex: String = ssid.bytes().map(|b| format!("{b:02X}")).collect();
    let (authentication, encryption) = match security {
        Security::Open => ("open", "none"),
        Security::WpaPersonal { tkip } => ("WPAPSK", if tkip { "TKIP" } else { "AES" }),
        Security::Wpa2Personal { tkip } => ("WPA2PSK", if tkip { "TKIP" } else { "AES" }),
        Security::Wpa3Personal => ("WPA3SAE", "AES"),
    };
    let key = if security.needs_password() {
        let kind = if password.chars().count() == 64 {
            "networkKey"
        } else {
            "passPhrase"
        };
        format!(
            "<sharedKey><keyType>{kind}</keyType><protected>false</protected>\
             <keyMaterial>{}</keyMaterial></sharedKey>",
            escape(password)
        )
    } else {
        String::new()
    };
    format!(
        "<?xml version=\"1.0\"?>\
         <WLANProfile xmlns=\"http://www.microsoft.com/networking/WLAN/profile/v1\">\
         <name>{name}</name>\
         <SSIDConfig><SSID><hex>{hex}</hex><name>{name}</name></SSID></SSIDConfig>\
         <connectionType>ESS</connectionType><connectionMode>auto</connectionMode>\
         <MSM><security><authEncryption><authentication>{authentication}</authentication>\
         <encryption>{encryption}</encryption><useOneX>false</useOneX></authEncryption>\
         {key}</security></MSM></WLANProfile>",
        name = escape(ssid),
    )
}

/// Une las entradas de una misma red (Windows la repite: una con su perfil guardado
/// y otra sin él) y las ordena como Windows: la conectada, luego por señal.
pub fn merge(entries: Vec<Network>) -> Vec<Network> {
    let mut networks: Vec<Network> = Vec::new();
    for entry in entries {
        // Las redes ocultas no tienen nombre que mostrar.
        if entry.ssid.is_empty() {
            continue;
        }
        match networks.iter_mut().find(|n| n.ssid == entry.ssid) {
            Some(known) => {
                known.signal = known.signal.max(entry.signal);
                known.connected |= entry.connected;
                known.secured |= entry.secured;
                if known.profile.is_none() || entry.connected {
                    known.profile = entry.profile.or(known.profile.take());
                }
            }
            None => networks.push(entry),
        }
    }
    networks.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(b.signal.cmp(&a.signal))
            .then_with(|| a.ssid.to_lowercase().cmp(&b.ssid.to_lowercase()))
    });
    networks
}

/// Sesión con el servicio WLAN, abierta en el hilo de sistema durante toda su vida.
pub struct Wlan {
    handle: HANDLE,
}
impl Wlan {
    pub fn open() -> Option<Self> {
        let (mut version, mut handle) = (0, HANDLE::default());
        // 2: versión de la API de Windows Vista en adelante.
        let result = unsafe { WlanOpenHandle(2, None, &mut version, &mut handle) };
        (result == 0).then_some(Self { handle })
    }

    /// Primer adaptador Wi-Fi del equipo.
    fn interface(&self) -> Option<GUID> {
        unsafe {
            let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
            if WlanEnumInterfaces(self.handle, None, &mut list) != 0 || list.is_null() {
                return None;
            }
            let guid =
                ((*list).dwNumberOfItems > 0).then(|| (*list).InterfaceInfo[0].InterfaceGuid);
            WlanFreeMemory(list as *const _);
            guid
        }
    }

    /// Pide al adaptador que busque redes; los resultados llegan en unos segundos.
    pub fn scan(&self) {
        if let Some(guid) = self.interface() {
            unsafe {
                let _ = WlanScan(self.handle, &guid, None, None, None);
            }
        }
    }

    /// Redes que el adaptador ve ahora; `None` si no hay adaptador Wi-Fi.
    pub fn networks(&self) -> Option<Vec<Network>> {
        let guid = self.interface()?;
        unsafe {
            let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
            if WlanGetAvailableNetworkList(self.handle, &guid, 0, None, &mut list) != 0
                || list.is_null()
            {
                return Some(Vec::new());
            }
            let count = (*list).dwNumberOfItems as usize;
            // La lista es de tamaño variable: los elementos siguen al primero.
            let items = std::slice::from_raw_parts((*list).Network.as_ptr(), count);
            let entries = items
                .iter()
                .map(|n| {
                    let len = (n.dot11Ssid.uSSIDLength as usize).min(32);
                    let profile_len = n.strProfileName.iter().position(|&c| c == 0).unwrap_or(256);
                    let profile = String::from_utf16_lossy(&n.strProfileName[..profile_len]);
                    Network {
                        ssid: String::from_utf8_lossy(&n.dot11Ssid.ucSSID[..len]).into_owned(),
                        profile: (n.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0
                            && !profile.is_empty())
                        .then_some(profile),
                        signal: n.wlanSignalQuality.min(100) as u8,
                        secured: n.bSecurityEnabled.as_bool(),
                        connected: n.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED != 0,
                        security: Security::from_algorithms(
                            n.dot11DefaultAuthAlgorithm.0,
                            n.dot11DefaultCipherAlgorithm.0,
                        ),
                    }
                })
                .collect();
            WlanFreeMemory(list as *const _);
            Some(merge(entries))
        }
    }

    /// Se conecta con un perfil guardado.
    pub fn connect(&self, profile: &str) {
        let Some(guid) = self.interface() else {
            return;
        };
        let name: Vec<u16> = profile.encode_utf16().chain(std::iter::once(0)).collect();
        let parameters = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: wlan_connection_mode_profile,
            strProfile: PCWSTR(name.as_ptr()),
            pDot11Ssid: std::ptr::null_mut(),
            pDesiredBssidList: std::ptr::null_mut(),
            dot11BssType: dot11_BSS_type_any,
            dwFlags: 0,
        };
        unsafe {
            let _ = WlanConnect(self.handle, &guid, &parameters, None);
        }
    }

    /// Guarda el perfil de una red nueva con su contraseña y se conecta con él.
    /// Devuelve el nombre del perfil, o `None` si Windows no lo aceptó.
    pub fn join(&self, ssid: &str, security: Security, password: &str) -> Option<String> {
        let guid = self.interface()?;
        let xml: Vec<u16> = profile_xml(ssid, security, password)
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut reason = 0;
        let result = unsafe {
            WlanSetProfile(
                self.handle,
                &guid,
                0,
                PCWSTR(xml.as_ptr()),
                PCWSTR::null(),
                true,
                None,
                &mut reason,
            )
        };
        if result != 0 {
            return None;
        }
        self.connect(ssid);
        Some(ssid.to_string())
    }

    /// Borra un perfil guardado (el de una red nueva cuya contraseña no funcionó).
    pub fn forget(&self, profile: &str) {
        let Some(guid) = self.interface() else {
            return;
        };
        let name: Vec<u16> = profile.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let _ = WlanDeleteProfile(self.handle, &guid, PCWSTR(name.as_ptr()), None);
        }
    }

    pub fn disconnect(&self) {
        if let Some(guid) = self.interface() {
            unsafe {
                let _ = WlanDisconnect(self.handle, &guid, None);
            }
        }
    }
}
impl Drop for Wlan {
    fn drop(&mut self) {
        unsafe {
            let _ = WlanCloseHandle(self.handle, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network(ssid: &str, profile: Option<&str>, signal: u8, connected: bool) -> Network {
        Network {
            ssid: ssid.into(),
            profile: profile.map(Into::into),
            signal,
            secured: true,
            connected,
            security: Some(Security::Wpa2Personal { tkip: false }),
        }
    }

    #[test]
    fn repeated_networks_merge_and_sort_like_windows() {
        let merged = merge(vec![
            network("Vecino", None, 90, false),
            network("Casa", None, 40, false),
            network("Casa", Some("Casa"), 60, true),
            network("", None, 99, false),
            network("cafetería", Some("cafetería"), 90, false),
        ]);
        let names: Vec<_> = merged.iter().map(|n| n.ssid.as_str()).collect();
        // La conectada primero; después por señal y, a igual señal, por nombre.
        assert_eq!(names, ["Casa", "cafetería", "Vecino"]);
        assert_eq!(merged[0].profile.as_deref(), Some("Casa"));
        assert_eq!(merged[0].signal, 60);
        assert!(merged[0].connected);
        assert_eq!(merged[2].profile, None);
    }

    #[test]
    fn security_and_password_rules_follow_windows() {
        assert_eq!(Security::from_algorithms(1, 0), Some(Security::Open));
        assert_eq!(
            Security::from_algorithms(7, 4),
            Some(Security::Wpa2Personal { tkip: false })
        );
        assert_eq!(
            Security::from_algorithms(9, 4),
            Some(Security::Wpa3Personal)
        );
        // Empresa (802.1X) y WEP se configuran en Windows.
        assert_eq!(Security::from_algorithms(6, 4), None);
        assert_eq!(Security::from_algorithms(1, 257), None);

        assert!(check_password("corta").is_err());
        assert!(check_password("ocho1234").is_ok());
        assert!(check_password(&"a".repeat(63)).is_ok());
        assert!(check_password(&"a".repeat(65)).is_err());
        assert!(check_password(&"ab".repeat(32)).is_ok());
        assert!(check_password(&"zz".repeat(32)).is_err());
    }

    #[test]
    fn profile_xml_escapes_and_matches_the_security() {
        let xml = profile_xml(
            "Café & <Té>",
            Security::Wpa2Personal { tkip: false },
            "p\"ass<word>",
        );
        assert!(xml.contains("<name>Café &amp; &lt;Té&gt;</name>"));
        // "Café" en UTF-8 lleva la "é" como C3 A9.
        assert!(xml.contains("<hex>436166C3A92026203C54C3A93E</hex>"));
        assert!(xml.contains("<authentication>WPA2PSK</authentication>"));
        assert!(xml.contains("<encryption>AES</encryption>"));
        assert!(xml.contains("<keyType>passPhrase</keyType>"));
        assert!(xml.contains("<keyMaterial>p&quot;ass&lt;word&gt;</keyMaterial>"));

        let open = profile_xml("Biblioteca", Security::Open, "");
        assert!(open.contains("<authentication>open</authentication>"));
        assert!(!open.contains("sharedKey"));
        let wpa3 = profile_xml("Casa", Security::Wpa3Personal, &"ab".repeat(32));
        assert!(wpa3.contains("<authentication>WPA3SAE</authentication>"));
        assert!(wpa3.contains("<keyType>networkKey</keyType>"));
    }
}
