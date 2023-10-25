use crate::Document;
use crate::Row;
use crate::Terminal;

use cli_clipboard::{ClipboardContext, ClipboardProvider};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use crossterm::style::{Color, Print};
use crossterm::execute;
use crossterm::terminal as CrossTerminal;
use std::env::{self, Args};
use std::error::Error as Err;
use std::io::Error as IOError;
use std::io::stdout;
use std::time::{Duration, Instant};
use thiserror::Error;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const STATUS_FG_COLOR: Color = Color::Rgb {
    r: 63,
    g: 63,
    b: 63,
};
const STATUS_BG_COLOR: Color = Color::Rgb {
    r: 239,
    g: 239,
    b: 239,
};
const QUIT_TIME: u8 = 1;

#[derive(Clone, Copy, PartialEq)]
pub enum SearchDirection {
    Forward,
    Backward,
}

#[derive(Default, Clone)]
pub struct Position {
    pub x: usize,
    pub y: usize,
}

struct StatusMessage {
    text: String,
    time: Instant,
}

impl StatusMessage {
    fn from(message: String) -> Self {
        Self {
            time: Instant::now(),
            text: message,
        }
    }
}

pub struct Revise {
    should_quit: bool,
    terminal: Terminal,
    cursor_position: Position,
    offset: Position,
    document: Document,
    status_message: StatusMessage,
    quit_times: u8,
    highlighted_word: Option<String>,
    clipboard: ClipboardContext,
}

#[derive(Debug, Error)]
#[error("Cannot copy content")]
pub struct CopyError;

impl Drop for Revise {
    fn drop(&mut self) {
        CrossTerminal::disable_raw_mode().expect("unable to turn off raw mode");
    }
}

impl Revise {
    pub fn new() -> Result<Self, Box<dyn Err>> {
        let mut args: Args = env::args();
        let mut initial_status =
            String::from("HELP: Ctrl-F = find | Ctrl-S = save | Ctrl-Q = quit");
        let document = if args.len() > 1 {
            let filename = args.nth(1);

            match filename {
                Some(f) => {
                    let doc = Document::open(f.as_str());

                    if let Ok(content) = doc {
                        content
                    } else {
                        initial_status = format!("ERR: Could not open file: {f}");
                        Document::default()
                    }
                }
                None => Document::default(),
            }
        } else {
            Document::default()
        };
        let terminal = Terminal::new()?;
        let clipboard = ClipboardContext::new()?;

        Ok(Self {
            should_quit: false,
            terminal,
            cursor_position: Position::default(),
            offset: Position::default(),
            document,
            status_message: StatusMessage::from(initial_status),
            quit_times: QUIT_TIME,
            highlighted_word: None,
            clipboard,
        })
    }

    pub fn run(&mut self) -> Result<(), Box<dyn Err>> {
        loop {
            if let Err(error) = self.refresh_screen() {
                match self.clipboard.clear() {
                    Ok(_) => return Err(error),
                    Err(e) => return Err(e),
                }
            }

            if self.should_quit {
                break;
            }

            if let Err(error) = self.process_event() {
                match self.clipboard.clear() {
                    Ok(_) => return Err(Box::new(error)),
                    Err(e) => return Err(e),
                }
            }
        }

        Ok(())
    }

    pub fn draw_row(&self, row: &Row) {
        let start = self.offset.x;
        let width = self.terminal.size().width as usize;
        let end = start.saturating_add(width);
        let row = row.render(start, end);

        execute!(stdout(), Print(row)).unwrap();
    }

