//! Campo de texto de una línea con el comportamiento de los cuadros de texto de
//! Windows: cursor, selección con Mayús o con el ratón (clic, arrastre, doble y
//! triple clic), saltos y borrado por palabras con Ctrl, y deshacer.
//! Las posiciones cuentan caracteres, no bytes, para no partir letras como "ñ".
use std::time::{Duration, Instant};

/// Longitud máxima, en caracteres.
const MAX_LEN: usize = 100;
/// Pasos que se pueden deshacer.
const MAX_UNDO: usize = 100;
/// Distancia máxima, en píxeles lógicos, entre dos clics que cuentan como doble.
const DOUBLE_CLICK_SLOP: f32 = 4.0;

#[derive(Default)]
pub struct TextField {
    text: String,
    caret: usize,
    /// Extremo fijo de la selección; la selección va de aquí al cursor.
    anchor: Option<usize>,
    /// Estados anteriores (texto y cursor) para Ctrl+Z, y los deshechos para Ctrl+Y.
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
    /// Última edición: escribir o borrar letra a letra se deshace de una vez.
    last_edit: Option<Edit>,
    /// Última pulsación del ratón (momento, x y cuántas seguidas), para el doble
    /// y el triple clic.
    last_press: Option<(Instant, f32, u8)>,
    /// Arrastre en curso tras pulsar el botón del ratón.
    drag: Option<Drag>,
}

/// Ediciones que se agrupan al deshacer si se repiten seguidas.
#[derive(Clone, Copy, PartialEq)]
enum Edit {
    Type,
    Erase,
}

/// Cómo amplía la selección el arrastre: letra a letra tras un clic, palabra a
/// palabra tras un doble clic (la palabra inicial siempre queda dentro).
#[derive(Clone, Copy)]
enum Drag {
    Chars,
    Words(usize, usize),
}

/// Clase de carácter para los saltos por palabra: un salto recorre una sola clase.
#[derive(PartialEq)]
enum Class {
    Space,
    Word,
    Symbol,
}
fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Symbol
    }
}

impl TextField {
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn caret(&self) -> usize {
        self.caret
    }
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
    fn len(&self) -> usize {
        self.text.chars().count()
    }
    fn byte_at(&self, index: usize) -> usize {
        self.text
            .char_indices()
            .nth(index)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }

