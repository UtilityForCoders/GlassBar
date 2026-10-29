//! Uso de aplicaciones que Windows ya registra (UserAssist) para su "Más usadas":
//! veces abiertas, tiempo en primer plano y última apertura. Solo se lee.
use std::collections::HashMap;
use windows::core::{w, PWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub runs: u32,
    pub focus_ms: u32,
    /// Última apertura, en segundos Unix.
    pub last: u64,
}

/// Uso por nombre de AppsFolder en minúsculas: AppUserModelID o ruta del .exe,
/// lo mismo que va tras `shell:AppsFolder\` en el catálogo.
pub fn load() -> HashMap<String, Usage> {
    let mut result = HashMap::new();
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\UserAssist\\{CEBFF5CD-ACE2-4F4F-9178-9926F41749EA}\\Count"),
            Some(0),
            KEY_READ,
            &mut key,
        )
        .is_err()
        {
            return result;
        }
        let mut name = vec![0u16; 2048];
        let mut data = [0u8; 128];
        for index in 0.. {
            let mut name_len = name.len() as u32;
            let mut data_len = data.len() as u32;
            let status = RegEnumValueW(
                key,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                None,
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            );
            if status.is_err() {
                // ERROR_MORE_DATA en un valor raro no corta la lista; el final sí.
                if status.0 == 234 {
                    continue;
                }
                break;
            }
            let Some(usage) = parse(&data[..data_len as usize]) else {
                continue;
            };
            let decoded = rot13(&String::from_utf16_lossy(&name[..name_len as usize]));
            result.insert(decoded.to_lowercase(), usage);
        }
        let _ = RegCloseKey(key);
    }
    result
}

/// Los nombres de UserAssist están codificados con ROT13.
pub fn rot13(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'a'..='z' => (((c as u8 - b'a') + 13) % 26 + b'a') as char,
            'A'..='Z' => (((c as u8 - b'A') + 13) % 26 + b'A') as char,
            _ => c,
        })
        .collect()
}

/// Registro de 72 bytes: veces en el byte 4, tiempo en primer plano (ms) en el 12
/// y la última apertura (FILETIME) en el 60.
pub fn parse(data: &[u8]) -> Option<Usage> {
    let u32_at = |i: usize| -> Option<u32> {
        Some(u32::from_le_bytes(data.get(i..i + 4)?.try_into().ok()?))
    };
    let filetime = u64::from_le_bytes(data.get(60..68)?.try_into().ok()?);
    Some(Usage {
        runs: u32_at(4)?,
        focus_ms: u32_at(12)?,
        last: (filetime / 10_000_000).saturating_sub(11_644_473_600),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_names_and_records() {
        assert_eq!(rot13("Puebzr"), "Chrome");
        assert_eq!(rot13(&rot13("dev.warp.Warp!App_1")), "dev.warp.Warp!App_1");
        let mut data = [0u8; 72];
        data[4..8].copy_from_slice(&15u32.to_le_bytes());
        data[12..16].copy_from_slice(&1141u32.to_le_bytes());
        // 2026-10-29 00:00:00 UTC como FILETIME.
        let unix = 1_793_232_000u64;
        data[60..68].copy_from_slice(&((unix + 11_644_473_600) * 10_000_000).to_le_bytes());
        assert_eq!(
            parse(&data),
            Some(Usage {
                runs: 15,
                focus_ms: 1141,
                last: unix
            })
        );
        assert_eq!(parse(&data[..40]), None);
    }
}
