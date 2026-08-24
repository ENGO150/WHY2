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

use crossterm::event::{ KeyCode, KeyEvent };

use why2_chat::config;

#[cfg(feature = "client_voice")]
use why2_chat::network::voice::client::options as voice_options;

use super::state::App;

//CONSTS
pub const MAX_PICKER_ROWS: usize = 8; //VISIBLE DEVICE ROWS BEFORE THE PICKER SCROLLS

#[cfg(feature = "client_voice")]
pub const DEFAULT_DEVICE: &str = "System default"; //SHOWN FOR AN EMPTY input_device/output_device

#[cfg(feature = "client_voice")]
const VOLUME_STEP: u32 = 5;

//ENUMS
pub enum Value
{
    //THE CONFIG KEY IS THE TRUTH, invert FLIPS IT FOR KEYS PHRASED AS A NEGATIVE (disable_colors)
    Toggle { on: bool, invert: bool },

    #[cfg(feature = "client_voice")]
    Volume(u32), //PERCENT

    //THE cpal DEVICE ID, WHICH IS WHAT client.toml HOLDS - THE LABEL IS LOOKED UP FOR DISPLAY ONLY
    #[cfg(feature = "client_voice")]
    Device { id: String, input: bool }, //EMPTY ID = SYSTEM DEFAULT
}

pub enum Row
{
    Header(&'static str),
    Item(Item),
}

//STRUCTS
pub struct Item
{
    pub label: &'static str,
    pub key: &'static str, //client.toml KEY THIS ROW OWNS
    pub value: Value,
}

//ONE DEVICE AS THE PICKER SHOWS IT. THE id IS WHAT client.toml HOLDS AND WHAT THE VOICE CLIENT OPENS -
//THE label IS DISPLAY ONLY, AND IS NOT UNIQUE (ALSA HANDS OUT THE SAME DESCRIPTION TO SEVERAL PCMs).
#[derive(Clone, Default)]
pub struct DeviceEntry
{
    pub id: String,
    pub label: String,
}

pub struct Picker //DEVICE LIST OPENED ON TOP OF THE SETTINGS ROWS
{
    pub title: &'static str,
    pub entries: Vec<DeviceEntry>, //ENTRY 0 IS ALWAYS THE SYSTEM DEFAULT
    pub selected: usize,
    pub row: usize, //THE SETTINGS ROW THAT OPENED IT
}

#[derive(Default)]
pub struct Devices //WHAT cpal REPORTED, ENUMERATED ONCE WHEN /settings IS TYPED
{
    #[cfg(feature = "client_voice")]
    pub input: Vec<DeviceEntry>,

    #[cfg(feature = "client_voice")]
    pub output: Vec<DeviceEntry>,
}

pub struct Settings //THE /settings OVERLAY
{
    pub open: bool,
    pub rows: Vec<Row>,
    pub selected: usize,
    pub picker: Option<Picker>,

    #[cfg(feature = "client_voice")]
    devices: Devices,
}

//IMPLEMENTATIONS
impl Default for Settings
{
    fn default() -> Self { Self::new() }
}

impl Settings
{
    pub fn new() -> Self
    {
        Self
        {
            open: false,
            rows: Vec::new(),
            selected: 0,
            picker: None,

            #[cfg(feature = "client_voice")]
            devices: Devices::default(),
        }
    }

