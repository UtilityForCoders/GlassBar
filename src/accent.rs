//! Color de énfasis de Windows (Configuración → Personalización → Colores), para
//! que los botones activos, las barras y las selecciones de GlassBar lo sigan.
//! Windows guarda el color con sus tonos más claros y más oscuros, que son los que
//! usa en sus propios controles; se leen una vez y se vuelven a leer cuando Windows
//! avisa de que cambió.
use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Accent {
    /// El color elegido.
    pub base: [u8; 4],
    /// Tono más oscuro: fondo de los botones activos, con texto blanco encima.
    pub dark: [u8; 4],
    /// Tono más claro: barras, líneas y selecciones sobre fondo oscuro.
    pub light: [u8; 4],
}

impl Accent {
    /// El azul por defecto de Windows, si no se puede leer el del usuario.
    const DEFAULT: Self = Self {
        base: [0, 120, 212, 255],
        dark: [0, 95, 184, 255],
        light: [76, 160, 255, 255],
    };
}

/// Color con otra opacidad, para resaltados semitransparentes.
pub fn with_alpha([r, g, b, _]: [u8; 4], alpha: u8) -> [u8; 4] {
    [r, g, b, alpha]
}

/// Lee la paleta que guarda Windows en `AccentPalette`: 8 colores de 4 bytes (rojo,
/// verde, azul y uno sin usar), del más claro al más oscuro: claro 3, 2 y 1, el
/// color, oscuro 1, 2 y 3, y uno más.
pub fn from_palette(bytes: &[u8]) -> Option<Accent> {
    if bytes.len() < 32 {
        return None;
    }
    let color = |index: usize| {
        let c = &bytes[index * 4..index * 4 + 3];
        [c[0], c[1], c[2], 255]
    };
    Some(Accent {
        base: color(3),
        dark: color(4),
        light: color(1),
    })
}

/// A partir de un solo color (el de DWM, en ARGB), si no hay paleta: tonos más
/// oscuro y más claro mezclando con negro y blanco.
pub fn from_color(argb: u32) -> Accent {
    let [r, g, b] = [(argb >> 16) as u8, (argb >> 8) as u8, argb as u8];
    let mix =
        |c: u8, with: u8, amount: f32| (c as f32 + (with as f32 - c as f32) * amount).round() as u8;
    Accent {
        base: [r, g, b, 255],
        dark: [mix(r, 0, 0.25), mix(g, 0, 0.25), mix(b, 0, 0.25), 255],
        light: [mix(r, 255, 0.35), mix(g, 255, 0.35), mix(b, 255, 0.35), 255],
    }
}

fn read() -> Accent {
    let _p = crate::profile::scope("accent.read (registro)");
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_BINARY};
    let mut bytes = [0u8; 32];
    let mut size = bytes.len() as u32;
    let palette = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent"),
            w!("AccentPalette"),
            RRF_RT_REG_BINARY,
            None,
            Some(bytes.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if palette.is_ok() {
        if let Some(accent) = from_palette(&bytes[..size as usize]) {
            return accent;
        }
    }
    let mut color = 0;
    let mut opaque = windows::core::BOOL::default();
    if unsafe { windows::Win32::Graphics::Dwm::DwmGetColorizationColor(&mut color, &mut opaque) }
        .is_ok()
    {
        return from_color(color);
    }
    Accent::DEFAULT
}

thread_local! {
    static CURRENT: Cell<Option<Accent>> = const { Cell::new(None) };
}

/// El color de énfasis actual (leído la primera vez y guardado).
pub fn current() -> Accent {
    CURRENT.with(|cached| {
        cached.get().unwrap_or_else(|| {
            let accent = read();
            cached.set(Some(accent));
            accent
        })
    })
}

/// Olvida el color guardado: Windows avisó de un cambio. Devuelve si cambió.
pub fn refresh() -> bool {
    let before = CURRENT.with(|cached| cached.take());
    before != Some(current())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_bytes_give_the_shades_windows_uses() {
        // Paleta del azul por defecto de Windows 11.
        let palette: [u8; 32] = [
            0x99, 0xeb, 0xff, 0x00, 0x4c, 0xc2, 0xff, 0x00, 0x00, 0x91, 0xf8, 0x00, 0x00, 0x78,
            0xd4, 0x00, 0x00, 0x5a, 0x9e, 0x00, 0x00, 0x42, 0x75, 0x00, 0x00, 0x26, 0x42, 0x00,
            0xf7, 0x63, 0x0c, 0x00,
        ];
        let accent = from_palette(&palette).unwrap();
        assert_eq!(accent.base, [0x00, 0x78, 0xd4, 255]);
        assert_eq!(accent.dark, [0x00, 0x5a, 0x9e, 255]);
        assert_eq!(accent.light, [0x4c, 0xc2, 0xff, 255]);
        assert_eq!(from_palette(&palette[..20]), None);
    }

    #[test]
    fn a_single_color_gets_darker_and_lighter_shades() {
        // Rojo de DWM en ARGB.
        let accent = from_color(0xffe8_1123);
        assert_eq!(accent.base, [0xe8, 0x11, 0x23, 255]);
        assert!(accent.dark[0] < accent.base[0]);
        assert!(accent.light[1] > accent.base[1]);
        assert_eq!(with_alpha(accent.base, 110)[3], 110);
    }
}
