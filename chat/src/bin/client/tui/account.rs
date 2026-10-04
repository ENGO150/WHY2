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

use crate::t;

use super::
{
    input::InputBuffer,
    login,
    state::App,
};

//ENUMS
#[derive(Clone, Copy, PartialEq)] //WHAT THE FORM DOES
pub enum Kind
{
    Passwd,
    Delete,
}

pub enum Request //A FINISHED FORM FOR THE LOOP TO SEND
{
    Passwd(String, String), //CURRENT, NEW
    Delete(String),         //CURRENT
}

//STRUCTS
pub struct Account //AN ACCOUNT FORM
{
    pub kind: Kind,
    pub fields: Vec<InputBuffer>,
    pub focus: usize,
    pub busy: bool,            //WAITING FOR THE SERVER
    pub armed: bool,           //DELETION ASKED ONCE, WAITING FOR THE SECOND ⏎
    pub error: Option<String>, //WHY THE LAST ONE DID NOT WORK
    new: Zeroizing<String>,    //THE PASSWORD SENT, FOR THE RECONNECT
    request: bool,             //A FINISHED FORM FOR THE LOOP TO SEND
}

//IMPLEMENTATIONS
impl Kind
{
    pub fn labels(&self) -> Vec<&'static str>
    {
        match self
        {
            Kind::Passwd => vec![t!("account.label.current"), t!("account.label.new"), t!("account.label.confirm")],
            Kind::Delete => vec![t!("account.label.current")],
        }
    }

    //WHAT AN EMPTY FIELD ASKS FOR
    fn missing(field: usize) -> &'static str
    {
        match field
        {
            0 => t!("account.missing.current"),
            1 => t!("account.missing.new"),
            _ => t!("account.missing.confirm"),
        }
    }

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
    pub fn new_password(&self) -> &str { &self.new }

    //START OVER WITH AN ERROR
    pub fn rejected(&mut self, error: String)
    {
        *self = Self::new(self.kind);
        self.error = Some(error);
    }

    //CHECK THE FORM, QUEUE IT
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

pub fn insert_str(app: &mut App, text: &str) //A PASTE INTO THE FOCUSED FIELD
{
    if let Some(form) = app.account.as_mut() && !form.busy
    {
        form.fields[form.focus].insert_str(&text.replace(['\r', '\n'], ""));
        form.armed = false;
    }
}
