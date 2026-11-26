//! Color del logo de Inicio: el azul de Windows 11, el color de énfasis de Windows,
//! uno sacado del fondo de pantalla o uno elegido con el selector de color
//! (`colorpick`). Siempre es un degradado de un tono claro (arriba a la izquierda) a uno
//! más intenso (abajo a la derecha), como el logo original.
//!
//! El del fondo se calcula en otro hilo (hay que abrir la imagen) y se vuelve a
//! calcular cuando Windows avisa de que cambió el fondo.
use std::cell::Cell;
use std::sync::atomic::{AtomicU32, Ordering};

/// Paradas del degradado: posición (0 a 1) y color RGBA.
pub type Stops = [(f32, [u8; 4]); 2];

/// El degradado del logo de siempre.
const BLUE: Stops = [(0.0, [124, 224, 255, 255]), (1.0, [16, 148, 244, 255])];

/// De dónde sale el color del logo.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    Blue,
    /// El color de énfasis de Windows (Configuración → Personalización → Colores).
    Accent,
    /// El tono que más destaca en el fondo de pantalla.
    Wallpaper,
    /// El elegido con el selector de color.
    Custom,
}

/// Opciones, con su nombre, en el orden del selector. La primera es la de siempre.
const COLORS: [(&str, Source); 4] = [
    ("Azul", Source::Blue),
    ("Énfasis", Source::Accent),
    ("Fondo", Source::Wallpaper),
    ("Personalizado", Source::Custom),
];

/// Cuántas opciones hay para elegir.
pub const COUNT: usize = COLORS.len();

/// Posición de "Personalizado" en la lista.
pub const CUSTOM: usize = 3;

/// Color personalizado si aún no se eligió ninguno.
pub const DEFAULT_CUSTOM: [u8; 3] = [124, 64, 232];

/// Nombre de la opción `index` (una desconocida es el azul).
pub fn name(index: usize) -> &'static str {
    COLORS.get(index).unwrap_or(&COLORS[0]).0
}

thread_local! {
    static MODE: Cell<usize> = const { Cell::new(0) };
    static CUSTOM_COLOR: Cell<[u8; 3]> = const { Cell::new(DEFAULT_CUSTOM) };
}

/// Color del fondo calculado: `0` si aún no hay; si no, `1 << 24 | 0xRRGGBB`.
static WALLPAPER: AtomicU32 = AtomicU32::new(0);

/// Elige la opción `index` de la lista y el color personalizado.
pub fn set_mode(index: usize, custom: [u8; 3]) {
    MODE.with(|mode| mode.set(if index < COUNT { index } else { 0 }));
    CUSTOM_COLOR.with(|color| color.set(custom));
}

/// El color elegido sale del fondo de pantalla.
pub fn follows_wallpaper() -> bool {
    COLORS[MODE.with(Cell::get)].1 == Source::Wallpaper
}

/// El color elegido sigue el de énfasis de Windows.
pub fn follows_accent() -> bool {
    COLORS[MODE.with(Cell::get)].1 == Source::Accent
}

/// El degradado con el que se pinta ahora el logo.
pub fn stops() -> Stops {
    let accent = || {
        let accent = crate::accent::current();
        [(0.0, accent.light), (1.0, accent.base)]
    };
    match COLORS[MODE.with(Cell::get)].1 {
        Source::Blue => BLUE,
        // Termina justo en el color elegido; empieza en su tono claro.
        Source::Custom => custom_stops(CUSTOM_COLOR.with(Cell::get)),
        Source::Accent => accent(),
        Source::Wallpaper => match WALLPAPER.load(Ordering::Relaxed) {
            0 => accent(),
            packed => gradient([(packed >> 16) as u8, (packed >> 8) as u8, packed as u8]),
        },
    }
}

/// Vuelve a calcular en otro hilo el color del fondo y, si cambió, avisa a la
/// ventana `hwnd` con `message` para que se redibuje el logo.
pub fn refresh_wallpaper(hwnd: isize, message: u32) {
    let _ = std::thread::Builder::new()
        .name("color del fondo".into())
        .spawn(move || {
            let packed = read_wallpaper().map_or(0, |[r, g, b]| {
                1 << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32
            });
            if WALLPAPER.swap(packed, Ordering::Relaxed) != packed {
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                        Some(windows::Win32::Foundation::HWND(hwnd as *mut _)),
                        message,
                        windows::Win32::Foundation::WPARAM(0),
                        windows::Win32::Foundation::LPARAM(0),
                    );
                }
            }
        });
}

