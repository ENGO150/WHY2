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
    collections::HashMap,
    io::Cursor,
    sync::{ Arc, mpsc::{ self, RecvTimeoutError } },
    thread::{ self, JoinHandle },
};

use pipewire::
{
    channel,
    context::ContextRc,
    main_loop::MainLoopRc,
    properties,
    keys::{ MEDIA_CATEGORY, MEDIA_ROLE, MEDIA_TYPE },
    stream::{ StreamFlags, StreamRc, StreamState },
    spa::
    {
        buffer::ChunkFlags,
        param::
        {
            ParamType,
            format::{ FormatProperties, MediaSubtype, MediaType },
            format_utils,
            video::{ VideoFormat, VideoInfoRaw },
        },
        pod::{ self, Pod, serialize::PodSerializer },
        utils::{ Direction, Fraction, Rectangle, SpaTypes },
    },
};

use zbus::
{
    blocking::{ Connection, Proxy },
    zvariant::{ OwnedFd, OwnedObjectPath, OwnedValue, Value },
};

use crate::network::screen::
{
    consts,
    client::capture::{ CapturedFrame, LatestFrame, PixelOrder, pack_rows },
};

//CONSTANTS
const DESTINATION: &str = "org.freedesktop.portal.Desktop";
const DESKTOP: &str = "/org/freedesktop/portal/desktop";
const SCREENCAST: &str = "org.freedesktop.portal.ScreenCast";

const SOURCE_MONITOR: u32 = 1;
const CURSOR_EMBEDDED: u32 = 2;
const MAX_DIMENSION: u32 = 16384;

