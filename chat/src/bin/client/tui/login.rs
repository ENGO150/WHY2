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

//! # Connect Box
//!
//! Getting in happens inside the TUI: one box that asks three times — the address, the username
//! and the password — relabelling the same field.
//!
//! Only the address step is driven by the client: [`connect`] dials in a task so the frame keeps
//! drawing, and numbers the attempt so a cancelled dial's late result is dropped. The server
//! drives the other two steps by asking for them; an answered step is handed to
//! [`submit`](crate::submit) like any other line.
//!
//! # Reconnect
//! A session the user did not end dials itself back ([`Reconnect`]). The username and password
//! are remembered as they are typed and only kept once the server accepts them, so what is
//! replayed is a pair that demonstrably worked. Each reconnect is bounded by
//! [`RECONNECT_ATTEMPTS`](consts::RECONNECT_ATTEMPTS), may replay two answers and no more, and is
//! given up as soon as the answer stops being ours to give (a logout, a cancel, a registration
//! prompt, a rejected username).

use std::
{
    io::Error,
    time::Instant,
};

use crossterm::event::
{
    KeyCode,
    KeyEvent,
    KeyModifiers,
};

use zeroize::Zeroizing;

use tokio::
{
    sync::mpsc::Sender,
    net::tcp::{ OwnedReadHalf, OwnedWriteHalf },
};

use crate::
{
    t,
    config,
    options,
    network::client::handshake,
};

use super::
{
    consts,
    input::InputBuffer,
    state::App,
};

//TYPES
//ONE FINISHED DIAL ATTEMPT, NUMBERED
/// One finished dial attempt: its number and its result.
pub type ConnectResult = (u64, Result<(OwnedReadHalf, OwnedWriteHalf), Error>);

//ENUMS
/// What the event loop has to do after a key in the connect box.
pub enum Action //WHAT THE LOOP HAS TO DO AFTER A KEYSTROKE
{
    /// Nothing.
    None,
    /// Dial the typed address.
    Connect,
    /// Send the answered username or password.
    Submit, //AN ANSWERED IDENTITY STEP
    /// Quit the client.
    Quit,
}

/// Which of its three questions the box is asking.
#[derive(Clone, Copy, PartialEq)] //WHICH OF THE THREE THINGS THE BOX IS ASKING FOR
pub enum Stage
{
    /// The server address.
    Address,
    /// The username.
    Username,
    /// The password, for an existing account or a new one.
    Password { register: bool },
}

//STRUCTS
/// The connect box.
pub struct Login //THE CONNECT PROMPT
{
    /// The field.
    pub input: InputBuffer,
    /// What it is asking for.
    pub stage: Stage,
    /// Whether a dial or an answer is in flight.
    pub busy: bool,            //A DIAL, OR AN ANSWER THE SERVER HAS NOT REPLIED TO YET
    /// Whether the socket is open.
    pub connected: bool,       //THE SOCKET IS OPEN
    /// Why the last attempt did not work.
    pub error: Option<String>, //WHY THE LAST ONE DID NOT WORK
    /// The server's rules for the step on screen.
    pub hint: Option<String>,  //THE SERVER'S RULES FOR THE STEP ON SCREEN
    /// Number of the newest dial; only its result is accepted.
    attempt: u64,              //ONLY THE NEWEST ATTEMPT'S RESULT IS ACCEPTED
}

//WHAT AN AUTOMATIC RE-DIAL NEEDS AFTER A DROPPED SESSION
/// The automatic re-dial after a dropped session.
#[derive(Default)]
pub struct Reconnect
{
    /// The username and password that got us in, once they did.
    credentials: Option<(String, Zeroizing<String>)>, //WHAT GOT US IN, ONCE IT ACTUALLY DID
    /// The answers of the steps in flight.
    typed: (String, Zeroizing<String>),               //THE ANSWERS OF THE STEPS IN FLIGHT
    /// When to dial again.
    due: Option<Instant>,                             //WHEN TO DIAL AGAIN
    /// Dials left before giving up.
    left: u32,                                        //ATTEMPTS BEFORE IT GIVES UP AND WAITS FOR A KEY
    /// Answers this dial may still replay.
    answers: u32,                                     //REPLIES THIS DIAL MAY STILL REPLAY - USERNAME AND PASSWORD
    /// Whether the box is busy because of a reconnect rather than the user.
    retrying: bool,                                   //THE BOX IS BUSY BECAUSE OF US, NOT BECAUSE OF A KEYPRESS
    /// Whether an answer is in the field for the tick to send.
    pub submit: bool,                                 //AN ANSWER IS IN THE FIELD FOR THE TICK TO SEND
}

