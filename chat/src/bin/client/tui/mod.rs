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
pub mod login;
pub mod markup;
pub mod math;
pub mod palette;
pub mod settings;
pub mod state;
pub mod theme;
pub mod tofu;

use std::
{
    sync::Arc,
    time::Duration,
    io::
    {
        self,
        Stdout,
        Write,
        Result,
    },
};

use base64::prelude::{ Engine, BASE64_STANDARD };

use crossterm::
{
    cursor::Show,
    terminal::
    {
        self,
        EnterAlternateScreen,
        LeaveAlternateScreen,
    },
    event::
    {
        Event,
        EventStream,
        KeyCode,
        KeyEvent,
        KeyEventKind,
        KeyModifiers,
        KeyboardEnhancementFlags,
        MouseButton,
        MouseEventKind,
        DisableMouseCapture,
        EnableMouseCapture,
        PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
};

use ratatui::
{
    Terminal,
    backend::CrosstermBackend,
};

use tokio::
{
    net::tcp::{ OwnedReadHalf, OwnedWriteHalf },
    time::{ self, MissedTickBehavior },
    sync::
    {
        mpsc::{ self, Receiver, Sender },
        Mutex as MutexAsync,
    },
};

use tokio_stream::StreamExt;

use crate::
{
    config,
    options,
    network::
    {
        self,
        codes::PacketCode,
        client::{ self, ClientEvent },
    },
    command::
    {
        self,
        Command,
    },
};

use login::{ Action, ConnectResult };

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
    pub fn enter() -> Result<Self>
    {
        let mouse = config::read_config::<bool>("mouse_capture");

        terminal::enable_raw_mode()?;
        crossterm::execute!(io::stdout(), EnterAlternateScreen)?;

        if mouse { crossterm::execute!(io::stdout(), EnableMouseCapture)?; }

        //Shift+Enter NEEDS KITTY; Alt+Enter ALWAYS WORKS
        let enhanced = terminal::supports_keyboard_enhancement().unwrap_or(false);
        if enhanced
        {
            crossterm::execute!(io::stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES))?;
        }

        Ok(Self { mouse, enhanced })
    }
}

impl Drop for TerminalGuard
{
    fn drop(&mut self)
    {
        let mut stdout = io::stdout();

        if self.enhanced { let _ = crossterm::execute!(stdout, PopKeyboardEnhancementFlags); }
        if self.mouse { let _ = crossterm::execute!(stdout, DisableMouseCapture); }

        let _ = crossterm::execute!(stdout, LeaveAlternateScreen, Show);
        let _ = terminal::disable_raw_mode();
        let _ = stdout.flush();
    }
}

//FUNCTIONS
//PUBLIC
//THE BRANCH GLYPHS FOR BLOCK-COMMAND LISTS
pub fn branch(last: bool) -> &'static str
{
    if last { "╰─ " } else { "├─ " }
}

pub fn install_panic_hook() //MANDATORY: THE RELEASE PROFILE USES panic = "abort"
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

    let _ = crossterm::execute!(stdout, PopKeyboardEnhancementFlags);
    let _ = crossterm::execute!(stdout, DisableMouseCapture);
    let _ = crossterm::execute!(stdout, LeaveAlternateScreen, Show);
    let _ = terminal::disable_raw_mode();
    let _ = stdout.flush();
}

//NOTHING DRAGS WHILE AN OVERLAY IS UP
fn selectable(app: &App) -> bool
{
    app.tofu.is_none() && app.login.is_none() && !app.settings.open
}

//COPY WITH OSC 52, WHICH WORKS OVER SSH
fn copy_to_clipboard(text: &str)
{
    let mut stdout = io::stdout();

    let _ = write!(stdout, "\x1b]52;c;{}\x07", BASE64_STANDARD.encode(text));
    let _ = stdout.flush();
}

pub fn init() -> Result<Tui>
{
    //NO Terminal::clear() - IT QUERIES THE CURSOR
    Terminal::new(CrosstermBackend::new(io::stdout()))
}