//STRUCTS
pub struct PortalRecorder //A SCREEN CAST SESSION AND THE THREAD READING IT
{
    connection: Connection,
    session: OwnedObjectPath,
    quit: channel::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for PortalRecorder
{
    fn drop(&mut self)
    {
        self.stop();
    }
}

fn token() -> String
{
    format!("why2_{}", rand::random::<u32>())
}

fn sender(connection: &Connection) -> Result<String, String> //THE CALLER'S NAME AS PORTAL PATHS SPELL IT
{
    connection.unique_name()
        .map(|name| name.trim_start_matches(':').replace('.', "_"))
        .ok_or_else(|| "no D-Bus unique name".to_owned())
}

//CALL A PORTAL METHOD AND WAIT FOR ITS RESPONSE
fn request<B>(connection: &Connection, portal: &Proxy, method: &str, token: &str, body: &B) -> Result<HashMap<String, OwnedValue>, String>
where
    B: zbus::export::serde::Serialize + zbus::zvariant::DynamicType,
{
    let path = format!("{DESKTOP}/request/{}/{token}", sender(connection)?);

    let request = Proxy::new(connection, DESTINATION, path, "org.freedesktop.portal.Request")
        .map_err(|error| error.to_string())?;

    //SUBSCRIBE BEFORE THE CALL
    let mut responses = request.receive_signal("Response").map_err(|error| error.to_string())?;

    portal.call_method(method, body).map_err(|error| error.to_string())?;

    let message = responses.next().ok_or_else(|| format!("{method} got no response"))?;

    let (code, results): (u32, HashMap<String, OwnedValue>) = message.body().deserialize()
        .map_err(|error| error.to_string())?;

    match code
    {
        0 => Ok(results),
        1 => Err(format!("{method} was cancelled")),
        _ => Err(format!("{method} failed ({code})")),
    }
}

//THE PIPEWIRE NODE OF THE FIRST STREAM
fn stream_node(results: &HashMap<String, OwnedValue>) -> Result<u32, String>
{
    let streams = results.get("streams").ok_or_else(|| "the portal named no stream".to_owned())?;

    let streams: Vec<(u32, HashMap<String, OwnedValue>)> = streams.try_clone()
        .map_err(|error| error.to_string())?
        .try_into()
        .map_err(|error: zbus::zvariant::Error| error.to_string())?;

    streams.first().map(|(node, _)| *node).ok_or_else(|| "the portal named no stream".to_owned())
}

fn format_params(fps: u32) -> Result<Vec<u8>, String> //WHAT WE ACCEPT, CAPPED AT OUR FRAME RATE
{
    let object = pod::object!(
        SpaTypes::ObjectParamFormat,
        ParamType::EnumFormat,
        pod::property!(FormatProperties::MediaType, Id, MediaType::Video),
        pod::property!(FormatProperties::MediaSubtype, Id, MediaSubtype::Raw),
        pod::property!(
            FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            VideoFormat::BGRx,
            VideoFormat::BGRx,
            VideoFormat::BGRA,
            VideoFormat::RGBx,
            VideoFormat::RGBA
        ),
        pod::property!(
            FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            Rectangle { width: 1920, height: 1080 },
            Rectangle { width: 1, height: 1 },
            Rectangle { width: MAX_DIMENSION, height: MAX_DIMENSION }
        ),
        pod::property!(
            FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            Fraction { num: 0, denom: 1 },
            Fraction { num: 0, denom: 1 },
            Fraction { num: fps, denom: 1 }
        ),
        //THE COMPOSITOR CAPTURES AT THIS RATE
        pod::property!(
            FormatProperties::VideoMaxFramerate,
            Choice,
            Range,
            Fraction,
            Fraction { num: fps, denom: 1 },
            Fraction { num: 1, denom: 1 },
            Fraction { num: fps, denom: 1 }
        ),
    );

    PodSerializer::serialize(Cursor::new(Vec::new()), &pod::Value::Object(object))
        .map(|(cursor, _)| cursor.into_inner())
        .map_err(|error| format!("{error:?}"))
}

fn pixel_order(format: VideoFormat) -> Option<PixelOrder>
{
    match format
    {
        VideoFormat::BGRx | VideoFormat::BGRA => Some(PixelOrder::Bgra),
        VideoFormat::RGBx | VideoFormat::RGBA => Some(PixelOrder::Rgba),
        _ => None,
    }
}

//READ THE STREAM UNTIL TOLD TO QUIT
fn run_stream
(
    fd: std::os::fd::OwnedFd,
    node: u32,
    fps: u32,
    latest: Arc<LatestFrame>,
    quit: channel::Receiver<()>,
    ready: mpsc::Sender<Result<(), String>>,
) -> Result<(), String>
{
    pipewire::init();

    let main_loop = MainLoopRc::new(None).map_err(|error| error.to_string())?;
    let context = ContextRc::new(&main_loop, None).map_err(|error| error.to_string())?;
    let core = context.connect_fd_rc(fd, None).map_err(|error| error.to_string())?;

    let stream = StreamRc::new(core, "WHY2", properties::properties!
    {
        *MEDIA_TYPE => "Video",
        *MEDIA_CATEGORY => "Capture",
        *MEDIA_ROLE => "Screen",
    }).map_err(|error| error.to_string())?;

    let frames = latest.clone();
    let failed = latest.clone();

    let _listener = stream
        .add_local_listener_with_user_data(VideoInfoRaw::default())
        .state_changed(move |_, _, _, state| match state
        {
            //NEGOTIATED, FRAMES MAY ONLY COME ON DAMAGE
            StreamState::Streaming => { ready.send(Ok(())).ok(); },

            //A DEAD STREAM ENDS THE SHARE
            StreamState::Error(reason) =>
            {
                ready.send(Err(reason)).ok();
                failed.end();
            },

            StreamState::Unconnected => failed.end(),
            _ => {},
        })
        .param_changed(|_, format, id, param|
        {
            let Some(param) = param else { return };

            if id != ParamType::Format.as_raw() { return; }

            if let Ok((MediaType::Video, MediaSubtype::Raw)) = format_utils::parse_format(param)
            {
                format.parse(param).ok();
            }
        })
        .process(move |stream, format|
        {
            let Some(mut buffer) = stream.dequeue_buffer() else { return };

            let Some(order) = pixel_order(format.format()) else { return };

            let (width, height) = (format.size().width as usize, format.size().height as usize);

            let Some(data) = buffer.datas_mut().first_mut() else { return };

            let chunk = data.chunk();

            //SKIP EMPTY AND CORRUPTED BUFFERS
            if chunk.size() == 0 || chunk.flags().contains(ChunkFlags::CORRUPTED) { return; }

            let stride = if chunk.stride() > 0 { chunk.stride() as usize } else { width * 4 };
            let offset = chunk.offset() as usize;

            let Some(source) = data.data().and_then(|bytes| bytes.get(offset..)) else { return };

            let mut pixels = frames.buffer();

            //THE PICTURE MUST FIT IN THE BUFFER
            if !pack_rows(source, width, height, stride, &mut pixels)
            {
                frames.recycle(pixels);
                return;
            }

            frames.put(CapturedFrame { width: width as u32, height: height as u32, order, data: pixels });
        })
        .register()
        .map_err(|error| error.to_string())?;

    let values = format_params(fps)?;
    let mut params = [Pod::from_bytes(&values).ok_or_else(|| "building the stream format failed".to_owned())?];

    stream.connect(Direction::Input, Some(node), StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS, &mut params)
        .map_err(|error| error.to_string())?;

    //QUIT ON REQUEST
    let _quit = quit.attach(main_loop.loop_(),
    {
        let main_loop = main_loop.clone();
        move |_| main_loop.quit()
    });

    main_loop.run();

    stream.disconnect().ok();

    Ok(())
}

impl PortalRecorder
{
    pub fn stop(&mut self) //END THE STREAM AND THE SESSION
    {
        let Some(thread) = self.thread.take() else { return };

        self.quit.send(()).ok();
        thread.join().ok();

        //END THE SESSION ON THE PORTAL'S SIDE
        if let Ok(session) = Proxy::new(&self.connection, DESTINATION, self.session.as_str(), "org.freedesktop.portal.Session")
        {
            session.call_method("Close", &()).ok();
        }
    }

