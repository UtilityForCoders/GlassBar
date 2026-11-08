//! Interruptores del panel rápido que no son radios: luz nocturna, modo avión y
//! ahorro de batería. Windows no tiene una API pública para cambiar ninguno de los
//! tres, así que cada uno usa lo mismo que su configuración:
//!
//! - **Luz nocturna:** su estado está en el registro, en el formato de CloudStore
//!   (ver `night_light_state` y `encode_night_light`). Windows lo vigila y aplica el
//!   cambio al momento.
//! - **Modo avión:** la interfaz interna `IRadioManager` del servicio de radios, la
//!   misma que usa el botón de Windows.
//! - **Ahorro de batería:** se activa al bajar la batería del umbral del plan de
//!   energía. Para encenderlo, el umbral pasa al 100 %; para apagarlo vuelve al que
//!   había (o queda por debajo de la carga actual). Con el cargador conectado Windows
//!   no lo deja encender.
//!
//! Todo se llama desde el hilo de sistema (ver `system`), con COM ya iniciado.
use std::sync::atomic::{AtomicU32, Ordering};
use windows::core::{GUID, HRESULT, HSTRING, PCWSTR};
use windows::Win32::System::Registry::{
    RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_BINARY, RRF_RT_REG_BINARY,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    NightLight,
    Airplane,
    Saver,
}

impl Toggle {
    pub const ALL: [Toggle; 3] = [Toggle::NightLight, Toggle::Airplane, Toggle::Saver];
}

/// Estado actual; `None` si no está disponible en este equipo (o ahora mismo).
pub fn read(toggle: Toggle) -> Option<bool> {
    match toggle {
        Toggle::NightLight => night_light(),
        Toggle::Airplane => airplane(),
        Toggle::Saver => saver(),
    }
}

pub fn set(toggle: Toggle, on: bool) {
    match toggle {
        Toggle::NightLight => set_night_light(on),
        Toggle::Airplane => set_airplane(on),
        Toggle::Saver => set_saver(on),
    }
}

// --- Luz nocturna -------------------------------------------------------------

const NIGHT_LIGHT_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\CloudStore\\Store\\DefaultAccount\\Current\\default$windows.data.bluelightreduction.bluelightreductionstate\\windows.data.bluelightreduction.bluelightreductionstate";

/// Comienzo del valor, antes de la fecha de modificación.
const PROLOGUE_START: [u8; 10] = [0x43, 0x42, 0x01, 0x00, 0x0a, 0x02, 0x01, 0x00, 0x2a, 0x06];
/// Entre la fecha y el largo del cuerpo.
const PROLOGUE_MIDDLE: [u8; 3] = [0x2a, 0x2b, 0x0e];
/// Tras el largo, justo antes del cuerpo.
const PROLOGUE_END: [u8; 3] = [0x43, 0x42, 0x01];
/// En el cuerpo: la luz está encendida.
const ACTIVE: [u8; 2] = [0x10, 0x00];
/// En el cuerpo: el último cambio fue a mano (no por el horario).
const MANUAL: [u8; 3] = [0xd0, 0x0a, 0x02];
/// En el cuerpo: precede a la hora del cambio (FILETIME).
const FILETIME: [u8; 2] = [0xc6, 0x14];

/// Lo que dice el valor guardado: si está encendida y la fecha (en segundos) del
/// último cambio. `None` si el valor no tiene el formato esperado.
fn night_light_state(data: &[u8]) -> Option<(bool, u64)> {
    let rest = data.strip_prefix(&PROLOGUE_START)?;
    let (epoch, rest) = read_varint(rest)?;
    let rest = rest.strip_prefix(&PROLOGUE_MIDDLE)?;
    let (len, rest) = read_varint(rest)?;
    let body = rest.strip_prefix(&PROLOGUE_END)?;
    if body.len() as u64 != len || body.first() != Some(&0) {
        return None;
    }
    Some((body[1..].starts_with(&ACTIVE), epoch))
}

/// Valor que guarda Windows para la luz nocturna encendida o apagada a mano.
/// `epoch` (segundos desde 1970) y `filetime` son la hora del cambio.
fn encode_night_light(active: bool, epoch: u64, filetime: u64) -> Vec<u8> {
    let mut body = vec![0u8];
    if active {
        body.extend(ACTIVE);
    }
    body.extend(MANUAL);
    body.extend(FILETIME);
    write_varint(&mut body, filetime);
    body.extend([0u8; 4]);
    let mut data = PROLOGUE_START.to_vec();
    write_varint(&mut data, epoch);
    data.extend(PROLOGUE_MIDDLE);
    write_varint(&mut data, body.len() as u64);
    data.extend(PROLOGUE_END);
    data.extend(body);
    data
}