/// Degradado de un color personalizado: termina justo en él y empieza en su tono
/// claro.
pub fn custom_stops([r, g, b]: [u8; 3]) -> Stops {
    [gradient([r, g, b])[0], (1.0, [r, g, b, 255])]
}

/// Tono (0 a 360), saturación y brillo (0 a 1).
pub fn to_hsv([r, g, b]: [u8; 3]) -> (f32, f32, f32) {
    let [r, g, b] = [r, g, b].map(|c| c as f32 / 255.0);
    let max = r.max(g).max(b);
    let delta = max - r.min(g).min(b);
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    let saturation = if max == 0.0 { 0.0 } else { delta / max };
    (hue, saturation, max)
}

pub fn from_hsv(hue: f32, saturation: f32, value: f32) -> [u8; 4] {
    let hue = hue.rem_euclid(360.0) / 60.0;
    let chroma = value * saturation;
    let x = chroma * (1.0 - (hue % 2.0 - 1.0).abs());
    let (r, g, b) = match hue as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = value - chroma;
    let channel = |c: f32| ((c + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    [channel(r), channel(g), channel(b), 255]
}

/// Degradado del logo a partir de un color: el final es el color, vivo y con buen
/// brillo; el principio, más claro y algo desplazado de tono, como el cian del
/// logo azul. Un color casi gris da un logo gris.
pub fn gradient(rgb: [u8; 3]) -> Stops {
    let (hue, saturation, value) = to_hsv(rgb);
    if saturation < 0.12 {
        let end = from_hsv(hue, saturation, value.clamp(0.55, 0.75));
        return [(0.0, from_hsv(hue, saturation * 0.5, 1.0)), (1.0, end)];
    }
    let saturation = saturation.max(0.7);
    let end = from_hsv(hue, saturation, value.clamp(0.8, 0.95));
    let start = from_hsv(hue - 15.0, saturation * 0.55, 1.0);
    [(0.0, start), (1.0, end)]
}

/// El tono que más destaca en `pixels`: se agrupan por tono los colores vivos
/// (los más saturados pesan más) y se toma la media del grupo más grande. Si casi
/// no hay color (un fondo en blanco y negro), la media de todos.
pub fn dominant(pixels: impl IntoIterator<Item = [u8; 3]>) -> Option<[u8; 3]> {
    const BINS: usize = 24;
    let mut weights = [0.0f32; BINS];
    let mut sums = [[0.0f32; 3]; BINS];
    let (mut average, mut count) = ([0.0f32; 3], 0usize);
    for pixel in pixels {
        count += 1;
        for (sum, c) in average.iter_mut().zip(pixel) {
            *sum += c as f32;
        }
        let (hue, saturation, value) = to_hsv(pixel);
        if saturation < 0.15 || value < 0.15 {
            continue;
        }
        let weight = saturation * saturation * value;
        let bin = (hue / 360.0 * BINS as f32) as usize % BINS;
        weights[bin] += weight;
        for (sum, c) in sums[bin].iter_mut().zip(pixel) {
            *sum += c as f32 * weight;
        }
    }
    if count == 0 {
        return None;
    }
    let near = |bin: usize| [(bin + BINS - 1) % BINS, bin, (bin + 1) % BINS];
    let best = (0..BINS)
        .max_by(|&a, &b| {
            let total = |bin| near(bin).iter().map(|&i| weights[i]).sum::<f32>();
            total(a).total_cmp(&total(b))
        })
        .unwrap_or(0);
    let weight: f32 = near(best).iter().map(|&i| weights[i]).sum();
    if weight / (count as f32) < 0.02 {
        return Some(average.map(|sum| (sum / count as f32).round() as u8));
    }
    let mut color = [0.0f32; 3];
    for i in near(best) {
        for (c, sum) in color.iter_mut().zip(sums[i]) {
            *c += sum;
        }
    }
    Some(color.map(|c| (c / weight).round().clamp(0.0, 255.0) as u8))
}

/// El color del fondo de pantalla: el de la imagen o, sin imagen, el color sólido.
fn read_wallpaper() -> Option<[u8; 3]> {
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETDESKWALLPAPER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };
    let mut path = [0u16; 520];
    unsafe {
        SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            path.len() as u32,
            Some(path.as_mut_ptr().cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .ok()?;
    }
    let end = path.iter().position(|&c| c == 0).unwrap_or(path.len());
    let path = String::from_utf16_lossy(&path[..end]);
    if path.is_empty() {
        use windows::Win32::Graphics::Gdi::{GetSysColor, COLOR_BACKGROUND};
        let color = unsafe { GetSysColor(COLOR_BACKGROUND) };
        return Some([color as u8, (color >> 8) as u8, (color >> 16) as u8]);
    }
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let pixels = small_picture(&path);
        if initialized {
            CoUninitialize();
        }
        dominant(pixels?)
    }
}

/// La imagen de `path` reducida a 32 × 32 (no hace falta más para el color).
unsafe fn small_picture(path: &str) -> Option<Vec<[u8; 3]>> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::GENERIC_READ;
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICImagingFactory,
        WICBitmapInterpolationModeFant, WICConvertBitmapSource, WICDecodeMetadataCacheOnDemand,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    const SIDE: u32 = 32;
    let factory: IWICImagingFactory =
        CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
    let decoder = factory
        .CreateDecoderFromFilename(
            &HSTRING::from(path),
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )
        .ok()?;
    let frame = decoder.GetFrame(0).ok()?;
    let scaler = factory.CreateBitmapScaler().ok()?;
    scaler
        .Initialize(&frame, SIDE, SIDE, WICBitmapInterpolationModeFant)
        .ok()?;
    let bitmap = WICConvertBitmapSource(&GUID_WICPixelFormat32bppBGRA, &scaler).ok()?;
    let mut bytes = vec![0u8; (SIDE * SIDE * 4) as usize];
    bitmap
        .CopyPixels(std::ptr::null(), SIDE * 4, &mut bytes)
        .ok()?;
    Some(bytes.chunks_exact(4).map(|p| [p[2], p[1], p[0]]).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blue_gradient_is_close_to_the_original_logo() {
        let [(_, start), (_, end)] = gradient([0, 120, 240]);
        assert_eq!(end, [0, 120, 240, 255]);
        // El original empieza en (112, 224, 255).
        assert!(start[0].abs_diff(112) < 12 && start[1].abs_diff(224) < 12);
        assert_eq!(start[2], 255);
    }

    #[test]
    fn grey_colors_give_a_grey_logo() {
        let [(_, start), (_, end)] = gradient([128, 128, 128]);
        assert!(start[0] > end[0]);
        assert_eq!((end[0], end[1]), (end[1], end[2]));
    }

    #[test]
    fn the_most_vivid_hue_wins_over_a_bigger_grey_area() {
        let mut pixels = vec![[90, 90, 95]; 700];
        pixels.extend(vec![[230, 40, 40]; 200]);
        pixels.extend(vec![[40, 40, 220]; 100]);
        let [r, g, b] = dominant(pixels).unwrap();
        assert!(r > 200 && g < 60 && b < 60);
        // Casi sin color: la media.
        assert_eq!(
            dominant(vec![[20, 20, 20], [40, 40, 40]]),
            Some([30, 30, 30])
        );
        assert_eq!(dominant(Vec::new()), None);
    }

    #[test]
    fn unknown_choices_fall_back_to_blue() {
        assert_eq!(name(0), "Azul");
        assert_eq!(name(99), "Azul");
        set_mode(99, DEFAULT_CUSTOM);
        assert!(!follows_wallpaper() && !follows_accent());
        assert_eq!(stops(), BLUE);
        set_mode(2, DEFAULT_CUSTOM);
        assert!(follows_wallpaper());
    }

    #[test]
    fn the_custom_color_ends_the_gradient() {
        assert_eq!(name(CUSTOM), "Personalizado");
        set_mode(CUSTOM, [16, 160, 92]);
        assert_eq!(stops()[0], gradient([16, 160, 92])[0]);
        assert_eq!(stops()[1].1, [16, 160, 92, 255]);
    }
}
