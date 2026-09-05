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

        //Shift+Enter IS ONLY DISTINGUISHABLE WITH THE KITTY PROTOCOL; Alt+Enter IS THE UNIVERSAL FALLBACK
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

    let _ = crossterm::execute!(stdout, PopKeyboardEnhancementFlags);
    let _ = crossterm::execute!(stdout, DisableMouseCapture);
    let _ = crossterm::execute!(stdout, LeaveAlternateScreen, Show);
    let _ = terminal::disable_raw_mode();
    let _ = stdout.flush();
}

pub fn init() -> Result<Tui>
{
    //NO Terminal::clear() HERE - IT QUERIES THE CURSOR POSITION (WHICH SOME TERMINALS NEVER ANSWER)
    //AND THE ALTERNATE SCREEN STARTS BLANK ANYWAY
    Terminal::new(CrosstermBackend::new(io::stdout()))
}

//THE SINGLE EVENT LOOP. EVERY TERMINAL WRITE IN THE CLIENT HAPPENS HERE - INCLUDING THE CONNECT PROMPT,
//WHICH IS WHY THE SOCKET IS OPENED HERE AND NOT BEFORE THE ALTERNATE SCREEN IS ENTERED.
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

    //NONE UNTIL THE CONNECT PROMPT HAS PRODUCED A SOCKET; WHILE IT IS NONE, THE PROMPT OWNS THE KEYBOARD
    let mut write_stream: Option<Arc<MutexAsync<OwnedWriteHalf>>> = None;

    let (connect_tx, mut connect_rx) = mpsc::channel::<ConnectResult>(1);

    //auto_connect DIALS WITHOUT WAITING FOR A KEYSTROKE, AND STILL DOES IT FROM INSIDE THE TUI
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

                //A LOST SESSION LEAVES A DEAD WRITE HALF BEHIND - DROPPING IT SHUTS THE WRITE SIDE DOWN AND
                //TAKES THE KEYBOARD BACK TO THE CONNECT BOX, WHICH IS THE ONLY THING ON SCREEN NOW ANYWAY
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
                    Some(Ok(event)) => handle_terminal_event(app, event, write_stream.as_ref(), &connect_tx, terminal).await,
                    Some(Err(_)) => app.quit(1, Some(String::from("Reading terminal input failed."))),
                    None => app.quit(0, None),
                }
            },

            _ = tick.tick() =>
            {
                //SILENT ROSTER REFRESH (/list IS REQUEST/RESPONSE, THE SIDEBAR NEEDS FEEDING)
                if app.refresh_online && let Some(write_stream) = write_stream.as_ref()
                {
                    app.refresh_online = false;

                    network::send(&mut *write_stream.lock().await,
                        PacketCode::List { users: None }, options::get_keys().as_ref()).await;
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

        //THE ADDRESS STAYS ON SCREEN WITH THE REASON UNDER IT, SO THE NEXT TRY IS ONE EDIT AWAY
        Err(error) =>
        {
            if let Some(prompt) = app.login.as_mut() { prompt.failed(&error); }

            return;
        },
    };

    let stream = Arc::new(MutexAsync::new(write_half));

    //THE HANDSHAKE (AND THE TOFU PROMPT INSIDE IT) RUNS FROM HERE ON, WITH THE TUI ALREADY UP
    let listen_stream = stream.clone();
    let listen_tx = tx.clone();

    tokio::spawn(async move
    {
        client::listen_server(&mut (&mut read_half, listen_stream), listen_tx).await;
    });

    *write_stream = Some(stream);

    if let Some(prompt) = app.login.as_mut() { prompt.connected = true; }

    //THE BOX STAYS UP, STILL BUSY: THE HANDSHAKE IS RUNNING, AND THE USERNAME PROMPT THAT FOLLOWS IT IS
    //THE SAME FIELD ASKING AGAIN. ClientEvent::Authenticated IS WHAT FINALLY CLOSES IT.
}

