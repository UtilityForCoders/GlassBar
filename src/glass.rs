//! Cristal gris de opacidad media: sin sombras ni reflejo superior.
use crate::apps::{fill_rounded, fill_rounded_gradient, stroke_rounded};
use crate::style::Appearance;
use std::sync::atomic::AtomicU8;
use tiny_skia::Pixmap;

/// Opacidad de los paneles y menús propios, en tanto por ciento: la elige el
/// usuario en el menú de GlassBar (ver `style::Styles::menu_opacity`).
static MENU_OPACITY: AtomicU8 = AtomicU8::new(DEFAULT_MENU_OPACITY);
pub const DEFAULT_MENU_OPACITY: u8 = 43;
/// Por debajo, el texto no se lee y los clics atravesarían el fondo.
pub const MIN_MENU_OPACITY: u8 = 10;

pub fn set_menu_opacity(percent: u8) {
    MENU_OPACITY.store(
        percent.clamp(MIN_MENU_OPACITY, 100),
        std::sync::atomic::Ordering::Relaxed,
    );
}
pub fn menu_opacity() -> u8 {
    MENU_OPACITY.load(std::sync::atomic::Ordering::Relaxed)
}

fn menu_alpha() -> u8 {
    (menu_opacity() as f32 * 2.55).round() as u8
}

/// Fondo de los paneles y menús propios: el gris del cristal, translúcido para que
/// se vea el desenfoque de debajo, con la opacidad elegida.
pub fn panel() -> [u8; 4] {
    [32, 32, 34, menu_alpha()]
}
/// El mismo fondo, algo más claro: los menús de opciones y lo que está bajo el cursor.
pub fn panel_light() -> [u8; 4] {
    [48, 48, 50, menu_alpha().saturating_add(15)]
}

/// Radio de las esquinas del dock, en unidades lógicas.
pub const DOCK_RADIUS: f32 = 16.0;

/// El fondo del dock con la apariencia de la situación actual. La normal tiene
/// opacidad media para separar los iconos del escritorio.
pub fn draw_dock(
    pixmap: &mut Pixmap,
    appearance: Appearance,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    scale: f32,
) {
    let ([left, middle, right], stroke) = appearance.colors();
    fill_rounded_gradient(
        pixmap,
        &[(0.0, left), (0.5, middle), (1.0, right)],
        (x, y),
        (x + w, y),
        x,
        y,
        w,
        h,
        DOCK_RADIUS * scale,
    );
    if stroke[3] > 0 {
        stroke_rounded(pixmap, stroke, x, y, w, h, DOCK_RADIUS * scale, scale);
    }
}
pub fn draw_hover(pixmap: &mut Pixmap, cx: f32, cy: f32, scale: f32) {
    fill_rounded(
        pixmap,
        [255, 255, 255, 17],
        cx - 19.0 * scale,
        cy - 19.0 * scale,
        38.0 * scale,
        38.0 * scale,
        10.0 * scale,
    );
}
/// Naranja con el que Windows 11 marca las apps que piden atención.
const ATTENTION: [u8; 3] = [247, 99, 12];

/// Fondo naranja tenue detrás del icono de una app que pide atención.
pub fn draw_attention(pixmap: &mut Pixmap, cx: f32, cy: f32, scale: f32) {
    let [r, g, b] = ATTENTION;
    fill_rounded(
        pixmap,
        [r, g, b, 64],
        cx - 19.0 * scale,
        cy - 19.0 * scale,
        38.0 * scale,
        38.0 * scale,
        10.0 * scale,
    );
}

/// Indicador de una app que pide atención: más ancho que el de abierta y naranja.
pub fn draw_attention_dot(pixmap: &mut Pixmap, cx: f32, cy: f32, scale: f32) {
    let [r, g, b] = ATTENTION;
    fill_rounded(
        pixmap,
        [r, g, b, 255],
        cx - 7.0 * scale,
        cy,
        14.0 * scale,
        2.0 * scale,
        scale,
    );
}

pub fn draw_running_dot(pixmap: &mut Pixmap, cx: f32, cy: f32, scale: f32) {
    fill_rounded(
        pixmap,
        [255, 255, 255, 185],
        cx - 3.0 * scale,
        cy,
        6.0 * scale,
        2.0 * scale,
        scale,
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn surface_is_transparent_and_has_no_top_reflection() {
        let mut image = tiny_skia::Pixmap::new(200, 60).unwrap();
        super::draw_dock(
            &mut image,
            super::Appearance::Normal,
            2.0,
            2.0,
            196.0,
            52.0,
            1.0,
        );
        assert_eq!(image.pixel(100, 10).unwrap().alpha(), 130);
        assert_eq!(image.pixel(100, 10), image.pixel(100, 40));
        assert_eq!(image.pixel(0, 0).unwrap().alpha(), 0);
    }
}
