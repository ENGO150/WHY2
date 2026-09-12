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
    KeyModifiers,
};

use crate::network::client::TofuRequest;

use super::
{
    consts,
    theme,
    state::App,
};

//ENUMS
//A MISMATCH IS ASKED TWICE
#[derive(PartialEq)]
pub enum Stage
{
    Warn,    //THE WARNING PLUS Reject/Trust
    Confirm, //TYPE-IT-OUT CHALLENGE, MISMATCH ONLY
}

//STRUCTS
//THE SERVER IDENTITY PROMPT, A MODAL OVERLAY
pub struct Prompt
{
    pub host: String,
    pub hash: String,
    pub pinned: Option<String>, //THE FINGERPRINT ON RECORD
    pub mismatch: bool,         //A PINNED KEY DIFFERS - THE LOUDER OF THE TWO WARNINGS
    pub accept: bool,           //SELECTED BUTTON, STARTING ON THE SAFE ONE
    pub stage: Stage,
    pub typed: String,          //WHAT HAS BEEN TYPED INTO THE CHALLENGE SO FAR
    pub wrong: bool,            //THE LAST ⏎ ON THE CHALLENGE DID NOT MATCH
    request: TofuRequest,
}

//IMPLEMENTATIONS
impl Prompt
{
    pub fn new(request: TofuRequest) -> Self
    {
        Self
        {
            host: request.host.clone(),
            hash: request.hash.clone(),
            pinned: request.pinned.clone(),
            mismatch: request.mismatch,
            accept: false,
            stage: Stage::Warn,
            typed: String::new(),
            wrong: false,
            request,
        }
    }

    pub fn title(&self) -> &'static str
    {
        match (self.mismatch, &self.stage)
        {
            (_, Stage::Confirm)  => " Confirm the new server key ",
            (true, Stage::Warn)  => " Server identity changed ",
            (false, Stage::Warn) => " Unknown server identity ",
        }
    }

    //THE 64 HEX CHARS, GROUPED IN EIGHTS
    pub fn fingerprint(&self) -> Vec<String>
    {
        Self::group(&self.hash)
    }

    pub fn pinned_fingerprint(&self) -> Vec<String>
    {
        self.pinned.as_deref().map(Self::group).unwrap_or_default()
    }

    fn group(hash: &str) -> Vec<String>
    {
        let groups = hash.as_bytes()
            .chunks(8)
            .map(|group| String::from_utf8_lossy(group).into_owned())
            .collect::<Vec<String>>();

        groups.chunks(4).map(|row| row.join(" ")).collect()
    }
}

//FUNCTIONS
//PUBLIC
pub fn handle_key(app: &mut App, key: KeyEvent)
{
    let Some(prompt) = app.tofu.as_mut() else { return };

    //REFUSING IS ALWAYS ONE KEY AWAY, TRUSTING NEVER
    if key.code == KeyCode::Esc
    {
        answer(app, false);
        return;
    }

    match prompt.stage
    {
        Stage::Warn => match key.code
        {
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => prompt.accept = !prompt.accept,

            KeyCode::Enter =>
            {
                let accept = prompt.accept;

                //A FIRST CONTACT IS ANSWERED HERE
                if accept && prompt.mismatch
                {
                    prompt.stage = Stage::Confirm;
                    prompt.typed.clear();
                    prompt.wrong = false;
                } else { answer(app, accept); }
            },

            _ => {},
        },

        Stage::Confirm => match key.code
        {
            //LETTERS ONLY, NO LONGER THAN THE WORD
            KeyCode::Char(character)
                if character.is_ascii_alphabetic()
                    && prompt.typed.chars().count() < consts::CHALLENGE.chars().count()
                    && !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                prompt.typed.push(character.to_ascii_lowercase());
                prompt.wrong = false;
            },

            KeyCode::Backspace =>
            {
                prompt.typed.pop();
                prompt.wrong = false;
            },

            //BACK OUT TO THE WARNING
            KeyCode::Left | KeyCode::BackTab =>
            {
                prompt.stage = Stage::Warn;
                prompt.accept = false;
                prompt.typed.clear();
                prompt.wrong = false;
            },

            KeyCode::Enter =>
            {
                if prompt.typed == consts::CHALLENGE { answer(app, true); } else { prompt.wrong = true; }
            },

            _ => {},
        },
    }

    app.dirty = true;
}

//PRIVATE
fn answer(app: &mut App, accept: bool)
{
    let Some(prompt) = app.tofu.take() else { return };

    //THE NETWORK TASK PINS THE KEY OR DISCONNECTS
    let _ = prompt.request.reply.send(accept);

    if accept
    {
        app.push_styled(format!("Server identity for {} accepted and saved. Reconnecting...", prompt.host),
            theme::OK);
    }

    app.dirty = true;
}
