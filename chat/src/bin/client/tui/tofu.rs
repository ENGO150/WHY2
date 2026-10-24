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

//! # Server Identity Prompt
//!
//! The modal overlay that asks the user to trust a server identity the client has not pinned,
//! or one that differs from the pinned one.
//!
//! Refusing is always one key away and trusting never is: the prompt opens on the safe button,
//! and a changed identity — the louder of the two warnings — is asked twice, the second time by
//! typing a word out.

use crossterm::event::
{
    KeyCode,
    KeyEvent,
    KeyModifiers,
};

use crate::
{
    t,
    network::client::TofuRequest,
};

use super::
{
    theme,
    state::App,
};

//ENUMS
//A MISMATCH IS ASKED TWICE
/// How far through the prompt the user is.
#[derive(PartialEq)]
pub enum Stage
{
    /// The warning, with Reject and Trust.
    Warn,    //THE WARNING PLUS Reject/Trust
    /// The type-it-out confirmation, for a changed identity only.
    Confirm, //TYPE-IT-OUT CHALLENGE, MISMATCH ONLY
}

//STRUCTS
//THE SERVER IDENTITY PROMPT, A MODAL OVERLAY
/// The server identity prompt.
pub struct Prompt
{
    /// The host the identity is pinned against.
    pub host: String,
    /// The identity's fingerprint.
    pub hash: String,
    /// The fingerprint on record, if any.
    pub pinned: Option<String>, //THE FINGERPRINT ON RECORD
    /// Whether a different identity is pinned.
    pub mismatch: bool,         //A PINNED KEY DIFFERS - THE LOUDER OF THE TWO WARNINGS
    /// Whether the Trust button is selected; starts on Reject.
    pub accept: bool,           //SELECTED BUTTON, STARTING ON THE SAFE ONE
    /// How far through the prompt the user is.
    pub stage: Stage,
    /// What has been typed into the confirmation.
    pub typed: String,          //WHAT HAS BEEN TYPED INTO THE CHALLENGE SO FAR
    /// Whether the last confirmation did not match.
    pub wrong: bool,            //THE LAST ⏎ ON THE CHALLENGE DID NOT MATCH
    request: TofuRequest,
}

//IMPLEMENTATIONS
impl Prompt
{
    /// Opens the prompt for a request from the handshake.
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

    /// Returns the prompt's title.
    pub fn title(&self) -> &'static str
    {
        match (self.mismatch, &self.stage)
        {
            (_, Stage::Confirm)  => t!("tofu.title.confirm"),
            (true, Stage::Warn)  => t!("tofu.title.changed"),
            (false, Stage::Warn) => t!("tofu.title.unknown"),
        }
    }

    //THE 64 HEX CHARS, GROUPED IN EIGHTS
    /// Returns the fingerprint, in groups of eight hex digits.
    pub fn fingerprint(&self) -> Vec<String>
    {
        Self::group(&self.hash)
    }

    /// Returns the pinned fingerprint, in groups of eight hex digits.
    pub fn pinned_fingerprint(&self) -> Vec<String>
    {
        self.pinned.as_deref().map(Self::group).unwrap_or_default()
    }

    /// Splits a hash into groups of eight characters.
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
/// Handles a key while the prompt is up.
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
                if character.is_alphabetic()
                    && prompt.typed.chars().count() < t!("tofu.challenge").chars().count()
                    && !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                prompt.typed.extend(character.to_lowercase());
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
                if prompt.typed == t!("tofu.challenge") { answer(app, true); } else { prompt.wrong = true; }
            },

            _ => {},
        },
    }

    app.dirty = true;
}

//PRIVATE
/// Answers the handshake and closes the prompt.
fn answer(app: &mut App, accept: bool)
{
    let Some(prompt) = app.tofu.take() else { return };

    //THE NETWORK TASK PINS THE KEY OR DISCONNECTS
    let _ = prompt.request.reply.send(accept);

    if accept
    {
        app.push_styled(t!("tofu.accepted", host = prompt.host), theme::ok());
    }

    app.dirty = true;
}
