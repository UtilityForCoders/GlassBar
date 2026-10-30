//! Vista previa de las ventanas de un programa al pasar el cursor por él en el
//! dock: una tarjeta por ventana, una al lado de otra como en Windows, con su
//! título y, debajo, su miniatura en vivo (la dibuja Windows con DWM). Aquí está la
//! distribución y el dibujo de las tarjetas; la ventana vive en `main`.
use crate::apps::{fill_rounded, stroke_rounded};
use crate::clock::{draw_icon_aligned, draw_text_on_baseline, fit_text, line_metrics};
use tiny_skia::Pixmap;

const PAD: f32 = 8.0;
const TITLE_H: f32 = 32.0;
/// La miniatura cabe en este recuadro conservando la proporción de la ventana.
const MAX_W: f32 = 240.0;
const MAX_H: f32 = 150.0;
const MIN_W: f32 = 180.0;
const CLOSE: f32 = 26.0;
/// Separación entre tarjetas.
const GAP: f32 = 8.0;

/// Una tarjeta, en coordenadas lógicas del panel; su miniatura y su botón de
/// cerrar también.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Card {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub thumb: (f32, f32, f32, f32),
    pub close: (f32, f32, f32, f32),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    pub cards: Vec<Card>,
}

/// Tarjeta de una ventana de `source` píxeles reales, en la posición `x`. La
/// miniatura se reduce hasta caber (sin agrandarse) y todo se encoge por `shrink`
/// cuando la fila no cabe en la pantalla.
fn card(source: (f32, f32), scale: f32, shrink: f32, x: f32) -> Card {
    let (w, h) = ((source.0 / scale).max(1.0), (source.1 / scale).max(1.0));
    let fit = (MAX_W * shrink / w).min(MAX_H * shrink / h).min(1.0);
    let (thumb_w, thumb_h) = (w * fit, h * fit);
    let width = (thumb_w + PAD * 2.0).max(MIN_W * shrink);
    Card {
        x,
        y: 0.0,
        width,
        height: TITLE_H + thumb_h + PAD,
        // Centrada si la tarjeta es más ancha que la miniatura.
        thumb: (x + (width - thumb_w) / 2.0, TITLE_H, thumb_w, thumb_h),
        close: (
            x + width - PAD - CLOSE + 2.0,
            (TITLE_H - CLOSE) / 2.0,
            CLOSE,
            CLOSE,
        ),
    }
}

/// Tarjetas de las ventanas `sources` (tamaños en píxeles reales), en fila. Si no
/// caben en `max_width` se reducen todas por igual. Quedan alineadas abajo, junto
/// al dock.
pub fn layout(sources: &[(f32, f32)], scale: f32, max_width: f32) -> Layout {
    let place = |shrink: f32| {
        let mut x = 0.0;
        let cards: Vec<Card> = sources
            .iter()
            .map(|&source| {
                let card = card(source, scale, shrink, x);
                x += card.width + GAP;
                card
            })
            .collect();
        cards
    };
    let width_of = |cards: &[Card]| cards.last().map_or(0.0, |c| c.x + c.width);
    // Se encoge poco a poco hasta que la fila cabe (con un límite, por si hay
    // tantas ventanas que ni así).
    let mut shrink: f32 = 1.0;
    let mut cards = place(shrink);
    while width_of(&cards) > max_width && shrink > 0.25 {
        shrink *= 0.9;
        cards = place(shrink);
    }
    let height = cards.iter().map(|c| c.height).fold(0.0, f32::max);
    for card in &mut cards {
        let drop = height - card.height;
        card.y = drop;
        card.thumb.1 += drop;
        card.close.1 += drop;
    }
    Layout {
        width: width_of(&cards),
        height,
        cards,
    }
}

fn inside((x, y, w, h): (f32, f32, f32, f32), px: f32, py: f32) -> bool {
    px >= x && px < x + w && py >= y && py < y + h
}

impl Layout {
    /// Tarjeta bajo el cursor.
    pub fn card_at(&self, x: f32, y: f32) -> Option<usize> {
        self.cards
            .iter()
            .position(|c| inside((c.x, c.y, c.width, c.height), x, y))
    }
    /// Tarjeta cuyo botón de cerrar está bajo el cursor.
    pub fn close_at(&self, x: f32, y: f32) -> Option<usize> {
        self.cards.iter().position(|c| inside(c.close, x, y))
    }
}

