//! Redes Wi-Fi cercanas mediante la API WLAN de Windows: buscarlas, conectarse a las
//! guardadas y desconectarse. Las redes nuevas (sin perfil guardado) necesitan
//! contraseña y por ahora se configuran en el panel de Windows.
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
}
