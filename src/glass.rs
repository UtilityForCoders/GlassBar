//! Cristal plano de opacidad media: sin sombras ni reflejo superior.
use crate::apps::{fill_rounded, stroke_rounded};
use tiny_skia::Pixmap;

pub fn draw_transparent_dock(pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, scale: f32) {
    // Opacidad media para separar los iconos del escritorio.
    fill_rounded(pixmap, [23, 28, 36, 130], x, y, w, h, 16.0 * scale);
    stroke_rounded(
        pixmap,
        [255, 255, 255, 32],
        x,
        y,
        w,
        h,
        16.0 * scale,
        0.75 * scale,
    );
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
        super::draw_transparent_dock(&mut image, 2.0, 2.0, 196.0, 52.0, 1.0);
        assert_eq!(image.pixel(100, 10).unwrap().alpha(), 130);
        assert_eq!(image.pixel(100, 10), image.pixel(100, 40));
        assert_eq!(image.pixel(0, 0).unwrap().alpha(), 0);
    }
}
