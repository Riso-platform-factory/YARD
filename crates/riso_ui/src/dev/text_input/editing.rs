//! The text, selection, IME composition, and undo history of a text input.
//! Pure state: nothing here knows about windows or layout.

use std::{
    ops::Range,
    time::{Duration, Instant},
};
use unicode_segmentation::UnicodeSegmentation;

/// Typing pauses longer than this start a new undo entry.
const TYPING_PAUSE: Duration = Duration::from_secs(1);

/// How an edit enters the undo history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edit {
    /// One typed character. Characters typed one after another without a
    /// pause are undone together.
    Typing,
    /// Anything else: a paste, a cut, or a deletion.
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    text: String,
    selection: Range<usize>,
    reversed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Punctuation,
}

fn class(grapheme: &str) -> Class {
    // A grapheme is classified by its first character, so a letter with
    // combining marks is a word character.
    match grapheme.chars().next() {
        Some(c) if c.is_whitespace() => Class::Space,
        Some(c) if c.is_alphanumeric() || c == '_' => Class::Word,
        _ => Class::Punctuation,
    }
}

/// Ranges and offsets are bytes. Native input methods may place their
/// selection inside a grapheme; keyboard and mouse movement use its boundaries.
pub(crate) struct Editing {
    text: String,
    selection: Range<usize>,
    /// The cursor is at the start of the selection instead of its end.
    reversed: bool,
    /// Text the IME is still composing.
    marked: Option<Range<usize>>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Where and when the last typed character ended, to join typing into one
    /// undo entry.
    typing: Option<(usize, Instant)>,
    /// The state before the IME composition began. It becomes the undo entry
    /// when the composition is committed.
    composition: Option<Snapshot>,
}

impl Editing {
    pub(crate) fn new() -> Self {
        Self {
            text: String::new(),
            selection: 0..0,
            reversed: false,
            marked: None,
            undo: Vec::new(),
            redo: Vec::new(),
            typing: None,
            composition: None,
        }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }
    pub(crate) fn selection(&self) -> Range<usize> {
        self.selection.clone()
    }
    pub(crate) fn reversed(&self) -> bool {
        self.reversed
    }
    pub(crate) fn marked(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }
    pub(crate) fn cursor(&self) -> usize {
        if self.reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }

