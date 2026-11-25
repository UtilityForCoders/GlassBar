//! Remitentes de avisos que no son apps de Inicio: los del sistema ("Energía y
//! batería", "Seguridad de Windows", "Bluetooth"...).
//!
//! Windows los registra en `Software\Classes\AppUserModelId\<id>`, con el nombre que
//! enseña el aviso (`DisplayName`, a menudo un recurso "@dll,-123") y su icono
//! (`IconUri`: una ruta, un "@{paquete?ms-resource://...}" o un "ms-resource://...").
use crate::apps::fill_rounded;
use std::sync::Mutex;
use tiny_skia::{Pixmap, PixmapPaint, Transform};

/// Icono genérico de Windows para los avisos del sistema que no tienen uno propio.
const DEFAULT_ICON: &str =
    "ms-resource://Windows.UI.ShellCommon/Files/Images/DefaultSystemNotification.png";
/// Dónde registra Windows los remitentes, en HKLM y en HKCU.
const REGISTERED: &str = r"Software\Classes\AppUserModelId";

/// Un remitente registrado: su nombre ya preparado para comparar y su `IconUri`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Registered {
    key: String,
    icon: String,
}

static REGISTRY: Mutex<Option<Vec<Registered>>> = Mutex::new(None);

/// El icono del remitente del sistema que se llama `sender`, si Windows lo registra.
/// Lee el registro la primera vez: hay que llamarla fuera del hilo de la interfaz.
pub fn icon(sender: &str) -> Option<Pixmap> {
    let wanted = crate::catalog::search_key(sender);
    if wanted.is_empty() {
        return None;
    }
    let candidates: Vec<String> = {
        let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        registry
            .get_or_insert_with(read)
            .iter()
            .filter(|r| r.key == wanted)
            .map(|r| r.icon.clone())
            .collect()
    };
    if candidates.is_empty() {
        return None;
    }
    // Varios pueden llamarse igual ("Audio", "Bluetooth"): el primero con icono.
    candidates
        .iter()
        .filter(|uri| !uri.is_empty())
        .chain(std::iter::once(&DEFAULT_ICON.to_string()))
        .find_map(|uri| load(uri))
}

/// Carga la imagen de un `IconUri` y la prepara para el fondo oscuro de la tarjeta.
fn load(uri: &str) -> Option<Pixmap> {
    let path = resolve(uri)?;
    let picture = larger_variants(&path)
        .iter()
        .find_map(|p| crate::icons::load_picture(p))
        .or_else(|| crate::icons::load_picture(&path))?;
    Some(prepare(picture))
}

/// Ruta del archivo de un `IconUri`.
fn resolve(uri: &str) -> Option<String> {
    let source = if uri.starts_with("ms-resource:") {
        // Recursos del propio Windows: se resuelven como los de un paquete.
        format!("@{{Windows?{uri}}}")
    } else {
        uri.to_string()
    };
    let path = crate::jumplist::indirect(&source);
    (!path.is_empty() && !path.starts_with('@') && std::path::Path::new(&path).is_file())
        .then_some(path)
}

/// La misma imagen a más resolución, si existe: un recurso se resuelve a su escala
/// del 100 %, pequeña para la tarjeta.
fn larger_variants(path: &str) -> Vec<String> {
    let Some(at) = path.find("scale-100") else {
        return Vec::new();
    };
    ["scale-400", "scale-200", "scale-150"]
        .iter()
        .map(|scale| format!("{}{scale}{}", &path[..at], &path[at + "scale-100".len()..]))
        .filter(|p| std::path::Path::new(p).is_file())
        .collect()
}

/// Los iconos de un solo color (casi todos los del sistema) se ven blancos sobre
/// un cuadrado redondeado, como la inicial de una app sin icono; los de color
/// (el escudo de Seguridad de Windows) quedan como están.
fn prepare(mut picture: Pixmap) -> Pixmap {
    let Some(dark) = monochrome(&picture) else {
        return picture;
    };
    if dark {
        whiten(&mut picture);
    }
    tile(&picture).unwrap_or(picture)
}

/// Si la imagen es de un solo tono gris: `Some(true)` si es oscura (negra sobre
/// transparente, pensada para el tema claro), `Some(false)` si es clara.
fn monochrome(picture: &Pixmap) -> Option<bool> {
    let mut light = 0u64;
    let mut weight = 0u64;
    for p in picture.data().chunks_exact(4) {
        let alpha = p[3];
        if alpha < 24 {
            continue;
        }
        // Quita la premultiplicación para comparar los canales.
        let channel = |c: u8| (c as u32 * 255 / alpha as u32).min(255) as u8;
        let (r, g, b) = (channel(p[0]), channel(p[1]), channel(p[2]));
        if r.max(g).max(b) - r.min(g).min(b) > 40 {
            return None;
        }
        light += (r as u64 + g as u64 + b as u64) / 3 * alpha as u64;
        weight += alpha as u64;
    }
    (weight > 0).then(|| light / weight < 128)
}

