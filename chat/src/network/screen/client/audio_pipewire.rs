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

//! # PipeWire Share Audio Capture
//!
//! Linux-only capture for a screen share. The default output's monitor contains WHY2's own
//! playback, so a viewer in the same voice channel hears themselves back; instead of cancelling
//! that afterwards, this backend never captures it.
//!
//! A capture [`StreamRc`] is linked directly to the playback streams of every other application.
//! The registry supplies the `Stream/Output/Audio` nodes and their `Port` globals; `link-factory`
//! objects connect each app's output ports to our input ports, and PipeWire mixes however many
//! links land on one input. WHY2's own nodes are skipped.
//!
//! # Selection
//! A node is ours when its PipeWire client belongs to this process, matched by the client's
//! `application.process.id`, cpal's `cpal-pulseaudio-{pid}` names, or the `PipeWire ALSA [{exe}]`
//! client the voice path opens through cpal's ALSA host. Node-level process ids may arrive after
//! the registry global, so the node's `client.id` is the reliable test. That covers the voice
//! output, voice notes, effects and a watched share's playback. Nodes are otherwise eligible if
//! their `media.class` is `Stream/Output/Audio`.
//!
//! # Ports
//! One-port nodes are mono and go to both inputs; wider nodes contribute their `FL`/`FR` ports
//! only (other channels are not downmixed), with port order as the fallback when the channel
//! names are absent. Links are reconciled from the registry caches on every node/port change, so
//! streams starting or stopping mid-share are picked up. The input side is rebuilt when the
//! stream's own ports are renegotiated.
//!
//! # Behaviour
//! Audio is taken at each application's own volume and mute, before the sink's volume and any
//! sink-side effect chain, and applications on any sink are included. Nothing playing means no
//! frames. A PipeWire or stream failure stops the share's audio while the picture continues; a
//! daemon restart needs a new share.
//!
//! # Lifecycle
//! Setup runs on a dedicated thread, like [`portal`](super::portal). Readiness is reported once
//! the capture stream's two input ports are visible in the registry; a setup failure or timeout
//! drops the signal and the caller falls back to the cpal monitor path. Dropping [`Capture`]
//! stops the loop.

use std::
{
    cell::RefCell,
    collections::{ HashMap, HashSet },
    io::Cursor,
    rc::Rc,
    thread,
};

use tokio::
{
    sync::{ mpsc::Sender, oneshot },
    time,
};

use pipewire::
{
    self,
    channel,
    context::ContextRc,
    core::CoreRc,
    keys,
    link::Link,
    main_loop::MainLoopRc,
    properties::PropertiesBox,
    registry::GlobalObject,
    spa::
    {
        self,
        buffer::ChunkFlags,
        param::
        {
            ParamType,
            audio::{ AudioFormat, AudioInfoRaw },
            format::{ MediaSubtype, MediaType },
            format_utils,
        },
        pod::{ self, Pod, serialize::PodSerializer },
        utils::{ dict::DictRef, Direction, SpaTypes },
    },
    stream::{ StreamFlags, StreamRc, StreamState },
    types::ObjectType,
};

use crate::network::
{
    screen::consts as screen_consts,
    voice::consts as voice_consts,
};

//STRUCTS
/// Stops the capture thread when dropped.
pub struct Capture
{
    quit: channel::Sender<()>,
}

impl Drop for Capture
{
    fn drop(&mut self)
    {
        self.quit.send(()).ok();
    }
}

/// One input or output port, reduced to what pairing needs.
struct PortRef //A PORT AND ITS CHANNEL
{
    global: u32,
    port_id: u32,
    channel: Option<String>,
}

/// A port from the registry.
struct PortInfo //A PORT AS THE REGISTRY DESCRIBED IT
{
    node: u32,
    input: bool,
    port_id: u32,
    channel: Option<String>,
}

/// A node from the registry.
struct NodeInfo //A NODE AS THE REGISTRY DESCRIBED IT
{
    client: Option<u32>,
    playback: bool,
    ours: bool,
}

/// The registry mirror and the links made from it.
struct Linker
{
    core: CoreRc,
    our_name: String,
    pid: String,
    exe_name: String,
    cpal_name: String,
    alsa_name: String,
    our_node: Option<u32>,
    clients: HashSet<u32>,
    nodes: HashMap<u32, NodeInfo>,
    ports: HashMap<u32, PortInfo>,
    links: HashMap<(u32, u32), Link>,
    ready: Option<oneshot::Sender<()>>,
}

