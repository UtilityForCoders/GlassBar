//! Centro de notificaciones: la lista de los avisos que han llegado, agrupados por
//! app, en el panel de la hora, junto al calendario (como en Windows 11).
//!
//! Windows no deja leer su propio historial sin identidad de paquete, así que
//! GlassBar guarda el suyo con lo que ve el vigilante de `notifications`: cada aviso
//! nuevo que sale en pantalla entra aquí, con la hora a la que llegó. Mientras
//! Windows lo sigue enseñando, pulsarlo pulsa el aviso original; después, abre la
//! app que lo envió. El historial vive en memoria: se pierde al cerrar GlassBar.
use crate::apps::{fill_rounded, stroke_rounded};
use crate::clock::{
    draw_icon_aligned, draw_text_on_baseline, draw_text_semibold_on_baseline, fit_text,
    line_metrics, text_width,
};
use crate::notifications::{keys, Key, Stack, Toast};
use std::time::{Duration, Instant, SystemTime};
use tiny_skia::Pixmap;

pub const WIDTH: f32 = crate::calendar::WIDTH;
/// Alto más grande del panel; con más avisos, la lista se desplaza.
pub const MAX_HEIGHT: f32 = 420.0;
/// Avisos que se guardan como mucho; los más viejos se van.
const MAX_ENTRIES: usize = 50;
/// Un aviso igual a otro que Windows quitó hace menos de esto es el mismo, que
/// dejó de leerse un momento, no uno nuevo.
const REPEAT: Duration = Duration::from_secs(3);

const PAD: f32 = 12.0;
const RADIUS: f32 = 12.0;
const HEADER_H: f32 = 44.0;
const HEADER_SIZE: f32 = 14.0;
const EMPTY_H: f32 = 64.0;
const GROUP_H: f32 = 28.0;
const GROUP_SIZE: f32 = 12.0;
const GROUP_GAP: f32 = 6.0;
const CARD_W: f32 = WIDTH - PAD * 2.0;
const CARD_PAD: f32 = 10.0;
const CARD_RADIUS: f32 = 10.0;
const CARD_GAP: f32 = 6.0;
const ICON: f32 = 24.0;
const TEXT_X: f32 = CARD_PAD + ICON + 10.0;
const TEXT_W: f32 = CARD_W - TEXT_X - CARD_PAD;
const TITLE_SIZE: f32 = 12.5;
const BODY_SIZE: f32 = 12.0;
const TIME_SIZE: f32 = 11.0;
const LINE: f32 = 16.0;
const BODY_LINES: usize = 2;
/// Botón de cerrar de una tarjeta o un grupo.
const CLOSE_R: f32 = 9.0;
/// "Borrar todo" y No molestar, en la cabecera.
const BUTTON_H: f32 = 28.0;
const BUTTON_SIZE: f32 = 12.5;
const DND_R: f32 = 14.0;
const CLEAR_ALL: &str = "Borrar todo";
const TITLE: &str = "Notificaciones";
const EMPTY: &str = "No hay notificaciones nuevas";
const GLYPH_CLOSE: char = '\u{E8BB}';
/// Luna de "No molestar".
const GLYPH_DND: char = '\u{E708}';
/// Lo que mueve la lista cada paso de la rueda.
pub const WHEEL_STEP: f32 = 48.0;

/// Parte de una tarjeta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Body,
    Close,
}

/// Lo que hay bajo el cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    ClearAll,
    DoNotDisturb,
    /// Cabecera del grupo de una app; `true` en su botón de cerrar.
    Group(String, bool),
    /// Tarjeta, por su `id`.
    Entry(u64, Part),
    /// El resto del panel.
    Panel,
}

/// Un aviso guardado.
pub struct Entry {
    pub id: u64,
    pub key: Key,
    pub toast: Toast,
    /// Cuándo llegó.
    pub at: SystemTime,
    /// La última vez que Windows lo enseñaba.
    last_live: Instant,
    /// Texto ya cortado al ancho.
    lines: Vec<String>,
    height: f32,
}