/// Pinta de blanco lo que se ve, conservando la transparencia.
fn whiten(picture: &mut Pixmap) {
    for p in picture.data_mut().chunks_exact_mut(4) {
        let alpha = p[3];
        p[0] = alpha;
        p[1] = alpha;
        p[2] = alpha;
    }
}

/// La imagen centrada sobre un cuadrado redondeado translúcido.
fn tile(picture: &Pixmap) -> Option<Pixmap> {
    const SIDE: f32 = 128.0;
    let mut image = Pixmap::new(SIDE as u32, SIDE as u32)?;
    fill_rounded(
        &mut image,
        [255, 255, 255, 40],
        0.0,
        0.0,
        SIDE,
        SIDE,
        SIDE * 0.24,
    );
    let inner = (SIDE * 0.6).round() as u32;
    let scaled = crate::apps::downscale(picture, inner);
    image.draw_pixmap(
        ((SIDE as u32 - scaled.width()) / 2) as i32,
        ((SIDE as u32 - scaled.height()) / 2) as i32,
        scaled.as_ref(),
        &PixmapPaint {
            quality: tiny_skia::FilterQuality::Bicubic,
            ..PixmapPaint::default()
        },
        Transform::identity(),
        None,
    );
    Some(image)
}

/// Los remitentes registrados con nombre, de HKCU y de HKLM.
fn read() -> Vec<Registered> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let mut found = Vec::new();
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        read_root(root, &mut found);
    }
    found
}

fn read_root(root: windows::Win32::System::Registry::HKEY, found: &mut Vec<Registered>) {
    use windows::core::{HSTRING, PCWSTR, PWSTR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, KEY_READ,
    };
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(root, &HSTRING::from(REGISTERED), None, KEY_READ, &mut key).is_err() {
            return;
        }
        for index in 0.. {
            let mut name = [0u16; 256];
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
            let id = PCWSTR(name.as_ptr());
            let Some(display) = value(key, id, "DisplayName") else {
                continue;
            };
            let display = crate::jumplist::indirect(&display);
            if display.is_empty() || display.starts_with('@') {
                continue;
            }
            found.push(Registered {
                key: crate::catalog::search_key(&display),
                icon: value(key, id, "IconUri").unwrap_or_default(),
            });
        }
        let _ = RegCloseKey(key);
    }
}

/// Un valor de texto de la subclave `id` de `key`.
unsafe fn value(
    key: windows::Win32::System::Registry::HKEY,
    id: windows::core::PCWSTR,
    name: &str,
) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ};
    let mut buffer = vec![0u16; 1024];
    let mut size = (buffer.len() * 2) as u32;
    RegGetValueW(
        key,
        id,
        &HSTRING::from(name),
        RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
        None,
        Some(buffer.as_mut_ptr().cast()),
        Some(&mut size),
    )
    .ok()
    .ok()?;
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(0);
    (end > 0).then(|| String::from_utf16_lossy(&buffer[..end]).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(rgba: [u8; 4]) -> Pixmap {
        let mut image = Pixmap::new(4, 4).unwrap();
        for p in image.data_mut().chunks_exact_mut(4) {
            p.copy_from_slice(&rgba);
        }
        image
    }

    #[test]
    fn single_tone_icons_are_told_apart_from_colored_ones() {
        assert_eq!(monochrome(&filled([0, 0, 0, 255])), Some(true));
        assert_eq!(monochrome(&filled([240, 240, 240, 255])), Some(false));
        // Premultiplicado: blanco a media opacidad sigue siendo claro.
        assert_eq!(monochrome(&filled([128, 128, 128, 128])), Some(false));
        assert_eq!(monochrome(&filled([0, 120, 215, 255])), None);
        assert_eq!(monochrome(&filled([0, 0, 0, 0])), None);
    }

    #[test]
    fn dark_glyphs_turn_white_and_keep_their_transparency() {
        let mut image = filled([0, 0, 0, 100]);
        whiten(&mut image);
        assert_eq!(&image.data()[..4], &[100, 100, 100, 100]);
    }

    #[test]
    fn single_tone_icons_go_on_a_tile_and_colored_ones_stay() {
        let glyph = prepare(filled([0, 0, 0, 255]));
        assert_eq!((glyph.width(), glyph.height()), (128, 128));
        let colored = prepare(filled([0, 120, 215, 255]));
        assert_eq!((colored.width(), colored.height()), (4, 4));
    }

    #[test]
    fn resources_use_their_largest_scale_only_if_it_exists() {
        assert!(larger_variants(r"C:\no\existe\icono.png").is_empty());
        assert!(larger_variants(r"C:\no\existe\icono.scale-100.png").is_empty());
    }

    #[test]
    fn system_senders_are_read_from_the_registry() {
        // "Energía y batería" en español, "Battery saver"... depende del idioma;
        // basta con que haya remitentes y alguno tenga icono.
        let registered = read();
        assert!(!registered.is_empty());
        assert!(registered.iter().any(|r| !r.icon.is_empty()));
    }
}