    /// Rango seleccionado `(inicio, fin)`, solo si no está vacío.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        (anchor != self.caret).then(|| (anchor.min(self.caret), anchor.max(self.caret)))
    }
    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        Some(self.text[self.byte_at(start)..self.byte_at(end)].to_string())
    }
    pub fn select_all(&mut self) {
        self.last_edit = None;
        self.anchor = Some(0);
        self.caret = self.len();
    }
    /// Vacía el campo y olvida lo que se podía deshacer.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Guarda el estado actual antes de cambiar el texto. Las ediciones del mismo
    /// tipo seguidas (`kind`) comparten un único paso de deshacer.
    fn save(&mut self, kind: Option<Edit>) {
        if kind.is_none() || kind != self.last_edit {
            self.undo.push((self.text.clone(), self.caret));
            if self.undo.len() > MAX_UNDO {
                self.undo.remove(0);
            }
        }
        self.last_edit = kind;
        self.redo.clear();
    }
    /// Ctrl+Z: vuelve al texto de antes de la última edición. Devuelve si había algo.
    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        let current = self.restore(previous);
        self.redo.push(current);
        true
    }
    /// Ctrl+Y: rehace lo último deshecho. Devuelve si había algo.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        let current = self.restore(next);
        self.undo.push(current);
        true
    }
    fn restore(&mut self, (text, caret): (String, usize)) -> (String, usize) {
        let current = (std::mem::replace(&mut self.text, text), self.caret);
        self.caret = caret.min(self.len());
        self.anchor = None;
        self.last_edit = None;
        current
    }

    /// Borra la selección, si la hay, y deja el cursor en su lugar.
    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        let range = self.byte_at(start)..self.byte_at(end);
        self.text.replace_range(range, "");
        self.caret = start;
        self.anchor = None;
        true
    }
    fn delete_range(&mut self, start: usize, end: usize) {
        let range = self.byte_at(start)..self.byte_at(end);
        self.text.replace_range(range, "");
        self.caret = start;
    }

    /// Escribe o pega texto en el cursor, sustituyendo la selección. Los saltos de
    /// línea y otros caracteres de control se descartan.
    pub fn insert(&mut self, text: &str) {
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        let selected = self.selection().is_some();
        if text.is_empty() && !selected {
            return;
        }
        // Una letra suelta se agrupa con las siguientes; pegar es un paso propio.
        let typing = text.chars().count() == 1 && !selected;
        self.save(typing.then_some(Edit::Type));
        self.delete_selection();
        let text: String = text
            .chars()
            .take(MAX_LEN.saturating_sub(self.len()))
            .collect();
        let at = self.byte_at(self.caret);
        self.text.insert_str(at, &text);
        self.caret += text.chars().count();
    }
    pub fn cut(&mut self) -> Option<String> {
        let text = self.selected_text()?;
        self.save(None);
        self.delete_selection();
        Some(text)
    }

    /// Retroceso; con Ctrl borra hasta el principio de la palabra.
    pub fn backspace(&mut self, word: bool) {
        self.save_erase(word, self.caret == 0);
        if self.delete_selection() || self.caret == 0 {
            return;
        }
        let start = if word {
            self.word_start(self.caret)
        } else {
            self.caret - 1
        };
        self.delete_range(start, self.caret);
    }
    /// Suprimir; con Ctrl borra hasta el principio de la palabra siguiente.
    pub fn delete(&mut self, word: bool) {
        self.save_erase(word, self.caret == self.len());
        if self.delete_selection() || self.caret == self.len() {
            return;
        }
        let end = if word {
            self.next_word(self.caret)
        } else {
            self.caret + 1
        };
        let caret = self.caret;
        self.delete_range(caret, end);
    }

    /// Guarda el estado antes de borrar, si hay algo que borrar: una selección, o
    /// texto al lado del cursor (`at_edge` dice que no lo hay). Borrar letra a
    /// letra se agrupa; una selección o una palabra entera es un paso propio.
    fn save_erase(&mut self, word: bool, at_edge: bool) {
        if self.selection().is_some() {
            self.save(None);
        } else if !at_edge {
            self.save((!word).then_some(Edit::Erase));
        }
    }

    /// Lleva el cursor a `index`; con `extend` (Mayús) amplía la selección.
    pub fn move_to(&mut self, index: usize, extend: bool) {
        self.last_edit = None;
        if extend {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = index.min(self.len());
    }
    /// Flecha izquierda. Sin Mayús y con selección, el cursor va a su inicio.
    pub fn left(&mut self, word: bool, extend: bool) {
        let target = match self.selection() {
            Some((start, _)) if !extend && !word => start,
            _ if word => self.word_start(self.caret),
            _ => self.caret.saturating_sub(1),
        };
        self.move_to(target, extend);
    }
    /// Flecha derecha. Sin Mayús y con selección, el cursor va a su final.
    pub fn right(&mut self, word: bool, extend: bool) {
        let target = match self.selection() {
            Some((_, end)) if !extend && !word => end,
            _ if word => self.next_word(self.caret),
            _ => self.caret + 1,
        };
        self.move_to(target, extend);
    }
    pub fn home(&mut self, extend: bool) {
        self.move_to(0, extend);
    }
    pub fn end(&mut self, extend: bool) {
        let len = self.len();
        self.move_to(len, extend);
    }

    /// Selecciona la palabra bajo el carácter `at` (doble clic), o los espacios
    /// seguidos si cae en ellos. Al final del texto, la palabra anterior.
    pub fn select_word(&mut self, at: usize) {
        if let Some((start, end)) = self.word_at(at) {
            self.last_edit = None;
            self.anchor = Some(start);
            self.caret = end;
        }
    }
    /// Límites `(inicio, fin)` de la palabra (o los espacios) bajo el carácter `at`.
    fn word_at(&self, at: usize) -> Option<(usize, usize)> {
        let chars: Vec<char> = self.text.chars().collect();
        if chars.is_empty() {
            return None;
        }
        let at = at.min(chars.len() - 1);
        let kind = class(chars[at]);
        let mut start = at;
        while start > 0 && class(chars[start - 1]) == kind {
            start -= 1;
        }
        let mut end = at + 1;
        while end < chars.len() && class(chars[end]) == kind {
            end += 1;
        }
        Some((start, end))
    }

    /// Pulsación del botón del ratón sobre el carácter `index`, en la posición `x`
    /// y el momento `now`. Un clic pone el cursor (con `extend`, Mayús, amplía la
    /// selección); dos seguidos en el mismo sitio, dentro del tiempo `double` de
    /// Windows, seleccionan la palabra; tres, todo el texto. Después, `drag` amplía
    /// la selección hasta soltar el botón.
    pub fn press(&mut self, index: usize, x: f32, extend: bool, now: Instant, double: Duration) {
        let count = match self.last_press {
            Some((at, px, count))
                if !extend
                    && now.saturating_duration_since(at) <= double
                    && (x - px).abs() <= DOUBLE_CLICK_SLOP =>
            {
                count % 3 + 1
            }
            _ => 1,
        };
        self.last_press = Some((now, x, count));
        self.drag = match count {
            2 => {
                self.select_word(index);
                self.selection().map(|(start, end)| Drag::Words(start, end))
            }
            3 => {
                self.select_all();
                None
            }
            _ => {
                self.move_to(index, extend);
                Some(Drag::Chars)
            }
        };
    }
    /// El botón sigue pulsado y el ratón está sobre el carácter `index`: amplía la
    /// selección hasta ahí (palabras enteras si empezó con doble clic).
    pub fn drag(&mut self, index: usize) {
        match self.drag {
            Some(Drag::Chars) => self.move_to(index, true),
            Some(Drag::Words(start, end)) => {
                let index = index.min(self.len());
                let (anchor, caret) = if index < start {
                    (end, self.word_at(index).map_or(index, |(s, _)| s))
                } else if index > end {
                    (start, self.word_at(index - 1).map_or(index, |(_, e)| e))
                } else {
                    (start, end)
                };
                self.anchor = Some(anchor);
                self.caret = caret;
            }
            None => {}
        }
    }
    /// Se ha soltado el botón del ratón.
    pub fn release(&mut self) {
        self.drag = None;
    }
    /// Hay un arrastre en curso (el botón sigue pulsado desde el campo).
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Principio de la palabra que queda antes de `from`: salta los espacios y
    /// después los caracteres de la misma clase, como Ctrl+← en Windows.
    fn word_start(&self, from: usize) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut i = from.min(chars.len());
        while i > 0 && class(chars[i - 1]) == Class::Space {
            i -= 1;
        }
        if i > 0 {
            let kind = class(chars[i - 1]);
            while i > 0 && class(chars[i - 1]) == kind {
                i -= 1;
            }
        }
        i
    }
    /// Principio de la palabra siguiente: salta la palabra actual y los espacios
    /// que la siguen, como Ctrl+→ en Windows.
    fn next_word(&self, from: usize) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut i = from.min(chars.len());
        if i < chars.len() && class(chars[i]) != Class::Space {
            let kind = class(chars[i]);
            while i < chars.len() && class(chars[i]) == kind {
                i += 1;
            }
        }
        while i < chars.len() && class(chars[i]) == Class::Space {
            i += 1;
        }
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(text: &str) -> TextField {
        let mut field = TextField::default();
        field.insert(text);
        field
    }

    #[test]
    fn select_word_takes_the_word_or_the_spaces_under_the_click() {
        let mut text = field("abrir  spotify.exe");
        text.select_word(3);
        assert_eq!(text.selected_text().as_deref(), Some("abrir"));
        text.select_word(5);
        assert_eq!(text.selected_text().as_deref(), Some("  "));
        text.select_word(9);
        assert_eq!(text.selected_text().as_deref(), Some("spotify"));
        // Al final del texto, la última palabra.
        text.select_word(100);
        assert_eq!(text.selected_text().as_deref(), Some("exe"));
        let mut empty = TextField::default();
        empty.select_word(0);
        assert_eq!(empty.selection(), None);
    }

    #[test]
    fn edits_unicode_at_the_caret() {
        let mut f = field("Diseño");
        f.left(false, false);
        f.insert("s");
        assert_eq!(f.text(), "Diseñso");
        f.backspace(false);
        assert_eq!(f.text(), "Diseño");
        f.delete(false);
        assert_eq!(f.text(), "Diseñ");
        assert_eq!(f.caret(), 5);
        // En los extremos no hace nada.
        f.delete(false);
        f.home(false);
        f.backspace(false);
        assert_eq!(f.text(), "Diseñ");
    }

    #[test]
    fn ctrl_backspace_and_ctrl_delete_remove_whole_words() {
        let mut f = field("visual studio  code");
        f.backspace(true);
        assert_eq!(f.text(), "visual studio  ");
        f.backspace(true);
        assert_eq!(f.text(), "visual ");
        f.backspace(true);
        assert!(f.is_empty());

        let mut f = field("visual studio code");
        f.home(false);
        f.delete(true);
        assert_eq!(f.text(), "studio code");
        // A mitad de palabra borra solo desde el cursor.
        f.move_to(3, false);
        f.backspace(true);
        assert_eq!(f.text(), "dio code");
        f.delete(true);
        assert_eq!(f.text(), "code");
        // Los signos cuentan como palabra aparte.
        let mut f = field("C:\\Program Files");
        f.backspace(true);
        assert_eq!(f.text(), "C:\\Program ");
        f.backspace(true);
        assert_eq!(f.text(), "C:\\");
        f.backspace(true);
        assert_eq!(f.text(), "C");
    }

    #[test]
    fn ctrl_arrows_jump_by_words() {
        let mut f = field("visual studio code");
        f.left(true, false);
        assert_eq!(f.caret(), 14);
        f.left(true, false);
        assert_eq!(f.caret(), 7);
        f.left(true, false);
        assert_eq!(f.caret(), 0);
        f.left(true, false);
        assert_eq!(f.caret(), 0);
        f.right(true, false);
        assert_eq!(f.caret(), 7);
        f.right(true, false);
        assert_eq!(f.caret(), 14);
        f.right(true, false);
        assert_eq!(f.caret(), 18);
    }

    #[test]
    fn shift_selects_and_typing_replaces_the_selection() {
        let mut f = field("visual studio");
        f.left(true, true);
        assert_eq!(f.selected_text().as_deref(), Some("studio"));
        f.left(false, true);
        assert_eq!(f.selected_text().as_deref(), Some(" studio"));
        // Sin Mayús, la flecha cierra la selección en su borde.
        f.right(false, false);
        assert_eq!(f.selection(), None);
        assert_eq!(f.caret(), 13);
        f.home(true);
        assert_eq!(f.selection(), Some((0, 13)));
        f.insert("code");
        assert_eq!(f.text(), "code");
        assert_eq!(f.caret(), 4);

        let mut f = field("spotify");
        f.select_all();
        assert_eq!(f.selection(), Some((0, 7)));
        f.backspace(false);
        assert!(f.is_empty());
        let mut f = field("spotify");
        f.select_all();
        f.delete(true);
        assert!(f.is_empty());
        // Volver al punto de partida deja la selección vacía.
        let mut f = field("ab");
        f.left(false, true);
        f.right(false, true);
        assert_eq!(f.selection(), None);
    }

    #[test]
    fn cut_copies_and_removes_the_selection() {
        let mut f = field("hola mundo");
        assert_eq!(f.cut(), None);
        f.left(true, true);
        assert_eq!(f.cut().as_deref(), Some("mundo"));
        assert_eq!(f.text(), "hola ");
        assert_eq!(f.caret(), 5);
    }

    #[test]
    fn undo_groups_typing_and_redo_restores_it() {
        let mut f = field("hola");
        f.insert(" ");
        f.insert("m");
        f.insert("u");
        assert_eq!(f.text(), "hola mu");
        // Escribir letra a letra es un solo paso; pegar "hola" fue otro.
        assert!(f.undo());
        assert_eq!(f.text(), "hola");
        assert_eq!(f.caret(), 4);
        assert!(f.redo());
        assert_eq!(f.text(), "hola mu");
        assert_eq!(f.caret(), 7);
        // Mover el cursor corta el grupo.
        f.left(false, false);
        f.insert("x");
        f.end(false);
        f.backspace(false);
        f.backspace(false);
        assert_eq!(f.text(), "hola m");
        assert!(f.undo());
        assert_eq!(f.text(), "hola mxu");
        assert!(f.undo());
        assert_eq!(f.text(), "hola mu");
        // Una edición nueva descarta lo que se podía rehacer.
        f.insert("!");
        assert!(!f.redo());
        // Borrar en el extremo no deja un paso vacío.
        let mut f = TextField::default();
        f.backspace(false);
        assert!(!f.undo());
        let mut f = field("abc");
        f.clear();
        assert!(!f.undo());
    }

    #[test]
    fn mouse_click_drag_and_multi_click_select() {
        let double = Duration::from_millis(500);
        let start = Instant::now();
        let mut f = field("visual studio code");
        // Clic y arrastre, en los dos sentidos.
        f.press(2, 20.0, false, start, double);
        assert!(f.dragging());
        f.drag(10);
        assert_eq!(f.selected_text().as_deref(), Some("sual stu"));
        f.drag(0);
        assert_eq!(f.selected_text().as_deref(), Some("vi"));
        f.release();
        assert!(!f.dragging());
        f.drag(10);
        assert_eq!(f.selected_text().as_deref(), Some("vi"));
        // Doble clic: la palabra, y el arrastre va por palabras enteras.
        let later = start + Duration::from_secs(5);
        f.press(9, 90.0, false, later, double);
        f.press(9, 92.0, false, later + Duration::from_millis(200), double);
        assert_eq!(f.selected_text().as_deref(), Some("studio"));
        f.drag(16);
        assert_eq!(f.selected_text().as_deref(), Some("studio code"));
        f.drag(2);
        assert_eq!(f.selected_text().as_deref(), Some("visual studio"));
        assert_eq!(f.caret(), 0);
        f.drag(10);
        assert_eq!(f.selected_text().as_deref(), Some("studio"));
        // Triple clic: todo.
        f.press(9, 90.0, false, later + Duration::from_millis(400), double);
        assert_eq!(f.selection(), Some((0, 18)));
        // Pasado el tiempo, o lejos del anterior, vuelve a ser un clic simple.
        f.press(9, 90.0, false, later + Duration::from_secs(2), double);
        assert_eq!(f.selection(), None);
        f.press(9, 90.0, false, later + Duration::from_millis(2100), double);
        f.press(9, 200.0, false, later + Duration::from_millis(2200), double);
        assert_eq!(f.selection(), None);
        // Mayús+clic amplía la selección desde el cursor.
        f.press(18, 300.0, true, later + Duration::from_secs(9), double);
        assert_eq!(f.selected_text().as_deref(), Some("udio code"));
    }

    #[test]
    fn paste_drops_line_breaks_and_respects_the_limit() {
        let mut f = field("a\r\nb\tc");
        assert_eq!(f.text(), "abc");
        f.insert(&"x".repeat(200));
        assert_eq!(f.text().chars().count(), MAX_LEN);
    }
}
