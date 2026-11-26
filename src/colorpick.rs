//! Selector de color propio, dentro de la ventana de configuración: una rueda de
//! tono y saturación, una barra de brillo, la vista previa del logo con el color y
//! los botones de cancelar y aplicar. Sustituye al diálogo de Windows, que no
//! encaja con el resto de GlassBar.
use crate::apps::{fill_rounded, fill_rounded_gradient, stroke_rounded};
use crate::clock::{draw_text_on_baseline, draw_text_semibold, line_metrics, text_width};
use crate::logo::{from_hsv, to_hsv};
use tiny_skia::{Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform};

/// Tarjeta del selector.
const CARD_W: f32 = 300.0;
const CARD_H: f32 = 404.0;
const PAD: f32 = 20.0;
/// Rueda: centro (respecto a la tarjeta) y radio.
const WHEEL_CY: f32 = 143.0;
const WHEEL_R: f32 = 95.0;
/// Barra del brillo.
const BRIGHTNESS_Y: f32 = 276.0;
const BRIGHTNESS_H: f32 = 10.0;
/// Fila de la vista previa.
const PREVIEW_Y: f32 = 300.0;
const PREVIEW_H: f32 = 36.0;
/// Botones.
const BUTTONS_Y: f32 = 352.0;
const BUTTON_H: f32 = 36.0;
const BUTTON_GAP: f32 = 10.0;
const TEXT: f32 = 13.0;

/// Parte del selector bajo el cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Wheel,
    Brightness,
    Cancel,
    Apply,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColorPicker {
    /// Tono (0 a 360), saturación y brillo (0 a 1).
    hue: f32,
    saturation: f32,
    value: f32,
    /// Esquina superior izquierda de la tarjeta, en la ventana.
    left: f32,
    top: f32,
    /// Arrastrando en la rueda o en la barra.
    pub dragging: Option<Part>,
}

impl ColorPicker {
    /// Con `rgb` elegido, centrado en una ventana de `width` × `height`.
    pub fn new(rgb: [u8; 3], (width, height): (f32, f32)) -> Self {
        let (hue, saturation, value) = to_hsv(rgb);
        Self {
            hue,
            saturation,
            value,
            left: ((width - CARD_W) / 2.0).round(),
            top: ((height - CARD_H) / 2.0).round(),
            dragging: None,
        }
    }

    /// El color elegido.
    pub fn rgb(&self) -> [u8; 3] {
        let [r, g, b, _] = from_hsv(self.hue, self.saturation, self.value);
        [r, g, b]
    }

    fn button_w() -> f32 {
        (CARD_W - PAD * 2.0 - BUTTON_GAP) / 2.0
    }

    /// Lo que hay en `(x, y)` (en la ventana); fuera de la tarjeta, nada.
    pub fn hit(&self, x: f32, y: f32) -> Option<Part> {
        let (x, y) = (x - self.left, y - self.top);
        let (dx, dy) = (x - CARD_W / 2.0, y - WHEEL_CY);
        if (dx * dx + dy * dy).sqrt() <= WHEEL_R + 4.0 {
            return Some(Part::Wheel);
        }
        let inside_x = (PAD - 6.0..CARD_W - PAD + 6.0).contains(&x);
        if inside_x && (BRIGHTNESS_Y - 9.0..BRIGHTNESS_Y + BRIGHTNESS_H + 9.0).contains(&y) {
            return Some(Part::Brightness);
        }
        if (BUTTONS_Y..BUTTONS_Y + BUTTON_H).contains(&y) {
            let w = Self::button_w();
            if (PAD..PAD + w).contains(&x) {
                return Some(Part::Cancel);
            }
            if (PAD + w + BUTTON_GAP..CARD_W - PAD).contains(&x) {
                return Some(Part::Apply);
            }
        }
        None
    }

    /// Mueve el punto de `part` (la rueda o la barra) a `(x, y)`, en la ventana.
    pub fn drag_to(&mut self, part: Part, x: f32, y: f32) {
        let (x, y) = (x - self.left, y - self.top);
        match part {
            Part::Wheel => {
                let (dx, dy) = (x - CARD_W / 2.0, y - WHEEL_CY);
                // Rojo a la derecha y el tono crece en sentido antihorario.
                self.hue = (-dy).atan2(dx).to_degrees().rem_euclid(360.0);
                self.saturation = ((dx * dx + dy * dy).sqrt() / WHEEL_R).min(1.0);
                // Con el brillo a cero no se vería qué se elige en la rueda.
                if self.value < 0.05 {
                    self.value = 1.0;
                }
            }
            Part::Brightness => {
                self.value = ((x - PAD) / (CARD_W - PAD * 2.0)).clamp(0.0, 1.0);
            }
            Part::Cancel | Part::Apply => {}
        }
    }

