//! Caret editing for the open screen's focused text field: the search box, the anvil's name
//! box, and an editable book's page or title. Every field edits through one [`ui::ChatEditor`]
//! that mirrors the field's string, within that field's own limits.

use protocol::MAX_BOOK_PAGE_BYTES;

use super::ScreenState;

/// Longest creative search text.
pub(super) const MAX_SEARCH_CHARS: usize = 32;
/// Longest item name an anvil accepts.
pub(super) const MAX_ANVIL_NAME_CHARS: usize = 50;
/// Characters a writable page holds (vanilla's `$max_page_length`).
const MAX_PAGE_CHARS: usize = 256;
/// Characters a book title holds (vanilla's `$max_title_length`).
const MAX_TITLE_CHARS: usize = 16;

/// Bytes the field editor holds: the largest field's byte bound.
const FIELD_EDITOR_BYTES: usize = MAX_BOOK_PAGE_BYTES;
const _: () = assert!(
    4 * MAX_SEARCH_CHARS <= FIELD_EDITOR_BYTES
        && 4 * MAX_ANVIL_NAME_CHARS <= FIELD_EDITOR_BYTES
        && FIELD_EDITOR_BYTES <= ui::MAX_TEXT_EDITOR_BYTES
);

/// What a field holds: at most `chars` characters in at most `bytes` bytes, with line breaks
/// only where `newlines` allows them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui_runtime) struct FieldLimit {
    pub chars: usize,
    pub bytes: usize,
    pub newlines: bool,
}

impl FieldLimit {
    pub(super) const SEARCH: Self = Self::single_line(MAX_SEARCH_CHARS);
    pub(super) const ANVIL_NAME: Self = Self::single_line(MAX_ANVIL_NAME_CHARS);
    /// A page also fits the book edit packet's page bytes.
    pub(in crate::ui_runtime) const BOOK_PAGE: Self = Self {
        chars: MAX_PAGE_CHARS,
        bytes: MAX_BOOK_PAGE_BYTES,
        newlines: true,
    };
    pub(in crate::ui_runtime) const BOOK_TITLE: Self = Self {
        chars: MAX_TITLE_CHARS,
        bytes: MAX_BOOK_PAGE_BYTES,
        newlines: false,
    };

    const fn single_line(chars: usize) -> Self {
        Self {
            chars,
            bytes: 4 * chars,
            newlines: false,
        }
    }

    /// The part of `text` a field still takes after keeping `kept_chars` characters in
    /// `kept_bytes` bytes: its admitted characters up to the first that no longer fits. Control
    /// characters are dropped, except line breaks where the field allows them.
    pub(in crate::ui_runtime) fn admit(
        self,
        kept_chars: usize,
        kept_bytes: usize,
        text: &str,
    ) -> String {
        let (mut chars, mut bytes) = (kept_chars, kept_bytes);
        let mut admitted = String::new();
        for ch in text
            .chars()
            .filter(|ch| !ch.is_control() || (self.newlines && *ch == '\n'))
        {
            if chars >= self.chars || bytes + ch.len_utf8() > self.bytes {
                break;
            }
            chars += 1;
            bytes += ch.len_utf8();
            admitted.push(ch);
        }
        admitted
    }
}

/// Which field the editor mirrors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui_runtime) enum FieldKind {
    Search,
    AnvilName,
    BookPage(usize),
    BookTitle,
}

/// The focused field's caret and selection. The field's string stays the source of truth: an
/// editor that no longer mirrors it reloads with the caret at the end.
#[derive(Clone, Debug)]
pub(super) struct FieldEditor {
    editor: ui::ChatEditor,
    /// Changes with every edit and caret move, restarting the caret blink.
    revision: u64,
    /// The field whose text the editor last took up, and the container window it was in.
    owner: Option<(FieldKind, Option<u64>)>,
}

impl Default for FieldEditor {
    fn default() -> Self {
        Self {
            editor: ui::ChatEditor::new(FIELD_EDITOR_BYTES)
                .expect("the field editor's size is checked at compile time"),
            revision: 0,
            owner: None,
        }
    }
}

/// Where the focused field's caret and selection sit, as byte offsets into its text.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FieldCaret {
    pub byte: usize,
    pub selection: Option<[usize; 2]>,
    /// Changes with every edit and caret move, restarting the caret blink.
    pub revision: u64,
}

impl ScreenState {
    /// Inserts typed text at the focused field's caret, replacing any selection and dropping
    /// control characters (except a page's line breaks) and whatever exceeds the field's limits.
    pub fn type_text(&mut self, text: &str) {
        self.edit_field(|editor, limit| {
            let selected = editor.selected_text().unwrap_or_default();
            let kept_chars = editor.as_str().chars().count() - selected.chars().count();
            let kept_bytes = editor.len_bytes() - selected.len();
            let insertion = limit.admit(kept_chars, kept_bytes, text);
            // Within the field's limits, so within the editor's bytes too.
            if !insertion.is_empty() {
                let _ = editor.insert(&insertion);
            }
        });
    }