//IMPLEMENTATIONS
impl Reconnect
{
    //KEEP WHAT WAS TYPED AT EACH STEP
    /// Remembers what was typed at a step.
    pub fn remember(&mut self, stage: Stage, text: &str)
    {
        match stage
        {
            Stage::Username => self.typed.0 = text.to_owned(),
            Stage::Password { .. } => self.typed.1 = Zeroizing::new(text.to_owned()),
            Stage::Address => {},
        }
    }

    //A CHANGED PASSWORD REPLACES THE ONE WE WOULD REPLAY
    /// Replaces the remembered password after a password change.
    pub fn passwd(&mut self, password: &str)
    {
        if let Some((_, stored)) = self.credentials.as_mut() { *stored = Zeroizing::new(password.to_owned()); }
    }

    //THE PAIR WORKED, SO IT IS WORTH REPLAYING
    /// Keeps the typed pair, now that the server accepted it, and refills the attempts.
    pub fn accepted(&mut self)
    {
        self.credentials = Some(self.typed.clone());
        self.left = consts::RECONNECT_ATTEMPTS;
        self.retrying = false;
        self.due = None;
    }

    //THE USER ASKED TO LEAVE, OR CANCELLED THE DIAL
    /// Forgets everything; nothing will be replayed.
    pub fn forget(&mut self) { *self = Self::default(); }

    //SCHEDULE A DIAL, IF THERE IS ANYTHING TO LOG BACK IN WITH
    /// Schedules a dial, if there is anything to log back in with and tries are left.
    ///
    /// # Returns
    /// `true` if a dial was scheduled.
    pub fn arm(&mut self) -> bool
    {
        if self.credentials.is_none() || self.left == 0
        {
            self.retrying = false; //OUT OF TRIES - THE NEXT DIAL IS THE USER'S OWN

            return false;
        }

        self.left -= 1;
        self.retrying = true;
        self.answers = 2; //A SERVER THAT KEEPS ASKING IS NOT ANSWERED FOREVER
        self.due = Some(Instant::now() + consts::RECONNECT_DELAY);

        true
    }

    //WHAT THE BOX SAYS WHILE IT IS DIALLING ITSELF BACK
    /// Returns what the box says while reconnecting.
    pub fn status(&self) -> Option<String>
    {
        self.retrying.then(|| t!("login.reconnecting", attempt = consts::RECONNECT_ATTEMPTS - self.left, attempts = consts::RECONNECT_ATTEMPTS))
    }

    //WHETHER THE WAIT IS UP
    /// Returns whether the scheduled dial is due, clearing it if so.
    pub fn take_due(&mut self) -> bool
    {
        if self.due.is_none_or(|due| Instant::now() < due) { return false; }

        self.due = None;

        true
    }

    //THE ANSWER THE SERVER'S NEXT STEP WANTS
    /// Returns the remembered answer for the server's next step, during a reconnect only.
    pub fn answer(&mut self, stage: Stage) -> Option<String>
    {
        //ONLY A DIAL WE STARTED REPLAYS ANYTHING - A DIAL THE USER TYPED IS THEIRS TO ANSWER
        if !self.retrying || self.answers == 0 { return None; }

        let (username, password) = self.credentials.as_ref()?;

        let answer = match stage
        {
            Stage::Username => username.clone(),
            Stage::Password { register: false } => password.to_string(),

            //A REGISTRATION MEANS THE ACCOUNT IS GONE - ASK
            _ => return None,
        };

        self.answers -= 1;

        Some(answer)
    }
}