impl Entry {
    fn new(id: u64, key: Key, toast: Toast) -> Self {
        let lines = wrap(&toast.body, BODY_SIZE, TEXT_W, BODY_LINES);
        let height = entry_height(&toast, lines.len());
        Self {
            id,
            key,
            toast,
            at: SystemTime::now(),
            last_live: Instant::now(),
            lines,
            height,
        }
    }
}

fn entry_height(toast: &Toast, body_lines: usize) -> f32 {
    let title = if toast.title.is_empty() && body_lines > 0 {
        0.0
    } else {
        LINE
    };
    let text = CARD_PAD + title + LINE * body_lines as f32 + CARD_PAD;
    text.max(CARD_PAD * 2.0 + ICON)
}

/// Fila de la lista, con su borde superior en la lista completa.
#[derive(Clone, Debug, PartialEq)]
enum Row {
    Group { sender: String, top: f32 },
    Card { index: usize, top: f32 },
}

/// Lo que sale en el centro de notificaciones.
#[derive(Default)]
pub struct Center {
    /// El más nuevo primero.
    entries: Vec<Entry>,
    /// Los avisos que Windows enseña ahora.
    live: Vec<Key>,
    next_id: u64,
    /// Avisos que llegaron desde la última vez que se abrió el panel.
    pub unread: usize,
    pub hovered: Option<Hit>,
    scroll: f32,
    /// Alto del panel, ya ajustado al sitio que hay (ver `fit`).
    height: f32,
}

impl Center {
    /// Apunta los avisos que hay ahora en pantalla. Devuelve las apps de los que
    /// llegaron nuevos.
    pub fn record(&mut self, toasts: &[Toast]) -> Vec<String> {
        let now = Instant::now();
        let keys = keys(toasts);
        let mut senders = Vec::new();
        // Los de arriba son los más nuevos: se meten al final para que queden primero.
        for (key, toast) in keys.iter().zip(toasts).rev() {
            if let Some(entry) = self.entries.iter_mut().find(|e| {
                &e.key == key
                    && (self.live.contains(key) || now.duration_since(e.last_live) < REPEAT)
            }) {
                entry.last_live = now;
                continue;
            }
            if self.live.contains(key) {
                // Ya se quitó del historial, pero Windows aún lo enseña.
                continue;
            }
            self.entries
                .insert(0, Entry::new(self.next_id, key.clone(), toast.clone()));
            self.next_id += 1;
            self.unread += 1;
            if !senders.contains(&toast.sender) {
                senders.push(toast.sender.clone());
            }
        }
        self.entries.truncate(MAX_ENTRIES);
        self.live = keys;
        senders
    }

