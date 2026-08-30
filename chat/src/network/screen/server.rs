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
    time::Instant,
    collections::HashMap,
    sync::{ Arc, LazyLock },
};

use tokio::
{
    task::AbortHandle,
    net::tcp::OwnedWriteHalf,
    sync::
    {
        Mutex,
        mpsc::{ self, Sender },
    },
};

use crate::
{
    crypto,
    consts::SharedKeys,
    network::
    {
        self,
        Streams,
        codes::PacketCode,
        server::
        {
            self,
            connection::Connection,
        },
        screen::{ self, ScreenPacketCode },
    },
};

//PRIVATE
//STATICS
//WHAT A MUTED SHARER'S VIEWERS GET INSTEAD OF THEIR SCREEN, AS ANNEX-B H.264. IT IS COMMITTED
//PRE-ENCODED (FROM `assets/muted.gif`) BECAUSE THE SERVER HAS NO ENCODER AND MUST NOT GROW ONE -
//`openh264` IS A CLIENT-ONLY DEPENDENCY. EVERY FRAME IS AN IDR CARRYING ITS OWN SPS/PPS, SO ANY
//ONE OF THEM CAN BE HANDED TO A VIEWER THAT ATTACHED HALF A LOOP AGO AND STILL DECODE:
//
//  ffmpeg -i assets/muted.gif -an -vf "pad=220:216:0:0:color=black,format=yuv420p" \
//      -c:v libx264 -profile:v baseline -preset veryslow -crf 20 \
//      -x264-params keyint=1:min-keyint=1:scenecut=0:repeat-headers=1 -f h264 assets/muted.h264
static MUTED_ANIMATION: LazyLock<Vec<Vec<u8>>> = LazyLock::new(|| split_access_units(include_bytes!("./assets/muted.h264")));

//STRUCTS
struct ScreenTransferGuard
{
    id: usize,
}

struct Viewer //ONE ATTACHED CLIENT, AND THE TASK THAT WRITES TO IT
{
    token: [u8; 32],              //THE ATTACHMENT THIS TASK WAS BUILT FOR
    tx: Sender<ScreenPacketCode>, //HANDOFF TO THAT TASK
    task: AbortHandle,            //THE TASK ITSELF
    needs_key: bool,              //SOMETHING WAS SHED - NOTHING IS DECODABLE UNTIL THE NEXT IDR
}

//IMPLEMENTATIONS
impl Drop for ScreenTransferGuard
{
    fn drop(&mut self)
    {
        if let Some(mut conn) = server::CONNECTIONS.iter_mut().find(|c| c.id() == Some(&self.id))
        {
            //REMOVE SCREEN STREAM
            conn.remove_screen_stream();
        }
    }
}

impl Drop for Viewer
{
    fn drop(&mut self)
    {
        //THE TASK MAY BE PARKED IN `write_all` ON A SOCKET THAT WILL NEVER DRAIN, SO CLOSING THE
        //CHANNEL IS NOT ENOUGH TO END IT - IT WOULD NEVER REACH THE NEXT `recv`
        self.task.abort();
    }
}

//FUNCTIONS
fn split_access_units(bitstream: &[u8]) -> Vec<Vec<u8>> //CUT AN ANNEX-B STREAM INTO ONE BUFFER PER FRAME
{
    let mut units = Vec::new();
    let mut start = None;

    let mut index = 0;
    while index + 3 < bitstream.len()
    {
        //NOT A START CODE
        if bitstream[index..index + 3] != [0, 0, 1] { index += 1; continue; }

        //AN SPS OPENS A FRAME (EVERY FRAME REPEATS ITS HEADERS), SO THE PREVIOUS ONE ENDS HERE
        if bitstream[index + 3] & 0x1f == 7
        {
            //BACK UP OVER THE LEADING ZERO OF A FOUR-BYTE START CODE
            let mut boundary = index;
            while boundary > 0 && bitstream[boundary - 1] == 0 { boundary -= 1; }

            if let Some(start) = start.replace(boundary)
            {
                units.push(bitstream[start..boundary].to_vec());
            }
        }

        index += 3;
    }

    //THE LAST FRAME RUNS TO THE END
    if let Some(start) = start { units.push(bitstream[start..].to_vec()); }

    units
}

fn is_keyframe(bitstream: &[u8]) -> bool //DOES THIS ACCESS UNIT STAND ON ITS OWN?
{
    //ONLY AN IDR (NAL TYPE 5), OR THE SPS (7) THE ENCODER REPEATS IN FRONT OF ONE, CAN BE DECODED
    //WITHOUT THE FRAMES BEFORE IT - WHICH IS THE ONLY THING A VIEWER THAT HAS BEEN SHED CAN USE
    let mut index = 0;
    while index + 3 < bitstream.len()
    {
        if bitstream[index..index + 3] != [0, 0, 1] { index += 1; continue; }

        if matches!(bitstream[index + 3] & 0x1f, 5 | 7) { return true; }

        index += 3;
    }

    false
}