    pub fn start(latest: Arc<LatestFrame>, fps: u32) -> Result<Self, String> //ASK THE PORTAL FOR A MONITOR, BLOCKING ON THE PICKER
    {
        let connection = Connection::session().map_err(|error| error.to_string())?;

        let portal = Proxy::new(&connection, DESTINATION, DESKTOP, SCREENCAST)
            .map_err(|error| error.to_string())?;

        //CREATE A SESSION
        let session_token = token();
        let create_token = token();

        request(&connection, &portal, "CreateSession", &create_token, &(HashMap::from(
        [
            ("handle_token", Value::from(create_token.as_str())),
            ("session_handle_token", Value::from(session_token.as_str())),
        ]),))?;

        //THE PATH FOLLOWS FROM THE TOKEN
        let session = OwnedObjectPath::try_from(format!("{DESKTOP}/session/{}/{session_token}", sender(&connection)?))
            .map_err(|error| error.to_string())?;

        //ONE MONITOR
        let select_token = token();

        let mut sources = HashMap::from(
        [
            ("handle_token", Value::from(select_token.as_str())),
            ("types", Value::from(SOURCE_MONITOR)),
            ("multiple", Value::from(false)),
        ]);

        //DRAW THE CURSOR INTO THE PICTURE
        if portal.get_property::<u32>("AvailableCursorModes").is_ok_and(|modes| modes & CURSOR_EMBEDDED != 0)
        {
            sources.insert("cursor_mode", Value::from(CURSOR_EMBEDDED));
        }

        request(&connection, &portal, "SelectSources", &select_token, &(&session, sources))?;

        //THE PICKER
        let start_token = token();

        let started = request(&connection, &portal, "Start", &start_token, &(&session, "", HashMap::from(
        [
            ("handle_token", Value::from(start_token.as_str())),
        ])))?;

        let node = stream_node(&started)?;

        let fd: OwnedFd = portal.call("OpenPipeWireRemote", &(&session, HashMap::<&str, Value>::new()))
            .map_err(|error| error.to_string())?;

        let fd: std::os::fd::OwnedFd = fd.into();

        let (quit_tx, quit_rx) = channel::channel();
        let (ready_tx, ready_rx) = mpsc::channel();

        let thread = thread::spawn(move ||
        {
            let failed = latest.clone();
            let ready = ready_tx.clone();

            if let Err(reason) = run_stream(fd, node, fps, latest, quit_rx, ready_tx) { ready.send(Err(reason)).ok(); }

            failed.end();
        });

        let recorder = Self { connection, session, quit: quit_tx, thread: Some(thread) };

        match ready_rx.recv_timeout(consts::RECORDER_FIRST_FRAME)
        {
            Ok(Ok(())) => Ok(recorder),
            Ok(Err(reason)) => Err(reason),
            Err(RecvTimeoutError::Timeout) => Err("the stream never started".to_owned()),
            Err(RecvTimeoutError::Disconnected) => Err("the PipeWire thread died".to_owned()),
        }
    }
}