impl Default for Login
{
    fn default() -> Self { Self::new() }
}

impl Login
{
    /// Opens the box at the address step, prefilled and dialing on its own with `auto_connect`.
    pub fn new() -> Self
    {
        let mut input = InputBuffer::new();

        //auto_connect IS THE ONE CASE THAT PREFILLS
        let auto = config::read_config::<bool>("auto_connect");
        if auto { input.insert_str(config::read_config::<String>("auto_connect_addr").trim()); }

        Self { input, stage: Stage::Address, busy: auto, connected: false, error: None, hint: None, attempt: 0 }
    }

    //COME BACK AT THE ADDRESS STEP AFTER A DROP
    /// Reopens the box at the address step after a drop, keeping the attempt counter.
    pub fn again(address: &str, attempt: u64, error: String) -> Self
    {
        let mut input = InputBuffer::new();
        input.insert_str(address);

        Self { input, stage: Stage::Address, busy: false, connected: false, error: Some(error), hint: None, attempt }
    }

    /// Returns the typed address.
    pub fn address(&self) -> String { self.input.text().trim().to_owned() }

    /// Returns the number of the newest dial.
    pub fn attempt(&self) -> u64 { self.attempt }

    //THE ATTEMPT A RESULT HAS TO BELONG TO
    /// Returns whether a dial result belongs to the attempt the box is waiting for.
    pub fn accepts(&self, attempt: u64) -> bool { self.busy && attempt == self.attempt }

    /// Shows a failed dial.
    pub fn failed(&mut self, error: &Error)
    {
        self.busy = false;
        self.error = Some(error.to_string());
    }

    //ASK THE NEXT STEP, KEEPING THE ERROR
    /// Asks for the next step, keeping the error from the last one.
    pub fn ask(&mut self, stage: Stage, hint: Option<String>)
    {
        self.stage = stage;
        self.hint = hint;
        self.busy = false;
        self.input = InputBuffer::new();
    }

    /// Returns whether the field is a password.
    pub fn masked(&self) -> bool { matches!(self.stage, Stage::Password { .. }) }

    /// Returns the box's title.
    pub fn title(&self) -> &'static str
    {
        match self.stage
        {
            Stage::Address => t!("login.title.connect"),
            Stage::Username => t!("login.title.identify"),
            Stage::Password { register: true } => t!("login.title.register"),
            Stage::Password { register: false } => t!("login.title.login"),
        }
    }

    /// Returns the field's label.
    pub fn label(&self) -> &'static str
    {
        match self.stage
        {
            Stage::Address => t!("login.label.address"),
            Stage::Username => t!("login.label.username"),
            Stage::Password { .. } => t!("login.label.password"),
        }
    }

    //THE STATUS ROW WHILE SOMETHING IS IN FLIGHT
    /// Returns the status row while something is in flight.
    pub fn waiting(&self) -> &'static str
    {
        match (self.stage, self.connected)
        {
            (Stage::Address, false) => t!("login.connecting"),
            (Stage::Address, true) => t!("login.exchanging_keys"), //THE SOCKET IS UP, THE HANDSHAKE IS NOT DONE
            _ => t!("login.waiting"),
        }
    }

    //ESC ABANDONS A DIAL, THEN LEAVES THE CLIENT
    /// Returns whether Esc cancels a dial rather than quitting.
    pub fn cancellable(&self) -> bool { self.busy && !self.connected && self.stage == Stage::Address }
}