//FUNCTIONS
/// Starts the capture, returning once the stream's ports are visible.
///
/// # Parameters
/// - `chunks`: Where interleaved 48 kHz stereo samples go.
///
/// # Returns
/// `Err` if PipeWire is unavailable or the stream never exposed its ports.
pub async fn start(chunks: Sender<Vec<f32>>) -> Result<Capture, String> //START PIPE CAPTURE
{
    let (quit, quit_rx) = channel::channel();
    let (ready, ready_rx) = oneshot::channel();

    thread::Builder::new()
        .name("why2-share-audio".to_owned())
        .spawn(move || { run(chunks, quit_rx, ready).ok(); })
        .map_err(|error| error.to_string())?;

    //DROP STOPS THE THREAD
    let capture = Capture { quit };

    match time::timeout(screen_consts::PIPEWIRE_SETUP_TIMEOUT, ready_rx).await
    {
        Ok(Ok(())) => Ok(capture),

        //THE THREAD STOPPED BEFORE ITS PORTS APPEARED
        Ok(Err(_)) => Err("the PipeWire capture stopped while starting".to_owned()),

        //STOP IT IN CASE IT COMES UP LATER
        Err(_) => Err("the PipeWire capture did not start".to_owned()),
    }
}

//THE CAPTURE THREAD
/// Runs the PipeWire loop until [`Capture`] is dropped.
fn run(chunks: Sender<Vec<f32>>, quit: channel::Receiver<()>, ready: oneshot::Sender<()>) -> Result<(), String>
{
    pipewire::init();

    let main_loop = MainLoopRc::new(None).map_err(|error| error.to_string())?;
    let context = ContextRc::new(&main_loop, None).map_err(|error| error.to_string())?;
    let core = context.connect_rc(None).map_err(|error| error.to_string())?;
    let registry = core.get_registry_rc().map_err(|error| error.to_string())?;

    let pid = std::process::id();
    let our_name = format!("why2-share-audio-{pid}");
    let exe_name = std::env::current_exe().ok()
        .and_then(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "why2".to_owned());

    let mut properties = PropertiesBox::new();
    properties.insert(*keys::MEDIA_TYPE, "Audio");
    properties.insert(*keys::MEDIA_CATEGORY, "Capture");
    properties.insert(*keys::MEDIA_ROLE, "Screen");
    properties.insert(*keys::NODE_NAME, our_name.clone());
    properties.insert(*keys::NODE_LATENCY, screen_consts::PIPEWIRE_NODE_LATENCY);

    let stream = StreamRc::new(core.clone(), "why2-share-audio", properties).map_err(|error| error.to_string())?;

    let linker = Rc::new(RefCell::new(Linker
    {
        core: core.clone(),
        our_name,
        pid: pid.to_string(),
        exe_name: exe_name.clone(),
        cpal_name: format!("cpal-pulseaudio-{pid}"),
        alsa_name: format!("PipeWire ALSA [{exe_name}]"),
        our_node: None,
        clients: HashSet::new(),
        nodes: HashMap::new(),
        ports: HashMap::new(),
        links: HashMap::new(),
        ready: Some(ready),
    }));

    let global_linker = linker.clone();
    let remove_linker = linker.clone();

    let _registry_listener = registry.add_listener_local()
        .global(move |global| handle_global(&global_linker, global))
        .global_remove(move |id| handle_remove(&remove_linker, id))
        .register();

    let stop = main_loop.clone();

    let _stream_listener = stream
        .add_local_listener_with_user_data(AudioInfoRaw::new())
        .state_changed(move |_, _, _, state|
        {
            if matches!(state, StreamState::Error(_)) { stop.quit(); }
        })
        .param_changed(|_, format, id, param|
        {
            let Some(param) = param else { return };
            if id != ParamType::Format.as_raw() { return; }

            if let Ok((MediaType::Audio, MediaSubtype::Raw)) = format_utils::parse_format(param)
            {
                let _ = format.parse(param);
            }
        })
        .process(move |stream, format|
        {
            let Some(mut buffer) = stream.dequeue_buffer() else { return };
            let Some(data) = buffer.datas_mut().first_mut() else { return };

            //COPY THE BUFFER SLICE FIRST
            let (offset, size, corrupted) =
            {
                let chunk = data.chunk();
                (chunk.offset() as usize, chunk.size() as usize, chunk.flags().contains(ChunkFlags::CORRUPTED))
            };

            if size == 0 || corrupted { return; }
            if format.format() != AudioFormat::F32LE || format.rate() != voice_consts::SAMPLE_RATE || format.channels() != 2 { return; }

            let Some(bytes) = data.data() else { return };
            let Some(bytes) = bytes.get(offset..offset + size) else { return };

            let mut samples = Vec::with_capacity(size / 4);
            for sample in bytes.chunks_exact(4)
            {
                samples.push(f32::from_le_bytes([ sample[0], sample[1], sample[2], sample[3] ]));
            }

            chunks.try_send(samples).ok();
        })
        .register()
        .map_err(|error| error.to_string())?;

    let values = format_params()?;
    let mut params = [Pod::from_bytes(&values).ok_or_else(|| "building the audio format failed".to_owned())?];

    stream.connect(Direction::Input, None, StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS, &mut params)
        .map_err(|error| error.to_string())?;

    let _quit = quit.attach(main_loop.loop_(),
    {
        let main_loop = main_loop.clone();
        move |_| main_loop.quit()
    });

    main_loop.run();

    Ok(())
}

