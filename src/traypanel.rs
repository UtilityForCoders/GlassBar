//! Panel propio de las apps en segundo plano (los iconos de la bandeja de los demás
//! programas), que se abre sobre la flecha del dock: una cuadrícula de iconos como
//! la de Windows 11, con el estilo de los demás paneles de GlassBar. Los iconos se
//! pueden arrastrar para cambiarlos de sitio; el orden se guarda en
//! `%LOCALAPPDATA%\GlassBar\tray-order.json`.
use crate::apps::{fill_rounded, stroke_rounded};
use crate::clock::{draw_text, text_width};
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

/// Iconos por fila, como en Windows.
const COLUMNS: usize = 5;
const CELL: f32 = 40.0;
const PAD: f32 = 8.0;
/// Lado del icono dentro de su celda (el de los iconos pequeños de Windows).
const ICON: f32 = 16.0;
const EMPTY_W: f32 = 220.0;
const EMPTY_H: f32 = 48.0;
const EMPTY_TEXT: &str = "No hay apps en segundo plano";
/// Distancia (lógica) que hay que mover el ratón con el botón pulsado para que el
/// clic pase a ser un arrastre.
const DRAG_START: f32 = 4.0;

pub struct TrayIcon {
    /// Para que el clic llegue a este icono aunque la lista cambie mientras tanto.
    pub key: crate::systray::Key,
    /// Identidad estable, con la que se guarda el orden.
    pub id: String,
    pub image: Option<Pixmap>,
    pub tip: String,
}

/// Botón pulsado sobre un icono: si el ratón se mueve lo bastante, se arrastra.
pub struct Drag {
    /// Posición actual del icono que se arrastra (cambia al pasar sobre otras celdas).
    pub index: usize,
    start: (f32, f32),
    at: (f32, f32),
    /// Ya se movió lo bastante: al soltar no hay clic, sino un orden nuevo.
    pub active: bool,
}

#[derive(Default)]
pub struct TrayPanel {
    pub icons: Vec<TrayIcon>,
    pub hovered: Option<usize>,
    pub drag: Option<Drag>,
}

impl TrayPanel {
    fn columns(&self) -> usize {
        self.icons.len().clamp(1, COLUMNS)
    }
    fn rows(&self) -> usize {
        self.icons.len().div_ceil(COLUMNS)
    }
    /// Tamaño lógico del panel.
    pub fn size(&self) -> (f32, f32) {
        if self.icons.is_empty() {
            return (EMPTY_W, EMPTY_H);
        }
        (
            PAD * 2.0 + self.columns() as f32 * CELL,
            PAD * 2.0 + self.rows() as f32 * CELL,
        )
    }
    /// Esquina superior izquierda de la celda `index`.
    pub fn cell(&self, index: usize) -> (f32, f32) {
        (
            PAD + (index % COLUMNS) as f32 * CELL,
            PAD + (index / COLUMNS) as f32 * CELL,
        )
    }
    pub fn hit_test(&self, x: f32, y: f32) -> Option<usize> {
        if x < PAD || y < PAD {
            return None;
        }
        let (column, row) = (((x - PAD) / CELL) as usize, ((y - PAD) / CELL) as usize);
        let index = row * COLUMNS + column;
        (column < self.columns() && index < self.icons.len()).then_some(index)
    }

    /// Celda más cercana a (`x`, `y`), aunque esté fuera de la cuadrícula: así el
    /// icono que se arrastra se puede llevar al principio o al final.
    fn nearest(&self, x: f32, y: f32) -> Option<usize> {
        let last = self.icons.len().checked_sub(1)?;
        let column = ((x - PAD) / CELL)
            .floor()
            .clamp(0.0, self.columns() as f32 - 1.0) as usize;
        let row = ((y - PAD) / CELL)
            .floor()
            .clamp(0.0, self.rows() as f32 - 1.0) as usize;
        Some((row * COLUMNS + column).min(last))
    }

    /// Botón izquierdo pulsado en (`x`, `y`): prepara un posible arrastre.
    pub fn press(&mut self, x: f32, y: f32) {
        self.drag = self.hit_test(x, y).map(|index| Drag {
            index,
            start: (x, y),
            at: (x, y),
            active: false,
        });
    }

    /// El ratón se movió con el botón pulsado. Devuelve si hay que redibujar.
    pub fn drag_to(&mut self, x: f32, y: f32) -> bool {
        let Some(target) = self.nearest(x, y) else {
            return false;
        };
        let Some(drag) = &mut self.drag else {
            return false;
        };
        drag.at = (x, y);
        if !drag.active {
            let (dx, dy) = (x - drag.start.0, y - drag.start.1);
            if dx * dx + dy * dy < DRAG_START * DRAG_START {
                return false;
            }
            drag.active = true;
        }
        if target != drag.index {
            let icon = self.icons.remove(drag.index);
            self.icons.insert(target, icon);
            drag.index = target;
        }
        self.hovered = Some(target);
        true
    }

