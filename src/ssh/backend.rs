use crate::cursive::backends::termion::termion;
use crate::cursive::backends::termion::termion::color as tcolor;
use crate::cursive::backends::termion::termion::event::Event as TEvent;
use crate::cursive::backends::termion::termion::event::Key as TKey;
use crate::cursive::backends::termion::termion::event::MouseButton as TMouseButton;
use crate::cursive::backends::termion::termion::event::MouseEvent as TMouseEvent;
use crate::cursive::backends::termion::termion::input::{Events, TermRead};
use crate::cursive::backends::termion::termion::style as tstyle;

use crate::cursive::backend;
use crate::cursive::event::{Event, Key, MouseButton, MouseEvent};
use crate::cursive::theme;
use crate::cursive::Vec2;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;

use std::cell::Cell;
use std::cell::RefCell;
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CursiveOutput {
    Data(Vec<u8>),
    Close,
}

/// Backend using termion
pub struct Backend {
    current_style: Cell<theme::ColorPair>,

    // Inner state required to parse input
    last_button: Option<MouseButton>,

    events: Events<File>,

    // Raw input file descriptor, to fix the file on exit, since we can't
    // (currently) get it from events.
    #[cfg(unix)]
    input_fd: std::os::unix::io::RawFd,

    running: Arc<AtomicBool>,

    output_sender: Sender<CursiveOutput>,
    resize_receiver: Receiver<Vec2>,
    relayout_sender: Sender<()>,
    size: Vec2,
    data: RefCell<Vec<u8>>,
}

