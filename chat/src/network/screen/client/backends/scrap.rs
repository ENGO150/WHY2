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
    thread,
    time::Duration,
    sync::{ Arc, Mutex },
};

use scrap::{ Capturer, Display };

use crate::network::screen::client::
{
    CaptureInfo,
    SharedFrame,
    compress,
};

pub fn start(monitor_idx: usize) -> (CaptureInfo, SharedFrame)
{
    //GET DISPLAY DIMS
    let (width, height) =
    {
        let d = Display::all().expect("Failed loading display").remove(monitor_idx);
        (d.width(), d.height())
    };
    let stride = width * 4;
    let buf_size = stride * height;

    //SHARED BUFFER
    let shared: SharedFrame = Arc::new(Mutex::new((Vec::new(), false)));
    let shared_cap = shared.clone();

    //CAPTURE THREAD
    thread::spawn(move ||
    {
        //INIT CAPTURE
        let mut cap = Capturer::new(Display::all().unwrap()
            .remove(monitor_idx)).expect("Failed to init capture");

        //LOOP CAPTURING
        loop
        {
            match cap.frame()
            {
                //DATA
                Ok(frame) =>
                {
                    //LOAD RAW DATA
                    let raw = &frame[..buf_size.min(frame.len())];

                    //COMPRESS
                    let local_compressed = compress(raw);

                    //RENDER
                    let mut g = shared_cap.lock().unwrap();
                    g.0 = local_compressed;
                    g.1 = true;
                },

                //SLEEP ON ERROR
                Err(_) => thread::sleep(Duration::from_millis(1)),
            }
        }
    });

    (CaptureInfo { width, height, stride }, shared)
}