    /// Botón soltado: devuelve si fue un arrastre (y no un clic).
    pub fn release(&mut self) -> bool {
        self.drag.take().is_some_and(|d| d.active)
    }

    /// Hay un icono arrastrándose.
    pub fn dragging(&self) -> bool {
        self.drag.as_ref().is_some_and(|d| d.active)
    }

    /// Identidades de los iconos, en el orden en que se ven.
    pub fn order(&self) -> Vec<String> {
        self.icons.iter().map(|i| i.id.clone()).collect()
    }

    pub fn draw(&self, image: &mut Pixmap, scale: f32) {
        image.fill(tiny_skia::Color::TRANSPARENT);
        let (w, h) = self.size();
        let (w, h) = ((w - 2.0) * scale, (h - 2.0) * scale);
        fill_rounded(image, [32, 32, 34, 246], scale, scale, w, h, 10.0 * scale);
        stroke_rounded(
            image,
            [255, 255, 255, 34],
            scale,
            scale,
            w,
            h,
            10.0 * scale,
            scale * 0.75,
        );
        if self.icons.is_empty() {
            let size = 12.0 * scale;
            draw_text(
                image,
                EMPTY_TEXT,
                (EMPTY_W * scale - text_width(EMPTY_TEXT, size)) / 2.0,
                (EMPTY_H / 2.0 - 8.0) * scale,
                size,
                0.6,
            );
            return;
        }
        let side = (ICON * scale).round().max(1.0);
        let dragging = self.drag.as_ref().filter(|d| d.active);
        for (index, icon) in self.icons.iter().enumerate() {
            let (x, y) = self.cell(index);
            let lifted = dragging.is_some_and(|d| d.index == index);
            if self.hovered == Some(index) || lifted {
                fill_rounded(
                    image,
                    [255, 255, 255, if lifted { 12 } else { 22 }],
                    (x + 2.0) * scale,
                    (y + 2.0) * scale,
                    (CELL - 4.0) * scale,
                    (CELL - 4.0) * scale,
                    6.0 * scale,
                );
            }
            // El que se arrastra va bajo el cursor; su celda queda como hueco.
            if lifted {
                continue;
            }
            if let Some(picture) = &icon.image {
                draw_icon(
                    image,
                    picture,
                    (x + CELL / 2.0) * scale,
                    (y + CELL / 2.0) * scale,
                    side,
                );
            }
        }
        if let Some(drag) = dragging {
            let (x, y) = (drag.at.0 * scale, drag.at.1 * scale);
            let box_side = (CELL - 6.0) * scale;
            let (left, top) = (x - box_side / 2.0, y - box_side / 2.0);
            fill_rounded(
                image,
                [62, 62, 66, 240],
                left,
                top,
                box_side,
                box_side,
                6.0 * scale,
            );
            stroke_rounded(
                image,
                [255, 255, 255, 40],
                left,
                top,
                box_side,
                box_side,
                6.0 * scale,
                scale * 0.75,
            );
            if let Some(picture) = &self.icons[drag.index].image {
                draw_icon(image, picture, x, y, side);
            }
        }
    }
}

/// Pinta `picture` de lado `side` centrado en (`cx`, `cy`), en píxeles.
fn draw_icon(image: &mut Pixmap, picture: &Pixmap, cx: f32, cy: f32, side: f32) {
    let left = (cx - side / 2.0).round();
    let top = (cy - side / 2.0).round();
    // Del tamaño justo se copia tal cual; si no, se escala con suavizado.
    let exact = picture.width() as f32 == side;
    let transform = Transform::from_scale(
        side / picture.width() as f32,
        side / picture.height() as f32,
    )
    .post_translate(left, top);
    image.draw_pixmap(
        0,
        0,
        picture.as_ref(),
        &PixmapPaint {
            quality: if exact {
                FilterQuality::Nearest
            } else {
                FilterQuality::Bicubic
            },
            ..Default::default()
        },
        transform,
        None,
    );
}

fn order_path() -> Option<std::path::PathBuf> {
    Some(
        std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("GlassBar")
            .join("tray-order.json"),
    )
}

