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

//! # Push-to-Talk
//!
//! Recording and playing voice messages from the keyboard and the mouse.
//!
//! `Ctrl+R` held records and sends on release; a tap toggles instead. A terminal only reports key
//! releases under the kitty keyboard protocol ([`App::key_release`]). With releases, a release
//! sends unless the key was down for less than [`KEY_TAP`](consts::KEY_TAP), which makes it a
//! toggle. Without them, presses closer than [`KEY_REPEAT_GAP`](consts::KEY_REPEAT_GAP) are the
//! terminal's auto-repeat, and repeats stopping for
//! [`KEY_RELEASE_GAP`](consts::KEY_RELEASE_GAP) is the release, noticed by [`tick`]. `/record`
//! is the same toggle, typed, and `Esc` discards.

use std::
{
    sync::Arc,
    time::Instant,
};

use crossterm::event::KeyEventKind;

use tokio::
{
    net::tcp::OwnedWriteHalf,
    sync::
    {
        Mutex as MutexAsync,
        mpsc::Sender,
    },
};

use crate::
{
    t,
    network::
    {
        client::{ ClientEvent, media },
        voice::client::message,
    },
};

use super::
{
    consts,
    state::
    {
        App,
        Entry,
        Playback,
        Recording,
    },
};

//FUNCTIONS
//PRIVATE
/// Starts a recording, reporting a failure to start.
fn start(app: &mut App, tx: &Sender<ClientEvent>)
{
    if !message::start_recording(tx.clone())
    {
        app.notify(t!("voice_message.busy"));
        return;
    }

    let now = Instant::now();

    //HELD ONLY IF RELEASES ARE REPORTED
    app.recording = Some(Recording { started: now, pressed: now, held: app.key_release, shown: 0 });
    app.dirty = true;
}

/// Ends the recording and uploads it, unless it was too short to send.
fn send(app: &mut App, write_stream: Option<&Arc<MutexAsync<OwnedWriteHalf>>>, tx: &Sender<ClientEvent>)
{
    app.recording = None;
    app.dirty = true;

    let Some(write_stream) = write_stream else { return message::cancel_recording() };

    match message::finish_recording()
    {
        Some(clip) => crate::send_voice(write_stream, clip, tx.clone()),
        None => app.notify(t!("voice_message.too_short")),
    }
}

//PUBLIC
//THE RECORD SHORTCUT WENT DOWN, OR REPEATED
/// Handles a press (or repeat) of the record key.
pub fn press(app: &mut App, kind: KeyEventKind, write_stream: &Arc<MutexAsync<OwnedWriteHalf>>, tx: &Sender<ClientEvent>)
{
    let now = Instant::now();

    let Some(recording) = app.recording.as_mut() else { return start(app, tx) };

    //THE TERMINAL REPEATING A HELD KEY
    if kind == KeyEventKind::Repeat || (!app.key_release && now - recording.pressed < consts::KEY_REPEAT_GAP)
    {
        if !app.key_release { recording.held = true; }

        recording.pressed = now;
        return;
    }

    //A FRESH PRESS ENDS A TAPPED ONE
    send(app, Some(write_stream), tx);
}

//THE RECORD SHORTCUT WAS LET GO
/// Handles a release of the record key.
pub fn release(app: &mut App, write_stream: Option<&Arc<MutexAsync<OwnedWriteHalf>>>, tx: &Sender<ClientEvent>)
{
    let Some(recording) = app.recording.as_mut().filter(|recording| recording.held) else { return };

    //A TAP KEEPS RECORDING
    if recording.started.elapsed() < consts::KEY_TAP
    {
        recording.held = false;
        app.dirty = true;

        return;
    }

    send(app, write_stream, tx);
}

//START OR SEND (/record)
/// Toggles recording, as `/record` does.
pub fn toggle(app: &mut App, write_stream: &Arc<MutexAsync<OwnedWriteHalf>>, tx: &Sender<ClientEvent>)
{
    match app.recording
    {
        Some(_) => send(app, Some(write_stream), tx),
        None =>
        {
            start(app, tx);

            if let Some(recording) = app.recording.as_mut() { recording.held = false; }
        },
    }
}

/// Discards the recording.
pub fn cancel(app: &mut App) //THROW THE RECORDING AWAY
{
    message::cancel_recording();

    app.recording = None;
    app.notify(t!("voice_message.discarded"));
}

//ONCE PER REDRAW TICK
/// Follows the player and the recorder once per redraw tick: sends a recording that hit its
/// limit, or whose key repeats stopped, and updates the shown duration.
pub fn tick(app: &mut App, write_stream: Option<&Arc<MutexAsync<OwnedWriteHalf>>>, tx: &Sender<ClientEvent>)
{
    app.sync_playing(media::playing().or_else(message::playing));

    let Some(recording) = app.recording.as_mut() else { return };

    //THE RECORDER GAVE UP ON ITS OWN
    let Some((recorded, full)) = message::recording() else
    {
        app.recording = None;
        app.dirty = true;

        return;
    };

    if full
    {
        app.notify(t!("voice_message.full"));
        return send(app, write_stream, tx);
    }

    //THE REPEATS STOPPED
    if !app.key_release && recording.held && recording.pressed.elapsed() > consts::KEY_RELEASE_GAP
    {
        return send(app, write_stream, tx);
    }

    let seconds = recorded / 1000;

    if seconds != recording.shown
    {
        recording.shown = seconds;
        app.dirty = true;
    }
}

//WHAT THE PANE'S BORDER SAYS WHILE RECORDING
/// Returns what the pane's border says while recording.
pub fn status(app: &App) -> Option<String>
{
    let recording = app.recording?;

    let duration = format!("{}:{:02}", recording.shown / 60, recording.shown % 60);
    let send = if recording.held { t!("voice_message.hold") } else { t!("voice_message.toggle") };

    Some(format!("{} · {send} · {}", t!("voice_message.recording", duration), t!("voice_message.cancel")))
}

//PLAY ONE CLIP, STOPPING WHATEVER PLAYS
/// Plays a clip, stopping whatever is playing.
pub fn play(app: &mut App, hash: [u8; 32], tx: &Sender<ClientEvent>)
{
    app.stop_loading();
    app.set_playback(&hash, Playback::Loading);

    message::play(hash, tx.clone());
}

//PLAY AN AUDIO OR VIDEO ATTACHMENT
pub fn play_media(app: &mut App, hash: [u8; 32], tx: &Sender<ClientEvent>)
{
    app.stop_loading();
    media::play(hash, tx.clone());
    app.dirty = true;
}

/// Toggles a playable attachment caption, returning whether the click was handled.
pub fn click_media(app: &mut App, entry: usize, tx: &Sender<ClientEvent>) -> bool
{
    let Some(Entry::Attachment { hash: Some(hash), filename, .. }) = app.messages.get(entry)
        else { return false };

    if !media::supported(filename) { return false; }
    let hash = *hash;

    match media::playing().is_some_and(|(playing, _)| playing == hash)
    {
        true => stop(app),
        false => play_media(app, hash, tx),
    }

    true
}

/// Stops playback.
pub fn stop(app: &mut App) //STOP PLAYBACK
{
    message::stop();

    app.stop_loading();
    app.sync_playing(None);
}

//A CLICKED BUTTON
/// Toggles playback of a clicked voice message.
pub fn click(app: &mut App, entry: usize, tx: &Sender<ClientEvent>)
{
    let Some((hash, active)) = app.voice_entry(entry) else { return };

    match active
    {
        true => stop(app),
        false => play(app, hash, tx),
    }
}