    pub fn entry(&self, id: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Windows aún enseña el aviso de `key`.
    pub fn is_live(&self, key: &Key) -> bool {
        self.live.contains(key)
    }

    /// Quita los avisos que cumplan `pick`. Devuelve los que Windows aún enseña,
    /// para quitarlos también allí.
    fn remove_where(&mut self, pick: impl Fn(&Entry) -> bool) -> Vec<Key> {
        let mut live = Vec::new();
        self.entries.retain(|e| {
            if !pick(e) {
                return true;
            }
            if self.live.contains(&e.key) {
                live.push(e.key.clone());
            }
            false
        });
        if matches!(&self.hovered, Some(Hit::Entry(id, _)) if self.entry(*id).is_none()) {
            self.hovered = None;
        }
        live
    }

    pub fn remove(&mut self, id: u64) -> Vec<Key> {
        self.remove_where(|e| e.id == id)
    }

    pub fn remove_sender(&mut self, sender: &str) -> Vec<Key> {
        self.remove_where(|e| e.toast.sender == sender)
    }

    pub fn clear(&mut self) -> Vec<Key> {
        self.hovered = None;
        self.remove_where(|_| true)
    }

    /// Se pulsó la tarjeta de `key` en pantalla: ya está atendido.
    pub fn forget(&mut self, key: &Key) {
        self.remove_where(|e| &e.key == key);
    }

    /// Filas de la lista y su alto total.
    fn rows(&self) -> (Vec<Row>, f32) {
        let mut senders: Vec<&str> = Vec::new();
        for entry in &self.entries {
            if !senders.contains(&entry.toast.sender.as_str()) {
                senders.push(&entry.toast.sender);
            }
        }
        let mut rows = Vec::new();
        let mut top = 0.0;
        for (n, sender) in senders.into_iter().enumerate() {
            if n > 0 {
                top += GROUP_GAP;
            }
            rows.push(Row::Group {
                sender: sender.to_string(),
                top,
            });
            top += GROUP_H;
            for (index, entry) in self.entries.iter().enumerate() {
                if entry.toast.sender == sender {
                    rows.push(Row::Card { index, top });
                    top += entry.height + CARD_GAP;
                }
            }
        }
        (rows, top)
    }

    fn content_height(&self) -> f32 {
        if self.entries.is_empty() {
            EMPTY_H
        } else {
            self.rows().1 + PAD - CARD_GAP
        }
    }

    /// Ajusta el alto del panel a `room` (unidades lógicas) como mucho y lo
    /// devuelve.
    pub fn fit(&mut self, room: f32) -> f32 {
        let max = MAX_HEIGHT.min(room).max(HEADER_H + EMPTY_H);
        self.height = (HEADER_H + self.content_height()).min(max);
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        self.height
    }

    fn view_height(&self) -> f32 {
        (self.height - HEADER_H).max(0.0)
    }

    fn max_scroll(&self) -> f32 {
        (self.content_height() - self.view_height()).max(0.0)
    }

    /// Desplaza la lista. Devuelve si se movió.
    pub fn scroll_by(&mut self, delta: f32) -> bool {
        let before = self.scroll;
        self.scroll = (self.scroll + delta).clamp(0.0, self.max_scroll());
        self.scroll != before
    }

    /// Bordes de "Borrar todo" y centro de No molestar, en la cabecera.
    fn clear_button(&self) -> Option<(f32, f32)> {
        if self.entries.is_empty() {
            return None;
        }
        let width = text_width(CLEAR_ALL, BUTTON_SIZE) + 16.0;
        Some((WIDTH - PAD - width, width))
    }

    fn dnd_center(&self) -> (f32, f32) {
        let right = self
            .clear_button()
            .map_or(WIDTH - PAD, |(left, _)| left - 6.0);
        (right - DND_R, HEADER_H / 2.0)
    }

    /// Qué hay en `(x, y)`, en unidades lógicas del panel.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<Hit> {
        if !(0.0..WIDTH).contains(&x) || !(0.0..self.height).contains(&y) {
            return None;
        }
        if y < HEADER_H {
            if let Some((left, width)) = self.clear_button() {
                if (left..left + width).contains(&x) && (y - HEADER_H / 2.0).abs() <= BUTTON_H / 2.0
                {
                    return Some(Hit::ClearAll);
                }
            }
            let (cx, cy) = self.dnd_center();
            if (x - cx).powi(2) + (y - cy).powi(2) <= DND_R.powi(2) {
                return Some(Hit::DoNotDisturb);
            }
            return Some(Hit::Panel);
        }
        let at = y - HEADER_H + self.scroll;
        let (rows, _) = self.rows();
        for row in rows {
            match row {
                Row::Group { sender, top } if (top..top + GROUP_H).contains(&at) => {
                    let (cx, cy) = (WIDTH - PAD - CLOSE_R, top + GROUP_H / 2.0);
                    let close = (x - cx).powi(2) + (at - cy).powi(2) <= (CLOSE_R + 2.0).powi(2);
                    return Some(Hit::Group(sender, close));
                }
                Row::Card { index, top } => {
                    let entry = &self.entries[index];
                    if !(top..top + entry.height).contains(&at) || !(PAD..PAD + CARD_W).contains(&x)
                    {
                        continue;
                    }
                    let (cx, cy) = close_center(top);
                    let close = (x - cx).powi(2) + (at - cy).powi(2) <= (CLOSE_R + 2.0).powi(2);
                    let part = if close { Part::Close } else { Part::Body };
                    return Some(Hit::Entry(entry.id, part));
                }
                _ => {}
            }
        }
        Some(Hit::Panel)
    }