//THE SINGLE EVENT LOOP AND EVERY TERMINAL WRITE
pub async fn run
(
    terminal: &mut Tui,
    app: &mut App,
    rx: &mut Receiver<ClientEvent>,
    tx: &Sender<ClientEvent>,
)
{
    let mut reader = EventStream::new();
    let mut tick = time::interval(REDRAW_INTERVAL);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut events_open = true;

    //NONE UNTIL THE CONNECT PROMPT HAS A SOCKET
    let mut write_stream: Option<Arc<MutexAsync<OwnedWriteHalf>>> = None;

    let (connect_tx, mut connect_rx) = mpsc::channel::<ConnectResult>(1);

    //auto_connect DIALS WITHOUT A KEYSTROKE
    if app.login.as_ref().is_some_and(|prompt| prompt.busy) { login::connect(app, &connect_tx); }

    loop
    {
        tokio::select!
        {
            result = connect_rx.recv() =>
            {
                if let Some((attempt, result)) = result { connected(app, tx, &mut write_stream, attempt, result); }
            },

            event = rx.recv(), if events_open =>
            {
                match event
                {
                    Some(event) => app.apply(event),
                    None => events_open = false,
                }

                //DROP THE DEAD WRITE HALF
                if app.drop_stream
                {
                    app.drop_stream = false;
                    write_stream = None;
                }
            },

            event = reader.next() =>
            {
                match event
                {
                    Some(Ok(event)) => handle_terminal_event(app, event, write_stream.as_ref(), tx, &connect_tx, terminal).await,
                    Some(Err(_)) => app.quit(1, Some(String::from("Reading terminal input failed."))),
                    None => app.quit(0, None),
                }
            },

            _ = tick.tick() =>
            {
                //THE TICK IS THE ANIMATIONS' CLOCK
                app.advance_animations();

                //SILENT ROSTER REFRESH
                if app.refresh_online && let Some(write_stream) = write_stream.as_ref()
                {
                    app.refresh_online = false;

                    network::send(&mut *write_stream.lock().await,
                        PacketCode::List { users: None }, options::get_keys().as_ref()).await;
                }

                //AND THE OFFERED PICTURES WE DO NOT HOLD
                if !app.image_requests.is_empty() && let Some(write_stream) = write_stream.as_ref()
                {
                    let keys = options::get_keys();
                    let mut stream = write_stream.lock().await;

                    for hash in app.image_requests.drain(..)
                    {
                        network::send(&mut *stream,
                            PacketCode::ImageData { hash, data: None }, keys.as_ref()).await;
                    }
                }

                app.expire_notice();

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
fn connected
(
    app: &mut App,
    tx: &Sender<ClientEvent>,
    write_stream: &mut Option<Arc<MutexAsync<OwnedWriteHalf>>>,
    attempt: u64,
    result: Result<(OwnedReadHalf, OwnedWriteHalf)>,
)
{
    if !app.login.as_ref().is_some_and(|prompt| prompt.accepts(attempt)) { return; }

    app.dirty = true;

    let (mut read_half, write_half) = match result
    {
        Ok(halves) => halves,

        //KEEP THE ADDRESS ON SCREEN WITH THE REASON
        Err(error) =>
        {
            if let Some(prompt) = app.login.as_mut() { prompt.failed(&error); }

            return;
        },
    };

    let stream = Arc::new(MutexAsync::new(write_half));

    //THE HANDSHAKE RUNS WITH THE TUI ALREADY UP
    let listen_stream = stream.clone();
    let listen_tx = tx.clone();

    tokio::spawn(async move
    {
        client::listen_server(&mut (&mut read_half, listen_stream), listen_tx).await;
    });

    *write_stream = Some(stream);

    if let Some(prompt) = app.login.as_mut() { prompt.connected = true; }

    //THE BOX STAYS UP UNTIL Authenticated
}

async fn handle_terminal_event
(
    app: &mut App,
    event: Event,
    write_stream: Option<&Arc<MutexAsync<OwnedWriteHalf>>>,
    tx: &Sender<ClientEvent>,
    connect_tx: &Sender<ConnectResult>,
    terminal: &Tui,
)
{
    match event
    {
        Event::Key(key) =>
        {
            if key.kind == KeyEventKind::Release { return; }

            handle_key(app, key, write_stream, connect_tx, message_viewport(terminal)).await;
        },

        Event::Mouse(mouse) =>
        {
            let viewport = message_viewport(terminal);

            match mouse.kind
            {
                //THE WHEEL DRIVES THE SETTINGS SELECTION
                MouseEventKind::ScrollUp if app.settings.open => settings::scroll(app, -1),
                MouseEventKind::ScrollDown if app.settings.open => settings::scroll(app, 1),

                MouseEventKind::ScrollUp => app.scroll_up(SCROLL_STEP, viewport),
                MouseEventKind::ScrollDown => app.scroll_down(SCROLL_STEP, viewport),

                //A PRESS ANCHORS A SELECTION
                MouseEventKind::Down(MouseButton::Left) if selectable(app) =>
                {
                    if !app.selection_start(mouse.column, mouse.row) { app.clear_selection(); }
                },

                MouseEventKind::Drag(MouseButton::Left) if selectable(app) =>
                {
                    app.selection_extend(mouse.column, mouse.row);
                },

                MouseEventKind::Up(MouseButton::Left) if selectable(app) =>
                {
                    //THE DRAG SAYS WHETHER THIS WAS A SELECTION
                    if app.selection.is_some_and(|selection| selection.dragged)
                    {
                        //COPY WHILE THE MOUSE IS CAPTURED
                        if let Some(text) = app.selection_text()
                        {
                            let lines = text.lines().count();

                            copy_to_clipboard(&text);
                            app.notify(format!("Copied {lines} line{} to the clipboard", if lines == 1 { "" } else { "s" }));
                        }
                    } else
                    {
                        app.clear_selection();

                        //A CLICK ON A CAPTION FETCHES THE PICTURE
                        if write_stream.is_some()
                            && let Some(entry) = app.image_at(mouse.column, mouse.row)
                            && let Some(hash) = app.request_image(entry)
                        {
                            client::fetch_image(hash, tx.clone());
                        }
                    }
                },

                _ => {},
            }

            app.dirty = true;
        },

        Event::Resize(..) | Event::FocusGained | Event::FocusLost => app.dirty = true,
        Event::Paste(text) =>
        {
            //A PASTE BELONGS TO THE CONNECT BOX
            if app.login.is_some()
            {
                login::insert_str(app, &text);
            } else
            {
                app.input.insert_str(&text);
                app.palette.update(&app.input.text(), app.role);
            }

            app.dirty = true;
        },
    }
}

async fn handle_key
(
    app: &mut App,
    key: KeyEvent,
    write_stream: Option<&Arc<MutexAsync<OwnedWriteHalf>>>,
    connect_tx: &Sender<ConnectResult>,
    viewport: u16,
)
{
    let control = key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT);
    let alt = key.modifiers.contains(KeyModifiers::ALT) && !key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    app.dirty = true;

    //THE SERVER-KEY PROMPT OUTRANKS EVERYTHING
    if app.tofu.is_some()
    {
        tofu::handle_key(app, key);

        return;
    }

    //THEN THE CONNECT BOX
    if app.login.is_some()
    {
        match login::handle_key(app, key)
        {
            Action::Connect => login::connect(app, connect_tx),

            //AN ANSWERED STEP IS AN ORDINARY SUBMITTED LINE
            Action::Submit =>
            {
                if let Some(write_stream) = write_stream
                {
                    let answer = login::take_input(app);

                    crate::submit(app, write_stream, answer).await;
                }
            },

            Action::Quit => app.quit(0, None),
            Action::None => {},
        }

        return;
    }

    //THE SETTINGS OVERLAY OWNS THE KEYBOARD
    if app.settings.open
    {
        //Ctrl+S BELONGS TO THE SERVER ROWS
        if control && settings_shortcut(key.code) && !(app.settings.server && key.code == KeyCode::Char('s'))
        {
            app.settings.close();
        } else
        {
            settings::handle_key(app, key);
        }

        //A PRESSED Save LANDS HERE - THE SOCKET IS OURS
        if let Some(settings) = app.settings.take_save()
        {
            match write_stream
            {
                Some(write_stream) =>
                {
                    network::send(&mut *write_stream.lock().await,
                        PacketCode::ServerSettings { settings: Some(settings), save: true },
                        options::get_keys().as_ref()).await;

                    //STORED IS NOT IN USE FOR THESE
                    if let Some(keys) = app.settings.restart_note.take()
                    {
                        app.push_styled(format!("{keys} takes effect when the server is restarted."), theme::NOTICE);
                    }
                },

                //NOTHING WENT OUT, SO THE ROWS STAY EDITABLE
                None =>
                {
                    app.settings.saving = false;
                    app.settings.restart_note = None;
                },
            }
        }

        //AND SO DOES A CONFIRMED Restart
        if app.settings.take_restart() && let Some(write_stream) = write_stream
        {
            network::send(&mut *write_stream.lock().await,
                PacketCode::ServerRestart, options::get_keys().as_ref()).await;

            app.push_styled(String::from("Restarting the server..."), theme::NOTICE);
        }

        return;
    }

    //EVERYTHING BELOW SENDS OR EDITS THE LINE
    let Some(write_stream) = write_stream else { return };

    //NEWLINE (Alt+Enter, OR Shift+Enter IF REPORTED)
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
                    //CLEAR THE HALF-TYPED LINE FIRST
                    app.input.clear();
                    app.palette.dismiss();

                    let command = info.command.to_string();
                    crate::submit(app, write_stream, command).await;
                }
            },

            _ => {},
        }

        app.palette.update(&app.input.text(), app.role);

        return;
    }

    match key.code
    {
        KeyCode::Char(c) =>
        {
            app.input.insert(c);
            app.palette.update(&app.input.text(), app.role);
        },

        KeyCode::Backspace =>
        {
            app.input.backspace();
            app.palette.update(&app.input.text(), app.role);
        },

        KeyCode::Delete =>
        {
            app.input.delete();
            app.palette.update(&app.input.text(), app.role);
        },

        KeyCode::Left => if alt { app.input.word_left() } else { app.input.left() },
        KeyCode::Right => if alt { app.input.word_right() } else { app.input.right() },
        KeyCode::Home => app.input.home(),
        KeyCode::End => app.input.end(),

        KeyCode::Up => if app.palette.is_active() { app.palette.previous() } else { app.input.up() },
        KeyCode::Down => if app.palette.is_active() { app.palette.next() } else { app.input.down() },

        KeyCode::PageUp => app.scroll_up(viewport.saturating_sub(1).max(1), viewport),
        KeyCode::PageDown => app.scroll_down(viewport.saturating_sub(1).max(1), viewport),

        KeyCode::Esc =>
        {
            app.palette.dismiss();
            app.clear_selection();
        },

        KeyCode::Tab => { complete_selection(app, true); },

        KeyCode::Enter =>
        {
            //COMPLETE A HIGHLIGHTED PALETTE ENTRY FIRST
            if complete_selection(app, false) { return; }

            app.palette.dismiss();

            let input = app.input.take();
            crate::submit(app, write_stream, input).await;
        },

        _ => {},
    }
}

