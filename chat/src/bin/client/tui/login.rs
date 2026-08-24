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

use std::io::Error;

use crossterm::event::
{
    KeyCode,
    KeyEvent,
    KeyModifiers,
};

use tokio::
{
    sync::mpsc::Sender,
    net::tcp::{ OwnedReadHalf, OwnedWriteHalf },
};

use crate::
{
    config,
    options,
    network::client,
};

use super::
{
    input::InputBuffer,
    state::App,
};

//TYPES
//ONE FINISHED DIAL ATTEMPT. THE ATTEMPT NUMBER IS WHAT LETS A CANCELLED CONNECTION BE THROWN AWAY
//INSTEAD OF LANDING ON THE USER AFTER THEY HAVE MOVED ON.
pub type ConnectResult = (u64, Result<(OwnedReadHalf, OwnedWriteHalf), Error>);

//ENUMS
pub enum Action //WHAT THE LOOP HAS TO DO AFTER A KEYSTROKE - THE PROMPT ITSELF NEVER TOUCHES A SOCKET
{
    None,
    Connect,
    Quit,
}

//STRUCTS
//THE CONNECT PROMPT. IT IS UP FROM THE FIRST FRAME UNTIL A SOCKET IS OPEN, WHICH IS WHAT PUTS THE USER
//STRAIGHT INTO THE TUI - THERE IS NO PRE-TUI PHASE LEFT. WHILE IT IS UP IT OWNS THE KEYBOARD; THE
//USERNAME/PASSWORD STEPS THAT FOLLOW ARE THE INPUT BOX'S JOB, NOT THIS ONE'S.
pub struct Login
{
    pub input: InputBuffer,
    pub connecting: bool,      //A DIAL IS IN FLIGHT
    pub error: Option<String>, //WHY THE LAST ONE DID NOT WORK
    attempt: u64,              //ONLY THE NEWEST ATTEMPT'S RESULT IS ACCEPTED
}

//IMPLEMENTATIONS
impl Default for Login
{
    fn default() -> Self { Self::new() }
}

impl Login
{
    pub fn new() -> Self
    {
        let mut input = InputBuffer::new();

        //auto_connect DIALS THE CONFIGURED ADDRESS ON ITS OWN, SO IT IS THE ONE CASE THAT PREFILLS THE FIELD
        let auto = config::read_config::<bool>("auto_connect");
        if auto { input.insert_str(config::read_config::<String>("auto_connect_addr").trim()); }

        Self { input, connecting: auto, error: None, attempt: 0 }
    }

    pub fn address(&self) -> String { self.input.text().trim().to_owned() }

    //THE ATTEMPT A RESULT HAS TO BELONG TO IN ORDER TO COUNT
    pub fn accepts(&self, attempt: u64) -> bool { self.connecting && attempt == self.attempt }

    pub fn failed(&mut self, error: &Error)
    {
        self.connecting = false;
        self.error = Some(error.to_string());
    }
}

//FUNCTIONS
//PUBLIC
pub fn handle_key(app: &mut App, key: KeyEvent) -> Action
{
    let Some(login) = app.login.as_mut() else { return Action::None };

    //ESC BACKS OUT OF A DIAL FIRST, AND ONLY LEAVES THE CLIENT ONCE THERE IS NOTHING TO BACK OUT OF
    if key.code == KeyCode::Esc
    {
        if !login.connecting { return Action::Quit; }

        //THE TASK IS LEFT TO FINISH ON ITS OWN - ITS RESULT NO LONGER MATCHES THE ATTEMPT NUMBER
        login.connecting = false;
        login.error = None;

        return Action::None;
    }

    if login.connecting { return Action::None; } //NOTHING IS EDITABLE WHILE THE SOCKET IS BEING OPENED

    if key.modifiers.contains(KeyModifiers::CONTROL)
    {
        match key.code
        {
            KeyCode::Char('a') => login.input.home(),
            KeyCode::Char('e') => login.input.end(),
            KeyCode::Char('u') => login.input.kill_to_start(),
            KeyCode::Char('k') => login.input.kill_to_end(),
            KeyCode::Char('w') => login.input.delete_word(),
            _ => {},
        }

        return Action::None;
    }

    match key.code
    {
        //ONE FIELD, ONE LINE - THERE IS NOTHING AN ADDRESS COULD DO WITH A NEWLINE
        KeyCode::Char(character) => login.input.insert(character),

        KeyCode::Backspace => login.input.backspace(),
        KeyCode::Delete => login.input.delete(),

        KeyCode::Left => login.input.left(),
        KeyCode::Right => login.input.right(),
        KeyCode::Home => login.input.home(),
        KeyCode::End => login.input.end(),

        KeyCode::Enter =>
        {
            if login.address().is_empty()
            {
                login.error = Some(String::from("Enter the address of a server."));
            } else { return Action::Connect; }
        },

        _ => {},
    }

    Action::None
}

pub fn insert_str(app: &mut App, text: &str) //A PASTE INTO THE ADDRESS FIELD
{
    if let Some(login) = app.login.as_mut() && !login.connecting
    {
        login.input.insert_str(&text.replace(['\r', '\n'], ""));
    }
}

//OPENS THE SOCKET IN A TASK OF ITS OWN, SO THE FRAME KEEPS BEING DRAWN WHILE A DEAD ADDRESS TIMES OUT
pub fn connect(app: &mut App, results: &Sender<ConnectResult>)
{
    let Some(login) = app.login.as_mut() else { return };

    let display = login.address();
    if display.is_empty() { return; }

    login.connecting = true;
    login.error = None;
    login.attempt += 1;

    let attempt = login.attempt;

    //THE TITLE ONLY SHOWS A PORT WHEN ONE WAS ASKED FOR, SO THE ADDRESS IS ALSO KEPT AS TYPED
    let mut address = display.clone();
    if !address.contains(':') { address.push_str(&format!(":{}", config::read_config::<u16>("default_port"))); }

    app.address = display;

    //THE RECONNECT AFTER PINNING A SERVER KEY DIALS THIS, SO IT HAS TO BE THE RESOLVED ADDRESS
    options::set_server_address(&address);

    let results = results.clone();

    tokio::spawn(async move
    {
        let _ = results.send((attempt, client::connect(address).await)).await;
    });
}
