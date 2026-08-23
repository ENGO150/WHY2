/*
This is part of WHY2
Copyright (C) 2022-2026 Václav Šmejkal

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

//MODULES
pub mod draw;
pub mod event;
pub mod input;
pub mod palette;
pub mod state;
pub mod theme;

use std::
{
    sync::Arc,
    time::Duration,
    io::
    {
        self,
        Stdout,
        Write,
    },
};

use crossterm::
{
    execute,
    terminal,
    cursor::Show,
    event::
    {
        Event,
        EventStream,
        KeyCode,
        KeyEventKind,
        KeyModifiers,
        KeyboardEnhancementFlags,
        MouseEventKind,
        DisableMouseCapture,
        EnableMouseCapture,
        PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
};

use ratatui::{ Terminal, backend::CrosstermBackend };

use tokio::
{
    net::tcp::OwnedWriteHalf,
    time::{ self, MissedTickBehavior },
    sync::
    {
        mpsc::Receiver,
        Mutex as MutexAsync,
    },
};

use tokio_stream::StreamExt;

use crate::
{
    command,
    config,
    options,
    network::client::ClientEvent,
};

pub use state::App;

//CONSTS
const REDRAW_INTERVAL: Duration = Duration::from_millis(33); //COALESCE REDRAWS - VoiceActivity FIRES PER VOICE PACKET
const SCROLL_STEP: u16 = 3;

//TYPES
pub type Tui = Terminal<CrosstermBackend<Stdout>>;

//STRUCTS
pub struct TerminalGuard //RESTORES THE TERMINAL ON DROP
{
    mouse: bool,
    enhanced: bool,
}

//IMPLEMENTATIONS
impl TerminalGuard
{
    pub fn enter() -> io::Result<Self>
    {
        let mouse = config::read_config::<bool>("mouse_capture");

        terminal::enable_raw_mode()?;
        execute!(io::stdout(), terminal::EnterAlternateScreen)?;

        if mouse { execute!(io::stdout(), EnableMouseCapture)?; }

        //Shift+Enter IS ONLY DISTINGUISHABLE WITH THE KITTY PROTOCOL; Alt+Enter IS THE UNIVERSAL FALLBACK
        let enhanced = terminal::supports_keyboard_enhancement().unwrap_or(false);
        if enhanced
        {
            execute!(io::stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES))?;
        }

        Ok(Self { mouse, enhanced })
    }
}

impl Drop for TerminalGuard
{
    fn drop(&mut self)
    {
        let mut stdout = io::stdout();

        if self.enhanced { let _ = execute!(stdout, PopKeyboardEnhancementFlags); }
        if self.mouse { let _ = execute!(stdout, DisableMouseCapture); }

        let _ = execute!(stdout, terminal::LeaveAlternateScreen, Show);
        let _ = terminal::disable_raw_mode();
        let _ = stdout.flush();
    }
}

//FUNCTIONS
//PUBLIC
//EVERY BLOCK-COMMAND LIST IS DRAWN AS A TREE, SO THE ROWS SHARE ONE SET OF BRANCH GLYPHS
pub fn branch(last: bool) -> &'static str
{
    if last { "╰─ " } else { "├─ " }
}

pub fn install_panic_hook() //MANDATORY: THE RELEASE PROFILE USES panic = "abort", SO Drop NEVER RUNS ON A PANIC
{
    let previous = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info|
    {
        restore_terminal();
        previous(info);
    }));
}

pub fn restore_terminal() //BEST-EFFORT, IDEMPOTENT
{
    let mut stdout = io::stdout();

    let _ = execute!(stdout, PopKeyboardEnhancementFlags);
    let _ = execute!(stdout, DisableMouseCapture);
    let _ = execute!(stdout, terminal::LeaveAlternateScreen, Show);
    let _ = terminal::disable_raw_mode();
    let _ = stdout.flush();
}

pub fn init() -> io::Result<Tui>
{
    //NO Terminal::clear() HERE - IT QUERIES THE CURSOR POSITION (WHICH SOME TERMINALS NEVER ANSWER)
    //AND THE ALTERNATE SCREEN STARTS BLANK ANYWAY
    Terminal::new(CrosstermBackend::new(io::stdout()))
}

//THE SINGLE EVENT LOOP. EVERY TERMINAL WRITE IN THE CLIENT HAPPENS HERE.
pub async fn run
(
    terminal: &mut Tui,
    app: &mut App,
    rx: &mut Receiver<ClientEvent>,
    write_stream: &Arc<MutexAsync<OwnedWriteHalf>>,
)
{
    let mut reader = EventStream::new();
    let mut tick = time::interval(REDRAW_INTERVAL);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut events_open = true;

    loop
    {
        tokio::select!
        {
            event = rx.recv(), if events_open =>
            {
                match event
                {
                    Some(event) => app.apply(event),
                    None => events_open = false,
                }
            },

            event = reader.next() =>
            {
                match event
                {
                    Some(Ok(event)) => handle_terminal_event(app, event, write_stream, terminal).await,
                    Some(Err(_)) => app.quit(1, Some(String::from("Reading terminal input failed."))),
                    None => app.quit(0, None),
                }
            },

            _ = tick.tick() =>
            {
                //SILENT ROSTER REFRESH (/list IS REQUEST/RESPONSE, THE SIDEBAR NEEDS FEEDING)
                if app.refresh_online
                {
                    app.refresh_online = false;

                    crate::network::send(&mut *write_stream.lock().await,
                        crate::network::codes::PacketCode::List { users: None }, options::get_keys().as_ref()).await;
                }

                if app.dirty
                {
                    app.dirty = false;
                    let _ = terminal.draw(|frame| draw::draw(frame, app));
                }
            },
        }

        if app.should_quit { break; }
    }
}

//PRIVATE
async fn handle_terminal_event
(
    app: &mut App,
    event: Event,
    write_stream: &Arc<MutexAsync<OwnedWriteHalf>>,
    terminal: &Tui,
)
{
    match event
    {
        Event::Key(key) =>
        {
            if key.kind == KeyEventKind::Release { return; }

            handle_key(app, key, write_stream, message_viewport(terminal)).await;
        },

        Event::Mouse(mouse) =>
        {
            let viewport = message_viewport(terminal);

            match mouse.kind
            {
                MouseEventKind::ScrollUp => app.scroll_up(SCROLL_STEP, viewport),
                MouseEventKind::ScrollDown => app.scroll_down(SCROLL_STEP, viewport),
                _ => {},
            }
        },

        Event::Resize(..) | Event::FocusGained | Event::FocusLost => app.dirty = true,
        Event::Paste(text) =>
        {
            app.input.insert_str(&text);
            app.palette.update(&app.input.text());
            app.dirty = true;
        },
    }
}

async fn handle_key
(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    write_stream: &Arc<MutexAsync<OwnedWriteHalf>>,
    viewport: u16,
)
{
    let control = key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT);
    let alt = key.modifiers.contains(KeyModifiers::ALT) && !key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    app.dirty = true;

    //NEWLINE (Alt+Enter EVERYWHERE, Shift+Enter WHERE THE TERMINAL REPORTS IT)
    if key.code == KeyCode::Enter && (alt || shift)
    {
        app.input.insert('\n');
        app.palette.dismiss();

        return;
    }

    if control
    {
        match key.code
        {
            KeyCode::Char('a') => app.input.home(),
            KeyCode::Char('e') => app.input.end(),
            KeyCode::Char('k') => app.input.kill_to_end(),
            KeyCode::Char('w') => app.input.delete_word(),
            KeyCode::Char('n') => app.palette.next(),
            KeyCode::Char('p') => app.palette.previous(),

            //COMMAND SHORTCUTS
            KeyCode::Char(c) =>
            {
                if let Some(info) = command::COMMAND_LIST.iter().find(|i| i.shortcut == Some(c))
                {
                    //CLEAR THE HALF-TYPED LINE FIRST (THE OLD read_input LEAKED IT)
                    app.input.clear();
                    app.palette.dismiss();

                    let command = info.command.to_string();
                    crate::submit(app, write_stream, command).await;
                }
            },

            _ => {},
        }

        app.palette.update(&app.input.text());

        return;
    }

    match key.code
    {
        KeyCode::Char(c) =>
        {
            app.input.insert(c);
            app.palette.update(&app.input.text());
        },

        KeyCode::Backspace =>
        {
            app.input.backspace();
            app.palette.update(&app.input.text());
        },

        KeyCode::Delete =>
        {
            app.input.delete();
            app.palette.update(&app.input.text());
        },

        KeyCode::Left => if alt { app.input.word_left() } else { app.input.left() },
        KeyCode::Right => if alt { app.input.word_right() } else { app.input.right() },
        KeyCode::Home => app.input.home(),
        KeyCode::End => app.input.end(),

        KeyCode::Up => if app.palette.is_active() { app.palette.previous() } else { app.input.history_up() },
        KeyCode::Down => if app.palette.is_active() { app.palette.next() } else { app.input.history_down() },

        KeyCode::PageUp => app.scroll_up(viewport.saturating_sub(1).max(1), viewport),
        KeyCode::PageDown => app.scroll_down(viewport.saturating_sub(1).max(1), viewport),

        KeyCode::Esc => app.palette.dismiss(),

        KeyCode::Tab =>
        {
            if let Some(info) = app.palette.selection()
            {
                complete(app, info);
            }
        },

        KeyCode::Enter =>
        {
            //A HIGHLIGHTED PALETTE ENTRY THE USER HASN'T FULLY TYPED COMPLETES FIRST
            if let Some(info) = app.palette.selection()
                && !info.triggers.iter().any(|t| t.eq_ignore_ascii_case(app.input.text().trim_start_matches(command::COMMAND_PREFIX)))
            {
                complete(app, info);
                return;
            }

            app.palette.dismiss();

            let input = app.input.take();
            crate::submit(app, write_stream, input).await;
        },

        _ => {},
    }
}

fn complete(app: &mut App, info: &'static command::CommandInfo)
{
    app.input.clear();
    app.input.insert_str(&format!("{}{}", command::COMMAND_PREFIX, info.triggers[0].to_lowercase()));

    //LEAVE ROOM FOR ARGUMENTS RIGHT AWAY
    if !info.args.is_empty() { app.input.insert(' '); }

    app.palette.update(&app.input.text());
}

fn message_viewport(terminal: &Tui) -> u16 //ROWS OF ACTUAL MESSAGE TEXT, FOR SCROLL CLAMPING
{
    terminal.size().map(|s| s.height.saturating_sub(5)).unwrap_or(1).max(1)
}