//BUILD THE FIXED CAPTURE FORMAT
/// Serializes the F32LE, 48 kHz, stereo `EnumFormat` the stream asks for.
fn format_params() -> Result<Vec<u8>, String>
{
    let mut audio_info = AudioInfoRaw::new();
    audio_info.set_format(AudioFormat::F32LE);
    audio_info.set_rate(voice_consts::SAMPLE_RATE);
    audio_info.set_channels(2);

    let mut position = [0u32; spa::sys::SPA_AUDIO_MAX_CHANNELS as usize];
    position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
    position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
    audio_info.set_position(position);

    let object = pod::Object
    {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };

    PodSerializer::serialize(Cursor::new(Vec::new()), &pod::Value::Object(object))
        .map(|(cursor, _)| cursor.into_inner())
        .map_err(|error| format!("{error:?}"))
}

//REGISTRY EVENTS
/// Mirrors nodes and ports, then reconciles the links.
fn handle_global(linker: &Rc<RefCell<Linker>>, global: &GlobalObject<&DictRef>)
{
    let Some(properties) = global.props else { return };

    match &global.type_
    {
        ObjectType::Node =>
        {
            let name = properties.get(*keys::NODE_NAME);
            let class = properties.get(*keys::MEDIA_CLASS);
            let pid = properties.get("application.process.id");
            let app_name = properties.get("application.name");
            let client = properties.get("client.id").and_then(|value| value.parse::<u32>().ok());

            {
                let mut state = linker.borrow_mut();

                //MATCH THE NAMES TOO; THE PID ARRIVES LATE
                let ours = pid == Some(state.pid.as_str())
                    || name == Some(state.cpal_name.as_str())
                    || app_name == Some(state.cpal_name.as_str())
                    || app_name == Some(state.alsa_name.as_str());

                state.nodes.insert(global.id, NodeInfo
                {
                    client,
                    playback: class == Some("Stream/Output/Audio"),
                    ours,
                });

                if name == Some(state.our_name.as_str()) { state.our_node = Some(global.id); }
            }

            reconcile(linker);
        },

        ObjectType::Client =>
        {
            let pid = properties.get("application.process.id");
            let app_name = properties.get("application.name");
            let binary = properties.get("application.process.binary");

            {
                let mut state = linker.borrow_mut();

                let ours = pid == Some(state.pid.as_str())
                    || app_name == Some(state.cpal_name.as_str())
                    || app_name == Some(state.alsa_name.as_str())
                    || binary == Some(state.exe_name.as_str());

                if ours { state.clients.insert(global.id); } else { state.clients.remove(&global.id); }
            }

            reconcile(linker);
        },

        ObjectType::Port =>
        {
            let node = properties.get(*keys::NODE_ID).and_then(|value| value.parse::<u32>().ok());
            let direction = properties.get(*keys::PORT_DIRECTION);
            let port_id = properties.get(*keys::PORT_ID).and_then(|value| value.parse::<u32>().ok()).unwrap_or(0);
            let channel = properties.get(*keys::AUDIO_CHANNEL).map(str::to_owned);

            if let Some(node) = node
            {
                linker.borrow_mut().ports.insert(global.id, PortInfo
                {
                    node,
                    input: direction == Some("in"),
                    port_id,
                    channel,
                });
            }

            reconcile(linker);
        },

        _ => {},
    }
}