    //OPEN THE OVERLAY, READING EVERY VALUE OUT OF THE CONFIG ONCE (THE DRAW PATH NEVER RE-READS IT)
    pub fn open(&mut self, devices: Devices)
    {
        let mut rows: Vec<Row> = Vec::new();

        #[cfg(feature = "client_voice")]
        {
            rows.push(Row::Header("Audio"));

            rows.push(Row::Item(Item
            {
                label: "Input device",
                key: "input_device",
                value: Value::Device { id: config::read_config::<String>("input_device"), input: true },
            }));

            rows.push(Row::Item(Item
            {
                label: "Output device",
                key: "output_device",
                value: Value::Device { id: config::read_config::<String>("output_device"), input: false },
            }));

            rows.push(Row::Item(Item
            {
                label: "Input volume",
                key: "input_volume",
                value: Value::Volume(voice_options::clamp_volume(config::read_config::<u32>("input_volume"))),
            }));

            rows.push(Row::Item(Item
            {
                label: "Output volume",
                key: "output_volume",
                value: Value::Volume(voice_options::clamp_volume(config::read_config::<u32>("output_volume"))),
            }));

            rows.push(Row::Item(Item
            {
                label: "Noise suppression",
                key: "noise_suppression",
                value: toggle_value("noise_suppression", false),
            }));

            rows.push(Row::Item(Item
            {
                label: "Automatic gain",
                key: "automatic_gain",
                value: toggle_value("automatic_gain", false),
            }));
        }

        rows.push(Row::Header("Interface"));

        rows.push(Row::Item(Item
        {
            label: "Message colors",
            key: "disable_colors",
            value: toggle_value("disable_colors", true),
        }));

        rows.push(Row::Item(Item
        {
            label: "Show client IDs",
            key: "show_id",
            value: toggle_value("show_id", false),
        }));

        self.rows = rows;
        self.picker = None;
        self.open = true;
        self.selected = 0;

        #[cfg(feature = "client_voice")]
        {
            self.devices = devices;
        }

        #[cfg(not(feature = "client_voice"))]
        let _ = devices;

        self.step(1); //LAND ON THE FIRST ITEM, NOT ON THE HEADER ABOVE IT
    }

    pub fn close(&mut self)
    {
        self.open = false;
        self.picker = None;
        self.rows = Vec::new();
    }

    //MOVE THE SELECTION BY delta ROWS, SKIPPING HEADERS AND STOPPING AT BOTH ENDS
    fn step(&mut self, delta: isize)
    {
        if self.rows.is_empty() { return; }

        let mut index = self.selected as isize;

        loop
        {
            index += delta;

            //RAN OUT OF ROWS - KEEP WHATEVER WAS SELECTED
            if index < 0 || index as usize >= self.rows.len() { return; }

            if matches!(self.rows[index as usize], Row::Item(_))
            {
                self.selected = index as usize;
                return;
            }
        }
    }

    //WHAT A STORED DEVICE ID IS CALLED - A DEVICE THAT IS NOT IN THE LIST ANY MORE FALLS BACK TO ITS RAW ID
    #[cfg(feature = "client_voice")]
    pub fn device_label(&self, id: &str, input: bool) -> String
    {
        if id.is_empty() { return String::from(DEFAULT_DEVICE); }

        let devices = if input { &self.devices.input } else { &self.devices.output };

        devices.iter().find(|device| device.id == id).map(|device| device.label.clone()).unwrap_or_else(|| String::from(id))
    }

