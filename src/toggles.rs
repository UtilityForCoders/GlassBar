//! Interruptores del panel rápido que no son radios: luz nocturna, modo avión y
//! ahorro de batería. Windows no tiene una API pública para cambiar ninguno de los
//! tres, así que cada uno usa lo mismo que su configuración:
//!
//! - **Luz nocturna:** su estado está en el registro, en el formato de CloudStore
//!   (ver `night_light_state` y `update_night_light`). Windows lo vigila y aplica el
//!   cambio al momento. Se cambia solo lo necesario y se conserva el resto.
//! - **Modo avión:** la interfaz interna `IRadioManager` del servicio de radios, la
//!   misma que usa el botón de Windows.
//! - **Ahorro de batería:** se activa al bajar la batería del umbral del plan de
//!   energía. Para encenderlo, el umbral pasa al 100 %; para apagarlo, queda por
//!   debajo de la carga actual. El umbral del usuario se guarda en disco y se le
//!   devuelve al conectar el cargador o al cerrar GlassBar (ver `restore_saver`):
//!   como con el botón de Windows, el ahorro no vuelve a encenderse solo la próxima
//!   vez que se desconecte. Con el cargador conectado Windows no lo deja encender.
//!
//! Lo llama el hilo de sistema (ver `system`), con COM ya iniciado, salvo
//! `restore_saver`, que lo llama el dock.
use std::sync::atomic::{AtomicU8, Ordering};
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

/// Cambia en el valor guardado `data` solo lo que dice si está encendida, que el
/// cambio fue a mano y su hora; el resto (campos que añada otra versión de Windows)
/// se conserva. `None` si no tiene el formato esperado.
fn update_night_light(data: &[u8], active: bool, epoch: u64, filetime: u64) -> Option<Vec<u8>> {
    let rest = data.strip_prefix(&PROLOGUE_START)?;
    let (_, rest) = read_varint(rest)?;
    let rest = rest.strip_prefix(&PROLOGUE_MIDDLE)?;
    let (len, rest) = read_varint(rest)?;
    let body = rest.strip_prefix(&PROLOGUE_END)?;
    if body.len() as u64 != len {
        return None;
    }
    let fields = body.strip_prefix(&[0])?;
    let fields = fields.strip_prefix(&ACTIVE).unwrap_or(fields);
    let fields = fields.strip_prefix(&MANUAL).unwrap_or(fields);
    let fields = fields.strip_prefix(&FILETIME)?;
    let (_, tail) = read_varint(fields)?;
    let mut new_body = vec![0u8];
    if active {
        new_body.extend(ACTIVE);
    }
    new_body.extend(MANUAL);
    new_body.extend(FILETIME);
    write_varint(&mut new_body, filetime);
    new_body.extend(tail);
    let mut out = PROLOGUE_START.to_vec();
    write_varint(&mut out, epoch);
    out.extend(PROLOGUE_MIDDLE);
    write_varint(&mut out, new_body.len() as u64);
    out.extend(PROLOGUE_END);
    out.extend(new_body);
    Some(out)
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
    let current = read_binary(NIGHT_LIGHT_KEY, "Data");
    let previous = current
        .as_deref()
        .and_then(night_light_state)
        .map(|(_, epoch)| epoch)
        .unwrap_or(0);
    let filetime = unsafe { windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime() };
    let filetime = (u64::from(filetime.dwHighDateTime) << 32) | u64::from(filetime.dwLowDateTime);
    // De FILETIME (intervalos de 100 ns desde 1601) a segundos desde 1970. Windows
    // ignora un cambio con una fecha que no avanza.
    let epoch = (filetime / 10_000_000)
        .saturating_sub(11_644_473_600)
        .max(previous + 2);
    // Si el valor tiene otro formato (o no existe), se escribe uno nuevo.
    let data = current
        .as_deref()
        .and_then(|data| update_night_light(data, on, epoch, filetime))
        .unwrap_or_else(|| encode_night_light(on, epoch, filetime));
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
/// Umbral que se usa si no se sabe el del usuario: el de Windows por defecto.
const DEFAULT_THRESHOLD: u32 = 30;
/// Si GlassBar cambió el umbral: 0 sin mirar aún, 1 no, 2 sí (está el archivo).
static CHANGED: AtomicU8 = AtomicU8::new(0);

fn saver() -> Option<bool> {
    use windows::System::Power::{EnergySaverStatus, PowerManager};
    // `Disabled`: sin batería o con el cargador, donde no puede encenderse.
    match PowerManager::EnergySaverStatus().ok()? {
        EnergySaverStatus::On => Some(true),
        EnergySaverStatus::Off => Some(false),
        _ => None,
    }
}

/// Umbral al apagarlo: el del usuario, pero por debajo de la carga actual, que si no
/// Windows lo volvería a encender.
fn threshold_when_off(original: u32, battery: Option<u32>) -> u32 {
    match battery {
        Some(percent) if percent <= original => percent.saturating_sub(1),
        _ => original,
    }
}

/// Archivo con el umbral del usuario mientras GlassBar tiene puesto otro; sobrevive
/// a un cierre forzado, para devolverlo en el siguiente arranque.
fn original_path() -> Option<std::path::PathBuf> {
    Some(
        std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("GlassBar")
            .join("saver-threshold.txt"),
    )
}

/// Umbral escrito en el archivo, si es válido.
fn parse_original(text: &str) -> Option<u32> {
    text.trim().parse().ok().filter(|value| *value <= 100)
}

fn saved_original() -> Option<u32> {
    parse_original(&std::fs::read_to_string(original_path()?).ok()?)
}

fn save_original(value: u32) {
    if let Some(path) = original_path() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::write(&path, value.to_string()).is_ok() {
            CHANGED.store(2, Ordering::Relaxed);
        }
    }
}