    /// Dibuja el panel en `image`, que mide lo mismo que él; los iconos salen de los
    /// que ya tienen las tarjetas (`icons`).
    pub fn draw(&self, image: &mut Pixmap, scale: f32, icons: &Stack, dnd: bool) {
        image.fill(tiny_skia::Color::TRANSPARENT);
        let s = |v: f32| v * scale;
        let (w, h) = (s(WIDTH - 2.0), s(self.height - 2.0));
        fill_rounded(image, crate::glass::panel(), scale, scale, w, h, s(RADIUS));
        fill_rounded(
            image,
            [255, 255, 255, 20],
            scale,
            s(HEADER_H).round(),
            w,
            scale.max(1.0),
            0.0,
        );
        stroke_rounded(
            image,
            [255, 255, 255, 34],
            scale,
            scale,
            w,
            h,
            s(RADIUS),
            scale * 0.75,
        );
        self.draw_header(image, scale, dnd);

        // La lista, en su propia imagen: lo que se sale al desplazarla queda fuera.
        let view = self.view_height();
        let (vw, vh) = (
            (s(WIDTH)).ceil() as u32,
            (s(view) - scale * 2.0).floor().max(1.0) as u32,
        );
        let Some(mut list) = Pixmap::new(vw, vh) else {
            return;
        };
        if self.entries.is_empty() {
            let size = s(13.0);
            let (ascent, descent) = line_metrics(size);
            let width = text_width(EMPTY, size);
            draw_text_on_baseline(
                &mut list,
                EMPTY,
                (s(WIDTH) - width) / 2.0,
                s(EMPTY_H / 2.0) + (ascent - descent) / 2.0,
                size,
                0.6,
            );
        } else {
            let (rows, _) = self.rows();
            for row in rows {
                match row {
                    Row::Group { sender, top } => {
                        let y = top - self.scroll;
                        if y + GROUP_H >= 0.0 && y <= view {
                            self.draw_group(&mut list, &sender, y, scale);
                        }
                    }
                    Row::Card { index, top } => {
                        let entry = &self.entries[index];
                        let y = top - self.scroll;
                        if y + entry.height >= 0.0 && y <= view {
                            self.draw_entry(&mut list, entry, y, scale, icons);
                        }
                    }
                }
            }
            // Barra de desplazamiento, si no cabe todo.
            let max = self.max_scroll();
            if max > 0.0 {
                let total = self.content_height();
                let bar = (view * view / total).max(24.0);
                let y = (view - bar) * self.scroll / max;
                fill_rounded(
                    &mut list,
                    [255, 255, 255, 60],
                    s(WIDTH - 5.0),
                    s(y + 2.0),
                    s(3.0),
                    s(bar - 4.0),
                    s(1.5),
                );
            }
        }
        image.draw_pixmap(
            0,
            s(HEADER_H).round() as i32 + scale.max(1.0).round() as i32,
            list.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::identity(),
            None,
        );
    }

