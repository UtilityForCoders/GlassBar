//! Idioma y distribución del teclado de la ventana activa, para el indicador del
//! dock ("ESP" y, debajo, "INTL"), y cambiarlos desde un menú. En Windows cada
//! aplicación tiene su propio idioma de entrada, así que se mira el de la activa.
use unicode_normalization::UnicodeNormalization;
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Globalization::{
    GetLocaleInfoEx, LCIDToLocaleName, LOCALE_SISO639LANGNAME2, LOCALE_SLOCALIZEDDISPLAYNAME,
    LOCALE_SNATIVELANGUAGENAME,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayout, GetKeyboardLayoutList, HKL};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Identificador del teclado en Windows (HKL).
    pub id: isize,
    /// Idioma en tres letras ("ESP").
    pub language: String,
    /// Distribución abreviada ("INTL"), solo si no es la normal de su idioma.
    pub variant: Option<String>,
    /// Nombre completo: "Español (España) · Estados Unidos-Internacional".
    pub name: String,
}

/// Idioma en tres letras, como Windows: las primeras del nombre del idioma en su
/// propia lengua ("Español" → "ESP", "Deutsch" → "DEU"), sin tildes. Si no está
/// escrito con letras latinas, el código ISO de tres letras ("jpn" → "JPN").
pub fn language_code(native: &str, iso: &str) -> String {
    let latin = native.nfd().next().is_some_and(|c| c.is_ascii_alphabetic());
    let plain: String = native
        .nfd()
        .filter(|c| c.is_ascii_alphabetic())
        .take(3)
        .collect();
    if latin && plain.len() == 3 {
        plain.to_uppercase()
    } else {
        iso.to_uppercase()
    }
}

/// Distribución abreviada a partir de su nombre en inglés del registro:
/// "United States-International" → "INTL", "US" → "US", "Latin American" → "LA",
/// "Spanish" → "SPA".
pub fn variant_code(text: &str) -> String {
    if text.to_lowercase().contains("international") {
        return "INTL".into();
    }
    let words: Vec<&str> = text
        .split(|c: char| c == ' ' || c == '-' || c == '(' || c == ')')
        .filter(|w| !w.is_empty())
        .collect();
    match words.as_slice() {
        [] => String::new(),
        [word] if word.len() <= 4 => word.to_uppercase(),
        [word] => word.chars().take(3).collect::<String>().to_uppercase(),
        _ => words
            .iter()
            .filter_map(|w| w.chars().next())
            .take(4)
            .collect::<String>()
            .to_uppercase(),
    }
}

fn locale_info(locale: &str, kind: u32) -> Option<String> {
    let name = HSTRING::from(locale);
    let mut buffer = [0u16; 128];
    let len = unsafe { GetLocaleInfoEx(&name, kind, Some(&mut buffer)) };
    (len > 1).then(|| String::from_utf16_lossy(&buffer[..len as usize - 1]))
}

/// Clave del registro de una distribución (KLID, p. ej. "00020409") a partir de la
/// parte alta del HKL: o es el propio KLID, o (0xFxxx) un "Layout Id" que hay que
/// buscar entre las distribuciones instaladas.
fn klid(device: u16) -> Option<String> {
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE,
        KEY_READ, RRF_RT_REG_SZ,
    };
    if device & 0xf000 != 0xf000 {
        return Some(format!("{:08X}", device as u32));
    }
    let wanted = format!("{:04x}", device & 0x0fff);
    unsafe {
        let mut key = HKEY::default();
        let root = HSTRING::from("SYSTEM\\CurrentControlSet\\Control\\Keyboard Layouts");
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, &root, None, KEY_READ, &mut key).is_err() {
            return None;
        }
        let mut found = None;
        for index in 0.. {
            let mut name = [0u16; 16];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(
                key,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
            .is_err()
            {
                break;
            }
            let mut value = [0u16; 16];
            let mut size = (value.len() * 2) as u32;
            let id = RegGetValueW(
                key,
                PCWSTR(name.as_ptr()),
                &HSTRING::from("Layout Id"),
                RRF_RT_REG_SZ,
                None,
                Some(value.as_mut_ptr().cast()),
                Some(&mut size),
            );
            let end = value.iter().position(|&c| c == 0).unwrap_or(0);
            if id.is_ok() && String::from_utf16_lossy(&value[..end]).eq_ignore_ascii_case(&wanted) {
                found = Some(String::from_utf16_lossy(&name[..len as usize]));
                break;
            }
        }
        let _ = RegCloseKey(key);
        found
    }
}