    pub fn backspace_text(&mut self) {
        self.edit_field(|editor, _| editor.backspace());
    }

    /// Applies a caret or deletion key to the focused field; see [`ui::ChatEditor::edit_key`].
    /// `false` when no field is focused or the key is not an editing key.
    pub fn edit_text_key(
        &mut self,
        key: semantic_input::KeyboardKey,
        selecting: bool,
        by_word: bool,
    ) -> bool {
        let mut handled = false;
        self.edit_field(|editor, _| handled = editor.edit_key(key, selecting, by_word));
        handled
    }

    /// Selects the focused field's whole text; `false` when none is focused.
    pub fn select_all_text(&mut self) -> bool {
        self.edit_field(|editor, _| editor.select_all())
    }

    /// The focused field's selected text.
    pub fn selected_text(&self) -> Option<&str> {
        self.mirrored_editor()?.selected_text()
    }

    /// Removes and returns the focused field's selected text.
    pub fn cut_text(&mut self) -> Option<String> {
        let mut cut = None;
        self.edit_field(|editor, _| cut = editor.cut_selection());
        cut
    }

    /// Pastes the clipboard's text at the focused field's caret, as typing it would; a clipboard
    /// longer than the paste bound pastes nothing. `false` when no field is focused.
    pub fn paste_text<C: ui::ChatClipboard>(&mut self, clipboard: &mut C) -> bool {
        if self.focused_field().is_none() {
            return false;
        }
        let bound = crate::ui_runtime::forms::engine_input::MAX_PASTE_BYTES;
        if let Ok(Some(text)) = clipboard.read_text_bounded(bound)
            && text.len() <= bound
        {
            self.type_text(&text);
        }
        true
    }

    /// The focused field's caret, once editing has placed one; a field the editor does not
    /// mirror shows its caret at the end.
    pub fn field_caret(&self) -> Option<FieldCaret> {
        let (field, ..) = self.focused_field()?;
        let revision = self.field_editor.revision;
        Some(match self.mirrored_editor() {
            Some(editor) => FieldCaret {
                byte: editor.cursor_byte(),
                selection: editor.selection().map(|range| [range.start, range.end]),
                revision,
            },
            None => FieldCaret {
                byte: field.len(),
                selection: None,
                revision,
            },
        })
    }

    /// Drops the focused field's caret and selection, so the next field to take focus starts
    /// with its caret at the end even when it holds the same text.
    pub(in crate::ui_runtime) fn release_field_editor(&mut self) {
        self.field_editor.owner = None;
    }

    /// The field editor while it holds the focused field's caret: it took up that field in
    /// this window and the field still has the editor's text.
    fn mirrored_editor(&self) -> Option<&ui::ChatEditor> {
        let (field, _, kind) = self.focused_field()?;
        let FieldEditor { editor, owner, .. } = &self.field_editor;
        (*owner == Some((kind, self.window)) && editor.as_str() == field).then_some(editor)
    }

    /// The field typing reaches: an editable book's page or title, else the focused anvil name
    /// or search box.
    fn focused_field(&self) -> Option<(&str, FieldLimit, FieldKind)> {
        match self.book.as_ref().filter(|book| book.editable) {
            Some(book) => Some(book.focused_field()),
            None if self.anvil_focused => Some((
                &self.anvil_name,
                FieldLimit::ANVIL_NAME,
                FieldKind::AnvilName,
            )),
            None if self.search_focused => {
                Some((&self.search, FieldLimit::SEARCH, FieldKind::Search))
            }
            None => None,
        }
    }

    /// Runs `edit` with the focused field's editor and limits, then writes the result back to
    /// the field. A changed search returns the result lists to their top. `false` when no field
    /// is focused.
    fn edit_field(&mut self, edit: impl FnOnce(&mut ui::ChatEditor, FieldLimit)) -> bool {
        let (field, limit, kind) = match self.book.as_mut().filter(|book| book.editable) {
            Some(book) => book.focused_field_mut(),
            None if self.anvil_focused => (
                &mut self.anvil_name,
                FieldLimit::ANVIL_NAME,
                FieldKind::AnvilName,
            ),
            None if self.search_focused => {
                (&mut self.search, FieldLimit::SEARCH, FieldKind::Search)
            }
            None => return false,
        };
        let owner = Some((kind, self.window));
        let FieldEditor {
            editor,
            revision,
            owner: mirrored,
        } = &mut self.field_editor;
        if *mirrored != owner || editor.as_str() != field.as_str() {
            editor.set_text(field);
            *mirrored = owner;
        }
        let before = (editor.cursor_byte(), editor.selection());
        edit(editor, limit);
        let changed = editor.as_str() != field.as_str();
        if changed {
            field.clear();
            field.push_str(editor.as_str());
        }
        if changed || before != (editor.cursor_byte(), editor.selection()) {
            *revision = revision.wrapping_add(1);
        }
        if changed && kind == FieldKind::Search {
            self.creative_row = 0;
            self.container_scroll.clear();
            self.container_scroll_drag = None;
        }
        true
    }
}