/// Set the given file to be read in non-blocking mode. That is, attempting a
/// read on the given file may return 0 bytes.
///
/// Copied from private function at https://docs.rs/nonblock/0.1.0/nonblock/.
///
/// The MIT License (MIT)
///
/// Copyright (c) 2016 Anthony Nowell
///
/// Permission is hereby granted, free of charge, to any person obtaining a copy
/// of this software and associated documentation files (the "Software"), to deal
/// in the Software without restriction, including without limitation the rights
/// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
/// copies of the Software, and to permit persons to whom the Software is
/// furnished to do so, subject to the following conditions:
///
/// The above copyright notice and this permission notice shall be included in all
/// copies or substantial portions of the Software.
///
/// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
/// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
/// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
/// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
/// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
/// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
/// SOFTWARE.
#[cfg(unix)]
fn set_blocking(fd: std::os::unix::io::RawFd, blocking: bool) -> std::io::Result<()> {
    use libc::{fcntl, F_GETFL, F_SETFL, O_NONBLOCK};

    let flags = unsafe { fcntl(fd, F_GETFL, 0) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let flags = if blocking {
        flags & !O_NONBLOCK
    } else {
        flags | O_NONBLOCK
    };
    let res = unsafe { fcntl(fd, F_SETFL, flags) };
    if res != 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(())
}

impl Backend {
    /// Creates a new termion-based backend using the given input and output files.
    pub fn init_ssh(
        input_file: File,
        output_sender: Sender<CursiveOutput>,
        resize_receiver: Receiver<Vec2>,
        relayout_sender: Sender<()>,
    ) -> std::io::Result<Box<dyn backend::Backend>> {
        #[cfg(unix)]
        use std::os::unix::io::AsRawFd;

        #[cfg(unix)]
        let input_fd = input_file.as_raw_fd();

        #[cfg(unix)]
        set_blocking(input_fd, false)?;

        let running = Arc::new(AtomicBool::new(true));

        let c = Backend {
            current_style: Cell::new(theme::ColorPair::from_256colors(0, 0)),

            last_button: None,
            events: input_file.events(),
            #[cfg(unix)]
            input_fd,
            running,
            output_sender,
            resize_receiver,
            relayout_sender,
            size: Vec2::new(1, 1),
            data: RefCell::new(Vec::new()),
        };

        c.write(format!("{}", termion::cursor::Hide));

        Ok(Box::new(c))
    }

    fn apply_colors(&self, colors: theme::ColorPair) {
        with_color(colors.front, |c| self.write(tcolor::Fg(c)));
        with_color(colors.back, |c| self.write(tcolor::Bg(c)));
    }

    fn map_key(&mut self, event: TEvent) -> Event {
        match event {
            TEvent::Unsupported(bytes) => Event::Unknown(bytes),
            TEvent::Key(TKey::Esc) => Event::Key(Key::Esc),
            TEvent::Key(TKey::Backspace) => Event::Key(Key::Backspace),
            TEvent::Key(TKey::Left) => Event::Key(Key::Left),
            TEvent::Key(TKey::Right) => Event::Key(Key::Right),
            TEvent::Key(TKey::Up) => Event::Key(Key::Up),
            TEvent::Key(TKey::Down) => Event::Key(Key::Down),
            TEvent::Key(TKey::Home) => Event::Key(Key::Home),
            TEvent::Key(TKey::End) => Event::Key(Key::End),
            TEvent::Key(TKey::PageUp) => Event::Key(Key::PageUp),
            TEvent::Key(TKey::PageDown) => Event::Key(Key::PageDown),
            TEvent::Key(TKey::Delete) => Event::Key(Key::Del),
            TEvent::Key(TKey::Insert) => Event::Key(Key::Ins),
            TEvent::Key(TKey::F(i)) if i < 12 => Event::Key(Key::from_f(i)),
            TEvent::Key(TKey::F(j)) => Event::Unknown(vec![j]),
            TEvent::Key(TKey::Char('\n')) => Event::Key(Key::Enter),
            TEvent::Key(TKey::Char('\t')) => Event::Key(Key::Tab),
            TEvent::Key(TKey::Char(c)) => Event::Char(c),
            TEvent::Key(TKey::Ctrl(c)) => Event::CtrlChar(c),
            TEvent::Key(TKey::Alt(c)) => Event::AltChar(c),
            TEvent::Mouse(TMouseEvent::Press(btn, x, y)) => {
                let position = (x - 1, y - 1).into();

                let event = match btn {
                    TMouseButton::Left => MouseEvent::Press(MouseButton::Left),
                    TMouseButton::Middle => MouseEvent::Press(MouseButton::Middle),
                    TMouseButton::Right => MouseEvent::Press(MouseButton::Right),
                    TMouseButton::WheelUp => MouseEvent::WheelUp,
                    TMouseButton::WheelDown => MouseEvent::WheelDown,
                };

                if let MouseEvent::Press(btn) = event {
                    self.last_button = Some(btn);
                }

                Event::Mouse {
                    event,
                    position,
                    offset: Vec2::zero(),
                }
            }
            TEvent::Mouse(TMouseEvent::Release(x, y)) if self.last_button.is_some() => {
                let event = MouseEvent::Release(self.last_button.unwrap());
                let position = (x - 1, y - 1).into();
                Event::Mouse {
                    event,
                    position,
                    offset: Vec2::zero(),
                }
            }
            TEvent::Mouse(TMouseEvent::Hold(x, y)) if self.last_button.is_some() => {
                let event = MouseEvent::Hold(self.last_button.unwrap());
                let position = (x - 1, y - 1).into();
                Event::Mouse {
                    event,
                    position,
                    offset: Vec2::zero(),
                }
            }
            _ => Event::Unknown(vec![]),
        }
    }

    fn write<T>(&self, content: T)
    where
        T: std::fmt::Display,
    {
        self.data
            .borrow_mut()
            .extend(format!("{}", content).as_bytes().to_vec());
    }

    fn close(&self) {
        // Flush the output queue.
        {
            let mut data = self.data.borrow_mut();
            if !data.is_empty() {
                self.output_sender
                    .blocking_send(CursiveOutput::Data(data.clone()))
                    .unwrap();
                data.clear();
            }
        }

        self.output_sender
            .blocking_send(CursiveOutput::Close)
            .unwrap();
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);

        #[cfg(unix)]
        set_blocking(self.input_fd, true).unwrap();

        self.write(format!(
            "{}{}",
            termion::cursor::Show,
            termion::cursor::Goto(1, 1)
        ));

        self.write(format!(
            "{}[49m{}[39m{}",
            27 as char,
            27 as char,
            termion::clear::All
        ));
        self.close();
    }
}

impl backend::Backend for Backend {
    fn name(&self) -> &str {
        "termion"
    }

    fn set_title(&mut self, title: String) {
        self.write(format!("\x1B]0;{}\x07", title));
    }

    fn set_color(&self, color: theme::ColorPair) -> theme::ColorPair {
        let current_style = self.current_style.get();

        if current_style != color {
            self.apply_colors(color);
            self.current_style.set(color);
        }

        current_style
    }

    fn set_effect(&self, effect: theme::Effect) {
        match effect {
            theme::Effect::Simple => (),
            theme::Effect::Reverse => self.write(tstyle::Invert),
            theme::Effect::Dim => self.write(tstyle::Faint),
            theme::Effect::Bold => self.write(tstyle::Bold),
            theme::Effect::Blink => self.write(tstyle::Blink),
            theme::Effect::Italic => self.write(tstyle::Italic),
            theme::Effect::Strikethrough => self.write(tstyle::CrossedOut),
            theme::Effect::Underline => self.write(tstyle::Underline),
        }
    }