/// Nombre de una distribución: el de Windows en el idioma del usuario
/// ("Estados Unidos-Internacional") y el del registro en inglés, para abreviarlo.
fn layout_names(klid: &str) -> Option<(String, String)> {
    use windows::Win32::System::Registry::{
        RegCloseKey, RegGetValueW, RegLoadMUIStringW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE,
        KEY_READ, RRF_RT_REG_SZ,
    };
    let path = HSTRING::from(format!(
        "SYSTEM\\CurrentControlSet\\Control\\Keyboard Layouts\\{klid}"
    ));
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, &path, None, KEY_READ, &mut key).is_err() {
            return None;
        }
        let mut text = [0u16; 128];
        let mut size = (text.len() * 2) as u32;
        let english = RegGetValueW(
            key,
            PCWSTR::null(),
            &HSTRING::from("Layout Text"),
            RRF_RT_REG_SZ,
            None,
            Some(text.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .is_ok()
        .then(|| {
            let end = text.iter().position(|&c| c == 0).unwrap_or(0);
            String::from_utf16_lossy(&text[..end])
        });
        let mut display = [0u16; 256];
        let mut used = 0;
        let localized = RegLoadMUIStringW(
            key,
            &HSTRING::from("Layout Display Name"),
            Some(PWSTR(display.as_mut_ptr())),
            (display.len() * 2) as u32,
            Some(&mut used),
            0,
            PCWSTR::null(),
        )
        .is_ok()
        .then(|| {
            let end = display.iter().position(|&c| c == 0).unwrap_or(0);
            String::from_utf16_lossy(&display[..end])
        })
        .filter(|s| !s.is_empty());
        let _ = RegCloseKey(key);
        let english = english?;
        Some((localized.unwrap_or_else(|| english.clone()), english))
    }
}

/// Describe un teclado instalado (HKL). Se mira cada segundo y consulta el
/// registro, así que cada teclado se describe una sola vez.
pub fn describe(hkl: isize) -> Layout {
    thread_local! {
        static KNOWN: std::cell::RefCell<std::collections::HashMap<isize, Layout>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    KNOWN.with(|known| {
        known
            .borrow_mut()
            .entry(hkl)
            .or_insert_with(|| read_layout(hkl))
            .clone()
    })
}

fn read_layout(hkl: isize) -> Layout {
    let language_id = (hkl as usize & 0xffff) as u32;
    let device = ((hkl as usize >> 16) & 0xffff) as u16;
    let mut buffer = [0u16; 85];
    let len = unsafe { LCIDToLocaleName(language_id, Some(&mut buffer), 0) };
    let locale = (len > 1)
        .then(|| String::from_utf16_lossy(&buffer[..len as usize - 1]))
        .unwrap_or_default();
    let language = language_code(
        &locale_info(&locale, LOCALE_SNATIVELANGUAGENAME).unwrap_or_default(),
        &locale_info(&locale, LOCALE_SISO639LANGNAME2).unwrap_or_default(),
    );
    let display = locale_info(&locale, LOCALE_SLOCALIZEDDISPLAYNAME).unwrap_or(locale);
    // La distribución normal del idioma tiene la parte alta igual al idioma.
    let names = (device as u32 != language_id)
        .then(|| klid(device))
        .flatten()
        .and_then(|klid| layout_names(&klid));
    match names {
        Some((localized, english)) => Layout {
            id: hkl,
            language,
            variant: Some(variant_code(&english)).filter(|v| !v.is_empty()),
            name: format!("{display} · {localized}"),
        },
        None => Layout {
            id: hkl,
            language,
            variant: None,
            name: display,
        },
    }
}

/// Teclados instalados.
pub fn installed() -> Vec<isize> {
    unsafe {
        let count = GetKeyboardLayoutList(None).max(0) as usize;
        let mut list = vec![HKL::default(); count];
        let filled = GetKeyboardLayoutList(Some(&mut list)).max(0) as usize;
        list.truncate(filled);
        list.into_iter().map(|h| h.0 as isize).collect()
    }
}

/// Teclado de la ventana activa; `None` si solo hay uno instalado (Windows tampoco
/// muestra el indicador entonces).
pub fn current() -> Option<Layout> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    if installed().len() < 2 {
        return None;
    }
    let hkl = unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        GetKeyboardLayout(thread)
    };
    Some(describe(hkl.0 as isize))
}

/// Pide a la ventana `window` que cambie al teclado `hkl`, como el menú de idioma
/// de Windows.
pub fn switch(window: windows::Win32::Foundation::HWND, hkl: isize) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_INPUTLANGCHANGEREQUEST};
    unsafe {
        let _ = PostMessageW(
            Some(window),
            WM_INPUTLANGCHANGEREQUEST,
            WPARAM(0),
            LPARAM(hkl),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_and_layout_read_like_windows() {
        assert_eq!(language_code("español", "spa"), "ESP");
        assert_eq!(language_code("English", "eng"), "ENG");
        assert_eq!(language_code("français", "fra"), "FRA");
        assert_eq!(language_code("Deutsch", "deu"), "DEU");
        assert_eq!(language_code("Ελληνικά", "ell"), "ELL");
        assert_eq!(language_code("日本語", "jpn"), "JPN");

        assert_eq!(variant_code("United States-International"), "INTL");
        assert_eq!(variant_code("US"), "US");
        assert_eq!(variant_code("Latin American"), "LA");
        assert_eq!(variant_code("Spanish"), "SPA");
        assert_eq!(variant_code("United States-Dvorak"), "USD");
        assert_eq!(variant_code(""), "");
    }
}
