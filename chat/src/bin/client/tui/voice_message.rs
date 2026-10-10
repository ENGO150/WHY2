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
        client::ClientEvent,
        voice::client::message,
    },
};

use super::
{
    consts,
    state::
    {
        App,
        Playback,
        Recording,
    },
};

//FUNCTIONS
//PRIVATE
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

pub fn cancel(app: &mut App) //THROW THE RECORDING AWAY
{
    message::cancel_recording();

    app.recording = None;
    app.notify(t!("voice_message.discarded"));
}

//ONCE PER REDRAW TICK
pub fn tick(app: &mut App, write_stream: Option<&Arc<MutexAsync<OwnedWriteHalf>>>, tx: &Sender<ClientEvent>)
{
    app.sync_playing(message::playing());

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
pub fn status(app: &App) -> Option<String>
{
    let recording = app.recording?;

    let duration = format!("{}:{:02}", recording.shown / 60, recording.shown % 60);
    let send = if recording.held { t!("voice_message.hold") } else { t!("voice_message.toggle") };

    Some(format!("{} · {send} · {}", t!("voice_message.recording", duration), t!("voice_message.cancel")))
}

//PLAY ONE CLIP, STOPPING WHATEVER PLAYS
pub fn play(app: &mut App, hash: [u8; 32], tx: &Sender<ClientEvent>)
{
    app.stop_loading();
    app.set_playback(&hash, Playback::Loading);

    message::play(hash, tx.clone());
}

pub fn stop(app: &mut App) //STOP PLAYBACK
{
    message::stop();

    app.stop_loading();
    app.sync_playing(None);
}

//A CLICKED BUTTON
pub fn click(app: &mut App, entry: usize, tx: &Sender<ClientEvent>)
{
    let Some((hash, active)) = app.voice_entry(entry) else { return };

    match active
    {
        true => stop(app),
        false => play(app, hash, tx),
    }
}