fn forget_original() {
    if let Some(path) = original_path() {
        let _ = std::fs::remove_file(path);
    }
    CHANGED.store(1, Ordering::Relaxed);
}

/// GlassBar tiene puesto un umbral que no es el del usuario. Barato: el archivo se
/// mira una sola vez.
pub fn saver_changed() -> bool {
    match CHANGED.load(Ordering::Relaxed) {
        0 => {
            let changed = saved_original().is_some();
            CHANGED.store(if changed { 2 } else { 1 }, Ordering::Relaxed);
            changed
        }
        state => state == 2,
    }
}

/// Devuelve al plan de energía el umbral del usuario, si GlassBar lo cambió. Se
/// llama al conectar el cargador y al cerrar GlassBar.
pub fn restore_saver() {
    if !saver_changed() {
        return;
    }
    if let Some(original) = saved_original() {
        if !write_threshold(original) {
            return;
        }
    }
    forget_original();
}

/// Umbral actual del plan de energía activo, con batería.
fn read_threshold() -> Option<u32> {
    use windows::Win32::System::Power::PowerReadDCValueIndex;
    with_scheme(|scheme| unsafe {
        let mut value = 0u32;
        (PowerReadDCValueIndex(
            None,
            Some(scheme),
            Some(&SUB_ENERGYSAVER),
            Some(&ESBATTTHRESHOLD),
            &mut value,
        ) == 0)
            .then_some(value)
    })
    .flatten()
}

/// Pone el umbral y vuelve a activar el plan, que si no no se aplica.
fn write_threshold(value: u32) -> bool {
    use windows::Win32::System::Power::{PowerSetActiveScheme, PowerWriteDCValueIndex};
    with_scheme(|scheme| unsafe {
        let written = PowerWriteDCValueIndex(
            None,
            &*scheme,
            Some(&SUB_ENERGYSAVER),
            Some(&ESBATTTHRESHOLD),
            value,
        ) == 0;
        if written {
            let _ = PowerSetActiveScheme(None, Some(scheme));
        }
        written
    })
    .unwrap_or(false)
}

