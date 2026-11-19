use blitz_traits::{
    events::{BlitzImeEvent, BlitzKeyEvent},
    node_id::NodeId,
    shell::ShellProvider,
};
use keyboard_types::{Key, Modifiers, NamedKey};
use parley::{ContentWidths, FontContext, LayoutContext};

use crate::util::ACTION_MOD;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
/// Parley Brush type for Blitz which contains the Blitz node id
pub struct TextBrush {
    /// The node id for the span
    pub id: NodeId,
}

impl TextBrush {
    pub(crate) fn from_id(id: NodeId) -> Self {
        Self { id }
    }
}

#[derive(Clone, Default)]
pub struct TextLayout {
    pub text: String,
    pub content_widths: Option<ContentWidths>,
    pub layout: parley::layout::Layout<TextBrush>,
    /// Block-axis offset (in CSS px) of the line boxes from the top of the container's content
    /// box, as applied by `align-content`.
    pub block_offset: f32,
    /// The out-of-flow positions for which some inline span with an inline box below it is a
    /// containing block (as `SPAN_*_CB` flags). The inline root claims those boxes in its place.
    /// Set by each `RunMode::PerformLayout` run of inline layout.
    pub span_cb_flags: stylo_taffy::StyleFlags,
}

impl TextLayout {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn content_widths(&mut self) -> ContentWidths {
        *self
            .content_widths
            .get_or_insert_with(|| self.layout.calculate_content_widths())
    }
}

impl std::fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextLayout")
    }
}

// TODO: support keypress events
pub enum GeneratedTextInputEvent {
    Input,
    Select,
    PreEditChange,
    Submit,
}

pub struct TextInputData {
    /// A parley TextEditor instance
    pub editor: Box<parley::PlainEditor<TextBrush>>,
    // PATCH(diaryx): an input's `placeholder`, laid out in the input's own
    // styles, which the painter draws faded while the input is empty.
    /// The input's `placeholder`, laid out as its text is, if it has one.
    pub placeholder: Option<Box<parley::PlainEditor<TextBrush>>>,
    /// Whether the input is a singleline or multiline input
    pub is_multiline: bool,
    /// The scroll offset of the text content within the input, in CSS (unscaled) pixels.
    ///
    /// For single-line inputs this is a horizontal offset; for multi-line inputs it is a
    /// vertical offset. It is kept up to date so that the caret remains visible within the
    /// input's content box.
    pub scroll_offset: f32,
    // PATCH(diaryx): a password input's text, kept apart from the editor.
    /// A password input's text, while the input is `type=password`. The
    /// editor then holds a bullet for each of its characters, so that only
    /// bullets are laid out, painted, selected and copied; the text itself
    /// is here, and the input's value is read from here
    /// ([`TextInputData::value`]). It is wiped when it changes and when it
    /// is dropped.
    pub secret: Option<Secret>,
}

/// What a password input draws for each character of its text.
pub const PASSWORD_MASK: char = '•';

/// A password input's text: wiped when it is replaced and when it is
/// dropped. A change is made into a new buffer, never in place, so that a
/// buffer is not grown by reallocating and the old one left unwiped.
#[derive(Default)]
pub struct Secret(zeroize::Zeroizing<String>);

