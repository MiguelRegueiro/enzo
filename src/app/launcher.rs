use std::{
    io::{self, BufWriter, Write},
    path::Path,
};

use anyhow::Result;
use crossterm::{
    cursor::MoveTo,
    execute,
    style::{Attribute, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};

use crate::{
    font_system::FontSystem,
    terminal::{TerminalGuard, clear_screen_and_images},
};

use super::{
    cli::media_path_from_drop_text,
    playback,
    terminal_input::{DropCommand, read_drop_events},
};

pub(super) fn run(
    sub_file: Option<&Path>,
    resume_enabled: bool,
    font_system: &FontSystem,
) -> Result<()> {
    let _terminal = TerminalGuard::enter()?;
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let mut status = None::<String>;

    loop {
        if crate::shutdown::requested() {
            return Ok(());
        }
        draw(&mut out, status.as_deref())?;
        let input = read_drop_events()?;
        if crate::shutdown::requested() {
            return Ok(());
        }
        if input.command == DropCommand::Quit {
            return Ok(());
        }
        let Some(text) = input.text else {
            continue;
        };

        match media_path_from_drop_text(&text) {
            Ok(path) => {
                clear_screen_and_images(&mut out)?;
                out.flush()?;
                drop(out);
                return playback::play(path, sub_file, resume_enabled, font_system);
            }
            Err(error) => {
                status = Some(error.to_string());
            }
        }
    }
}

fn draw(out: &mut impl Write, status: Option<&str>) -> io::Result<()> {
    let (cols, rows) = terminal::size().unwrap_or((80, 24));
    execute!(out, Clear(ClearType::All))?;

    write_centered(
        out,
        cols,
        rows.saturating_div(2).saturating_sub(2),
        "Drop files or URLs to play",
        true,
    )?;
    write_centered(out, cols, rows.saturating_div(2), "q to quit", false)?;
    if let Some(status) = status.filter(|status| !status.is_empty()) {
        write_centered(
            out,
            cols,
            rows.saturating_div(2).saturating_add(2),
            status,
            false,
        )?;
    }

    out.flush()
}

fn write_centered(
    out: &mut impl Write,
    cols: u16,
    row: u16,
    text: &str,
    bold: bool,
) -> io::Result<()> {
    let width = text.chars().count().min(u16::MAX as usize) as u16;
    let col = cols.saturating_sub(width) / 2;
    execute!(
        out,
        MoveTo(col, row),
        SetForegroundColor(crossterm::style::Color::White)
    )?;
    if bold {
        execute!(out, SetAttribute(Attribute::Bold))?;
    }
    execute!(out, Print(text), SetAttribute(Attribute::Reset), ResetColor)
}
