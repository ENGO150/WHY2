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
    sync::
    {
        Arc,
        LazyLock,
        Mutex as MutexSync,
    },
};

use dashmap::DashMap;

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

//EVERY RUNNING SHARE, REACHABLE FROM WHERE A VIEWER ATTACHES RATHER THAN ONLY FROM THE SHARE'S OWN TASK
static SHARES: LazyLock<DashMap<usize, Arc<Share>>> = LazyLock::new(|| DashMap::new());

//STRUCTS
struct ScreenTransferGuard
{
    id: usize,
}

struct Share //WHAT A SHARE LEAVES WHERE AN ATTACHING VIEWER CAN REACH IT
{
    keyframe: MutexSync<Option<Vec<u8>>>,     //THE LAST PICTURE THAT STANDS ON ITS OWN
    pending: MutexSync<Vec<(usize, Viewer)>>, //VIEWERS BUILT AT THE ATTACH, WAITING TO BE PICKED UP
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
        //NOTHING MAY ATTACH TO A SHARE THAT IS OVER
        SHARES.remove(&self.id);

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

    //A VIEWER THAT HAS JUST ATTACHED HAS NO PICTURE TO PREDICT FROM EITHER, SO IT STARTS IN THE
    //SAME STATE AS A SHED ONE: NOTHING BUT AN IDR IS WORTH THE BANDWIDTH UNTIL IT HAS ONE
    Some(Viewer { token, tx, task, needs_key: true })
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

    log::info!("Screen share ended (upload socket closed): {}", server::log_addr(&id));

    //DEATTACH EVERY VIEWER
    if let Some(username) = username
    {
        server::deattach(id, &username).await;
    }

    //TELL THE SHARER ITS SHARE IS GONE
    network::send(&mut *write_stream.lock().await, PacketCode::Screen { token: None }, keys.as_ref()).await;
}