/// Entero en grupos de 7 bits, el menos significativo primero (LEB128).
fn read_varint(bytes: &[u8]) -> Option<(u64, &[u8])> {
    let mut value = 0u64;
    for (i, &byte) in bytes.iter().enumerate().take(10) {
        value |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((value, &bytes[i + 1..]));
        }
    }
    None
}

fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn night_light() -> Option<bool> {
    night_light_state(&read_binary(NIGHT_LIGHT_KEY, "Data")?).map(|(on, _)| on)
}

fn set_night_light(on: bool) {
    let previous = read_binary(NIGHT_LIGHT_KEY, "Data")
        .and_then(|data| night_light_state(&data))
        .map(|(_, epoch)| epoch)
        .unwrap_or(0);
    let filetime = unsafe { windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime() };
    let filetime = (u64::from(filetime.dwHighDateTime) << 32) | u64::from(filetime.dwLowDateTime);
    // De FILETIME (intervalos de 100 ns desde 1601) a segundos desde 1970. Windows
    // ignora un cambio con una fecha que no avanza.
    let epoch = (filetime / 10_000_000)
        .saturating_sub(11_644_473_600)
        .max(previous + 2);
    let data = encode_night_light(on, epoch, filetime);
    let (key, value) = (HSTRING::from(NIGHT_LIGHT_KEY), HSTRING::from("Data"));
    unsafe {
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            REG_BINARY.0,
            Some(data.as_ptr().cast()),
            data.len() as u32,
        );
    }
}

fn read_binary(key: &str, value: &str) -> Option<Vec<u8>> {
    let (key, value) = (HSTRING::from(key), HSTRING::from(value));
    let mut bytes = vec![0u8; 256];
    let mut size = bytes.len() as u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_BINARY,
            None,
            Some(bytes.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .ok()
        .ok()?;
    }
    bytes.truncate(size as usize);
    Some(bytes)
}

// --- Modo avión ---------------------------------------------------------------

/// Servicio de radios de Windows; su interfaz no está documentada. Solo se declaran
/// los métodos que se usan y los anteriores, que fijan su posición.
#[windows::core::interface("db3afbfb-08e6-46c6-aa70-bf9a34c30ab7")]
unsafe trait IRadioManager: windows::core::IUnknown {
    fn is_supported(&self, supported: *mut u32) -> HRESULT;
    fn ui_radio_instances(&self, instances: *mut *mut core::ffi::c_void) -> HRESULT;
    /// `enabled` es 1 con las radios permitidas, es decir, sin modo avión.
    fn system_radio_state(&self, enabled: *mut i32, unknown: *mut i32, reason: *mut i32)
        -> HRESULT;
    fn set_system_radio_state(&self, enabled: i32) -> HRESULT;
}

const RADIO_MANAGEMENT: GUID = GUID::from_u128(0x581333f6_28db_41be_bc7a_ff201f12f3f6);

fn radio_manager() -> Option<IRadioManager> {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    let manager: IRadioManager =
        unsafe { CoCreateInstance(&RADIO_MANAGEMENT, None, CLSCTX_ALL) }.ok()?;
    let mut supported = 0;
    unsafe { manager.is_supported(&mut supported) }.ok().ok()?;
    (supported != 0).then_some(manager)
}

fn airplane() -> Option<bool> {
    let manager = radio_manager()?;
    let (mut enabled, mut unknown, mut reason) = (0, 0, 0);
    unsafe { manager.system_radio_state(&mut enabled, &mut unknown, &mut reason) }
        .ok()
        .ok()?;
    Some(enabled == 0)
}

fn set_airplane(on: bool) {
    if let Some(manager) = radio_manager() {
        unsafe {
            let _ = manager.set_system_radio_state(if on { 0 } else { 1 });
        }
    }
}

// --- Ahorro de batería ----------------------------------------------------------

/// Ajustes del plan de energía: el grupo del ahorro y su umbral de batería.
const SUB_ENERGYSAVER: GUID = GUID::from_u128(0xde830923_a562_41af_a086_e3a2c6bad2da);
const ESBATTTHRESHOLD: GUID = GUID::from_u128(0xe69653ca_cf7f_4f05_aa73_cb833fa90ad4);
/// Umbral que había antes de encenderlo, para devolverlo al apagar; si GlassBar se
/// cerró entre medias, el de Windows por defecto.
static PREVIOUS_THRESHOLD: AtomicU32 = AtomicU32::new(DEFAULT_THRESHOLD);
const DEFAULT_THRESHOLD: u32 = 30;

