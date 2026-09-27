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


use crossterm::event::
{
    KeyCode,
    KeyEvent,
};

use zeroize::Zeroizing;

use super::
{
    input::InputBuffer,
    login,
    state::App,
};

//CONSTS
pub const LABELS: [&str; 3] = [ "Current password", "New password", "Confirm new password" ]; //ONE FIELD EACH

//STRUCTS
pub struct Passwd //THE PASSWORD CHANGE FORM
{
    pub fields: [InputBuffer; 3],
    pub focus: usize,
    pub busy: bool,            //WAITING FOR THE SERVER
    pub error: Option<String>, //WHY THE LAST ONE DID NOT WORK
    new: Zeroizing<String>,    //THE PASSWORD SENT, FOR THE RECONNECT
    request: bool,             //A FINISHED CHANGE FOR THE LOOP TO SEND
}

//IMPLEMENTATIONS
impl Default for Passwd
{
    fn default() -> Self { Self::new() }
}

impl Passwd
{
    pub fn new() -> Self
    {
        Self
        {
            fields: [InputBuffer::new(), InputBuffer::new(), InputBuffer::new()],
            focus: 0,
            busy: false,
            error: None,
            new: Zeroizing::default(),
            request: false,
        }
    }

    //THE PAIR TO SEND, ONCE
    pub fn take_request(&mut self) -> Option<(String, String)>
    {
        if !self.request { return None; }

        self.request = false;

        Some((self.fields[0].text(), self.new.to_string()))
    }

    //THE NEW PASSWORD, FOR THE RECONNECT
    pub fn new_password(&self) -> &str { &self.new }

    //START OVER WITH AN ERROR
    pub fn rejected(&mut self, error: String)
    {
        *self = Self::new();
        self.error = Some(error);
    }

    //CHECK THE FORM, QUEUE IT
    fn submit(&mut self)
    {
        //AN EMPTY FIELD TAKES THE CARET
        if let Some(empty) = self.fields.iter().position(InputBuffer::is_empty)
        {
            self.focus = empty;
            self.error = Some(format!("Enter the {}.", LABELS[empty].to_lowercase()));

            return;
        }

        let new = Zeroizing::new(self.fields[1].text());

        //MISMATCH, RETYPE THE CONFIRMATION
        if *new != self.fields[2].text()
        {
            self.fields[2].clear();
            self.focus = 2;
            self.error = Some(String::from("Passwords do not match."));

            return;
        }

        self.new = new;
        self.error = None;
        self.busy = true;
        self.request = true;
    }
}

//FUNCTIONS
//PUBLIC
pub fn handle_key(app: &mut App, key: KeyEvent)
{
    let Some(passwd) = app.passwd.as_mut() else { return };

    //NOTHING UNTIL THE SERVER ANSWERS
    if passwd.busy { return; }

    let last = passwd.fields.len() - 1;

    match key.code
    {
        //ESC CLOSES
        KeyCode::Esc => app.passwd = None,

        //MOVE BETWEEN FIELDS
        KeyCode::Up | KeyCode::BackTab => passwd.focus = passwd.focus.saturating_sub(1),
        KeyCode::Down | KeyCode::Tab => passwd.focus = (passwd.focus + 1).min(last),

        //ENTER STEPS DOWN, THE LAST ONE SUBMITS
        KeyCode::Enter => match passwd.focus == last
        {
            true => passwd.submit(),
            false => passwd.focus += 1,
        },

        _ => { login::edit(&mut passwd.fields[passwd.focus], key); },
    }
}

pub fn insert_str(app: &mut App, text: &str) //A PASTE INTO THE FOCUSED FIELD
{
    if let Some(passwd) = app.passwd.as_mut() && !passwd.busy
    {
        passwd.fields[passwd.focus].insert_str(&text.replace(['\r', '\n'], ""));
    }
}
