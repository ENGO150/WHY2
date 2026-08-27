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
    sync::Mutex,
    task::AbortHandle,
    net::tcp::OwnedWriteHalf,
};

use why2::stream::RexStream;

use crate::
{
    crypto,
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

    //VIEWER MAPS
    let mut viewer_seqs = HashMap::<usize, usize>::new();
    let mut viewer_streams = HashMap::<usize, ([u8; 32], RexStream)>::new();

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

        //COLLECT ALL ATTACHED CLIENT STREAMS
        let entries: Vec<(usize, Arc<Mutex<OwnedWriteHalf>>)> = server::CONNECTIONS.iter().filter_map(|entry|
        {
            match entry.value()
            {
                Connection::Authenticated { id: client_id, attached_screen, keys, .. } =>
                {
                    //FILTER ATTACHED CLIENTS
                    if let Some(attached_screen) = attached_screen && attached_screen.target_id == id
                    {
                        //CHECK FOR EXISTING REX STREAM
                        if viewer_streams.get(client_id).map(|(t, _)| t != &attached_screen.token).unwrap_or(true)
                        {
                            //INIT REX STREAM
                            let rex_stream = crypto::init_rex_stream(&keys, &attached_screen.token).unwrap();

                            viewer_streams.insert(*client_id, (attached_screen.token, rex_stream));
                        }

                        //PREVENT FEEDBACK
                        if *client_id == id && matches!(read, ScreenPacketCode::Audio { .. })
                        {
                            return None;
                        }

                        //FOUND, COLLECT
                        Some((*client_id, attached_screen.stream.clone()))
                    } else { None }
                },
                _ => None,
            }
        }).collect();

        //FORWARD PACKET
        for (client_id, stream) in entries
        {
            let viewer_seq = viewer_seqs.entry(client_id).or_insert(0);
            let viewer_stream = viewer_streams.get_mut(&client_id).unwrap();

            screen::send_frame(&mut *stream.lock().await, read.clone(), &mut viewer_stream.1, Some(viewer_seq)).await;
        }
    }

    //THE UPLOAD SOCKET DIED - NOBODY ELSE KNOWS THE SHARE IS OVER
    end_share(id).await;
}