    fn process_event(&mut self) -> Result<(), IOError> {
        let event = Terminal::read_event()?;

        let _ = match event {
            Event::Key(k) => self.process_key(k),
            _ => Ok(()),
            // Key::Ctrl('c') => match self.copy_content() {
            //     Ok(_) => (),
            //     Err(err) => self.status_message = StatusMessage::from(format!("{err}")),
            // },
            // Key::Ctrl('v') => match self.paste_content() {
            //     Ok(v) => {
            //         for c in v.chars().rev() {
            //             match self.document.insert(&self.cursor_position, c) {
            //                 Ok(_) => (),
            //                 Err(err) => {
            //                     self.status_message =
            //                         StatusMessage::from(format!("Failed to paste content: {err}"))
            //                 }
            //             }
            //         }
            //     }
            //     Err(err) => {
            //         self.status_message =
            //             StatusMessage::from(format!("Failed to paste content: {err}"))
            //     }
            // },
            // Key::Ctrl('q') => return self.quit(),
            // Key::Ctrl('s') => self.save(),
            // Key::Ctrl('f') => self.search(),
            // Key::Char(c) => match self.document.insert(&self.cursor_position, c) {
            //     Ok(_) => self.move_cursor(Key::Right),
            //     Err(err) => {
            //         self.status_message =
            //             StatusMessage::from(format!("Failed to paste content: {err}"))
            //     }
            // },
            // Key::Delete => match self.document.delete(&self.cursor_position) {
            //     Ok(_) => (),
            //     Err(err) => {
            //         self.status_message =
            //             StatusMessage::from(format!("Failed to remove content: {err}"))
            //     }
            // },
            // Key::Backspace => {
            //     if self.cursor_position.x > 0 || self.cursor_position.y > 0 {
            //         self.move_cursor(Key::Left);

            //         match self.document.delete(&self.cursor_position) {
            //             Ok(_) => (),
            //             Err(err) => {
            //                 self.status_message =
            //                     StatusMessage::from(format!("Failed to remove character: {err}"))
            //             }
            //         }
            //     }
            // }
            // Key::Up
            // | Key::Down
            // | Key::Left
            // | Key::Right
            // | Key::PageUp
            // | Key::PageDown
            // | Key::End
            // | Key::Home => self.move_cursor(pressed_key),
            // _ => (),
        };

        self.scroll();

        if self.quit_times < QUIT_TIME {
            self.quit_times = QUIT_TIME;
            self.status_message = StatusMessage::from(String::new());
        }

        Ok(())
    }