fn spawn_viewer //ONE TASK PER VIEWER, SO A SLOW ONE BLOCKS ONLY ITSELF
(
    stream: Arc<Mutex<OwnedWriteHalf>>,
    keys: &SharedKeys,
    token: [u8; 32],
) -> Option<Viewer>
{
    //THE REX STREAM AND THE SEQUENCE NUMBER ARE PER VIEWER, SO THEY MOVE INTO THE TASK WITH IT
    let mut rex_stream = crypto::init_rex_stream(keys, &token)?;
    let (tx, mut rx) = mpsc::channel(screen::consts::VIEWER_CHANNEL_BOUND);

    let task = tokio::spawn(async move
    {
        let mut seq = 0usize;

        while let Some(code) = rx.recv().await
        {
            screen::send_frame(&mut *stream.lock().await, code, &mut rex_stream, Some(&mut seq)).await;
        }
    }).abort_handle();

    Some(Viewer { token, tx, task, needs_key: false })
}

fn muted_frame(started: &Instant) -> Option<usize> //INDEX OF THE PLACEHOLDER FRAME DUE RIGHT NOW
{
    //THE ANIMATION IS PLAYED OFF THE WALL CLOCK RATHER THAN OFF ARRIVING FRAMES: THE SHARER'S
    //FRAME RATE IS WHATEVER THEIR DESKTOP IS DOING, AND ADVANCING PER ARRIVAL WOULD PLAY THE LOOP
    //AT THAT SPEED. THE FLIP SIDE IS THAT A *STILL* DESKTOP ONLY SENDS ONE FRAME EVERY
    //`FORCED_INTRA_INTERVAL`, WHICH IS ALL THE PLACEHOLDER GETS TO ADVANCE ON
    let frames = MUTED_ANIMATION.len();
    if frames == 0 { return None; }

    Some((started.elapsed().as_millis() / screen::consts::MUTED_FRAME_INTERVAL.as_millis()) as usize % frames)
}

async fn end_share(id: usize) //TEAR THE SHARE DOWN AND TELL EVERYONE ABOUT IT
{
    //TAKE THE SHARE STATE (WITHOUT ABORTING - WE *ARE* THE SHARE TASK)
    let (write_stream, keys, username) =
    {
        let mut conn = match server::CONNECTIONS.iter_mut().find(|c| c.id() == Some(&id))
        {
            Some(c) => c,
            None => return
        };

        //ALREADY TORN DOWN (AND NOTIFIED) BY SOMEBODY ELSE
        if conn.take_screen_stream().is_none() { return; }

        (conn.write_stream().clone(), conn.keys().cloned(), conn.username().cloned())
    };

    //DEATTACH EVERY VIEWER
    if let Some(username) = username
    {
        server::deattach(id, &username).await;
    }

    //TELL THE SHARER ITS SHARE IS GONE
    network::send(&mut *write_stream.lock().await, PacketCode::Screen { token: None }, keys.as_ref()).await;
}