    fn unset_effect(&self, effect: theme::Effect) {
        match effect {
            theme::Effect::Simple => (),
            theme::Effect::Reverse => self.write(tstyle::NoInvert),
            theme::Effect::Dim | theme::Effect::Bold => self.write(tstyle::NoFaint),
            theme::Effect::Blink => self.write(tstyle::NoBlink),
            theme::Effect::Italic => self.write(tstyle::NoItalic),
            theme::Effect::Strikethrough => self.write(tstyle::NoCrossedOut),
            theme::Effect::Underline => self.write(tstyle::NoUnderline),
        }
    }

    fn has_colors(&self) -> bool {
        // TODO: color support detection?
        true
    }

    fn screen_size(&self) -> Vec2 {
        self.size.clone()
    }

    fn clear(&self, color: theme::Color) {
        self.apply_colors(theme::ColorPair {
            front: color,
            back: color,
        });

        self.write(termion::clear::All);
    }

    fn refresh(&mut self) {
        // TODO: Is this important for ssh connections?
    }

    fn print_at(&self, pos: Vec2, text: &str) {
        self.write(format!(
            "{}{}",
            termion::cursor::Goto(1 + pos.x as u16, 1 + pos.y as u16),
            text
        ));
    }

    fn print_at_rep(&self, pos: Vec2, repetitions: usize, text: &str) {
        if repetitions > 0 {
            self.write(format!(
                "{}{}",
                termion::cursor::Goto(1 + pos.x as u16, 1 + pos.y as u16),
                text
            ));

            let mut dupes_left = repetitions - 1;
            while dupes_left > 0 {
                self.write(format!("{}", text));
                dupes_left -= 1;
            }
        }
    }

    fn poll_event(&mut self) -> Option<Event> {
        {
            let mut data = self.data.borrow_mut();
            if !data.is_empty() {
                self.output_sender
                    .blocking_send(CursiveOutput::Data(data.clone()))
                    .unwrap();
                data.clear();
            }
        }
        if let Ok(size) = self.resize_receiver.try_recv() {
            self.size = size;
            self.relayout_sender.blocking_send(()).unwrap();
        }
        if let Some(Ok(event)) = self.events.next() {
            Some(self.map_key(event))
        } else {
            None
        }
    }
}

fn with_color<F, R>(clr: theme::Color, f: F) -> R
where
    F: FnOnce(&dyn tcolor::Color) -> R,
{
    match clr {
        theme::Color::TerminalDefault => f(&tcolor::Reset),
        theme::Color::Dark(theme::BaseColor::Black) => f(&tcolor::Black),
        theme::Color::Dark(theme::BaseColor::Red) => f(&tcolor::Red),
        theme::Color::Dark(theme::BaseColor::Green) => f(&tcolor::Green),
        theme::Color::Dark(theme::BaseColor::Yellow) => f(&tcolor::Yellow),
        theme::Color::Dark(theme::BaseColor::Blue) => f(&tcolor::Blue),
        theme::Color::Dark(theme::BaseColor::Magenta) => f(&tcolor::Magenta),
        theme::Color::Dark(theme::BaseColor::Cyan) => f(&tcolor::Cyan),
        theme::Color::Dark(theme::BaseColor::White) => f(&tcolor::White),

        theme::Color::Light(theme::BaseColor::Black) => f(&tcolor::LightBlack),
        theme::Color::Light(theme::BaseColor::Red) => f(&tcolor::LightRed),
        theme::Color::Light(theme::BaseColor::Green) => f(&tcolor::LightGreen),
        theme::Color::Light(theme::BaseColor::Yellow) => f(&tcolor::LightYellow),
        theme::Color::Light(theme::BaseColor::Blue) => f(&tcolor::LightBlue),
        theme::Color::Light(theme::BaseColor::Magenta) => f(&tcolor::LightMagenta),
        theme::Color::Light(theme::BaseColor::Cyan) => f(&tcolor::LightCyan),
        theme::Color::Light(theme::BaseColor::White) => f(&tcolor::LightWhite),

        theme::Color::Rgb(r, g, b) => f(&tcolor::Rgb(r, g, b)),
        theme::Color::RgbLowRes(r, g, b) => f(&tcolor::AnsiValue::rgb(r, g, b)),
    }
}
