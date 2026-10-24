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

//! # Account Form
//!
//! The overlay behind `/account passwd` and `/account delete`: a small form of password fields,
//! checked locally and then handed to the event loop to send. Deleting asks twice.


use crossterm::event::
{
    KeyCode,
    KeyEvent,
};

use zeroize::Zeroizing;

use crate::t;

use super::
{
    input::InputBuffer,
    login,
    state::App,
};

//ENUMS
/// Which form it is.
#[derive(Clone, Copy, PartialEq)] //WHAT THE FORM DOES
pub enum Kind
{
    /// Change the password.
    Passwd,
    /// Delete the account.
    Delete,
}

/// A finished form for the event loop to send.
pub enum Request //A FINISHED FORM FOR THE LOOP TO SEND
{
    /// A password change: the current and the new password.
    Passwd(String, String), //CURRENT, NEW
    /// An account deletion: the current password.
    Delete(String),         //CURRENT
}

//STRUCTS
/// An account form.
pub struct Account //AN ACCOUNT FORM
{
    /// Which form it is.
    pub kind: Kind,
    /// The password fields.
    pub fields: Vec<InputBuffer>,
    /// Which field has the caret.
    pub focus: usize,
    /// Whether the form is waiting for the server.
    pub busy: bool,            //WAITING FOR THE SERVER
    /// Whether a deletion was asked once and waits for a second Enter.
    pub armed: bool,           //DELETION ASKED ONCE, WAITING FOR THE SECOND ⏎
    /// Why the last attempt did not work.
    pub error: Option<String>, //WHY THE LAST ONE DID NOT WORK
    /// The new password, kept for the reconnect.
    new: Zeroizing<String>,    //THE PASSWORD SENT, FOR THE RECONNECT
    /// Whether a finished form waits for the event loop.
    request: bool,             //A FINISHED FORM FOR THE LOOP TO SEND
}

//IMPLEMENTATIONS
impl Kind
{
    /// Returns the field labels.
    pub fn labels(&self) -> Vec<&'static str>
    {
        match self
        {
            Kind::Passwd => vec![t!("account.label.current"), t!("account.label.new"), t!("account.label.confirm")],
            Kind::Delete => vec![t!("account.label.current")],
        }
    }

    //WHAT AN EMPTY FIELD ASKS FOR
    /// Returns the message for an empty field.
    fn missing(field: usize) -> &'static str
    {
        match field
        {
            0 => t!("account.missing.current"),
            1 => t!("account.missing.new"),
            _ => t!("account.missing.confirm"),
        }
    }

    /// Returns the form's title.
    pub fn title(&self) -> &'static str
    {
        match self
        {
            Kind::Passwd => t!("account.title.passwd"),
            Kind::Delete => t!("account.title.delete"),
        }
    }
}

impl Account
{
    /// Opens an empty form.
    pub fn new(kind: Kind) -> Self
    {
        Self
        {
            kind,
            fields: kind.labels().iter().map(|_| InputBuffer::new()).collect(),
            focus: 0,
            busy: false,
            armed: false,
            error: None,
            new: Zeroizing::default(),
            request: false,
        }
    }

    //THE REQUEST TO SEND, ONCE
    /// Takes the finished form, once.
    pub fn take_request(&mut self) -> Option<Request>
    {
        if !self.request { return None; }

        self.request = false;

        Some(match self.kind
        {
            Kind::Passwd => Request::Passwd(self.fields[0].text(), self.new.to_string()),
            Kind::Delete => Request::Delete(self.fields[0].text()),
        })
    }

    //THE NEW PASSWORD, FOR THE RECONNECT
    /// Returns the new password, for the reconnect.
    pub fn new_password(&self) -> &str { &self.new }

    //START OVER WITH AN ERROR
    /// Clears the form after a refusal, showing why.
    pub fn rejected(&mut self, error: String)
    {
        *self = Self::new(self.kind);
        self.error = Some(error);
    }

    //CHECK THE FORM, QUEUE IT
    /// Checks the form and queues it: empty fields and a mismatched confirmation are refused here,
    /// and a deletion is armed before it is sent.
    fn submit(&mut self)
    {
        //AN EMPTY FIELD TAKES THE CARET
        if let Some(empty) = self.fields.iter().position(InputBuffer::is_empty)
        {
            self.focus = empty;
            self.error = Some(Kind::missing(empty).to_owned());

            return;
        }

        match self.kind
        {
            Kind::Passwd =>
            {
                let new = Zeroizing::new(self.fields[1].text());

                //MISMATCH, RETYPE THE CONFIRMATION
                if *new != self.fields[2].text()
                {
                    self.fields[2].clear();
                    self.focus = 2;
                    self.error = Some(t!("account.mismatch").to_owned());

                    return;
                }

                self.new = new;
            },

            //ASK TWICE
            Kind::Delete => if !self.armed
            {
                self.armed = true;
                self.error = None;

                return;
            },
        }

        self.error = None;
        self.busy = true;
        self.request = true;
    }
}

//FUNCTIONS
//PUBLIC
/// Handles a key while the form is up.
pub fn handle_key(app: &mut App, key: KeyEvent)
{
    let Some(form) = app.account.as_mut() else { return };

    //NOTHING UNTIL THE SERVER ANSWERS
    if form.busy { return; }

    let last = form.fields.len() - 1;

    match key.code
    {
        //ESC CLOSES
        KeyCode::Esc => app.account = None,

        //MOVE BETWEEN FIELDS
        KeyCode::Up | KeyCode::BackTab => form.focus = form.focus.saturating_sub(1),
        KeyCode::Down | KeyCode::Tab => form.focus = (form.focus + 1).min(last),

        //ENTER STEPS DOWN, THE LAST ONE SUBMITS
        KeyCode::Enter => match form.focus == last
        {
            true => form.submit(),
            false => form.focus += 1,
        },

        //AN EDIT TAKES THE CONFIRMATION BACK
        _ => if login::edit(&mut form.fields[form.focus], key) { form.armed = false; },
    }
}

/// Pastes text into the focused field.
pub fn insert_str(app: &mut App, text: &str) //A PASTE INTO THE FOCUSED FIELD
{
    if let Some(form) = app.account.as_mut() && !form.busy
    {
        form.fields[form.focus].insert_str(&text.replace(['\r', '\n'], ""));
        form.armed = false;
    }
}
