//! "Iniciar con Windows": Entrada en el registro del usuario (HKCU\Software\Microsoft\Windows\CurrentVersion\Run).

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE: PCWSTR = w!("GlassBar");

pub fn is_enabled() -> bool {
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE,
            RRF_RT_REG_SZ,
            None,
            None,
            None,
        ) == ERROR_SUCCESS
    }
}

pub fn set(enabled: bool) {
    unsafe {
        if !enabled {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE);
            return;
        }
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let command: Vec<u16> = format!("\"{}\"", exe.display())
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE,
            REG_SZ.0,
            Some(command.as_ptr().cast()),
            (command.len() * 2) as u32,
        );
    }
}