    fn draw_header(&self, image: &mut Pixmap, scale: f32, dnd: bool) {
        let s = |v: f32| v * scale;
        let cy = HEADER_H / 2.0;
        let size = s(HEADER_SIZE);
        let (ascent, descent) = line_metrics(size);
        draw_text_semibold_on_baseline(
            image,
            TITLE,
            s(PAD + 4.0),
            s(cy) + (ascent - descent) / 2.0,
            size,
            1.0,
        );
        if let Some((left, width)) = self.clear_button() {
            if self.hovered == Some(Hit::ClearAll) {
                fill_rounded(
                    image,
                    [255, 255, 255, 22],
                    s(left),
                    s(cy - BUTTON_H / 2.0),
                    s(width),
                    s(BUTTON_H),
                    s(6.0),
                );
            }
            let size = s(BUTTON_SIZE);
            let (ascent, descent) = line_metrics(size);
            draw_text_on_baseline(
                image,
                CLEAR_ALL,
                s(left + 8.0),
                s(cy) + (ascent - descent) / 2.0,
                size,
                0.9,
            );
        }
        let (cx, cy) = self.dnd_center();
        let over = self.hovered == Some(Hit::DoNotDisturb);
        let fill = if dnd {
            let accent = crate::accent::current();
            if over {
                accent.base
            } else {
                accent.dark
            }
        } else {
            [255, 255, 255, if over { 30 } else { 0 }]
        };
        if fill[3] > 0 {
            fill_rounded(
                image,
                fill,
                s(cx - DND_R),
                s(cy - DND_R),
                s(DND_R * 2.0),
                s(DND_R * 2.0),
                s(DND_R),
            );
        }
        draw_icon_aligned(
            image,
            GLYPH_DND,
            s(cx),
            s(cy),
            s(13.0),
            if dnd || over { 1.0 } else { 0.75 },
        );
    }

    fn draw_group(&self, image: &mut Pixmap, sender: &str, y: f32, scale: f32) {
        let s = |v: f32| v * scale;
        let size = s(GROUP_SIZE);
        let (ascent, descent) = line_metrics(size);
        let cy = y + GROUP_H / 2.0;
        let name = fit_text(sender, size, s(CARD_W - CLOSE_R * 2.0 - 12.0));
        draw_text_semibold_on_baseline(
            image,
            &name,
            s(PAD + 4.0),
            s(cy) + (ascent - descent) / 2.0,
            size,
            0.75,
        );
        let hovered = match &self.hovered {
            Some(Hit::Group(name, close)) if name == sender => Some(*close),
            _ => None,
        };
        if let Some(over) = hovered {
            draw_close(image, WIDTH - PAD - CLOSE_R, cy, over, scale);
        }
    }

    fn draw_entry(&self, image: &mut Pixmap, entry: &Entry, y: f32, scale: f32, icons: &Stack) {
        let s = |v: f32| v * scale;
        let hovered = match self.hovered {
            Some(Hit::Entry(id, part)) if id == entry.id => Some(part),
            _ => None,
        };
        let x = PAD;
        fill_rounded(
            image,
            [255, 255, 255, if hovered.is_some() { 22 } else { 12 }],
            s(x),
            s(y),
            s(CARD_W),
            s(entry.height),
            s(CARD_RADIUS),
        );
        icons.draw_icon(
            image,
            &entry.toast.sender,
            s(x + CARD_PAD),
            s(y + CARD_PAD),
            s(ICON),
        );

        // Título (o la primera línea del texto) y la hora a la derecha; al pasar el
        // cursor, la hora deja su sitio al botón de cerrar.
        let tx = x + TEXT_X;
        let time = ago(entry.at.elapsed().unwrap_or_default());
        let time_w = text_width(&time, s(TIME_SIZE));
        let (mut lines, title) = if entry.toast.title.is_empty() {
            let mut lines = entry.lines.iter();
            (
                lines.clone().skip(1).collect::<Vec<_>>(),
                lines.next().cloned(),
            )
        } else {
            (
                entry.lines.iter().collect(),
                Some(entry.toast.title.clone()),
            )
        };
        let first = y + CARD_PAD;
        if let Some(title) = title {
            let size = s(TITLE_SIZE);
            let (ascent, descent) = line_metrics(size);
            let room = s(TEXT_W) - time_w.max(s(CLOSE_R * 2.0)) - s(6.0);
            let title = fit_text(&title, size, room);
            let baseline = s(first + LINE / 2.0) + (ascent - descent) / 2.0;
            if entry.toast.title.is_empty() {
                draw_text_on_baseline(image, &title, s(tx), baseline, s(BODY_SIZE), 0.85);
            } else {
                draw_text_semibold_on_baseline(image, &title, s(tx), baseline, size, 1.0);
            }
        }
        let (cx, cy) = close_center(y);
        if let Some(part) = hovered {
            draw_close(image, cx, cy, part == Part::Close, scale);
        } else {
            let size = s(TIME_SIZE);
            let (ascent, descent) = line_metrics(size);
            draw_text_on_baseline(
                image,
                &time,
                s(x + CARD_W - CARD_PAD) - time_w,
                s(cy) + (ascent - descent) / 2.0,
                size,
                0.5,
            );
        }
        let size = s(BODY_SIZE);
        let (ascent, descent) = line_metrics(size);
        let mut top = first + LINE;
        for line in lines.drain(..) {
            draw_text_on_baseline(
                image,
                line,
                s(tx),
                s(top + LINE / 2.0) + (ascent - descent) / 2.0,
                size,
                0.8,
            );
            top += LINE;
        }
    }
}