//WRITE THE HIGHLIGHTED ROW ONTO THE LINE
fn complete_selection(app: &mut App, force: bool) -> bool
{
    if let Some(values) = app.palette.values()
    {
        let input = app.input.text();

        let Some(value) = values.selection().filter(|_| force || !values.typed(&input)) else { return false };

        //KEEP EVERYTHING UP TO THE HALF-TYPED VALUE
        let kept = input.chars().take(values.start).collect::<String>();

        app.input.clear();
        app.input.insert_str(&format!("{kept}{value}"));
        app.palette.update(&app.input.text(), app.role);

        return true;
    }

    let Some(entry) = app.palette.selection().filter(|entry| force || !entry.typed(&app.input.text())) else { return false };

    complete(app, entry);

    true
}

fn complete(app: &mut App, entry: palette::Entry)
{
    app.input.clear();
    app.input.insert_str(&entry.name());

    //LEAVE ROOM FOR ARGUMENTS RIGHT AWAY
    if !entry.args().is_empty() { app.input.insert(' '); }

    app.palette.update(&app.input.text(), app.role);
}

fn settings_shortcut(code: KeyCode) -> bool //Ctrl+<SHORTCUT OF /settings>
{
    let KeyCode::Char(c) = code else { return false };

    command::COMMAND_LIST.iter()
        .find(|info| info.command == Command::Settings)
        .is_some_and(|info| info.shortcut == Some(c))
}

fn message_viewport(terminal: &Tui) -> u16 //ROWS OF ACTUAL MESSAGE TEXT, FOR SCROLL CLAMPING
{
    terminal.size().map(|s| s.height.saturating_sub(5)).unwrap_or(1).max(1)
}