impl Secret {
    pub fn new(text: &str) -> Self {
        let mut s = String::with_capacity(text.len());
        s.push_str(text);
        Secret(zeroize::Zeroizing::new(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn chars(&self) -> usize {
        self.0.chars().count()
    }

    /// The byte offset of the `n`th character.
    fn byte_at(&self, n: usize) -> usize {
        self.0
            .char_indices()
            .nth(n)
            .map_or(self.0.len(), |(i, _)| i)
    }

    /// The characters `from..to` replaced by `with`, into a new buffer.
    fn splice(&self, from: usize, to: usize, with: &str) -> Secret {
        let (a, b) = (self.byte_at(from), self.byte_at(to.max(from)));
        let mut s = String::with_capacity(self.0.len() - (b - a) + with.len());
        s.push_str(&self.0[..a]);
        s.push_str(with);
        s.push_str(&self.0[b..]);
        Secret(zeroize::Zeroizing::new(s))
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret({} characters)", self.chars())
    }
}

fn mask(chars: usize) -> String {
    std::iter::repeat_n(PASSWORD_MASK, chars).collect()
}

// FIXME: Implement Clone for PlainEditor
impl Clone for TextInputData {
    fn clone(&self) -> Self {
        TextInputData::new(self.is_multiline)
    }
}

impl TextInputData {
    pub fn new(is_multiline: bool) -> Self {
        let editor = Box::new(parley::PlainEditor::new(16.0));
        Self {
            editor,
            placeholder: None,
            is_multiline,
            scroll_offset: 0.0,
            secret: None,
        }
    }

    pub fn set_text(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        text: &str,
    ) {
        if let Some(secret) = &self.secret {
            if secret.as_str() != text {
                let chars = text.chars().count();
                self.secret = Some(Secret::new(text));
                self.editor.set_text(&mask(chars));
                self.editor.driver(font_ctx, layout_ctx).refresh_layout();
            }
            return;
        }
        if self.editor.text() != text {
            self.editor.set_text(text);
            self.editor.driver(font_ctx, layout_ctx).refresh_layout();
        }
    }

    // PATCH(diaryx): password inputs.
    /// The input's value: a password input's text, or the editor's.
    pub fn value(&self) -> &str {
        match &self.secret {
            Some(s) => s.as_str(),
            None => self.editor.raw_text(),
        }
    }

    pub fn is_password(&self) -> bool {
        self.secret.is_some()
    }

    /// Make the input a password input, its text drawn as bullets, or a
    /// plain one again (a password shown), keeping its text and caret.
    pub fn set_password(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        password: bool,
    ) {
        if password == self.is_password() || self.is_multiline {
            return;
        }
        let mut driver = self.editor.driver(font_ctx, layout_ctx);
        driver.clear_compose();
        let caret = driver.editor.raw_selection().focus().index();
        if password {
            let text = self.editor.raw_text();
            let chars = text.chars().count();
            let caret = text[..caret.min(text.len())].chars().count();
            let secret = Secret::new(text);
            // Over the text in place, before anything else is laid out.
            self.editor.set_text(&mask(chars));
            self.secret = Some(secret);
            let mut driver = self.editor.driver(font_ctx, layout_ctx);
            driver.move_to_byte(caret * PASSWORD_MASK.len_utf8());
        } else if let Some(secret) = self.secret.take() {
            let caret = secret.byte_at(caret / PASSWORD_MASK.len_utf8());
            self.editor.set_text(secret.as_str());
            let mut driver = self.editor.driver(font_ctx, layout_ctx);
            driver.move_to_byte(caret);
        }
    }

    /// Where a password input's selection starts, in characters: taken
    /// before an edit, for [`Self::reconcile`] after it.
    fn mask_start(&self) -> usize {
        self.editor.raw_selection().text_range().start / PASSWORD_MASK.len_utf8()
    }

    /// After an edit to a password input: what the edit put in the editor,
    /// among the bullets, moved into the text, and the editor all bullets
    /// again. Every edit leaves the caret just after what it inserted, so
    /// the text the edit replaced runs from where it began (`start`, or the
    /// caret, whichever is first) to as many characters from the end as
    /// follow the caret.
    fn reconcile(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        start: usize,
    ) {
        let Some(secret) = &self.secret else { return };
        let raw = self.editor.raw_text();
        let caret_byte = self.editor.raw_selection().focus().index().min(raw.len());
        let caret = raw[..caret_byte].chars().count();
        let after = raw[caret_byte..].chars().count();
        let from = start.min(caret);
        let inserted = zeroize::Zeroizing::new(
            raw.chars()
                .skip(from)
                .take(caret - from)
                .collect::<String>(),
        );
        let to = secret.chars().saturating_sub(after).max(from);
        let next = secret.splice(from, to, &inserted);
        let chars = next.chars();
        self.secret = Some(next);
        self.editor.set_text(&mask(chars));
        let mut driver = self.editor.driver(font_ctx, layout_ctx);
        driver.move_to_byte(caret * PASSWORD_MASK.len_utf8());
    }

    /// An edit's result, reconciled into a password input's text.
    fn edited(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        start: usize,
        event: Option<GeneratedTextInputEvent>,
    ) -> Option<GeneratedTextInputEvent> {
        if self.is_password() && matches!(event, Some(GeneratedTextInputEvent::Input)) {
            self.reconcile(font_ctx, layout_ctx, start);
        }
        event
    }

    /// Recompute [`Self::scroll_offset`] so that the caret stays visible within the input's
    /// content box.
    ///
    /// `content_box_width` and `content_box_height` are the dimensions of the input's content
    /// box in CSS (unscaled) pixels.
    pub fn clamp_scroll_offset(&mut self, content_box_width: f32, content_box_height: f32) {
        let Some(layout) = self.editor.try_layout() else {
            return;
        };
        // Parley lays out at the editor's scale, so its geometry is in scaled (device) pixels.
        // We convert into CSS (unscaled) pixels to match `scroll_offset` and the content box.
        let scale = layout.scale();

        // The caret geometry relative to the start of the text content.
        let Some(caret) = self.editor.cursor_geometry(1.5) else {
            return;
        };

        // Caret bounds and content/viewport extents along the scrolling axis (CSS pixels).
        let (caret_start, caret_end, content, viewport) = if self.is_multiline {
            (
                caret.y0 as f32 / scale,
                caret.y1 as f32 / scale,
                layout.height() / scale,
                content_box_height,
            )
        } else {
            (
                caret.x0 as f32 / scale,
                caret.x1 as f32 / scale,
                layout.full_width() / scale,
                content_box_width,
            )
        };

        let mut offset = self.scroll_offset;

        // Scroll so that both edges of the caret are within the visible region.
        if caret_end > offset + viewport {
            offset = caret_end - viewport;
        }
        if caret_start < offset {
            offset = caret_start;
        }

        // Never scroll past the content, and never scroll into negative space. The content
        // extent includes the caret so that a caret at the very end remains fully visible
        // (its rendered width extends slightly past the text).
        let max_offset = (content.max(caret_end) - viewport).max(0.0);
        self.scroll_offset = offset.clamp(0.0, max_offset);
    }

    /// The maximum valid value of [`Self::scroll_offset`] (in CSS pixels) given the input's
    /// content box, i.e. the extent by which the text content overflows the content box along
    /// the input's scroll axis.
    ///
    /// `content_box_width` and `content_box_height` are the dimensions of the input's content
    /// box in CSS (unscaled) pixels.
    pub fn max_scroll_offset(&self, content_box_width: f32, content_box_height: f32) -> f32 {
        let Some(layout) = self.editor.try_layout() else {
            return 0.0;
        };
        let scale = layout.scale();
        let (content, viewport) = if self.is_multiline {
            (layout.height() / scale, content_box_height)
        } else {
            (layout.full_width() / scale, content_box_width)
        };
        (content - viewport).max(0.0)
    }

    /// Scroll the input's text content by `delta` CSS pixels along its scroll axis (horizontal
    /// for single-line inputs, vertical for multi-line inputs), clamping to the scrollable
    /// range.
    ///
    /// Returns the portion of `delta` that could not be consumed (because the input was already
    /// scrolled to its limit), so the caller can bubble it up to an ancestor scroller.
    pub fn scroll_by(
        &mut self,
        delta: f32,
        content_box_width: f32,
        content_box_height: f32,
    ) -> f32 {
        let max_offset = self.max_scroll_offset(content_box_width, content_box_height);
        if max_offset <= 0.0 {
            return delta;
        }

        // Match the sign convention used for block scrolling: a positive delta decreases the
        // scroll offset.
        let new_offset = (self.scroll_offset - delta).clamp(0.0, max_offset);
        let consumed = self.scroll_offset - new_offset;
        self.scroll_offset = new_offset;
        delta - consumed
    }

    pub(crate) fn apply_keypress_event(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        shell_provider: &dyn ShellProvider,
        event: BlitzKeyEvent,
    ) -> Option<GeneratedTextInputEvent> {
        // PATCH(diaryx): a password is neither copied nor cut, as a
        // browser's is not; what else edits it is reconciled into its text.
        if self.is_password()
            && event.state.is_pressed()
            && event.modifiers.contains(ACTION_MOD)
            && matches!(&event.key, Key::Character(c) if matches!(c.to_lowercase().as_str(), "c" | "x"))
        {
            return None;
        }
        let start = self.mask_start();
        let r = self.apply_keypress_event_unmasked(font_ctx, layout_ctx, shell_provider, event);
        self.edited(font_ctx, layout_ctx, start, r)
    }

    fn apply_keypress_event_unmasked(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        shell_provider: &dyn ShellProvider,
        event: BlitzKeyEvent,
    ) -> Option<GeneratedTextInputEvent> {
        // Do nothing if it is a keyup event
        if !event.state.is_pressed() {
            return None;
        }

        let mods = event.modifiers;
        let shift = mods.contains(Modifiers::SHIFT);
        let action_mod = mods.contains(ACTION_MOD);

        let is_multiline = self.is_multiline;
        let editor = &mut self.editor;
        let mut driver = editor.driver(font_ctx, layout_ctx);
        match event.key {
            Key::Character(c) if action_mod && matches!(c.as_str(), "c" | "x" | "v") => {
                match c.to_lowercase().as_str() {
                    "c" => {
                        if let Some(text) = driver.editor.selected_text() {
                            let _ = shell_provider.set_clipboard_text(text.to_owned());
                        }
                    }
                    "x" => {
                        if let Some(text) = driver.editor.selected_text() {
                            let _ = shell_provider.set_clipboard_text(text.to_owned());
                            driver.delete_selection()
                        }
                    }
                    "v" => {
                        let text = shell_provider.get_clipboard_text().unwrap_or_default();
                        driver.insert_or_replace_selection(&text)
                    }
                    _ => unreachable!(),
                }

                return Some(GeneratedTextInputEvent::Input);
            }
            Key::Character(c) if action_mod && matches!(c.to_lowercase().as_str(), "a") => {
                if shift {
                    driver.collapse_selection()
                } else {
                    driver.select_all()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Named(NamedKey::ArrowLeft) => {
                if action_mod {
                    if shift {
                        driver.select_word_left()
                    } else {
                        driver.move_word_left()
                    }
                } else if shift {
                    driver.select_left()
                } else {
                    driver.move_left()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Named(NamedKey::ArrowRight) => {
                if action_mod {
                    if shift {
                        driver.select_word_right()
                    } else {
                        driver.move_word_right()
                    }
                } else if shift {
                    driver.select_right()
                } else {
                    driver.move_right()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Named(NamedKey::ArrowUp) => {
                if shift {
                    driver.select_up()
                } else {
                    driver.move_up()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Named(NamedKey::ArrowDown) => {
                if shift {
                    driver.select_down()
                } else {
                    driver.move_down()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Named(NamedKey::Home) => {
                if action_mod {
                    if shift {
                        driver.select_to_text_start()
                    } else {
                        driver.move_to_text_start()
                    }
                } else if shift {
                    driver.select_to_line_start()
                } else {
                    driver.move_to_line_start()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Named(NamedKey::End) => {
                if action_mod {
                    if shift {
                        driver.select_to_text_end()
                    } else {
                        driver.move_to_text_end()
                    }
                } else if shift {
                    driver.select_to_line_end()
                } else {
                    driver.move_to_line_end()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Named(NamedKey::Delete) => {
                if action_mod {
                    driver.delete_word()
                } else {
                    driver.delete()
                }
                return Some(GeneratedTextInputEvent::Input);
            }

            // On macOS this is handled by the apple standard keybindings
            #[cfg(not(target_os = "macos"))]
            Key::Named(NamedKey::Backspace) => {
                if action_mod {
                    driver.backdelete_word()
                } else {
                    driver.backdelete()
                }
                return Some(GeneratedTextInputEvent::Input);
            }

            Key::Character(c) if c == "\n" => {
                if is_multiline {
                    driver.insert_or_replace_selection("\n");
                    return Some(GeneratedTextInputEvent::Input);
                } else {
                    return Some(GeneratedTextInputEvent::Submit);
                }
            }
            Key::Named(NamedKey::Enter) => {
                if is_multiline {
                    driver.insert_or_replace_selection("\n");
                    return Some(GeneratedTextInputEvent::Input);
                } else {
                    return Some(GeneratedTextInputEvent::Submit);
                }
            }
            Key::Character(s)
                if !mods.contains(Modifiers::CONTROL) && !mods.contains(Modifiers::META) =>
            {
                driver.insert_or_replace_selection(&s);
                return Some(GeneratedTextInputEvent::Input);
            }
            _ => {}
        };

        None
    }

    pub(crate) fn apply_apple_standard_keybinding(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        shell_provider: &dyn ShellProvider,
        command: &str,
    ) -> Option<GeneratedTextInputEvent> {
        // PATCH(diaryx): as a key pressed: no copying or cutting a password.
        if self.is_password() && matches!(command, "copy:" | "cut:") {
            return None;
        }
        let start = self.mask_start();
        let r = self.apply_apple_standard_keybinding_unmasked(
            font_ctx,
            layout_ctx,
            shell_provider,
            command,
        );
        self.edited(font_ctx, layout_ctx, start, r)
    }

    fn apply_apple_standard_keybinding_unmasked(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        shell_provider: &dyn ShellProvider,
        command: &str,
    ) -> Option<GeneratedTextInputEvent> {
        let editor = &mut self.editor;
        let mut driver = editor.driver(font_ctx, layout_ctx);
        let is_multiline = self.is_multiline;

        match command {
            // Inserting Content

            // Inserts a backtab character.
            "insertBacktab:" => {}
            // Inserts a container break, such as a new page break.
            "insertContainerBreak:" => {}
            // Inserts a double quotation mark without substituting a curly quotation mark.
            "insertDoubleQuoteIgnoringSubstitution:" => {
                driver.insert_or_replace_selection("\"");
                return Some(GeneratedTextInputEvent::Input);
            }
            // Inserts a line break character.
            "insertLineBreak:" => {
                driver.insert_or_replace_selection("\n");
                return Some(GeneratedTextInputEvent::Input);
            }
            // Inserts a newline character.
            "insertNewline:" => {
                if is_multiline {
                    driver.insert_or_replace_selection("\n");
                    return Some(GeneratedTextInputEvent::Input);
                } else {
                    return Some(GeneratedTextInputEvent::Submit);
                }
            }
            // Inserts a newline character without invoking the field editor’s normal handling to end editing.
            "insertNewlineIgnoringFieldEditor:" => {
                driver.insert_or_replace_selection("\n");
                return Some(GeneratedTextInputEvent::Input);
            }
            // Inserts a paragraph separator.
            "insertParagraphSeparator:" => {
                driver.insert_or_replace_selection("\n");
                return Some(GeneratedTextInputEvent::Input);
            }
            "insertSingleQuoteIgnoringSubstitution:" => {
                driver.insert_or_replace_selection("'");
                return Some(GeneratedTextInputEvent::Input);
            }
            // Inserts a tab character.
            "insertTab:" | "insertTabIgnoringFieldEditor:" => {
                // Ignore for now seeing as parley has poor support for laying out tabs
            }
            // Inserts the text you specify.
            "insertText:" => {}

            // Deleting Content

            // Deletes content moving backward from the current insertion point.
            // TODO: handle deleteBackwardByDecomposingPreviousCharacter separately
            "deleteBackward:" | "deleteBackwardByDecomposingPreviousCharacter:" => {
                driver.backdelete();
                return Some(GeneratedTextInputEvent::Input);
            }
            "deleteForward:" => {
                driver.delete();
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes content from the insertion point to the beginning of the current line.
            "deleteToBeginningOfLine:" => {
                if driver.editor.raw_selection().is_collapsed() {
                    driver.select_to_line_start();
                }
                driver.delete_selection();
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes content from the insertion point to the beginning of the current paragraph.
            "deleteToEndOfLine:" => {
                if driver.editor.raw_selection().is_collapsed() {
                    driver.select_to_line_end();
                }
                driver.delete_selection();
                return Some(GeneratedTextInputEvent::Input);
            }
            "deleteToBeginningOfParagraph:" => {
                if driver.editor.raw_selection().is_collapsed() {
                    driver.select_to_hard_line_start();
                }
                driver.delete_selection();
                return Some(GeneratedTextInputEvent::Input);
            }

            // Deletes content from the insertion point to the end of the current line.
            "deleteToEndOfParagraph:" => {
                if driver.editor.raw_selection().is_collapsed() {
                    driver.select_to_hard_line_end();
                }
                driver.delete_selection();
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes content from the insertion point to the end of the current paragraph.
            "deleteWordBackward:" => {
                driver.backdelete_word();
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes the word preceding the current insertion point.
            "deleteWordForward:" => {
                driver.delete_word();
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes the current selection, placing it in a temporary buffer, such as the Clipboard.
            "yank:" => {
                if let Some(text) = driver.editor.selected_text() {
                    let _ = shell_provider.set_clipboard_text(text.to_owned());
                    driver.delete_selection();
                    return Some(GeneratedTextInputEvent::Input);
                }
            }

            // Moving the Insertion Pointer

            // Moves the insertion pointer backward in the current content.
            "moveBackward:" => {
                driver.move_left(); // TODO: Bidi-aware
                return Some(GeneratedTextInputEvent::Select);
            }

            // Moves the insertion pointer down in the current content.
            "moveDown:" => {
                driver.move_down();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Moves the insertion pointer forward in the current content.
            "moveForward:" => {
                driver.move_right();
                return Some(GeneratedTextInputEvent::Select);
            } // TODO: Bidi-aware

            // Moves the insertion pointer left in the current content.
            "moveLeft:" => {
                driver.move_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Moves the insertion pointer right in the current content.
            "moveRight:" => {
                driver.move_right();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Moves the insertion pointer up in the current content.
            "moveUp:" => {
                driver.move_up();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Modifying the Selection

            // Extends the selection to include the content before the current selection.
            "moveBackwardAndModifySelection:" => {
                driver.select_left(); // TODO: Bidi-aware
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content below the current selection.
            "moveDownAndModifySelection:" => {
                driver.select_down();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content after the current selection.
            "moveForwardAndModifySelection:" => {
                driver.select_right(); // TODO: Bidi-aware
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content to the left of the current selection.
            "moveLeftAndModifySelection:" => {
                driver.select_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content to the right of the current selection.
            "moveRightAndModifySelection:" => {
                driver.select_right();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content above the current selection.
            "moveUpAndModifySelection:" => {
                driver.select_up();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Changing the Selection
            "selectAll:" => {
                driver.select_all();
                return Some(GeneratedTextInputEvent::Select);
            }
            "selectLine:" => {
                driver.move_to_line_start();
                driver.select_to_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "selectParagraph:" => {
                driver.move_to_hard_line_start();
                driver.select_to_hard_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "selectWord:" => {
                // TODO
            }

            // Moving the Selection in Documents
            "moveToBeginningOfDocument:" => {
                driver.move_to_text_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToBeginningOfDocumentAndModifySelection:" => {
                driver.select_to_text_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfDocument:" => {
                driver.move_to_text_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfDocumentAndModifySelection:" => {
                driver.move_to_text_end();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Moving the Selection in Paragraphs
            "moveParagraphBackwardAndModifySelection:" => {}
            "moveParagraphForwardAndModifySelection:" => {}
            "moveToBeginningOfParagraph:" => {
                driver.move_to_hard_line_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToBeginningOfParagraphAndModifySelection:" => {
                driver.select_to_hard_line_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfParagraph:" => {
                driver.move_to_hard_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfParagraphAndModifySelection:" => {
                driver.select_to_hard_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Moving the Selection in Lines of Text
            "moveToBeginningOfLine:" => {
                driver.move_to_line_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToBeginningOfLineAndModifySelection:" => {
                driver.select_to_line_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfLine:" => {
                driver.move_to_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfLineAndModifySelection:" => {
                driver.select_to_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToLeftEndOfLine:" => {
                driver.move_to_text_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToLeftEndOfLineAndModifySelection:" => {
                driver.select_to_line_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToRightEndOfLine:" => {
                driver.move_to_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToRightEndOfLineAndModifySelection:" => {
                driver.select_to_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Moving the Selection by Word Boundaries
            "moveWordBackward:" => {
                driver.move_word_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordBackwardAndModifySelection:" => {
                driver.select_word_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordForward:" => {
                driver.move_word_right();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordForwardAndModifySelection:" => {
                driver.select_word_right();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordLeft:" => {
                driver.move_word_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordLeftAndModifySelection:" => {
                driver.select_word_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordRight:" => {
                driver.move_word_right();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordRightAndModifySelection:" => {
                driver.select_word_right();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Scrolling Content

            // Scrolls the content down by a page.
            "scrollPageDown:" => {}
            // Scrolls the content up by a page.
            "scrollPageUp:" => {}
            // Scrolls the content down by a line.
            "scrollLineDown:" => {}
            // Scrolls the content up by a line.
            "scrollLineUp:" => {}
            // Scrolls the content to the beginning of the document.
            "scrollToBeginningOfDocument:" => {}
            // Scrolls the content to the end of the document.
            "scrollToEndOfDocument:" => {}
            // Moves the visible content region down by a page.
            "pageDown:" => {}
            // Moves the visible content region up by a page.
            "pageUp:" => {}
            // Moves the visible content region down by a page, and extends the current selection.
            "pageDownAndModifySelection:" => {}
            // Moves the visible content region up by a page, and extends the current selection.
            "pageUpAndModifySelection:" => {}
            // Moves the visible content region so the current selection is visually centered.
            "centerSelectionInVisibleArea:" => {}

            // Transposing Elements

            // Transposes the content around the current selection.
            "transpose:" => {}
            // Transposes the words around the current selection.
            "transposeWords:" => {}

            // Indenting Content
            // Indents the content at the current selection.
            "indent:" => {}

            // Canceling Operations
            // Cancels the current operation.
            "cancelOperation:" => {}

            // Supporting QuickLook
            // Invokes QuickLook to preview the current selection.
            "quickLookPreviewItems:" => {}

            // Supporting Writing Directions
            "makeBaseWritingDirectionLeftToRight:" => {}
            "makeBaseWritingDirectionNatural:" => {}
            "makeBaseWritingDirectionRightToLeft:" => {}
            "makeTextWritingDirectionLeftToRight:" => {}
            "makeTextWritingDirectionNatural:" => {}
            "makeTextWritingDirectionRightToLeft:" => {}

            // Changing Capitalization
            "capitalizeWord:" => {}
            "changeCaseOfLetter:" => {}
            "lowercaseWord:" => {}
            "uppercaseWord:" => {}

            // Supporting Marked Selections
            "setMark:" => {}
            "selectToMark:" => {}
            "deleteToMark:" => {}
            "swapWithMark:" => {}

            // Supporting Autocomplete
            "complete:" => {}

            // Instance Methods
            "showContextMenuForSelection:" => {}

            // Unknown command
            _ => {}
        };

        None
    }

    pub(crate) fn apply_ime_event(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        event: BlitzImeEvent,
    ) -> Option<GeneratedTextInputEvent> {
        // PATCH(diaryx): a password input takes an input method's committed
        // text, and never shows what it is composing. What it deletes round
        // the selection is counted in the text's bytes, which the bullets'
        // are not: it is deleted by the characters those bytes are.
        if self.is_password() {
            let event = match event {
                BlitzImeEvent::Preedit(..) => {
                    self.editor.driver(font_ctx, layout_ctx).clear_compose();
                    return None;
                }
                BlitzImeEvent::DeleteSurrounding {
                    before_bytes,
                    after_bytes,
                } => {
                    let secret = self.secret.as_ref().map(Secret::as_str).unwrap_or("");
                    let range = self.editor.raw_selection().text_range();
                    let width = PASSWORD_MASK.len_utf8();
                    let (lo, hi) = (range.start / width, range.end / width);
                    let lo_byte = secret
                        .char_indices()
                        .nth(lo)
                        .map_or(secret.len(), |(i, _)| i);
                    let hi_byte = secret
                        .char_indices()
                        .nth(hi)
                        .map_or(secret.len(), |(i, _)| i);
                    let before = secret[..lo_byte]
                        .char_indices()
                        .rev()
                        .take_while(|(i, _)| lo_byte - i <= before_bytes)
                        .count();
                    let after = secret[hi_byte..]
                        .char_indices()
                        .take_while(|(i, c)| i + c.len_utf8() <= after_bytes)
                        .count();
                    BlitzImeEvent::DeleteSurrounding {
                        before_bytes: before * width,
                        after_bytes: after * width,
                    }
                }
                other => other,
            };
            let start = self.mask_start();
            let r = self.apply_ime_event_unmasked(font_ctx, layout_ctx, event);
            return self.edited(font_ctx, layout_ctx, start, r);
        }
        self.apply_ime_event_unmasked(font_ctx, layout_ctx, event)
    }

    fn apply_ime_event_unmasked(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        event: BlitzImeEvent,
    ) -> Option<GeneratedTextInputEvent> {
        let editor = &mut self.editor;
        let mut driver = editor.driver(font_ctx, layout_ctx);

        match event {
            BlitzImeEvent::Enabled => {
                // Do nothing
                None
            }
            BlitzImeEvent::Disabled => {
                driver.clear_compose();
                Some(GeneratedTextInputEvent::PreEditChange)
            }
            BlitzImeEvent::Commit(text) => {
                driver.insert_or_replace_selection(&text);
                Some(GeneratedTextInputEvent::Input)
            }
            BlitzImeEvent::Preedit(text, cursor) => {
                if text.is_empty() {
                    driver.clear_compose();
                } else {
                    driver.set_compose(&text, cursor);
                }
                Some(GeneratedTextInputEvent::PreEditChange)
            }
            BlitzImeEvent::DeleteSurrounding {
                before_bytes,
                after_bytes,
            } => {
                // The input method's text either side of the selection, gone
                // before its next step: a correction, a word composed again,
                // or Backspace from a keyboard that edits text rather than
                // sending keys (Android's, through an application's own
                // connection to it).
                if let Some(after) = std::num::NonZeroUsize::new(after_bytes) {
                    driver.delete_bytes_after_selection(after);
                }
                if let Some(before) = std::num::NonZeroUsize::new(before_bytes) {
                    driver.delete_bytes_before_selection(before);
                }
                (before_bytes > 0 || after_bytes > 0).then_some(GeneratedTextInputEvent::Input)
            }
        }
    }
}

#[cfg(test)]
mod password_tests {
    use std::sync::Mutex;

    use blitz_traits::events::{BlitzImeEvent, BlitzKeyEvent, KeyState};
    use blitz_traits::shell::{ClipboardError, ShellProvider};
    use keyboard_types::{Code, Key, Location, Modifiers, NamedKey};
    use parley::{FontContext, LayoutContext};

    use super::{GeneratedTextInputEvent, TextBrush, TextInputData};
    use crate::util::ACTION_MOD;

    #[derive(Default)]
    struct Clipboard(Mutex<String>);

    impl ShellProvider for Clipboard {
        fn get_clipboard_text(&self) -> Result<String, ClipboardError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn set_clipboard_text(&self, text: String) -> Result<(), ClipboardError> {
            *self.0.lock().unwrap() = text;
            Ok(())
        }
    }

    struct Field {
        data: TextInputData,
        font: FontContext,
        layout: LayoutContext<TextBrush>,
        shell: Clipboard,
    }

    impl Field {
        fn password(text: &str) -> Field {
            let mut f = Field {
                data: TextInputData::new(false),
                font: FontContext::new(),
                layout: LayoutContext::new(),
                shell: Clipboard::default(),
            };
            f.data.editor.set_scale(1.0);
            f.data.editor.set_text(text);
            f.data
                .editor
                .driver(&mut f.font, &mut f.layout)
                .move_to_text_end();
            f.data.set_password(&mut f.font, &mut f.layout, true);
            f
        }

        fn press(&mut self, key: Key, mods: Modifiers) -> Option<GeneratedTextInputEvent> {
            self.data.apply_keypress_event(
                &mut self.font,
                &mut self.layout,
                &self.shell,
                BlitzKeyEvent {
                    key,
                    code: Code::Unidentified,
                    modifiers: mods,
                    location: Location::Standard,
                    is_auto_repeating: false,
                    is_composing: false,
                    state: KeyState::Pressed,
                    text: None,
                },
            )
        }

        fn type_text(&mut self, s: &str) {
            for c in s.chars() {
                self.press(Key::Character(c.to_string()), Modifiers::empty());
            }
        }

        fn named(&mut self, key: NamedKey) {
            self.press(Key::Named(key), Modifiers::empty());
        }

        fn ime(&mut self, event: BlitzImeEvent) -> Option<GeneratedTextInputEvent> {
            self.data
                .apply_ime_event(&mut self.font, &mut self.layout, event)
        }

        /// What the editor holds: what is laid out and drawn.
        fn shown(&self) -> &str {
            self.data.editor.raw_text()
        }
    }

    fn bullets(n: usize) -> String {
        "•".repeat(n)
    }

    #[test]
    fn only_bullets_are_laid_out() {
        let mut f = Field::password("");
        f.type_text("hunter2");
        assert_eq!(f.data.value(), "hunter2");
        assert_eq!(f.shown(), bullets(7));
        assert!(f.data.editor.try_layout().is_some());
    }

    #[test]
    fn edits_in_the_middle_land_in_the_text() {
        let mut f = Field::password("pässwörd");
        f.named(NamedKey::Home);
        f.named(NamedKey::ArrowRight);
        f.type_text("X");
        assert_eq!(f.data.value(), "pXässwörd");
        f.named(NamedKey::Delete);
        assert_eq!(f.data.value(), "pXsswörd");
        #[cfg(not(target_os = "macos"))]
        {
            f.named(NamedKey::Backspace);
            assert_eq!(f.data.value(), "psswörd");
        }
        f.named(NamedKey::End);
        f.type_text("•!");
        assert!(f.data.value().ends_with("wörd•!"), "{}", f.data.value());
        assert_eq!(f.shown(), bullets(f.data.value().chars().count()));
    }

    #[test]
    fn a_selection_is_replaced() {
        let mut f = Field::password("secret");
        f.press(Key::Character("a".into()), ACTION_MOD);
        f.type_text("new");
        assert_eq!(f.data.value(), "new");
        assert_eq!(f.shown(), bullets(3));
    }

    #[test]
    fn it_is_pasted_into_but_never_copied_or_cut() {
        let mut f = Field::password("abc");
        f.press(Key::Character("a".into()), ACTION_MOD);
        assert!(f.press(Key::Character("c".into()), ACTION_MOD).is_none());
        assert!(f.press(Key::Character("x".into()), ACTION_MOD).is_none());
        assert_eq!(
            *f.shell.0.lock().unwrap(),
            "",
            "nothing reached the clipboard"
        );
        assert_eq!(f.data.value(), "abc");
        *f.shell.0.lock().unwrap() = "pasted".into();
        f.press(Key::Character("v".into()), ACTION_MOD);
        assert_eq!(f.data.value(), "pasted");
        assert_eq!(f.shown(), bullets(6));
    }

    #[test]
    fn an_input_method_commits_but_does_not_compose_in_view() {
        let mut f = Field::password("ab");
        assert!(
            f.ime(BlitzImeEvent::Preedit("ç".into(), Some((0, 2))))
                .is_none()
        );
        assert_eq!(f.shown(), bullets(2), "what it composes is not drawn");
        f.ime(BlitzImeEvent::Commit("çd".into()));
        assert_eq!(f.data.value(), "abçd");
        // Three bytes of the text before the caret: "çd", two characters,
        // not the one bullet three bytes are.
        f.ime(BlitzImeEvent::DeleteSurrounding {
            before_bytes: 3,
            after_bytes: 0,
        });
        assert_eq!(f.data.value(), "ab");
        assert_eq!(f.shown(), bullets(2));
    }

    #[test]
    fn shown_and_hidden_again_it_keeps_its_text_and_caret() {
        let mut f = Field::password("abcd");
        f.named(NamedKey::ArrowLeft);
        f.data.set_password(&mut f.font, &mut f.layout, false);
        assert_eq!(f.shown(), "abcd");
        assert_eq!(f.data.value(), "abcd");
        assert!(!f.data.is_password());
        f.type_text("X");
        assert_eq!(f.data.value(), "abcXd", "the caret stayed before the d");
        f.data.set_password(&mut f.font, &mut f.layout, true);
        assert_eq!(f.shown(), bullets(5));
        f.type_text("Y");
        assert_eq!(f.data.value(), "abcXYd");
    }

    #[test]
    fn a_value_set_from_the_page_replaces_the_text() {
        let mut f = Field::password("old");
        f.data.set_text(&mut f.font, &mut f.layout, "brand new");
        assert_eq!(f.data.value(), "brand new");
        assert_eq!(f.shown(), bullets(9));
        assert_eq!(
            format!("{:?}", f.data.secret.as_ref().unwrap()),
            "Secret(9 characters)"
        );
    }
}