    fn process_key(&mut self, event: KeyEvent) -> Result<(), IOError> {
        match event {
            KeyEvent {
                code: KeyCode::Char('q'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press,
                state: _,
            } => return self.quit(),
            KeyEvent {
                code: KeyCode::Char('s'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press,
                state: _,
            } => self.save(),
            KeyEvent {
                code: KeyCode::Char('f'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press,
                state: _,
            } => self.search(),
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers: KeyModifiers::NONE | KeyModifiers::SHIFT,
                kind: KeyEventKind::Press,
                state: _,
            } => {
                match self.document.insert(&self.cursor_position, c) {
                    Ok(_) => self.move_cursor(KeyEvent {
                        code: KeyCode::Right,
                        modifiers: KeyModifiers::NONE,
                        kind: KeyEventKind::Press,
                        state: KeyEventState::NONE,
                    }),
                    Err(err) => self.status_message = StatusMessage::from(format!("Failed to paste content: {err}"))
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn quit(&mut self) -> Result<(), IOError> {
        if self.quit_times > 0 && self.document.is_changed() {
            self.status_message = StatusMessage::from(format!(
                "WARNING! File has unsaved changes. Press Ctrl-Q {} more time to quit.",
                self.quit_times
            ));
            self.quit_times -= 1;

            return Ok(());
        }
        self.should_quit = true;

        Ok(())
    }

    fn refresh_screen(&mut self) -> Result<(), Box<dyn Err>> {
        Terminal::cursor_hide();
        Terminal::cursor_position(&Position::default());

        if self.should_quit {
            Terminal::clear_screen();
        } else {
            match self.document.highlight(
                &self.highlighted_word,
                Some(
                    self.offset
                        .y
                        .saturating_add(self.terminal.size().height as usize),
                ),
            ) {
                Ok(_) => {
                    self.draw_rows();
                    self.draw_status_bar();
                    self.draw_message_bar();
                    Terminal::cursor_position(&Position {
                        x: self.cursor_position.x.saturating_sub(self.offset.x),
                        y: self.cursor_position.y.saturating_sub(self.offset.y),
                    });
                }
                Err(err) => return Err(err),
            }
        }

        Terminal::cursor_show();

        match Terminal::flush() {
            Ok(_) => Ok(()),
            Err(err) => return Err(Box::new(err)),
        }
    }

    fn draw_rows(&self) {
        let height = self.terminal.size().height;

        for terminal_row in 0..height {
            Terminal::clear_current_line();

            if let Some(row) = self
                .document
                .row(self.offset.y.saturating_add(terminal_row as usize))
            {
                self.draw_row(row);
            } else if self.document.is_empty() && terminal_row == height / 3 {
                self.draw_welcome_message();
            } else {
                println!("~\r");
            }
        }
    }

    fn draw_welcome_message(&self) {
        let mut welcome_message = format!("Revise | v{VERSION}");
        let width = self.terminal.size().width as usize;
        let len = welcome_message.len();
        let padding = width.saturating_sub(len) / 2;
        let spaces = " ".repeat(padding.saturating_sub(1));

        welcome_message = format!("~{spaces}{welcome_message}");
        welcome_message.truncate(width);

        println!("{welcome_message}\r");
    }

    fn move_cursor(&mut self, k: KeyEvent) {
        let terminal_height = self.terminal.size().height as usize;
        let Position { mut y, mut x } = self.cursor_position;
        let height = self.document.len();
        let mut width = if let Some(row) = self.document.row(y) {
            row.len()
        } else {
            0
        };

        match k {
            KeyEvent {
                code: KeyCode::Up,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            } => y = y.saturating_sub(1),
            KeyEvent {
                code: KeyCode::Down,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            } => {
                if y < height {
                    y = y.saturating_add(1);
                }
            }
            KeyEvent {
                code: KeyCode::Left,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            } => {
                if x > 0 {
                    x -= 1;
                } else if y > 0 {
                    y -= 1;

                    if let Some(row) = self.document.row(y) {
                        x = row.len();
                    } else {
                        x = 0;
                    }
                }
            }
            KeyEvent {
                code: KeyCode::Right,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            } => {
                if x < width {
                    x += 1;
                } else if y < height {
                    y += 1;
                    x = 0;
                }
            }
            KeyEvent {
                code: KeyCode::PageUp,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            } => {
                y = if y > terminal_height {
                    y.saturating_sub(terminal_height)
                } else {
                    0
                }
            }
            KeyEvent {
                code: KeyCode::PageDown,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            } => {
                y = if y.saturating_add(terminal_height) < height {
                    y.saturating_add(terminal_height)
                } else {
                    height
                }
            }
            KeyEvent {
                code: KeyCode::Home,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            } => x = 0,
            KeyEvent {
                code: KeyCode::End,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            } => x = width,
            _ => (),
        }

        width = if let Some(row) = self.document.row(y) {
            row.len()
        } else {
            0
        };

        if x > width {
            x = width;
        }

        self.cursor_position = Position { x, y }
    }

    fn scroll(&mut self) {
        let Position { x, y } = self.cursor_position;
        let width = self.terminal.size().width as usize;
        let height = self.terminal.size().height as usize;
        let offset = &mut self.offset;

        if y < offset.y {
            offset.y = y;
        } else if y >= offset.y.saturating_add(height) {
            offset.y = y.saturating_sub(height).saturating_add(1);
        }

        if x < offset.x {
            offset.x = x;
        } else if x >= offset.x.saturating_add(width) {
            offset.x = x.saturating_sub(width).saturating_add(1);
        }
    }

    fn draw_status_bar(&self) {
        let mut status;
        let width = self.terminal.size().width as usize;
        let changed_indicator = if self.document.is_changed() {
            " (changed)"
        } else {
            ""
        };
        let mut filename = "[No Name]".to_owned();

        if let Some(name) = &self.document.filename {
            filename = name.clone();
            filename.truncate(20);
        }

        status = format!(
            "{filename} - {} lines{changed_indicator}",
            self.document.len(),
        );
        let line_indicator = format!(
            "{} | {}/{}",
            self.document.file_type(),
            self.cursor_position.y.saturating_add(1),
            self.document.len(),
        );
        let len = status.len() + line_indicator.len();

        status.push_str(&" ".repeat(width.saturating_sub(len)));
        status = format!("{status}{line_indicator}");
        status.truncate(width);
        Terminal::set_bg_color(STATUS_BG_COLOR);
        Terminal::set_fg_color(STATUS_FG_COLOR);
        println!("{status}\r");
        Terminal::reset_color();
    }

    fn draw_message_bar(&self) {
        Terminal::clear_current_line();
        let message = &self.status_message;

        if message.time.elapsed() < Duration::new(5, 0) {
            let mut text = message.text.clone();

            text.truncate(self.terminal.size().width as usize);
            print!("{text}");
        }
    }

    fn prompt<C>(&mut self, prompt: &str, mut callback: C) -> Result<Option<String>, Box<dyn Err>>
    where
        C: FnMut(&mut Self, Event, &String),
    {
        let mut result = String::new();

        loop {
            self.status_message = StatusMessage::from(format!("{prompt}{result}"));
            self.refresh_screen()?;

            let event = Terminal::read_event()?;

            match event {
                Event::Key(k) => match k {
                    KeyEvent {
                        code: KeyCode::Backspace,
                        modifiers: KeyModifiers::NONE,
                        kind: KeyEventKind::Press,
                        state: _,
                    } => result.truncate(result.len().saturating_sub(1)),
                    KeyEvent {
                        code: KeyCode::Char('\n'),
                        modifiers: KeyModifiers::NONE,
                        kind: KeyEventKind::Press,
                        state: _,
                    } => break,
                    KeyEvent {
                        code: KeyCode::Char(c),
                        modifiers: KeyModifiers::NONE,
                        kind: KeyEventKind::Press,
                        state: _,
                    } => {
                        if !c.is_control() {
                            result.push(c);
                        }
                    }
                    KeyEvent {
                        code: KeyCode::Esc,
                        modifiers: KeyModifiers::NONE,
                        kind: KeyEventKind::Press,
                        state: _,
                    } => {
                        result.truncate(0);
                        break;
                    }
                    _ => (),
                },
                _ => (),
            }
            callback(self, event, &result);
        }

        self.status_message = StatusMessage::from(String::new());

        if result.is_empty() {
            return Ok(None);
        }

        Ok(Some(result))
    }

    fn save(&mut self) {
        if self.document.filename.is_none() {
            let new_name = self.prompt("Save as: ", |_, _, _| {}).unwrap_or_default();

            if new_name.is_none() {
                self.status_message = StatusMessage::from("Save aborted.".to_owned());
                return;
            }
            self.document.filename = new_name;
        }

        if self.document.save().is_ok() {
            self.status_message = StatusMessage::from("File saved successfully.".to_owned());
        } else {
            self.status_message = StatusMessage::from("Error writing file!".to_owned());
        }
    }

    fn search(&mut self) {
        let old_position = self.cursor_position.clone();
        let mut direction = SearchDirection::Forward;
        let query = self
            .prompt(
                "Search (ESC to cancel, Arrows to navigate): ",
                |revise, event, query| {
                    let mut moved = false;

                    match event {
                        Event::Key(k) => match k {
                            KeyEvent {
                                code: KeyCode::Right,
                                modifiers: KeyModifiers::NONE,
                                kind: KeyEventKind::Press,
                                state: _,
                            }
                            | KeyEvent {
                                code: KeyCode::Down,
                                modifiers: KeyModifiers::NONE,
                                kind: KeyEventKind::Press,
                                state: _,
                            } => {
                                direction = SearchDirection::Forward;
                                revise.move_cursor(KeyEvent {
                                    code: KeyCode::Right,
                                    modifiers: KeyModifiers::NONE,
                                    kind: KeyEventKind::Press,
                                    state: KeyEventState::NONE,
                                });
                                moved = true;
                            }
                            KeyEvent {
                                code: KeyCode::Left,
                                modifiers: KeyModifiers::NONE,
                                kind: KeyEventKind::Press,
                                state: _,
                            }
                            | KeyEvent {
                                code: KeyCode::Up,
                                modifiers: KeyModifiers::NONE,
                                kind: KeyEventKind::Press,
                                state: _,
                            } => direction = SearchDirection::Backward,
                            _ => direction = SearchDirection::Forward,
                        },
                        _ => (),
                    }

                    if let Some(position) =
                        revise
                            .document
                            .find(query, &revise.cursor_position, direction)
                    {
                        revise.cursor_position = position;
                        revise.scroll();
                    } else if moved {
                        revise.move_cursor(KeyEvent {
                            code: KeyCode::Left,
                            modifiers: KeyModifiers::NONE,
                            kind: KeyEventKind::Press,
                            state: KeyEventState::NONE,
                        });
                    }

                    revise.highlighted_word = Some(query.to_owned());
                },
            )
            .unwrap_or_default();

        if query.is_none() {
            self.cursor_position = old_position;
            self.scroll();
        }

        self.highlighted_word = None;
    }

    fn copy_content(&mut self) -> Result<(), Box<dyn Err>> {
        let row = self.document.row(self.cursor_position.y);

        match row {
            Some(v) => self.clipboard.set_contents(v.as_string().to_owned()),
            None => Err(Box::new(CopyError)),
        }
    }

    fn paste_content(&mut self) -> Result<String, Box<dyn Err>> {
        let content = self.clipboard.get_contents();

        match content {
            Ok(mut v) => {
                if v.is_empty() {
                    v = String::from(" ");
                    self.cursor_position.y = self.cursor_position.y.saturating_add(1);
                }

                Ok(v)
            }
            Err(err) => Err(err),
        }
    }
}
