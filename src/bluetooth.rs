//! Dispositivos Bluetooth emparejados para el panel rápido: nombre, tipo y si están
//! conectados (API de dispositivos de Windows), y conectar o desconectar los de audio.
//!
//! Windows no ofrece a las apps una forma general de conectar un dispositivo
//! emparejado. Para los de audio (auriculares, altavoces) sí hay una vía: pedírselo
//! al controlador de audio Bluetooth con las propiedades "reconectar"/"desconectar"
//! de su filtro de kernel streaming, como hace la configuración de sonido. Ratones y
//! teclados se conectan solos al encenderlos.

/// Tipo de dispositivo, para su icono.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Audio,
    Input,
    Phone,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub address: u64,
    pub kind: Kind,
    pub connected: bool,
    /// Tiene audio Bluetooth que el panel puede conectar y desconectar.
    pub audio: bool,
}

/// Une los que aparecen dos veces (Bluetooth clásico y de bajo consumo con la misma
/// dirección) y ordena como Windows: conectados primero y luego por nombre.
pub fn merge(entries: Vec<Device>) -> Vec<Device> {
    let mut devices: Vec<Device> = Vec::new();
    for entry in entries {
        match devices.iter_mut().find(|d| d.address == entry.address) {
            Some(known) => {
                known.connected |= entry.connected;
                known.audio |= entry.audio;
                if known.kind == Kind::Other {
                    known.kind = entry.kind;
                }
                if known.name.is_empty() {
                    known.name = entry.name;
                }
            }
            None => devices.push(entry),
        }
    }
    devices.retain(|d| !d.name.trim().is_empty());
    devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    devices
}

/// La dirección como aparece en los identificadores de dispositivo de Windows.
fn address_text(address: u64) -> String {
    format!("{address:012X}")
}

/// Emparejados ahora mismo; vacío si no hay Bluetooth.
pub fn devices() -> Vec<Device> {
    use windows::Devices::Bluetooth::{
        BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice, BluetoothMajorClass,
    };
    use windows::Devices::Enumeration::DeviceInformation;
    let audio = audio_controls();
    let has_audio = |address: u64| {
        let text = address_text(address);
        audio.iter().any(|(id, _)| id.contains(&text))
    };
    let mut entries = Vec::new();
    let find = |selector: windows::core::Result<windows::core::HSTRING>| {
        selector
            .and_then(|s| DeviceInformation::FindAllAsyncAqsFilter(&s))
            .and_then(|op| op.join())
            .map(|found| found.into_iter().collect::<Vec<_>>())
            .unwrap_or_default()
    };
    for info in find(BluetoothDevice::GetDeviceSelectorFromPairingState(true)) {
        let Ok(device) = info
            .Id()
            .and_then(|id| BluetoothDevice::FromIdAsync(&id))
            .and_then(|op| op.join())
        else {
            continue;
        };
        let address = device.BluetoothAddress().unwrap_or_default();
        let kind = match device.ClassOfDevice().and_then(|c| c.MajorClass()) {
            Ok(BluetoothMajorClass::AudioVideo) => Kind::Audio,
            Ok(BluetoothMajorClass::Peripheral) => Kind::Input,
            Ok(BluetoothMajorClass::Phone) => Kind::Phone,
            _ => Kind::Other,
        };
        entries.push(Device {
            name: device.Name().map(|n| n.to_string()).unwrap_or_default(),
            address,
            kind,
            connected: device
                .ConnectionStatus()
                .is_ok_and(|s| s == BluetoothConnectionStatus::Connected),
            audio: has_audio(address),
        });
    }
    for info in find(BluetoothLEDevice::GetDeviceSelectorFromPairingState(true)) {
        let Ok(device) = info
            .Id()
            .and_then(|id| BluetoothLEDevice::FromIdAsync(&id))
            .and_then(|op| op.join())
        else {
            continue;
        };
        let address = device.BluetoothAddress().unwrap_or_default();
        // Categorías de "Appearance" de Bluetooth LE: 15 es dispositivo de entrada
        // (HID: teclados y ratones), 0x25 auriculares de audio.
        let kind = match device.Appearance().and_then(|a| a.Category()) {
            Ok(15) => Kind::Input,
            Ok(0x25 | 0x21 | 0x22) => Kind::Audio,
            Ok(1) => Kind::Phone,
            _ => Kind::Other,
        };
        entries.push(Device {
            name: device.Name().map(|n| n.to_string()).unwrap_or_default(),
            address,
            kind,
            connected: device
                .ConnectionStatus()
                .is_ok_and(|s| s == BluetoothConnectionStatus::Connected),
            audio: has_audio(address),
        });
    }
    merge(entries)
}