/// Drops a removed global from the caches and the links that touched it.
fn handle_remove(linker: &Rc<RefCell<Linker>>, id: u32)
{
    {
        let mut state = linker.borrow_mut();

        state.clients.remove(&id);
        state.nodes.remove(&id);
        state.ports.remove(&id);

        if state.our_node == Some(id) { state.our_node = None; }

        //IDS ARE REUSED; DROP DEAD LINKS
        state.links.retain(|(output, input), _| *output != id && *input != id);
    }

    reconcile(linker);
}

//MAKE THE LINKS MATCH THE GRAPH
/// Brings the link set in line with the applications currently playing.
fn reconcile(linker: &Rc<RefCell<Linker>>)
{
    let mut state = linker.borrow_mut();
    let Some(our_node) = state.our_node else { return };

    //OUR INPUT PORTS, FL THEN FR
    let mut inputs: Vec<PortRef> = state.ports.iter()
        .filter(|(_, port)| port.node == our_node && port.input)
        .map(|(global, port)| PortRef { global: *global, port_id: port.port_id, channel: port.channel.clone() })
        .collect();

    inputs.sort_by_key(|port| port.port_id);
    inputs.truncate(2);

    let input_fl = inputs.iter().find(|port| port.channel.as_deref() == Some("FL"))
        .or_else(|| inputs.first())
        .map(|port| port.global);

    let input_fr = inputs.iter().find(|port| port.channel.as_deref() == Some("FR"))
        .or_else(|| inputs.get(1))
        .map(|port| port.global);

    let (Some(input_fl), Some(input_fr)) = (input_fl, input_fr) else { return };

    //WHAT THE GRAPH SHOULD LOOK LIKE
    let mut wanted: HashSet<(u32, u32)> = HashSet::new();

    for (&node, info) in &state.nodes
    {
        //THE VOICE PATH IS AN ALSA CLIENT, SO THE CLIENT ID IS THE RELIABLE TEST
        let ours = info.ours || info.client.is_some_and(|client| state.clients.contains(&client));
        if !info.playback || ours { continue; }

        let mut outputs: Vec<PortRef> = state.ports.iter()
            .filter(|(_, port)| port.node == node && !port.input)
            .map(|(global, port)| PortRef { global: *global, port_id: port.port_id, channel: port.channel.clone() })
            .collect();

        outputs.sort_by_key(|port| port.port_id);

        match outputs.len()
        {
            //MONO PLAYS ON BOTH SIDES
            0 => {},
            1 =>
            {
                wanted.insert((outputs[0].global, input_fl));
                wanted.insert((outputs[0].global, input_fr));
            },

            //WIDE STREAMS CONTRIBUTE ONLY FL AND FR
            _ =>
            {
                let left = outputs.iter().find(|port| port.channel.as_deref() == Some("FL"));
                let right = outputs.iter().find(|port| port.channel.as_deref() == Some("FR"));

                match (left, right)
                {
                    (Some(left), Some(right)) =>
                    {
                        wanted.insert((left.global, input_fl));
                        wanted.insert((right.global, input_fr));
                    },

                    (Some(left), None) => { wanted.insert((left.global, input_fl)); },
                    (None, Some(right)) => { wanted.insert((right.global, input_fr)); },

                    //NO CHANNEL NAMES; TRUST PORT ORDER
                    (None, None) =>
                    {
                        wanted.insert((outputs[0].global, input_fl));
                        wanted.insert((outputs[1].global, input_fr));
                    },
                }
            },
        }
    }

    //DROP LINKS THAT NO LONGER BELONG
    let stale: Vec<(u32, u32)> = state.links.keys().filter(|key| !wanted.contains(*key)).copied().collect();

    for key in stale
    {
        state.links.remove(&key);
    }

    //ADD THE MISSING ONES
    for key @ (output_port, input_port) in wanted
    {
        if state.links.contains_key(&key) { continue; }

        let Some(output_node) = state.ports.get(&output_port).map(|port| port.node) else { continue };

        let mut properties = PropertiesBox::new();
        properties.insert("link.output.node", output_node.to_string());
        properties.insert("link.output.port", output_port.to_string());
        properties.insert("link.input.node", our_node.to_string());
        properties.insert("link.input.port", input_port.to_string());

        let Ok(link) = state.core.create_object::<Link>("link-factory", &properties) else { continue };

        state.links.insert(key, link);
    }

    //BOTH INPUTS ARE UP
    if inputs.len() == 2
    {
        if let Some(ready) = state.ready.take() { ready.send(()).ok(); }
    }
}