/// Centro del botón de cerrar de la tarjeta que empieza en `top`.
fn close_center(top: f32) -> (f32, f32) {
    (
        PAD + CARD_W - CARD_PAD - CLOSE_R + 2.0,
        top + CARD_PAD + LINE / 2.0,
    )
}

fn draw_close(image: &mut Pixmap, cx: f32, cy: f32, over: bool, scale: f32) {
    let s = |v: f32| v * scale;
    fill_rounded(
        image,
        [255, 255, 255, if over { 44 } else { 22 }],
        s(cx - CLOSE_R),
        s(cy - CLOSE_R),
        s(CLOSE_R * 2.0),
        s(CLOSE_R * 2.0),
        s(CLOSE_R),
    );
    draw_icon_aligned(image, GLYPH_CLOSE, s(cx), s(cy), s(7.5), 0.95);
}

/// Cuánto hace que llegó un aviso, corto: "ahora", "5 min", "2 h", "3 d".
pub fn ago(elapsed: Duration) -> String {
    let minutes = elapsed.as_secs() / 60;
    match minutes {
        0 => "ahora".into(),
        1..=59 => format!("{minutes} min"),
        60..=1439 => format!("{} h", minutes / 60),
        _ => format!("{} d", minutes / 1440),
    }
}

/// Corta `text` en líneas de `width` como mucho; la última, si no cabe todo, acaba
/// en "…".
fn wrap(text: &str, size: f32, width: f32, max_lines: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut lines = Vec::new();
    let mut at = 0;
    while at < words.len() && lines.len() < max_lines {
        if lines.len() + 1 == max_lines {
            lines.push(fit_text(&words[at..].join(" "), size, width));
            break;
        }
        let mut line = words[at].to_string();
        at += 1;
        while at < words.len() {
            let longer = format!("{line} {}", words[at]);
            if text_width(&longer, size) > width {
                break;
            }
            line = longer;
            at += 1;
        }
        lines.push(fit_text(&line, size, width));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toast(sender: &str, title: &str, body: &str) -> Toast {
        Toast {
            sender: sender.into(),
            title: title.into(),
            body: body.into(),
            actions: Vec::new(),
            attribution: String::new(),
        }
    }

    #[test]
    fn each_toast_is_recorded_once_while_windows_shows_it() {
        let mut center = Center::default();
        let ana = toast("Chat", "Ana", "Hola");
        assert_eq!(center.record(std::slice::from_ref(&ana)), vec!["Chat"]);
        assert!(center.record(std::slice::from_ref(&ana)).is_empty());
        let mail = toast("Correo", "Factura", "Ya está");
        assert_eq!(center.record(&[mail, ana]), vec!["Correo"]);
        assert_eq!(center.entries.len(), 2);
        assert_eq!(center.unread, 2);
        // El más nuevo, primero.
        assert_eq!(center.entries[0].toast.sender, "Correo");
    }

    #[test]
    fn a_toast_that_vanishes_for_a_moment_is_not_recorded_again() {
        let mut center = Center::default();
        let ana = toast("Chat", "Ana", "Hola");
        center.record(std::slice::from_ref(&ana));
        center.record(&[]);
        center.record(std::slice::from_ref(&ana));
        assert_eq!(center.entries.len(), 1);
        // Pasado un rato, el mismo texto es un aviso nuevo.
        center.entries[0].last_live -= REPEAT * 2;
        center.record(&[]);
        center.record(std::slice::from_ref(&ana));
        assert_eq!(center.entries.len(), 2);
    }

    #[test]
    fn removing_returns_what_windows_still_shows() {
        let mut center = Center::default();
        center.record(&[toast("Chat", "Ana", "Hola")]);
        center.record(&[]);
        center.record(&[toast("Chat", "Luis", "Adiós"), toast("Correo", "Ya", "")]);
        assert_eq!(center.remove_sender("Chat").len(), 1);
        assert_eq!(center.entries.len(), 1);
        // Quitado del historial, no vuelve aunque Windows lo siga enseñando.
        let left = center.clear();
        assert_eq!(left.len(), 1);
        center.record(&[toast("Chat", "Luis", "Adiós"), toast("Correo", "Ya", "")]);
        assert!(center.entries.is_empty());
    }

    #[test]
    fn the_history_keeps_only_the_newest() {
        let mut center = Center::default();
        for i in 0..MAX_ENTRIES + 5 {
            center.record(&[toast("Chat", &format!("Mensaje {i}"), "")]);
        }
        assert_eq!(center.entries.len(), MAX_ENTRIES);
        assert_eq!(
            center.entries[0].toast.title,
            format!("Mensaje {}", MAX_ENTRIES + 4)
        );
    }

    #[test]
    fn entries_are_grouped_by_app_newest_group_first() {
        let mut center = Center::default();
        center.record(&[toast("Chat", "1", "")]);
        center.record(&[toast("Correo", "2", "")]);
        center.record(&[toast("Chat", "3", "")]);
        let (rows, _) = center.rows();
        let groups: Vec<_> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Group { sender, .. } => Some(sender.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(groups, ["Chat", "Correo"]);
        assert_eq!(rows.len(), 5);
    }

    #[test]
    fn the_panel_fits_the_room_and_scrolls() {
        let mut center = Center::default();
        assert_eq!(center.fit(1000.0), HEADER_H + EMPTY_H);
        for i in 0..20 {
            center.record(&[toast("Chat", &format!("Mensaje {i}"), "Texto")]);
        }
        let height = center.fit(300.0);
        assert_eq!(height, 300.0);
        assert!(center.scroll_by(100.0));
        assert!(!center.scroll_by(-1000.0) || center.scroll == 0.0);
        assert_eq!(center.scroll, 0.0);
        assert!(!center.scroll_by(-10.0));
    }

    #[test]
    fn hit_test_finds_cards_and_buttons() {
        let mut center = Center::default();
        center.record(&[toast("Chat", "Ana", "Hola")]);
        center.fit(1000.0);
        let id = center.entries[0].id;
        assert_eq!(
            center.hit_test(WIDTH / 2.0, HEADER_H + GROUP_H + 20.0),
            Some(Hit::Entry(id, Part::Body))
        );
        let (cx, cy) = close_center(GROUP_H);
        assert_eq!(
            center.hit_test(cx, HEADER_H + cy),
            Some(Hit::Entry(id, Part::Close))
        );
        let (dx, dy) = center.dnd_center();
        assert_eq!(center.hit_test(dx, dy), Some(Hit::DoNotDisturb));
        assert_eq!(
            center.hit_test(WIDTH - PAD - 4.0, HEADER_H / 2.0),
            Some(Hit::ClearAll)
        );
        assert_eq!(center.hit_test(-1.0, 10.0), None);
    }

    #[test]
    fn elapsed_time_is_short() {
        assert_eq!(ago(Duration::from_secs(20)), "ahora");
        assert_eq!(ago(Duration::from_secs(5 * 60)), "5 min");
        assert_eq!(ago(Duration::from_secs(3 * 3600)), "3 h");
        assert_eq!(ago(Duration::from_secs(2 * 86400)), "2 d");
    }
}