    //RE-READ THE DEVICE ROWS OUT OF THE CONFIG - THE VOICE CLIENT PUTS THE OLD PAIR BACK WHEN A SWITCH FAILS
    #[cfg(feature = "client_voice")]
    pub fn refresh_devices(&mut self)
    {
        for row in self.rows.iter_mut()
        {
            let Row::Item(item) = row else { continue };

            if let Value::Device { input, .. } = item.value
            {
                item.value = Value::Device { id: config::read_config::<String>(item.key), input };
            }
        }
    }
}

//READ A BOOLEAN SETTING AS THE ROW SHOWS IT - invert IS FOR KEYS PHRASED AS A NEGATIVE (disable_colors)
fn toggle_value(key: &str, invert: bool) -> Value
{
    let stored = config::read_config::<bool>(key);

    Value::Toggle { on: if invert { !stored } else { stored }, invert }
}

//FUNCTIONS
//PUBLIC
//ONE KEYPRESS WHILE THE OVERLAY IS UP. EVERY CHANGE IS WRITTEN THROUGH IMMEDIATELY - THERE IS NO SAVE BUTTON.
pub fn handle_key(app: &mut App, key: KeyEvent)
{
    //THE DEVICE PICKER OWNS THE KEYBOARD WHILE IT IS OPEN
    if app.settings.picker.is_some()
    {
        handle_picker_key(app, key);
        return;
    }

    match key.code
    {
        KeyCode::Esc => app.settings.close(),

        KeyCode::Up => app.settings.step(-1),
        KeyCode::Down => app.settings.step(1),

        KeyCode::Left => adjust(app, -1),
        KeyCode::Right => adjust(app, 1),

        KeyCode::Enter | KeyCode::Char(' ') => activate(app),

        _ => {},
    }
}

//MOUSE WHEEL - MOVES WHICHEVER LIST IS IN FRONT
pub fn scroll(app: &mut App, delta: isize)
{
    match app.settings.picker.as_mut()
    {
        Some(picker) =>
        {
            let next = picker.selected as isize + delta;
            picker.selected = next.clamp(0, picker.entries.len() as isize - 1) as usize;
        },

        None => app.settings.step(delta),
    }
}

//PRIVATE
fn handle_picker_key(app: &mut App, key: KeyEvent)
{
    //CLOSING THE PICKER TAKES IT OUT OF App, SO THOSE TWO CASES COME BEFORE THE BORROW BELOW
    match key.code
    {
        KeyCode::Esc =>
        {
            app.settings.picker = None;
            return;
        },

        KeyCode::Enter =>
        {
            let Some(picker) = app.settings.picker.take() else { return };

            //ENTRY 0 IS THE SYSTEM DEFAULT, WHICH IS AN EMPTY CONFIG VALUE
            let chosen = picker.entries.get(picker.selected).map(|entry| entry.id.clone()).unwrap_or_default();

            set_device(app, picker.row, chosen);
            return;
        },

        _ => {},
    }

    let Some(picker) = app.settings.picker.as_mut() else { return };

    match key.code
    {
        KeyCode::Up => picker.selected = if picker.selected == 0 { picker.entries.len() - 1 } else { picker.selected - 1 },
        KeyCode::Down => picker.selected = (picker.selected + 1) % picker.entries.len(),

        KeyCode::Home => picker.selected = 0,
        KeyCode::End => picker.selected = picker.entries.len() - 1,

        _ => {},
    }
}

//WHAT THE SELECTED ROW HOLDS, COPIED OUT SO THE ACTIONS BELOW CAN TOUCH App AGAIN
enum Selected
{
    Toggle(bool),