//PUBLIC
pub async fn screen(token: [u8; 32], id: usize, streams: &mut Streams<'_>, task: AbortHandle)
{
    //GET CLIENT KEYS
    let keys =
    {
        //FIND CONNECTION BY ID
        let conn = server::CONNECTIONS.iter_mut()
            .find(|e| e.value().id() == Some(&id));

        match conn
        {
            Some(mut c) =>
            {
                let keys = match c.keys()
                {
                    Some(k) => k.clone(),
                    None => return
                };

                //ADD SCREEN STREAM
                c.set_screen_stream(task);

                keys
            },
            None => return
        }
    };

    //DISCONNECT GUARD
    let _guard = ScreenTransferGuard { id };

    //LOCAL SEQ
    let mut seq = 0usize;

    //ONE ENTRY PER ATTACHED VIEWER, EACH WITH ITS OWN WRITER TASK
    let mut viewers = HashMap::<usize, Viewer>::new();

    //INIT REX STREAM
    let mut rex_stream = crypto::init_rex_stream(&keys, &token).unwrap();

    //PLACEHOLDER PLAYBACK STATE
    let started = Instant::now();
    let mut sent_muted_frame = None;

    //LOOP READING
    loop
    {
        //READ
        let read = match screen::receive_frame(streams, &mut rex_stream, &mut seq).await
        {
            Some(r) => r,
            None => break
        };

        //IS THE SHARER MUTED? (COLLECT AND DROP THE GUARD - IT MUST NOT BE HELD ACROSS THE SENDS BELOW)
        let muted = server::CONNECTIONS.iter()
            .find(|c| c.id() == Some(&id))
            .map(|c| *c.muted())
            .unwrap_or(false);

        //SILENCE MUTED USERS - THEIR SCREEN NEVER LEAVES THE SERVER, THE PLACEHOLDER GOES OUT IN ITS PLACE
        let read = match (muted, read)
        {
            //NOT MUTED, FORWARD WHATEVER CAME IN
            (false, read) =>
            {
                sent_muted_frame = None;

                read
            },

            //MUTED AUDIO IS SIMPLY DROPPED - THERE IS NOTHING TO PUT IN ITS PLACE
            (true, ScreenPacketCode::Audio { .. }) => continue,

            (true, ScreenPacketCode::Video { .. }) =>
            {
                let Some(frame) = muted_frame(&started) else { continue; };

                //THE SHARER SENDS FAR FASTER THAN THE PLACEHOLDER ADVANCES - RESENDING THE SAME
                //FRAME WOULD ONLY COST EVERY VIEWER AN IDR TO REDRAW THE PICTURE THEY ALREADY HAVE
                if sent_muted_frame == Some(frame) { continue; }
                sent_muted_frame = Some(frame);

                ScreenPacketCode::Video { data: MUTED_ANIMATION[frame].clone() }
            },
        };

        //COLLECT ALL ATTACHED CLIENT STREAMS (THE KEYS COME ALONG ONLY WHEN A TASK STILL HAS TO BE BUILT)
        let entries: Vec<(usize, Arc<Mutex<OwnedWriteHalf>>, [u8; 32], Option<SharedKeys>)> = server::CONNECTIONS.iter().filter_map(|entry|
        {
            match entry.value()
            {
                Connection::Authenticated { id: client_id, attached_screen, keys, .. } =>
                {
                    //FILTER ATTACHED CLIENTS
                    if let Some(attached_screen) = attached_screen && attached_screen.target_id == id
                    {
                        //A VIEWER WE ARE ALREADY SERVING KEEPS ITS TASK, AND WITH IT ITS REX STREAM
                        let known = viewers.get(client_id).is_some_and(|v| v.token == attached_screen.token);

                        //FOUND, COLLECT
                        Some((*client_id, attached_screen.stream.clone(), attached_screen.token,
                            if known { None } else { Some(keys.clone()) }))
                    } else { None }
                },
                _ => None,
            }
        }).collect();

        //RETIRE WHOEVER LEFT - DROPPING A `Viewer` ABORTS ITS TASK AND CLOSES ITS SOCKET
        viewers.retain(|client_id, _| entries.iter().any(|(e, ..)| e == client_id));

        //FORWARD PACKET
        let keyframe = matches!(&read, ScreenPacketCode::Video { data } if is_keyframe(data));

        for (client_id, stream, token, keys) in entries
        {
            //A NEW ATTACHMENT (OR A RE-ATTACHMENT UNDER A NEW TOKEN) NEEDS A TASK OF ITS OWN
            if let Some(keys) = keys
            {
                let Some(viewer) = spawn_viewer(stream, &keys, token) else { continue; };

                viewers.insert(client_id, viewer);
            }

            let Some(viewer) = viewers.get_mut(&client_id) else { continue; };

            //PREVENT FEEDBACK
            if client_id == id && matches!(read, ScreenPacketCode::Audio { .. }) { continue; }

            //A VIEWER THAT MISSED A FRAME CANNOT DECODE A PREDICTED ONE - IT HOLDS ITS LAST PICTURE
            //UNTIL AN IDR COMES ROUND (AT MOST `FORCED_INTRA_INTERVAL`) RATHER THAN BE HANDED RUBBISH
            if viewer.needs_key && matches!(read, ScreenPacketCode::Video { .. })
            {
                if !keyframe { continue; }

                viewer.needs_key = false;
            }

            //A FULL QUEUE MEANS *THIS* VIEWER'S LINK CANNOT CARRY THE SHARE. SHEDDING THE FRAME IS
            //THE WHOLE POINT: THE SHARE RUNS AT THE SHARER'S RATE AND A SLOW VIEWER PAYS ALONE,
            //WHERE FORWARDING INLINE MADE EVERYBODY WAIT FOR THE WORST LINK ON THE SERVER
            if viewer.tx.try_send(read.clone()).is_err() && matches!(read, ScreenPacketCode::Video { .. })
            {
                viewer.needs_key = true;
            }
        }
    }

    //THE UPLOAD SOCKET DIED - NOBODY ELSE KNOWS THE SHARE IS OVER
    end_share(id).await;
}