async fn handle_terminal_event
(
    app: &mut App,
    event: Event,
    write_stream: Option<&Arc<MutexAsync<OwnedWriteHalf>>>,
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
                //THE WHEEL DRIVES THE SETTINGS SELECTION WHILE THE OVERLAY IS UP
                MouseEventKind::ScrollUp if app.settings.open => settings::scroll(app, -1),
                MouseEventKind::ScrollDown if app.settings.open => settings::scroll(app, 1),

                MouseEventKind::ScrollUp => app.scroll_up(SCROLL_STEP, viewport),
                MouseEventKind::ScrollDown => app.scroll_down(SCROLL_STEP, viewport),

                //A CLICK ON AN IMAGE CAPTION FETCHES THE PICTURE. THE HISTORY REPLAYS HASHES RATHER THAN
                //BYTES, SO THIS IS THE ONLY THING THAT EVER PUTS A STORED PICTURE ON THE WIRE
                MouseEventKind::Down(MouseButton::Left) if app.tofu.is_none() && app.login.is_none() && !app.settings.open =>
                {
                    if let Some(write_stream) = write_stream
                        && let Some(entry) = app.image_at(mouse.column, mouse.row)
                        && let Some(hash) = app.request_image(entry)
                    {
                        network::send(&mut *write_stream.lock().await,
                            PacketCode::ImageData { hash, data: None }, options::get_keys().as_ref()).await;
                    }
                },
                _ => {},
            }

            app.dirty = true;
        },

        Event::Resize(..) | Event::FocusGained | Event::FocusLost => app.dirty = true,
        Event::Paste(text) =>
        {
            //A PASTE BELONGS TO THE CONNECT BOX WHILE IT IS UP, NOT TO THE CHAT LINE BEHIND IT
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

    //THE SERVER-KEY PROMPT OUTRANKS EVERYTHING, THE CONNECT BOX INCLUDED: THE NETWORK TASK IS PARKED ON
    //ITS ANSWER, AND NOTHING MAY REACH A SERVER THE USER HAS NOT ACCEPTED YET
    if app.tofu.is_some()
    {
        tofu::handle_key(app, key);

        return;
    }

    //THEN THE CONNECT BOX, WHICH OWNS THE KEYBOARD ALL THE WAY THROUGH ADDRESS, USERNAME AND PASSWORD
    if app.login.is_some()
    {
        match login::handle_key(app, key)
        {
            Action::Connect => login::connect(app, connect_tx),

            //AN ANSWERED IDENTITY STEP IS AN ORDINARY SUBMITTED LINE - submit() TURNS IT INTO ITS PACKET
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

    //THE SETTINGS OVERLAY OWNS THE KEYBOARD WHILE IT IS UP - EXCEPT FOR ITS OWN SHORTCUT, WHICH CLOSES IT
    if app.settings.open
    {
        //Ctrl+S BELONGS TO THE SERVER ROWS, SO THE OVERLAY IS ASKED FIRST AND ONLY THEN THE SHORTCUT
        if control && settings_shortcut(key.code) && !(app.settings.server && key.code == KeyCode::Char('s'))
        {
            app.settings.close();
        } else
        {
            settings::handle_key(app, key);
        }

        //THE OVERLAY EDITS server.toml BUT DOES NOT OWN THE SOCKET - A PRESSED Save LANDS HERE
        if let Some(settings) = app.settings.take_save()
        {
            match write_stream
            {
                Some(write_stream) =>
                {
                    network::send(&mut *write_stream.lock().await,
                        PacketCode::ServerSettings { settings: Some(settings), save: true },
                        options::get_keys().as_ref()).await;

                    //STORED IS NOT THE SAME AS IN USE FOR THESE - SAY SO ONCE, WHERE THE USER READS THINGS
                    if let Some(keys) = app.settings.restart_note.take()
                    {
                        app.push_styled(format!("{keys} takes effect when the server is restarted."), theme::NOTICE);
                    }
                },

                //NOTHING WENT OUT, SO NOTHING IS COMING BACK - THE ROWS STAY EDITABLE INSTEAD OF WAITING FOREVER
                None =>
                {
                    app.settings.saving = false;
                    app.settings.restart_note = None;
                },
            }
        }

        //AND SO DOES A CONFIRMED Restart. IT IS THE LAST THING THIS SOCKET CARRIES: THE SERVER ANSWERS BY
        //DISCONNECTING EVERYBODY AND GOING DOWN, WHICH LANDS US BACK IN THE CONNECT BOX LIKE ANY OTHER DROP
        if app.settings.take_restart() && let Some(write_stream) = write_stream
        {
            network::send(&mut *write_stream.lock().await,
                PacketCode::ServerRestart, options::get_keys().as_ref()).await;

            app.push_styled(String::from("Restarting the server..."), theme::NOTICE);
        }

        return;
    }

    //EVERYTHING BELOW EITHER SENDS SOMETHING OR EDITS THE LINE THAT WILL, SO IT NEEDS THE SOCKET THE
    //CONNECT PROMPT OPENED (WHICH IS GONE BY NOW, SO THIS ONLY GUARDS THE UNREACHABLE CASE)
    let Some(write_stream) = write_stream else { return };

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

        KeyCode::Up => if app.palette.is_active() { app.palette.previous() } else { app.input.history_up() },
        KeyCode::Down => if app.palette.is_active() { app.palette.next() } else { app.input.history_down() },

        KeyCode::PageUp => app.scroll_up(viewport.saturating_sub(1).max(1), viewport),
        KeyCode::PageDown => app.scroll_down(viewport.saturating_sub(1).max(1), viewport),

        KeyCode::Esc => app.palette.dismiss(),

        KeyCode::Tab => { complete_selection(app, true); },

        KeyCode::Enter =>
        {
            //A HIGHLIGHTED PALETTE ENTRY THE USER HASN'T FULLY TYPED COMPLETES FIRST
            if complete_selection(app, false) { return; }

            app.palette.dismiss();

            let input = app.input.take();
            crate::submit(app, write_stream, input).await;
        },

        _ => {},
    }
}

//WRITE THE HIGHLIGHTED ROW ONTO THE LINE, WHETHER IT IS A COMMAND OR ONE ANSWER OF A PARAMETER.
//force IS Tab, WHICH COMPLETES WHATEVER IS HIGHLIGHTED; Enter ONLY COMPLETES WHAT IS NOT SPELLED OUT ALREADY,
//SO A FINISHED LINE IS SENT INSTEAD OF BEING REWRITTEN. RETURNS WHETHER THE LINE WAS TOUCHED
fn complete_selection(app: &mut App, force: bool) -> bool
{
    if let Some(values) = app.palette.values()
    {
        let input = app.input.text();

        let Some(value) = values.selection().filter(|_| force || !values.typed(&input)) else { return false };

        //EVERYTHING UP TO THE HALF-TYPED VALUE STAYS - THE PARAMETERS BEFORE IT WERE ANSWERED ALREADY
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

    //LEAVE ROOM FOR ARGUMENTS RIGHT AWAY - AN ACTION WORD COUNTS AS ONE, SO /server OPENS ITS OWN MENU
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