    #[cfg(feature = "client_voice")]
    Volume(&'static str, u32),

    #[cfg(feature = "client_voice")]
    Device(String, bool),
}

fn selected(app: &App) -> Option<Selected>
{
    let Some(Row::Item(item)) = app.settings.rows.get(app.settings.selected) else { return None };

    Some(match &item.value
    {
        Value::Toggle { on, .. } => Selected::Toggle(*on),

        #[cfg(feature = "client_voice")]
        Value::Volume(percent) => Selected::Volume(item.key, *percent),

        #[cfg(feature = "client_voice")]
        Value::Device { id, input } => Selected::Device(id.clone(), *input),
    })
}

//LEFT/RIGHT: SLIDE A VOLUME, FLIP A TOGGLE, OR CYCLE A DEVICE WITHOUT OPENING THE PICKER
fn adjust(app: &mut App, direction: i32)
{
    let _row = app.settings.selected; //ONLY THE AUDIO ROWS NEED TO KNOW WHICH ROW THEY ARE

    match selected(app)
    {
        //A TOGGLE ONLY HAS TWO STATES, SO EITHER DIRECTION MEANS THE OTHER ONE
        Some(Selected::Toggle(on)) => if (direction > 0) != on { toggle(app) },

        #[cfg(feature = "client_voice")]
        Some(Selected::Volume(key, percent)) =>
        {
            let next = if direction > 0
            {
                voice_options::clamp_volume(percent.saturating_add(VOLUME_STEP))
            } else
            {
                percent.saturating_sub(VOLUME_STEP)
            };

            if next == percent { return; }

            if let Some(Row::Item(item)) = app.settings.rows.get_mut(_row) { item.value = Value::Volume(next); }

            config::client_write_int(key, next as i64);
            apply_volume(key, next);
        },

        #[cfg(feature = "client_voice")]
        Some(Selected::Device(id, input)) =>
        {
            let entries = device_entries(app, input);
            let current = entries.iter().position(|entry| entry.id == id).unwrap_or(0);

            let next = (current as isize + direction as isize).rem_euclid(entries.len() as isize) as usize;
            let chosen = entries[next].id.clone();

            set_device(app, _row, chosen);
        },

        None => {},
    }
}

//ENTER/SPACE: FLIP A TOGGLE, OPEN THE DEVICE PICKER, OR NUDGE A VOLUME UP
fn activate(app: &mut App)
{
    let _row = app.settings.selected; //ONLY THE AUDIO ROWS NEED TO KNOW WHICH ROW THEY ARE

    match selected(app)
    {
        Some(Selected::Toggle(_)) => toggle(app),

        #[cfg(feature = "client_voice")]
        Some(Selected::Volume(..)) => adjust(app, 1),

        #[cfg(feature = "client_voice")]
        Some(Selected::Device(id, input)) =>
        {
            let entries = device_entries(app, input);

            app.settings.picker = Some(Picker
            {
                title: if input { " Input device " } else { " Output device " },
                selected: entries.iter().position(|entry| entry.id == id).unwrap_or(0),
                entries,
                row: _row,
            });
        },

        None => {},
    }
}

fn toggle(app: &mut App)
{
    //FLIP THE ROW FIRST, THEN LET GO OF IT - THE FOLLOW-UP TOUCHES App AS A WHOLE
    let changed = match app.settings.rows.get_mut(app.settings.selected)
    {
        Some(Row::Item(item)) => match &item.value
        {
            Value::Toggle { on, invert } =>
            {
                let (next, invert) = (!*on, *invert);
                item.value = Value::Toggle { on: next, invert };

                Some((item.key, next, invert))
            },

            #[allow(unreachable_patterns)] //THE OTHER VARIANTS ONLY EXIST IN A VOICE BUILD
            _ => None,
        },

        _ => None,
    };

    let Some((key, next, invert)) = changed else { return };

    config::client_write_bool(key, if invert { !next } else { next });

    //disable_colors AND show_id ARE READ THROUGH App::theme, AND APPLY TO THE WHOLE PANE AT ONCE
    app.reload_theme();

    #[cfg(feature = "client_voice")]
    match key
    {
        "noise_suppression" => voice_options::set_noise_suppression(next),
        "automatic_gain" => voice_options::set_automatic_gain(next),
        _ => {},
    }
}

#[cfg(feature = "client_voice")]
fn set_device(app: &mut App, row: usize, chosen: String)
{
    let Some(Row::Item(item)) = app.settings.rows.get_mut(row) else { return };

    let Value::Device { id, input } = &item.value else { return };

    if *id == chosen { return; }

    let (key, input) = (item.key, *input);
    item.value = Value::Device { id: chosen.clone(), input };

    config::client_write(key, &chosen);

    //A RUNNING VOICE SESSION REBUILDS ITS CPAL STREAMS ON THIS, WITHOUT DROPPING THE SESSION ITSELF
    voice_options::mark_devices_changed();
}

#[cfg(not(feature = "client_voice"))]
fn set_device(_app: &mut App, _row: usize, _chosen: String) {}

//THE SYSTEM DEFAULT PLUS EVERY DEVICE cpal REPORTED, WITH THE CONFIGURED ONE GUARANTEED TO BE IN THE LIST
#[cfg(feature = "client_voice")]
fn device_entries(app: &App, input: bool) -> Vec<DeviceEntry>
{
    let devices = if input { &app.settings.devices.input } else { &app.settings.devices.output };

    let mut entries = vec![DeviceEntry { id: String::new(), label: String::from(DEFAULT_DEVICE) }];
    entries.extend(devices.iter().cloned());

    //A DEVICE THAT IS CONFIGURED BUT CURRENTLY UNPLUGGED STILL DESERVES A ROW
    let configured = config::read_config::<String>(if input { "input_device" } else { "output_device" });
    if !configured.is_empty() && !entries.iter().any(|entry| entry.id == configured)
    {
        entries.push(DeviceEntry { label: configured.clone(), id: configured });
    }

    entries
}

#[cfg(feature = "client_voice")]
fn apply_volume(key: &str, percent: u32) //LIVE-UPDATE THE RUNNING AUDIO STREAMS
{
    match key
    {
        "input_volume" => voice_options::set_input_volume(percent),
        "output_volume" => voice_options::set_output_volume(percent),
        _ => {},
    }
}
