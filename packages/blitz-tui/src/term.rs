//! The terminal side: raw mode and the alternate screen, and a grid written
//! out as the cells that changed, its pictures over them.

use std::io::{self, Stdout, stdout};

use blitz_dom::BaseDocument;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::{cursor, execute, terminal};
use ratatui::backend::CrosstermBackend;
use ratatui::style::{Color, Modifier};

use crate::paint::Grid;
use crate::pictures::{Decoder, Pictures};

/// Ask the terminal whether it is dark (an `OSC 11` query, with a timeout,
/// which terminal-colorsaurus answers from the controlling tty without
/// stealing a keystroke). `None` when it does not say. Ask before
/// [`Terminal::enter`].
pub fn ask_dark() -> Option<bool> {
    let palette = terminal_colorsaurus::color_palette(Default::default()).ok()?;
    Some(palette.theme_mode() == terminal_colorsaurus::ThemeMode::Dark)
}

pub struct Terminal {
    term: ratatui::Terminal<CrosstermBackend<Stdout>>,
    pictures: Pictures,
}

impl Terminal {
    /// Raw mode, the alternate screen, the mouse, bracketed paste and a bar
    /// cursor; then the terminal is asked which graphics protocol it speaks.
    pub fn enter() -> io::Result<Self> {
        let mut out = stdout();
        terminal::enable_raw_mode()?;
        execute!(
            out,
            terminal::EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste,
            cursor::SetCursorStyle::SteadyBar,
        )?;
        let mut pictures = Pictures::new();
        pictures.query();
        let mut term = ratatui::Terminal::new(CrosstermBackend::new(out))?;
        term.clear()?;
        Ok(Self { term, pictures })
    }

    /// Everything [`Terminal::enter`] turned on, off again.
    pub fn leave(&mut self) -> io::Result<()> {
        execute!(
            self.term.backend_mut(),
            DisableBracketedPaste,
            DisableMouseCapture,
            cursor::SetCursorStyle::DefaultUserShape,
            cursor::Show,
            terminal::LeaveAlternateScreen
        )?;
        terminal::disable_raw_mode()
    }

    pub fn pictures(&mut self) -> &mut Pictures {
        &mut self.pictures
    }

    /// Read pictures' sources with `decoder` before Blitz's image data.
    pub fn set_decoder(&mut self, decoder: Decoder) {
        self.pictures.set_decoder(decoder);
    }

    /// Forget what is on screen: the next draw writes every cell.
    pub fn clear(&mut self) -> io::Result<()> {
        self.term.clear()
    }

    /// Draw `g`, painted from `doc`: its cells, then its pictures over them.
    /// ratatui writes only the cells that changed since the last frame.
    pub fn draw(&mut self, g: &Grid, doc: &BaseDocument) -> io::Result<()> {
        let pictures = &mut self.pictures;
        self.term.draw(|f| {
            let area = f.area();
            let buf = f.buffer_mut();
            for r in 0..g.rows.min(area.height) {
                for c in 0..g.cols.min(area.width) {
                    let cell = g.get(c, r);
                    let out = &mut buf[(c, r)];
                    // The second half of a wide character: its first half
                    // drew it, and ratatui skips this cell.
                    out.set_symbol(if cell.text.is_empty() {
                        " "
                    } else {
                        &cell.text
                    });
                    out.set_fg(rgb(cell.fg));
                    out.set_bg(rgb(cell.bg));
                    let mut m = Modifier::empty();
                    m.set(Modifier::BOLD, cell.bold);
                    m.set(Modifier::ITALIC, cell.italic);
                    m.set(Modifier::UNDERLINED, cell.underline);
                    out.modifier = m;
                }
            }
            pictures.paint(f, g, doc);
            if let Some((c, r)) = g.cursor {
                f.set_cursor_position((c, r));
            }
        })?;
        Ok(())
    }
}

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::Rgb(r, g, b)
}