    pub fn draw(&self, image: &mut Pixmap, scale: f32, window: (f32, f32), hovered: Option<Part>) {
        let s = |v: f32| v * scale;
        let (l, t) = (self.left, self.top);
        // Lo de debajo, oscurecido: el selector espera respuesta.
        fill_rounded(
            image,
            [0, 0, 0, 120],
            s(1.0),
            s(1.0),
            s(window.0 - 2.0),
            s(window.1 - 2.0),
            s(12.0),
        );
        fill_rounded(
            image,
            [38, 38, 41, 252],
            s(l),
            s(t),
            s(CARD_W),
            s(CARD_H),
            s(10.0),
        );
        stroke_rounded(
            image,
            [255, 255, 255, 34],
            s(l),
            s(t),
            s(CARD_W),
            s(CARD_H),
            s(10.0),
            scale * 0.75,
        );
        draw_text_semibold(
            image,
            "Color del logo",
            s(l + PAD),
            s(t + 16.0),
            s(13.5),
            1.0,
        );

        // Rueda de tono y saturación, a todo brillo.
        let (cx, cy) = (l + CARD_W / 2.0, t + WHEEL_CY);
        if let Some(wheel) = wheel(s(WHEEL_R)) {
            let side = wheel.width() as f32;
            image.draw_pixmap(
                (s(cx) - side / 2.0).round() as i32,
                (s(cy) - side / 2.0).round() as i32,
                wheel.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        let angle = self.hue.to_radians();
        let reach = self.saturation * WHEEL_R;
        let knob = (cx + angle.cos() * reach, cy - angle.sin() * reach);
        draw_knob(image, s(knob.0), s(knob.1), s(6.0), scale);

        // Brillo: de negro al color a todo brillo.
        let size = s(TEXT);
        draw_text_on_baseline(image, "Brillo", s(l + PAD), s(t + 264.0), size, 0.8);
        let bright = from_hsv(self.hue, self.saturation, 1.0);
        let (bar_l, bar_w) = (l + PAD, CARD_W - PAD * 2.0);
        fill_rounded_gradient(
            image,
            &[(0.0, [0, 0, 0, 255]), (1.0, bright)],
            (s(bar_l), 0.0),
            (s(bar_l + bar_w), 0.0),
            s(bar_l),
            s(t + BRIGHTNESS_Y),
            s(bar_w),
            s(BRIGHTNESS_H),
            s(BRIGHTNESS_H / 2.0),
        );
        draw_knob(
            image,
            s(bar_l + bar_w * self.value),
            s(t + BRIGHTNESS_Y + BRIGHTNESS_H / 2.0),
            s(7.0),
            scale,
        );

        // Vista previa: el logo con el color y una muestra.
        let row = t + PREVIEW_Y;
        fill_rounded(
            image,
            [255, 255, 255, 14],
            s(l + PAD),
            s(row),
            s(CARD_W - PAD * 2.0),
            s(PREVIEW_H),
            s(6.0),
        );
        let stops = crate::logo::custom_stops(self.rgb());
        crate::apps::draw_start_icon_with(
            image,
            s(l + PAD + PREVIEW_H / 2.0),
            s(row + PREVIEW_H / 2.0),
            20.0,
            scale,
            &stops,
        );
        let [r, g, b] = self.rgb();
        let swatch_w = 56.0;
        fill_rounded(
            image,
            [r, g, b, 255],
            s(l + CARD_W - PAD - swatch_w - 6.0),
            s(row + 6.0),
            s(swatch_w),
            s(PREVIEW_H - 12.0),
            s(4.0),
        );
        stroke_rounded(
            image,
            [255, 255, 255, 40],
            s(l + CARD_W - PAD - swatch_w - 6.0),
            s(row + 6.0),
            s(swatch_w),
            s(PREVIEW_H - 12.0),
            s(4.0),
            scale * 0.75,
        );
        let (ascent, descent) = line_metrics(size);
        let baseline = s(row + PREVIEW_H / 2.0) + (ascent - descent) / 2.0;
        draw_text_on_baseline(
            image,
            "Vista previa",
            s(l + PAD + 40.0),
            baseline,
            size,
            0.8,
        );

        // Cancelar y aplicar.
        let w = Self::button_w();
        let baseline = s(t + BUTTONS_Y + BUTTON_H / 2.0) + (ascent - descent) / 2.0;
        for (part, left, label, base) in [
            (Part::Cancel, l + PAD, "Cancelar", 18u8),
            (Part::Apply, l + PAD + w + BUTTON_GAP, "Aplicar", 40u8),
        ] {
            let alpha = if hovered == Some(part) {
                base + 16
            } else {
                base
            };
            fill_rounded(
                image,
                [255, 255, 255, alpha],
                s(left),
                s(t + BUTTONS_Y),
                s(w),
                s(BUTTON_H),
                s(6.0),
            );
            let tx = s(left + w / 2.0) - text_width(label, size) / 2.0;
            draw_text_on_baseline(image, label, tx, baseline, size, 0.95);
        }
    }
}

/// Rueda de tono (alrededor) y saturación (del centro al borde), de radio `radius`
/// en píxeles, con el borde suavizado.
fn wheel(radius: f32) -> Option<Pixmap> {
    let side = (radius * 2.0).ceil() as u32 + 2;
    let mut image = Pixmap::new(side, side)?;
    let center = side as f32 / 2.0;
    for (i, pixel) in image.data_mut().chunks_exact_mut(4).enumerate() {
        let x = (i as u32 % side) as f32 + 0.5 - center;
        let y = (i as u32 / side) as f32 + 0.5 - center;
        let distance = (x * x + y * y).sqrt();
        let coverage = (radius - distance + 0.5).clamp(0.0, 1.0);
        if coverage <= 0.0 {
            continue;
        }
        let hue = (-y).atan2(x).to_degrees();
        let [r, g, b, _] = from_hsv(hue, (distance / radius).min(1.0), 1.0);
        // Premultiplicado, como lo guarda tiny-skia.
        let alpha = (coverage * 255.0).round() as u8;
        let premultiply = |c: u8| (c as f32 * coverage).round() as u8;
        pixel.copy_from_slice(&[premultiply(r), premultiply(g), premultiply(b), alpha]);
    }
    Some(image)
}

/// Punto que se arrastra: un círculo con borde blanco y otro oscuro alrededor, que
/// se ve sobre cualquier color.
fn draw_knob(image: &mut Pixmap, x: f32, y: f32, radius: f32, scale: f32) {
    let Some(circle) = PathBuilder::from_circle(x, y, radius) else {
        return;
    };
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    for (color, width) in [([0, 0, 0, 140], 4.0), ([255, 255, 255, 255], 2.0)] {
        let [r, g, b, a] = color;
        paint.set_color_rgba8(r, g, b, a);
        let stroke = Stroke {
            width: width * scale,
            ..Stroke::default()
        };
        image.stroke_path(&circle, &paint, &stroke, Transform::identity(), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: (f32, f32) = (420.0, 450.0);

    #[test]
    fn the_color_round_trips_and_the_wheel_sets_hue_and_saturation() {
        let mut picker = ColorPicker::new([255, 0, 0], WINDOW);
        assert_eq!(picker.rgb(), [255, 0, 0]);
        let (cx, cy) = (picker.left + CARD_W / 2.0, picker.top + WHEEL_CY);
        // Arriba del todo, en el borde: tono 90 (entre amarillo y verde), saturado.
        assert_eq!(picker.hit(cx, cy - WHEEL_R + 2.0), Some(Part::Wheel));
        picker.drag_to(Part::Wheel, cx, cy - WHEEL_R * 2.0);
        assert!((picker.hue - 90.0).abs() < 0.01);
        assert_eq!(picker.saturation, 1.0);
        // En el centro, blanco.
        picker.drag_to(Part::Wheel, cx, cy);
        assert_eq!(picker.rgb(), [255, 255, 255]);
    }

    #[test]
    fn brightness_and_buttons() {
        let mut picker = ColorPicker::new([0, 120, 240], WINDOW);
        let y = picker.top + BRIGHTNESS_Y + BRIGHTNESS_H / 2.0;
        assert_eq!(picker.hit(picker.left + 60.0, y), Some(Part::Brightness));
        picker.drag_to(Part::Brightness, picker.left - 50.0, y);
        assert_eq!(picker.rgb(), [0, 0, 0]);
        // Con el brillo a cero, tocar la rueda lo vuelve a subir.
        let (cx, cy) = (picker.left + CARD_W / 2.0, picker.top + WHEEL_CY);
        picker.drag_to(Part::Wheel, cx + WHEEL_R, cy);
        assert_eq!(picker.rgb(), [255, 0, 0]);
        let by = picker.top + BUTTONS_Y + BUTTON_H / 2.0;
        assert_eq!(picker.hit(picker.left + PAD + 10.0, by), Some(Part::Cancel));
        assert_eq!(
            picker.hit(picker.left + CARD_W - PAD - 10.0, by),
            Some(Part::Apply)
        );
        assert_eq!(picker.hit(picker.left + 5.0, picker.top + 5.0), None);
    }

    #[test]
    fn the_card_fits_in_the_settings_window() {
        let picker = ColorPicker::new([0, 0, 0], (420.0, crate::settings::height()));
        assert!(picker.left >= 0.0 && picker.top >= 0.0);
    }
}
