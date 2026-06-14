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

//MODULES
pub mod audio;
pub mod capture;
pub mod compress;
pub mod frame;

use std::
{
    thread,
    sync::
    {
        Arc,
        atomic::AtomicBool,
    },
};


pub fn foo()
{
    //FIND PRIMARY MONITOR
    let monitor = capture::get_primary_monitor();
    let width = monitor.width().expect("Failed to get monitor width");
    let height = monitor.height().expect("Failed to get monitor height");

    //SHARED STATE
    let (tx, rx) = crossbeam_channel::bounded(2);
    let (audio_tx, audio_rx) = crossbeam_channel::bounded(2);

    let running = Arc::new(AtomicBool::new(true));

    //SPAWN CAPTURE THREAD
    let running_capture = running.clone();
    let _capture_thread = thread::spawn(move || capture::capture_loop(monitor, tx, running_capture, 30));
    let _audio_capture_thread = audio::spawn_audio_capture(audio_tx, running.clone());

    //TODO: Send
    loop
    {
        //HANDLE VIDEO FRAMES
        if let Ok(compressed_frame) = rx.try_recv()
        {
            //let bytes_to_send = compressed_frame.compressed_data.len();
        }

        //HANDLE AUDIO FRAMES
        if let Ok(audio_chunk) = audio_rx.try_recv()
        {
            //let chunk_size = audio_chunk.data.len();
        }
    }
}