/// Fondo, título y botón de cerrar de cada tarjeta; las miniaturas las pone
/// Windows encima.
pub fn draw(
    image: &mut Pixmap,
    layout: &Layout,
    titles: &[String],
    hovered: Option<usize>,
    close_hovered: Option<usize>,
    scale: f32,
) {
    image.fill(tiny_skia::Color::TRANSPARENT);
    let size = 12.0 * scale;
    let (ascent, descent) = line_metrics(size);
    for (index, card) in layout.cards.iter().enumerate() {
        let (x, y) = ((card.x + 1.0) * scale, (card.y + 1.0) * scale);
        let (w, h) = ((card.width - 2.0) * scale, (card.height - 2.0) * scale);
        // La tarjeta bajo el cursor, algo más clara, como en Windows.
        let fill = if hovered == Some(index) {
            [52, 52, 56, 248]
        } else {
            [32, 32, 34, 246]
        };
        fill_rounded(image, fill, x, y, w, h, 8.0 * scale);
        stroke_rounded(
            image,
            [255, 255, 255, 34],
            x,
            y,
            w,
            h,
            8.0 * scale,
            scale * 0.75,
        );
        let title = titles.get(index).map_or("", String::as_str);
        let room = (card.close.0 - card.x - PAD * 2.0) * scale;
        draw_text_on_baseline(
            image,
            &fit_text(title, size, room),
            (card.x + PAD + 4.0) * scale,
            (card.y + TITLE_H / 2.0) * scale + (ascent - descent) / 2.0,
            size,
            0.95,
        );
        let (cx, cy, cw, ch) = card.close;
        if close_hovered == Some(index) {
            // Rojo de cerrar, como el de las ventanas de Windows.
            fill_rounded(
                image,
                [196, 43, 28, 255],
                cx * scale,
                cy * scale,
                cw * scale,
                ch * scale,
                5.0 * scale,
            );
        }
        // La ✕ solo en la tarjeta bajo el cursor, para no recargar la fila.
        if hovered == Some(index) || layout.cards.len() == 1 {
            draw_icon_aligned(
                image,
                '\u{E8BB}',
                (cx + cw / 2.0) * scale,
                (cy + ch / 2.0) * scale,
                10.0 * scale,
                0.9,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_keeps_its_proportion_and_fits() {
        // Ventana de 1920 × 1080 a 100 %: se reduce a 240 de ancho, 16:9.
        let one = layout(&[(1920.0, 1080.0)], 1.0, 2000.0);
        let card = one.cards[0];
        assert_eq!((card.thumb.2, card.thumb.3), (240.0, 135.0));
        assert_eq!(one.width, 240.0 + PAD * 2.0);
        assert_eq!(one.height, TITLE_H + 135.0 + PAD);
        // Ventana alta: manda el alto; la tarjeta no baja del mínimo y la centra.
        let tall = layout(&[(600.0, 1200.0)], 1.0, 2000.0).cards[0];
        assert_eq!((tall.thumb.2, tall.thumb.3), (75.0, MAX_H));
        assert_eq!(tall.width, MIN_W);
        assert_eq!(tall.thumb.0, (MIN_W - 75.0) / 2.0);
        // A 150 % la ventana real mide más píxeles, pero la tarjeta es igual.
        assert_eq!(
            layout(&[(2880.0, 1620.0)], 1.5, 2000.0),
            layout(&[(1920.0, 1080.0)], 1.0, 2000.0)
        );
        // Una ventana pequeña no se agranda.
        assert_eq!(
            layout(&[(200.0, 100.0)], 1.0, 2000.0).cards[0].thumb.2,
            200.0
        );
    }

    #[test]
    fn several_windows_go_side_by_side_and_shrink_to_fit() {
        let sources = [(1920.0, 1080.0), (600.0, 1200.0), (1280.0, 720.0)];
        let row = layout(&sources, 1.0, 5000.0);
        assert_eq!(row.cards.len(), 3);
        // En fila, sin solaparse, con la separación entre ellas.
        for pair in row.cards.windows(2) {
            assert_eq!(pair[1].x, pair[0].x + pair[0].width + GAP);
        }
        assert_eq!(row.width, row.cards[2].x + row.cards[2].width);
        // Alineadas abajo: la más alta manda y las demás bajan.
        let tallest = row.cards.iter().map(|c| c.height).fold(0.0, f32::max);
        assert_eq!(row.height, tallest);
        for card in &row.cards {
            assert_eq!(card.y + card.height, row.height);
        }
        // Cada tarjeta responde en su sitio, y su ✕ también.
        let second = row.cards[1];
        assert_eq!(row.card_at(second.x + 5.0, second.y + 5.0), Some(1));
        let (cx, cy, cw, ch) = second.close;
        assert_eq!(row.close_at(cx + cw / 2.0, cy + ch / 2.0), Some(1));
        assert_eq!(
            row.card_at(row.cards[0].width + GAP / 2.0, row.height - 1.0),
            None
        );

        // Con poco sitio se reducen todas hasta caber.
        let narrow = layout(&sources, 1.0, 500.0);
        assert!(narrow.width <= 500.0 + 0.01);
        assert!(narrow.cards[0].thumb.2 < row.cards[0].thumb.2);
    }
}
