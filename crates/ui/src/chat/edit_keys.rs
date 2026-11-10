//! Desktop editing keys on a [`ChatEditor`]: caret moves, selection and deletion, and the
//! selection a copy or cut shortcut takes.

use semantic_input::KeyboardKey;

use super::ChatEditor;

impl ChatEditor {
    /// Applies a caret or deletion key: arrows, Home, End, Backspace and Delete. `selecting`
    /// (Shift) extends the selection instead of moving the caret, and `by_word` (the
    /// Control/Command shortcut modifier) moves or deletes whole words. Returns `false`, leaving
    /// the editor unchanged, for any other key.
    pub fn edit_key(&mut self, key: KeyboardKey, selecting: bool, by_word: bool) -> bool {
        match key {
            KeyboardKey::ArrowLeft if by_word => self.move_word_left(selecting),
            KeyboardKey::ArrowRight if by_word => self.move_word_right(selecting),
            KeyboardKey::Backspace if by_word => self.backspace_word(),
            KeyboardKey::Delete if by_word => self.delete_word(),
            KeyboardKey::ArrowLeft if selecting => self.select_left(),
            KeyboardKey::ArrowRight if selecting => self.select_right(),
            KeyboardKey::ArrowLeft => self.move_left(),
            KeyboardKey::ArrowRight => self.move_right(),
            KeyboardKey::Home => self.move_home(selecting),
            KeyboardKey::End => self.move_end(selecting),
            KeyboardKey::Backspace => self.backspace(),
            KeyboardKey::Delete => self.delete(),
            _ => return false,
        }
        true
    }

    /// Selects the whole text, with the caret at its end.
    pub fn select_all(&mut self) {
        self.move_home(false);
        self.move_end(true);
    }

    /// The selected text, if any.
    pub fn selected_text(&self) -> Option<&str> {
        self.selection().map(|range| &self.text[range])
    }

    /// Removes and returns the selected text; `None` leaves the editor unchanged.
    pub fn cut_selection(&mut self) -> Option<String> {
        let cut = self.selected_text()?.to_owned();
        self.delete_selection();
        Some(cut)
    }
}