    /// Replaces everything and forgets the undo history.
    pub(crate) fn set_text(&mut self, text: String) {
        let end = text.len();
        *self = Self::new();
        self.text = text;
        self.selection = end..end;
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            selection: self.selection.clone(),
            reversed: self.reversed,
        }
    }

    pub(crate) fn grapheme_boundary(&self, offset: usize) -> usize {
        if offset >= self.text.len() {
            return self.text.len();
        }
        self.text
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .take_while(|index| *index <= offset)
            .last()
            .unwrap_or(0)
    }

    // Boundaries

    pub(crate) fn previous_grapheme(&self, offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .take_while(|index| *index < offset)
            .last()
            .unwrap_or(0)
    }
    pub(crate) fn next_grapheme(&self, offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .find(|index| *index > offset)
            .unwrap_or(self.text.len())
    }
    /// The start of the word before `offset`, skipping spaces first.
    pub(crate) fn previous_word(&self, offset: usize) -> usize {
        let mut graphemes = self.text[..offset].grapheme_indices(true).rev().peekable();
        let mut start = offset;
        while let Some((index, _)) = graphemes.next_if(|(_, g)| class(g) == Class::Space) {
            start = index;
        }
        if let Some((index, first)) = graphemes.next() {
            start = index;
            let kind = class(first);
            while let Some((index, _)) = graphemes.next_if(|(_, g)| class(g) == kind) {
                start = index;
            }
        }
        start
    }
    /// The end of the word after `offset`, skipping spaces first.
    pub(crate) fn next_word(&self, offset: usize) -> usize {
        let mut graphemes = self.text[offset..].grapheme_indices(true).peekable();
        let mut end = offset;
        while let Some((index, g)) = graphemes.next_if(|(_, g)| class(g) == Class::Space) {
            end = offset + index + g.len();
        }
        if let Some((index, first)) = graphemes.next() {
            end = offset + index + first.len();
            let kind = class(first);
            while let Some((index, g)) = graphemes.next_if(|(_, g)| class(g) == kind) {
                end = offset + index + g.len();
            }
        }
        end
    }
    /// The word, run of spaces, or run of punctuation around `offset`.
    pub(crate) fn word_at(&self, offset: usize) -> Range<usize> {
        let Some(grapheme) = self.text[offset..]
            .graphemes(true)
            .next()
            .or_else(|| self.text[..offset].graphemes(true).next_back())
        else {
            return 0..0;
        };
        let kind = class(grapheme);
        let start = self.text[..offset]
            .grapheme_indices(true)
            .rev()
            .take_while(|(_, g)| class(g) == kind)
            .last()
            .map_or(offset, |(index, _)| index);
        let end = self.text[offset..]
            .grapheme_indices(true)
            .take_while(|(_, g)| class(g) == kind)
            .last()
            .map_or(offset, |(index, g)| offset + index + g.len());
        start..end
    }
    // UTF-16, which the platform's input methods count in

    pub(crate) fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for c in self.text.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += c.len_utf16();
            utf8 += c.len_utf8();
        }
        utf8
    }
    pub(crate) fn offset_to_utf16(&self, offset: usize) -> usize {
        self.text[..offset.min(self.text.len())]
            .chars()
            .map(char::len_utf16)
            .sum()
    }
    pub(crate) fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }
    pub(crate) fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    // Selection

    pub(crate) fn move_to(&mut self, offset: usize) {
        let offset = self.grapheme_boundary(offset);
        self.selection = offset..offset;
        self.reversed = false;
        self.typing = None;
    }
    /// Moves the cursor and keeps the other end of the selection.
    pub(crate) fn select_to(&mut self, offset: usize) {
        let offset = self.grapheme_boundary(offset);
        if self.reversed {
            self.selection.start = offset;
        } else {
            self.selection.end = offset;
        }
        if self.selection.end < self.selection.start {
            self.reversed = !self.reversed;
            self.selection = self.selection.end..self.selection.start;
        }
        self.typing = None;
    }
    pub(crate) fn select(&mut self, range: Range<usize>) {
        self.selection = range;
        self.reversed = false;
        self.typing = None;
    }

    // Editing

    fn push_undo(&mut self, before: Snapshot) {
        if before.text != self.text {
            self.undo.push(before);
            self.redo.clear();
        }
    }

    /// Ends an IME composition, making it one undo entry.
    fn end_composition(&mut self) {
        self.marked = None;
        if let Some(before) = self.composition.take() {
            self.push_undo(before);
        }
    }

    /// Replaces `range`, or else the composing text, or else the selection.
    /// The cursor ends up after the new text. Returns whether the text changed.
    pub(crate) fn replace(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        edit: Edit,
    ) -> bool {
        let composing = self.marked.is_some();
        let range = range
            .or(self.marked.clone())
            .unwrap_or(self.selection.clone());
        if range.is_empty() && new_text.is_empty() {
            if composing {
                self.end_composition();
            }
            return false;
        }
        let before = self.snapshot();
        let changed = before.text[range.clone()] != *new_text;
        let now = Instant::now();
        let joins_typing = !composing
            && edit == Edit::Typing
            && range.is_empty()
            && self.typing.is_some_and(|(end, at)| {
                end == range.start && now.duration_since(at) < TYPING_PAUSE
            });
        self.text.replace_range(range.clone(), new_text);
        let cursor = range.start + new_text.len();
        self.selection = cursor..cursor;
        self.reversed = false;
        if composing {
            self.end_composition();
        } else if !joins_typing {
            self.push_undo(before);
        } else {
            self.redo.clear();
        }
        self.typing =
            (edit == Edit::Typing && !composing && new_text.chars().all(|c| !c.is_whitespace()))
                .then_some((cursor, now));
        changed
    }

    /// Replaces `range`, or else the composing text, or else the selection,
    /// with text the IME is still composing. `selected` is the selection inside
    /// the new text, in UTF-16 units.
    pub(crate) fn compose(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        selected: Option<Range<usize>>,
    ) {
        if self.composition.is_none() {
            self.composition = Some(self.snapshot());
        }
        let range = range
            .or(self.marked.clone())
            .unwrap_or(self.selection.clone());
        self.text.replace_range(range.clone(), new_text);
        let end = range.start + new_text.len();
        self.selection = match selected {
            Some(selected) => {
                let start_utf16 = self.offset_to_utf16(range.start);
                let length_utf16 = new_text.encode_utf16().count();
                self.range_from_utf16(
                    &(start_utf16 + selected.start.min(length_utf16)
                        ..start_utf16 + selected.end.min(length_utf16)),
                )
            }
            None => end..end,
        };
        self.reversed = false;
        self.typing = None;
        if new_text.is_empty() {
            self.end_composition();
        } else {
            self.marked = Some(range.start..end);
        }
    }

    /// Keeps the composing text as it is and ends the composition.
    pub(crate) fn unmark(&mut self) {
        self.end_composition();
    }

    pub(crate) fn undo(&mut self) -> bool {
        self.swap_history(true)
    }
    pub(crate) fn redo(&mut self) -> bool {
        self.swap_history(false)
    }
    fn swap_history(&mut self, undo: bool) -> bool {
        if self.marked.is_some() {
            return false;
        }
        let (from, to) = if undo {
            (&mut self.undo, &mut self.redo)
        } else {
            (&mut self.redo, &mut self.undo)
        };
        let Some(target) = from.pop() else {
            return false;
        };
        to.push(Snapshot {
            text: std::mem::replace(&mut self.text, target.text),
            selection: std::mem::replace(&mut self.selection, target.selection),
            reversed: std::mem::replace(&mut self.reversed, target.reversed),
        });
        self.typing = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(text: &str) -> Editing {
        let mut editing = Editing::new();
        for grapheme in text.graphemes(true) {
            editing.replace(None, grapheme, Edit::Typing);
        }
        editing
    }

    #[test]
    fn graphemes_are_moved_over_and_deleted_whole() {
        // "ệ" written as e + two combining marks, then a flag (two scalars).
        let editing = typed("ae\u{0302}\u{0323}🇻🇳");
        let end = editing.text().len();
        let flag = editing.previous_grapheme(end);
        assert_eq!(&editing.text()[flag..], "🇻🇳");
        let e = editing.previous_grapheme(flag);
        assert_eq!(&editing.text()[e..flag], "e\u{0302}\u{0323}");
        assert_eq!(editing.previous_grapheme(e), 0);
        assert_eq!(editing.previous_grapheme(0), 0);
        assert_eq!(editing.next_grapheme(0), 1);
        assert_eq!(editing.next_grapheme(e), flag);
        assert_eq!(editing.next_grapheme(end), end);
    }

    #[test]
    fn words_include_letters_with_marks_and_stop_at_punctuation() {
        let editing = typed("xin chào, thế giới");
        let text = editing.text();
        let comma = text.find(',').unwrap();
        assert_eq!(editing.next_word(0), 3);
        assert_eq!(editing.next_word(3), comma);
        assert_eq!(editing.next_word(comma), comma + 1);
        assert_eq!(
            editing.previous_word(text.len()),
            text.rfind("giới").unwrap()
        );
        assert_eq!(editing.previous_word(comma + 1), comma);
        assert_eq!(editing.previous_word(comma), 4);
        assert_eq!(editing.previous_word(4), 0);
        assert_eq!(editing.word_at(5), 4..comma);
        assert_eq!(editing.word_at(3), 3..4);
        assert_eq!(
            editing.word_at(text.len()),
            text.rfind("giới").unwrap()..text.len()
        );
        assert_eq!(Editing::new().word_at(0), 0..0);
    }

    #[test]
    fn utf16_offsets_round_trip() {
        let editing = typed("aệ😀b");
        let text = editing.text();
        for (offset, _) in text.char_indices().chain([(text.len(), ' ')]) {
            let utf16 = editing.offset_to_utf16(offset);
            assert_eq!(editing.offset_from_utf16(utf16), offset);
        }
        // "😀" is two UTF-16 units.
        assert_eq!(editing.offset_to_utf16(text.len()), 5);
    }

    #[test]
    fn selection_extends_and_flips_around_its_anchor() {
        let mut editing = typed("abcdef");
        editing.move_to(3);
        editing.select_to(5);
        assert_eq!((editing.selection(), editing.cursor()), (3..5, 5));
        editing.select_to(1);
        assert_eq!((editing.selection(), editing.cursor()), (1..3, 1));
        assert!(editing.reversed());
        editing.select_to(4);
        assert_eq!((editing.selection(), editing.cursor()), (3..4, 4));
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut editing = typed("hello world");
        editing.select(0..5);
        editing.replace(None, "bye", Edit::Other);
        assert_eq!(editing.text(), "bye world");
        assert_eq!(editing.selection(), 3..3);
    }

    #[test]
    fn typing_without_a_pause_is_one_undo_entry() {
        let mut editing = typed("abc");
        assert!(editing.undo());
        assert_eq!(editing.text(), "");
        assert!(!editing.undo());
        assert!(editing.redo());
        assert_eq!(editing.text(), "abc");
        assert_eq!(editing.selection(), 3..3);
    }

    #[test]
    fn other_edits_and_moves_start_new_undo_entries() {
        let mut editing = typed("ab");
        editing.replace(None, "PASTED", Edit::Other);
        editing.replace(None, "c", Edit::Typing);
        editing.move_to(0);
        editing.replace(None, "d", Edit::Typing);
        assert_eq!(editing.text(), "dabPASTEDc");
        let mut states = vec![];
        while editing.undo() {
            states.push(editing.text().to_owned());
        }
        assert_eq!(states, ["abPASTEDc", "abPASTED", "ab", ""]);
    }

    #[test]
    fn a_new_edit_clears_redo() {
        let mut editing = typed("abc");
        editing.undo();
        editing.replace(None, "x", Edit::Typing);
        assert!(!editing.redo());
        assert_eq!(editing.text(), "x");
    }

    #[test]
    fn an_edit_that_changes_nothing_is_not_recorded() {
        let mut editing = typed("abc");
        assert!(!editing.replace(None, "", Edit::Other));
        editing.undo();
        assert_eq!(editing.text(), "");
    }

    #[test]
    fn a_composition_is_one_undo_entry_made_when_it_is_committed() {
        let mut editing = typed("xin ");
        // Telex "vieetj" composes "việt" step by step.
        editing.compose(None, "v", None);
        editing.compose(None, "vi", None);
        editing.compose(None, "viê", None);
        assert_eq!(editing.marked(), Some(4..4 + "viê".len()));
        assert_eq!(editing.text(), "xin viê");
        // Undo does nothing while composing.
        assert!(!editing.undo());
        editing.replace(None, "việt", Edit::Typing);
        assert_eq!(editing.text(), "xin việt");
        assert_eq!(editing.marked(), None);
        assert_eq!(editing.cursor(), editing.text().len());

        assert!(editing.undo());
        assert_eq!(editing.text(), "xin ");
        assert!(editing.undo());
        assert_eq!(editing.text(), "");
    }

    #[test]
    fn a_composition_replaces_the_selection_and_can_place_the_cursor() {
        let mut editing = typed("hello world");
        editing.select(6..11);
        editing.compose(None, "ab", Some(1..1));
        assert_eq!(editing.text(), "hello ab");
        assert_eq!(editing.marked(), Some(6..8));
        assert_eq!(editing.selection(), 7..7);
    }

    #[test]
    fn a_canceled_composition_leaves_no_undo_entry() {
        let mut editing = typed("ab");
        editing.compose(None, "x", None);
        editing.compose(None, "", None);
        assert_eq!(editing.text(), "ab");
        assert_eq!(editing.marked(), None);
        editing.undo();
        assert_eq!(editing.text(), "");
    }

    #[test]
    fn unmark_keeps_the_composed_text() {
        let mut editing = Editing::new();
        editing.compose(None, "tiê", None);
        editing.unmark();
        assert_eq!((editing.text(), editing.marked()), ("tiê", None));
        assert!(editing.undo());
        assert_eq!(editing.text(), "");
    }

    #[test]
    fn set_text_forgets_history() {
        let mut editing = typed("abc");
        editing.set_text("new".into());
        assert_eq!(editing.selection(), 3..3);
        assert!(!editing.undo());
        assert_eq!(editing.text(), "new");
    }
}