//PUBLIC
pub fn attach //BUILD A VIEWER WHERE IT ATTACHES, AND GIVE IT SOMETHING TO SHOW STRAIGHT AWAY
(
    sharer_id: usize,
    client_id: usize,
    keys: &SharedKeys,
    stream: Arc<Mutex<OwnedWriteHalf>>,
    token: [u8; 32],
) -> bool
{
    //THE SHARE IS OVER (OR NEVER STARTED)
    let Some(share) = SHARES.get(&sharer_id).map(|share| share.clone()) else { return false; };

    let Some(viewer) = spawn_viewer(stream, keys, token) else { return false; };

    //THE WINDOW OPENS ON THE SHARE'S LAST KEYFRAME RATHER THAN ON BLACK. IT IS AT MOST
    //`FORCED_INTRA_INTERVAL` OLD AND THE FRAMES SINCE IT WENT TO SOMEBODY ELSE, SO THE VIEWER STAYS
    //`needs_key` AND SNAPS TO LIVE ON THE NEXT IDR - A STILL PICTURE THAT IS A LITTLE BEHIND READS AS
    //A SHARE THAT IS STARTING, WHERE AN EMPTY ONE READS AS A SHARE THAT IS BROKEN
    if let Some(frame) = share.keyframe.lock().ok().and_then(|frame| frame.clone())
    {
        let _ = viewer.tx.try_send(ScreenPacketCode::Video { data: frame });
    }

    //PICKED UP BY THE SHARE LOOP ON ITS NEXT FRAME
    match share.pending.lock()
    {
        Ok(mut pending) =>
        {
            pending.push((client_id, viewer));

            true
        },
        Err(_) => false,
    }
}

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

    //THE SHARE'S OWN SOCKET IS AUXILIARY - EVERY LINE ABOUT IT IS KEYED BY THE MAIN CONNECTION
    let owner = server::log_addr(&id);

    log::info!("Screen share started: {owner}");

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

    //REACHABLE FROM THE ACCEPT LOOP FROM HERE ON - A VIEWER IS BUILT WHERE IT ATTACHES
    let share = Arc::new(Share
    {
        keyframe: MutexSync::new(None),
        pending: MutexSync::new(Vec::new()),
    });

    SHARES.insert(id, share.clone());

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

        //TAKE ON WHOEVER ATTACHED SINCE THE LAST FRAME. THEIR TASK WAS BUILT AT THE ATTACH AND HAS
        //ALREADY BEEN HANDED THE PICTURE THE SHARE STOOD ON THEN - INSERTING OVER AN OLD ENTRY DROPS
        //IT, WHICH IS WHAT RETIRES A RE-ATTACHMENT'S PREVIOUS TASK
        let arrivals = share.pending.lock().map(|mut pending| pending.drain(..).collect::<Vec<_>>()).unwrap_or_default();

        for (client_id, viewer) in arrivals
        {
            viewers.insert(client_id, viewer);

            log::info!("Screen viewer serving ({} attached): share of {owner}", viewers.len());
        }

        //COLLECT WHO IS STILL ATTACHED TO US, BY THE ATTACHMENT THEIR TASK WAS BUILT FOR
        let entries: Vec<(usize, [u8; 32])> = server::CONNECTIONS.iter().filter_map(|entry|
        {
            match entry.value()
            {
                Connection::Authenticated { id: client_id, attached_screen, .. } =>
                {
                    //FILTER ATTACHED CLIENTS
                    if let Some(attached_screen) = attached_screen && attached_screen.target_id == id
                    {
                        Some((*client_id, attached_screen.token))
                    } else { None }
                },
                _ => None,
            }
        }).collect();

        //RETIRE WHOEVER LEFT - DROPPING A `Viewer` ABORTS ITS TASK AND CLOSES ITS SOCKET
        viewers.retain(|client_id, viewer| entries.iter().any(|(e, token)| e == client_id && *token == viewer.token));

        //FORWARD PACKET
        let keyframe = matches!(&read, ScreenPacketCode::Video { data } if is_keyframe(data));

        for (client_id, viewer) in viewers.iter_mut()
        {
            //PREVENT FEEDBACK
            if *client_id == id && matches!(read, ScreenPacketCode::Audio { .. }) { continue; }

            //A VIEWER THAT MISSED A FRAME CANNOT DECODE A PREDICTED ONE - IT HOLDS ITS LAST PICTURE
            //UNTIL AN IDR COMES ROUND (AT MOST `FORCED_INTRA_INTERVAL`) RATHER THAN BE HANDED RUBBISH
            if viewer.needs_key && matches!(read, ScreenPacketCode::Video { .. })
            {
                if !keyframe { continue; }

                log::debug!("Screen viewer recovered on a keyframe: share of {owner}");

                viewer.needs_key = false;
            }

            //A FULL QUEUE MEANS *THIS* VIEWER'S LINK CANNOT CARRY THE SHARE. SHEDDING THE FRAME IS
            //THE WHOLE POINT: THE SHARE RUNS AT THE SHARER'S RATE AND A SLOW VIEWER PAYS ALONE,
            //WHERE FORWARDING INLINE MADE EVERYBODY WAIT FOR THE WORST LINK ON THE SERVER
            if viewer.tx.try_send(read.clone()).is_err()
            {
                //A LINE PER SHED FRAME WOULD BE ONE PER FRAME ON A LINK THAT CANNOT CARRY THE SHARE AT ALL,
                //SO ONLY THE FIRST OF A RUN IS WORTH SAYING: THE REST ARE THE SAME VIEWER STILL BEHIND
                if matches!(read, ScreenPacketCode::Video { .. })
                {
                    if !viewer.needs_key { log::warn!("Screen viewer shed (link too slow): share of {owner}"); }

                    viewer.needs_key = true;
                }
            }
        }

        //KEEP THE LAST PICTURE THAT STANDS ON ITS OWN. IT IS WHAT THE NEXT ATTACH IS HANDED, AND IT
        //IS KEPT *AFTER* THE FORWARD SO A VIEWER IS NEVER HANDED THE FRAME IT IS ABOUT TO BE SENT
        if keyframe && let ScreenPacketCode::Video { data } = &read
        {
            if let Ok(mut cached) = share.keyframe.lock() { *cached = Some(data.clone()); }
        }
    }

    //THE UPLOAD SOCKET DIED - NOBODY ELSE KNOWS THE SHARE IS OVER
    end_share(id).await;
}