fn saver() -> Option<bool> {
    use windows::System::Power::{EnergySaverStatus, PowerManager};
    // `Disabled`: sin batería o con el cargador, donde no puede encenderse.
    match PowerManager::EnergySaverStatus().ok()? {
        EnergySaverStatus::On => Some(true),
        EnergySaverStatus::Off => Some(false),
        _ => None,
    }
}

/// Umbral al apagarlo: el que había, pero por debajo de la carga actual, que si no
/// Windows lo volvería a encender.
fn threshold_when_off(previous: u32, battery: Option<u32>) -> u32 {
    match battery {
        Some(percent) if percent <= previous => percent.saturating_sub(1),
        _ => previous,
    }
}

fn set_saver(on: bool) {
    use windows::Win32::System::Power::{
        GetSystemPowerStatus, PowerGetActiveScheme, PowerReadDCValueIndex, PowerSetActiveScheme,
        PowerWriteDCValueIndex, SYSTEM_POWER_STATUS,
    };
    unsafe {
        let mut scheme = std::ptr::null_mut();
        if PowerGetActiveScheme(None, &mut scheme).is_err() || scheme.is_null() {
            return;
        }
        let mut current = 0u32;
        let read = PowerReadDCValueIndex(
            None,
            Some(scheme),
            Some(&SUB_ENERGYSAVER),
            Some(&ESBATTTHRESHOLD),
            &mut current,
        );
        let threshold = if on {
            if read == 0 && current < 100 {
                PREVIOUS_THRESHOLD.store(current, Ordering::Relaxed);
            }
            100
        } else {
            let mut status = SYSTEM_POWER_STATUS::default();
            let battery = (GetSystemPowerStatus(&mut status).is_ok()
                && status.BatteryLifePercent <= 100)
                .then_some(u32::from(status.BatteryLifePercent));
            threshold_when_off(PREVIOUS_THRESHOLD.load(Ordering::Relaxed), battery)
        };
        if PowerWriteDCValueIndex(
            None,
            &*scheme,
            Some(&SUB_ENERGYSAVER),
            Some(&ESBATTTHRESHOLD),
            threshold,
        ) == 0
        {
            // El cambio no se aplica hasta volver a activar el plan.
            let _ = PowerSetActiveScheme(None, Some(scheme));
        }
        let _ = windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            scheme.cast(),
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Valor real de Windows 11 con la luz nocturna encendida por el horario.
    const SCHEDULED_ON: [u8; 40] = [
        0x43, 0x42, 0x01, 0x00, 0x0a, 0x02, 0x01, 0x00, 0x2a, 0x06, 0xb8, 0xe1, 0xbe, 0xd7, 0x06,
        0x2a, 0x2b, 0x0e, 0x12, 0x43, 0x42, 0x01, 0x00, 0x10, 0x00, 0xc6, 0x14, 0xe7, 0xff, 0xec,
        0xdf, 0xda, 0xe1, 0xdb, 0xee, 0x01, 0x00, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn reads_the_night_light_state_windows_saves() {
        let (on, epoch) = night_light_state(&SCHEDULED_ON).unwrap();
        assert!(on);
        // 8 de noviembre de 2026.
        assert_eq!(epoch, 1_794_093_240);
        // Cortado o con otro formato no se adivina.
        assert_eq!(night_light_state(&SCHEDULED_ON[..30]), None);
        assert_eq!(night_light_state(&[0x43, 0x42]), None);
    }

    #[test]
    fn writes_values_that_read_back_the_same() {
        let filetime = 134_070_000_000_000_000;
        for on in [true, false] {
            let data = encode_night_light(on, 1_794_100_000, filetime);
            assert_eq!(night_light_state(&data), Some((on, 1_794_100_000)));
            assert!(data.windows(MANUAL.len()).any(|w| w == MANUAL));
        }
        // Encendida lleva sus dos bytes más que apagada.
        let on = encode_night_light(true, 1_794_100_000, filetime);
        let off = encode_night_light(false, 1_794_100_000, filetime);
        assert_eq!(on.len(), off.len() + 2);
    }

    #[test]
    fn varints_round_trip() {
        for value in [0u64, 1, 127, 128, 1_794_093_240, u64::MAX >> 1] {
            let mut bytes = Vec::new();
            write_varint(&mut bytes, value);
            assert_eq!(read_varint(&bytes), Some((value, &[][..])));
        }
        assert_eq!(read_varint(&[0x80, 0x80]), None);
    }

    #[test]
    fn turning_the_saver_off_leaves_the_threshold_below_the_charge() {
        assert_eq!(threshold_when_off(30, Some(80)), 30);
        assert_eq!(threshold_when_off(30, Some(25)), 24);
        assert_eq!(threshold_when_off(30, Some(0)), 0);
        assert_eq!(threshold_when_off(30, None), 30);
    }
}