//FUNCTIONS
//PUBLIC
/// Handles a key while the box is up.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Action
{
    let Some(login) = app.login.as_mut() else { return Action::None };

    //ESC BACKS OUT OF A DIAL FIRST
    if key.code == KeyCode::Esc
    {
        if login.connected || login.stage != Stage::Address || !login.busy { return Action::Quit; }

        //LEAVE THE TASK - ITS RESULT NO LONGER COUNTS
        login.busy = false;
        login.error = None;

        //A CANCELLED DIAL IS NOT ONE TO REPEAT
        app.reconnect.forget();

        return Action::None;
    }

    if login.busy { return Action::None; } //NOTHING IS EDITABLE WHILE AN ANSWER IS IN FLIGHT

    if edit(&mut login.input, key) || key.code != KeyCode::Enter { return Action::None; }

    match login.stage
    {
        Stage::Address =>
        {
            if login.address().is_empty()
            {
                login.error = Some(t!("login.missing.address").to_owned());
            } else { return Action::Connect; }
        },

        //A PASSWORD IS TAKEN AS TYPED
        _ =>
        {
            if login.input.text().is_empty()
            {
                login.error = Some(match login.stage
                {
                    Stage::Username => t!("login.missing.username"),
                    _ => t!("login.missing.password"),
                }.to_owned());
            } else { return Action::Submit; }
        },
    }

    Action::None
}

//EDIT A ONE-LINE FIELD, TRUE IF THE KEY WAS TAKEN
/// Edits a one-line field.
///
/// # Returns
/// `true` if the key was taken.
pub fn edit(input: &mut InputBuffer, key: KeyEvent) -> bool
{
    if key.modifiers.contains(KeyModifiers::CONTROL)
    {
        match key.code
        {
            KeyCode::Char('a') => input.home(),
            KeyCode::Char('e') => input.end(),
            KeyCode::Char('u') => input.kill_to_start(),
            KeyCode::Char('k') => input.kill_to_end(),
            KeyCode::Char('w') => input.delete_word(),
            _ => {},
        }

        return true;
    }

    match key.code
    {
        //ONE FIELD, ONE LINE
        KeyCode::Char(character) => input.insert(character),

        KeyCode::Backspace => input.backspace(),
        KeyCode::Delete => input.delete(),

        KeyCode::Left => input.left(),
        KeyCode::Right => input.right(),
        KeyCode::Home => input.home(),
        KeyCode::End => input.end(),

        _ => return false,
    }

    true
}

/// Pastes text into the box's field, without newlines.
pub fn insert_str(app: &mut App, text: &str) //A PASTE INTO WHICHEVER FIELD IS UP
{
    if let Some(login) = app.login.as_mut() && !login.busy
    {
        login.input.insert_str(&text.replace(['\r', '\n'], ""));
    }
}

//TAKE THE ANSWERED STEP OFF THE FIELD, GO BUSY
/// Takes the answered step off the field, marks the box busy and remembers the answer.
pub fn take_input(app: &mut App) -> String
{
    let Some(login) = app.login.as_mut() else { return String::new() };

    //ONLY THE ADDRESS IS TRIMMED HERE
    let text = login.input.text();
    let stage = login.stage;

    login.input = InputBuffer::new();
    login.error = None;
    login.hint = None;
    login.busy = true;

    //A RECONNECT REPLAYS THIS
    app.reconnect.remember(stage, &text);

    text
}

//DIAL IN A TASK, SO THE FRAME KEEPS DRAWING
/// Dials the typed address in a task, adding the default port if none is given.
pub fn connect(app: &mut App, results: &Sender<ConnectResult>)
{
    let Some(login) = app.login.as_mut() else { return };

    let display = login.address();
    if display.is_empty() { return; }

    login.busy = true;
    login.error = None;
    login.attempt += 1;

    let attempt = login.attempt;

    //KEEP THE ADDRESS AS TYPED FOR THE TITLE
    let mut address = display.clone();
    if !address.contains(':') { address.push_str(&format!(":{}", config::read_config::<u16>("default_port"))); }

    app.address = display;

    //THE RECONNECT AFTER PINNING DIALS THIS
    options::set_server_address(&address);

    //A NEW CONNECTION COUNTS FROM ZERO
    options::set_seq(0);
    options::set_server_seq(0);

    let results = results.clone();

    tokio::spawn(async move
    {
        let _ = results.send((attempt, handshake::connect(address).await)).await;
    });
}