/// Conecta (`true`) o desconecta el audio del dispositivo con esa dirección.
pub fn set_audio_connected(address: u64, connected: bool) {
    use windows::Win32::Media::KernelStreaming::{
        KSIDENTIFIER, KSIDENTIFIER_0, KSIDENTIFIER_0_0, KSPROPERTY_TYPE_GET,
    };
    // KSPROPSETID_BtAudio, con KSPROPERTY_ONESHOT_RECONNECT (0) y _DISCONNECT (1).
    const BT_AUDIO: windows::core::GUID =
        windows::core::GUID::from_u128(0x7fa06c40_b8f6_4c7e_8556_e8c33a12e54d);
    let text = address_text(address);
    let property = KSIDENTIFIER {
        Anonymous: KSIDENTIFIER_0 {
            Anonymous: KSIDENTIFIER_0_0 {
                Set: BT_AUDIO,
                Id: if connected { 0 } else { 1 },
                Flags: KSPROPERTY_TYPE_GET,
            },
        },
    };
    for (_, control) in audio_controls().iter().filter(|(id, _)| id.contains(&text)) {
        let mut returned = 0;
        unsafe {
            let _ = control.KsProperty(
                &property,
                std::mem::size_of::<KSIDENTIFIER>() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
            );
        }
    }
}

/// Filtros de audio Bluetooth de todos los altavoces y micrófonos (también los
/// desconectados), con el identificador de su dispositivo, que lleva la dirección.
fn audio_controls() -> Vec<(String, windows::Win32::Media::KernelStreaming::IKsControl)> {
    use windows::core::Interface;
    use windows::Win32::Media::Audio::{
        eAll, IDeviceTopology, IMMDeviceEnumerator, IPart, MMDeviceEnumerator, DEVICE_STATE,
        DEVICE_STATE_ACTIVE, DEVICE_STATE_UNPLUGGED,
    };
    use windows::Win32::Media::KernelStreaming::IKsControl;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, CLSCTX_INPROC_SERVER,
    };
    let mut controls = Vec::new();
    unsafe {
        let Ok(enumerator) =
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        else {
            return controls;
        };
        let Ok(endpoints) = enumerator.EnumAudioEndpoints(
            eAll,
            DEVICE_STATE(DEVICE_STATE_ACTIVE.0 | DEVICE_STATE_UNPLUGGED.0),
        ) else {
            return controls;
        };
        for index in 0..endpoints.GetCount().unwrap_or(0) {
            let control = (|| -> windows::core::Result<(String, IKsControl)> {
                let endpoint = endpoints.Item(index)?;
                let topology: IDeviceTopology = endpoint.Activate(CLSCTX_ALL, None)?;
                // El conector del extremo lleva al filtro del controlador.
                let part: IPart = topology.GetConnector(0)?.GetConnectedTo()?.cast()?;
                let raw = part.GetTopologyObject()?.GetDeviceId()?;
                let id = raw.to_string().unwrap_or_default().to_uppercase();
                CoTaskMemFree(Some(raw.0 as *const _));
                let mut control = std::ptr::null_mut();
                part.Activate(CLSCTX_INPROC_SERVER.0, &IKsControl::IID, Some(&mut control))?;
                Ok((id, IKsControl::from_raw(control)))
            })();
            // Solo los filtros de Bluetooth (perfiles de audio y manos libres).
            if let Ok((id, control)) = control {
                if id.contains("BTHENUM") || id.contains("BTHHFENUM") {
                    controls.push((id, control));
                }
            }
        }
    }
    controls
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str, address: u64, kind: Kind, connected: bool, audio: bool) -> Device {
        Device {
            name: name.into(),
            address,
            kind,
            connected,
            audio,
        }
    }

    #[test]
    fn devices_merge_by_address_and_connected_ones_go_first() {
        let merged = merge(vec![
            device("Teclado", 1, Kind::Input, false, false),
            device("Auriculares", 2, Kind::Other, false, false),
            device("Auriculares", 2, Kind::Audio, true, true),
            device("", 3, Kind::Other, false, false),
            device("altavoz", 4, Kind::Audio, false, true),
        ]);
        let names: Vec<_> = merged.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Auriculares", "altavoz", "Teclado"]);
        assert_eq!(merged[0].kind, Kind::Audio);
        assert!(merged[0].connected && merged[0].audio);
    }

    #[test]
    fn address_matches_windows_device_ids() {
        assert_eq!(address_text(0x00_11_22_aa_bb_cc), "001122AABBCC");
        let id = "{2}.\\\\?\\BTHENUM#{0000110B-0000-1000-8000-00805F9B34FB}_VID&0001001D#8&2A&0&001122AABBCC_C00000000";
        assert!(id.contains(&address_text(0x0011_22aa_bbcc)));
    }
}