/// Llama a `f` con el plan de energía activo.
fn with_scheme<T>(f: impl FnOnce(*const GUID) -> T) -> Option<T> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::System::Power::PowerGetActiveScheme;
    unsafe {
        let mut scheme = std::ptr::null_mut();
        if PowerGetActiveScheme(None, &mut scheme).is_err() || scheme.is_null() {
            return None;
        }
        let result = f(scheme);
        let _ = LocalFree(Some(HLOCAL(scheme.cast())));
        Some(result)
    }
}

fn set_saver(on: bool) {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    // El umbral del usuario: el guardado si GlassBar ya lo había cambiado.
    let original = saved_original().or_else(|| {
        let current = read_threshold()?;
        // Un 100 % que no puso GlassBar no sirve para devolverlo al apagar.
        Some(if current >= 100 {
            DEFAULT_THRESHOLD
        } else {
            current
        })
    });
    let original = original.unwrap_or(DEFAULT_THRESHOLD);
    let threshold = if on {
        100
    } else {
        let mut status = SYSTEM_POWER_STATUS::default();
        let battery = unsafe {
            (GetSystemPowerStatus(&mut status).is_ok() && status.BatteryLifePercent <= 100)
                .then_some(u32::from(status.BatteryLifePercent))
        };
        threshold_when_off(original, battery)
    };
    if !write_threshold(threshold) {
        return;
    }
    // Mientras no sea el del usuario, se guarda el suyo para devolvérselo.
    if threshold == original {
        forget_original();
    } else {
        save_original(original);
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
    fn updating_keeps_fields_it_does_not_know() {
        let filetime = 134_070_000_000_000_000;
        // Un campo desconocido antes del relleno final.
        let mut data = encode_night_light(true, 1_794_100_000, filetime);
        let tail = data.len() - 4;
        data.splice(tail..tail, [0xe0, 0x1f, 0x05]);
        let len_at = PROLOGUE_START.len() + 5 + PROLOGUE_MIDDLE.len();
        data[len_at] += 3;
        assert_eq!(night_light_state(&data), Some((true, 1_794_100_000)));
        let off = update_night_light(&data, false, 1_794_100_010, filetime + 1).unwrap();
        assert_eq!(night_light_state(&off), Some((false, 1_794_100_010)));
        assert!(off.windows(3).any(|w| w == [0xe0, 0x1f, 0x05]));
        // El valor real de Windows (encendida por el horario) pasa a apagada a mano.
        let off = update_night_light(&SCHEDULED_ON, false, 1_794_100_000, filetime).unwrap();
        assert_eq!(night_light_state(&off), Some((false, 1_794_100_000)));
        assert!(off.windows(MANUAL.len()).any(|w| w == MANUAL));
        let on = update_night_light(&off, true, 1_794_100_002, filetime).unwrap();
        assert_eq!(night_light_state(&on), Some((true, 1_794_100_002)));
        assert_eq!(on.len(), off.len() + 2);
        // Sin el formato esperado no se toca.
        assert_eq!(update_night_light(&SCHEDULED_ON[..30], true, 1, 1), None);
    }

    #[test]
    fn the_saved_threshold_is_a_percentage() {
        assert_eq!(parse_original("30"), Some(30));
        assert_eq!(parse_original(" 0\n"), Some(0));
        assert_eq!(parse_original("101"), None);
        assert_eq!(parse_original("treinta"), None);
    }

    #[test]
    fn turning_the_saver_off_leaves_the_threshold_below_the_charge() {
        assert_eq!(threshold_when_off(30, Some(80)), 30);
        assert_eq!(threshold_when_off(30, Some(25)), 24);
        assert_eq!(threshold_when_off(30, Some(0)), 0);
        assert_eq!(threshold_when_off(30, None), 30);
    }
}