/// Orden guardado de los iconos (sus identidades); vacío si no hay.
pub fn load_order() -> Vec<String> {
    order_path()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Guarda `shown` (el orden actual del panel) seguido de los iconos guardados que
/// ahora no están, para que un programa cerrado recupere su sitio al volver.
pub fn save_order(shown: &[String]) -> std::io::Result<()> {
    let path = order_path().ok_or_else(|| std::io::Error::other("LOCALAPPDATA no disponible"))?;
    let order = merge_order(shown, &load_order());
    std::fs::create_dir_all(path.parent().unwrap())?;
    let pending = path.with_extension("tmp");
    std::fs::write(&pending, serde_json::to_vec_pretty(&order)?)?;
    std::fs::rename(pending, path)
}

fn merge_order(shown: &[String], saved: &[String]) -> Vec<String> {
    let mut order = shown.to_vec();
    order.extend(saved.iter().filter(|id| !shown.contains(id)).cloned());
    order
}

/// Ordena `icons` según `order`. Los que no aparecen van al final, en el orden en
/// que llegaron (`sort_by_key` es estable).
pub fn sort_icons(icons: &mut [TrayIcon], order: &[String]) {
    icons.sort_by_key(|icon| {
        order
            .iter()
            .position(|id| *id == icon.id)
            .unwrap_or(usize::MAX)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(count: usize) -> TrayPanel {
        TrayPanel {
            icons: (0..count)
                .map(|i| TrayIcon {
                    key: Default::default(),
                    id: format!("app{i}"),
                    image: None,
                    tip: format!("App {i}"),
                })
                .collect(),
            hovered: None,
            drag: None,
        }
    }

    fn ids(panel: &TrayPanel) -> Vec<&str> {
        panel.icons.iter().map(|i| i.id.as_str()).collect()
    }

    #[test]
    fn icons_fill_rows_of_five_like_windows() {
        // 15 iconos: tres filas de cinco, como en la captura del panel de Windows.
        let full = panel(15);
        assert_eq!(
            full.size(),
            (PAD * 2.0 + 5.0 * CELL, PAD * 2.0 + 3.0 * CELL)
        );
        // Pocos iconos: el panel solo es tan ancho como ellos.
        assert_eq!(panel(2).size(), (PAD * 2.0 + 2.0 * CELL, PAD * 2.0 + CELL));
        assert_eq!(panel(0).size(), (EMPTY_W, EMPTY_H));

        // Cada celda responde a su icono; los huecos de la última fila, no.
        let partial = panel(7);
        for index in 0..7 {
            let (x, y) = partial.cell(index);
            assert_eq!(
                partial.hit_test(x + CELL / 2.0, y + CELL / 2.0),
                Some(index)
            );
        }
        let (x, y) = partial.cell(8);
        assert_eq!(partial.hit_test(x + CELL / 2.0, y + CELL / 2.0), None);
        assert_eq!(partial.hit_test(PAD / 2.0, PAD / 2.0), None);
    }

    #[test]
    fn dragging_an_icon_moves_it_and_a_short_press_stays_a_click() {
        let mut p = panel(7);
        // Un temblor de un par de píxeles no es arrastre: al soltar es un clic.
        let (x, y) = p.cell(0);
        p.press(x + 20.0, y + 20.0);
        assert!(!p.drag_to(x + 22.0, y + 21.0));
        assert!(!p.release());
        assert_eq!(ids(&p)[0], "app0");

        // El primero, a la tercera celda: los de en medio se corren.
        p.press(x + 20.0, y + 20.0);
        let (tx, ty) = p.cell(2);
        assert!(p.drag_to(tx + 20.0, ty + 20.0));
        assert_eq!(
            ids(&p),
            ["app1", "app2", "app0", "app3", "app4", "app5", "app6"]
        );
        // Más allá del último icono (hueco de la segunda fila): queda el último.
        let (bx, by) = p.cell(5);
        p.drag_to(bx + 200.0, by + 60.0);
        assert_eq!(ids(&p).last(), Some(&"app0"));
        assert!(p.release());
        assert!(p.drag.is_none());

        // Fuera del panel, por arriba a la izquierda: va al principio.
        let (lx, ly) = p.cell(6);
        p.press(lx + 20.0, ly + 20.0);
        p.drag_to(-30.0, -30.0);
        assert_eq!(ids(&p)[0], "app0");
    }

    #[test]
    fn saved_order_sorts_known_icons_and_keeps_new_ones_last() {
        let mut p = panel(4);
        let order: Vec<String> = ["app3", "gone", "app1"].map(String::from).to_vec();
        sort_icons(&mut p.icons, &order);
        assert_eq!(ids(&p), ["app3", "app1", "app0", "app2"]);
        // Al guardar se conservan detrás los que ahora no están.
        assert_eq!(
            merge_order(&p.order(), &order),
            ["app3", "app1", "app0", "app2", "gone"]
        );
    }
}
