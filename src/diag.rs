//! Registro de diagnóstico: qué interfaces internas de Windows (sin documentar)
//! respondieron y cuáles no, con la compilación de Windows. Cada actualización de
//! Windows puede cambiarlas; con este registro se sabe qué falló y dónde sin
//! adivinar.
//!
//! Se escribe en `%LOCALAPPDATA%\GlassBar\diagnostico.log`, se empieza de cero en
//! cada arranque y solo guarda nombres de interfaces y códigos de error: nada de
//! programas, archivos ni datos de la cuenta. Cada interfaz se anota una vez por
//! sesión (la primera vez que se usa) y otra si su resultado cambia.
use std::collections::HashMap;
use std::sync::Mutex;

/// Último resultado anotado de cada interfaz.
static SEEN: Mutex<Option<HashMap<&'static str, bool>>> = Mutex::new(None);

fn path() -> Option<std::path::PathBuf> {
    Some(
        std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("GlassBar")
            .join("diagnostico.log"),
    )
}

/// Empieza el registro de esta sesión con la versión de GlassBar y de Windows.
pub fn start() {
    let Some(path) = path() else {
        return;
    };
    if let Some(folder) = path.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let _ = std::fs::write(
        &path,
        format!(
            "GlassBar {} en Windows {}\n",
            env!("CARGO_PKG_VERSION"),
            windows_build().unwrap_or_else(|| "(compilación desconocida)".into())
        ),
    );
}

/// Anota el resultado de usar `feature` (p. ej. "IPolicyConfig"): `Ok` si
/// respondió, o el motivo del fallo. Solo escribe si es la primera vez o cambió.
pub fn report(feature: &'static str, result: Result<(), String>) {
    let ok = result.is_ok();
    {
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        let seen = seen.get_or_insert_with(HashMap::new);
        if seen.insert(feature, ok) == Some(ok) {
            return;
        }
    }
    let line = line(feature, &result);
    let Some(path) = path() else {
        return;
    };
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

fn line(feature: &str, result: &Result<(), String>) -> String {
    match result {
        Ok(()) => format!("{feature}: disponible"),
        Err(reason) => format!("{feature}: no disponible ({reason})"),
    }
}

/// "10.0.26100.2314": la compilación y su revisión, del registro.
fn windows_build() -> Option<String> {
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };
    const KEY: windows::core::PCWSTR = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    unsafe {
        let mut text = [0u16; 32];
        let mut size = (text.len() * 2) as u32;
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            KEY,
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(text.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .ok()
        .ok()?;
        let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
        let build = String::from_utf16_lossy(&text[..end]);
        let mut revision = 0u32;
        let mut size = 4u32;
        let has_revision = RegGetValueW(
            HKEY_LOCAL_MACHINE,
            KEY,
            w!("UBR"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut revision as *mut u32).cast()),
            Some(&mut size),
        )
        .is_ok();
        Some(if has_revision {
            format!("10.0.{build}.{revision}")
        } else {
            format!("10.0.{build}")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_say_what_answered_and_why_not() {
        assert_eq!(line("IRadioManager", &Ok(())), "IRadioManager: disponible");
        assert_eq!(
            line("IPolicyConfig", &Err("0x80004002".into())),
            "IPolicyConfig: no disponible (0x80004002)"
        );
        let build = windows_build().unwrap();
        assert!(build.starts_with("10.0."), "{build}");
    }
}
