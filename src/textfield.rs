//! Campo de texto de una línea con el comportamiento de los cuadros de texto de
//! Windows: cursor, selección con Mayús, y saltos y borrado por palabras con Ctrl.
//! Las posiciones cuentan caracteres, no bytes, para no partir letras como "ñ".

/// Longitud máxima, en caracteres.
const MAX_LEN: usize = 100;

#[derive(Default)]
pub struct TextField {
    text: String,
    caret: usize,
    /// Extremo fijo de la selección; la selección va de aquí al cursor.
    anchor: Option<usize>,
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
        self.anchor = Some(0);
        self.caret = self.len();
    }
    pub fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
        self.anchor = None;
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
        self.delete_selection();
        let text: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_LEN.saturating_sub(self.len()))
            .collect();
        let at = self.byte_at(self.caret);
        self.text.insert_str(at, &text);
        self.caret += text.chars().count();
    }
    pub fn cut(&mut self) -> Option<String> {
        let text = self.selected_text()?;
        self.delete_selection();
        Some(text)
    }

    /// Retroceso; con Ctrl borra hasta el principio de la palabra.
    pub fn backspace(&mut self, word: bool) {
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

    /// Lleva el cursor a `index`; con `extend` (Mayús) amplía la selección.
    pub fn move_to(&mut self, index: usize, extend: bool) {
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
    fn paste_drops_line_breaks_and_respects_the_limit() {
        let mut f = field("a\r\nb\tc");
        assert_eq!(f.text(), "abc");
        f.insert(&"x".repeat(200));
        assert_eq!(f.text().chars().count(), MAX_LEN);
    }
}
