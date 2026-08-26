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

use crossterm::event::
{
    KeyCode,
    KeyEvent,
    KeyModifiers,
};

use why2_chat::
{
    config,
    network::codes::{ ServerSetting, SettingValue },
};

#[cfg(feature = "client_voice")]
use why2_chat::network::voice::client::options as voice_options;

use super::state::App;

//CONSTS
pub const MAX_PICKER_ROWS: usize = 8; //VISIBLE DEVICE ROWS BEFORE THE PICKER SCROLLS

pub const SAVE_LABEL: &str = "Save"; //THE BUTTON THE SERVER ROWS ARE SENT BACK WITH

#[cfg(feature = "client_voice")]
pub const DEFAULT_DEVICE: &str = "System default"; //SHOWN FOR AN EMPTY input_device/output_device

#[cfg(feature = "client_voice")]
const VOLUME_STEP: u32 = 5;

//ENUMS
pub enum Value
{
    //THE CONFIG KEY IS THE TRUTH, invert FLIPS IT FOR KEYS PHRASED AS A NEGATIVE (disable_colors)
    Toggle { on: bool, invert: bool },

    //THE TWO DATATYPES ONLY THE SERVER ROWS HAVE - BOTH ARE EDITED BY TYPING INTO THE ROW
    Number(i64),
    Text(String),

    #[cfg(feature = "client_voice")]
    Volume(u32), //PERCENT

    //THE cpal DEVICE ID, WHICH IS WHAT client.toml HOLDS - THE LABEL IS LOOKED UP FOR DISPLAY ONLY
    #[cfg(feature = "client_voice")]
    Device { id: String, input: bool }, //EMPTY ID = SYSTEM DEFAULT
}

pub enum Row
{
    Header(String),
    Item(Item),
    Action(&'static str), //A BUTTON - THE SERVER ROWS ARE THE ONLY THING THAT NEEDS ONE
}

//STRUCTS
pub struct Item
{
    pub label: String,
    pub key: String, //THE CONFIG KEY THIS ROW OWNS - client.toml's OR THE SERVER'S
    pub value: Value,
    pub hint: String,   //THE COMMENT THE SERVER SENT ALONG (EMPTY ON A CLIENT ROW)
    pub changed: bool,  //EDITED AND NOT SAVED YET - ONLY A SERVER ROW IS EVER LEFT UNSAVED
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

pub struct Settings //THE /settings OVERLAY, IN EITHER OF ITS TWO MODES
{
    pub open: bool,
    pub rows: Vec<Row>,
    pub selected: usize,
    pub picker: Option<Picker>,

    //THE ROWS BELONG TO server.toml, WHICH IS NOT OURS TO WRITE - IT IS EDITED HERE AND SAVED IN ONE GO
    pub server: bool,
    pub edit: Option<String>, //WHAT IS BEING TYPED INTO THE SELECTED ROW
    pub saving: bool,         //A SAVE IS ON THE WIRE, WAITING FOR THE SERVER TO ANSWER WITH WHAT IT STORED

    save: Option<Vec<ServerSetting>>, //ROWS THE EVENT LOOP STILL HAS TO PUT ON THE WIRE

    #[cfg(feature = "client_voice")]
    devices: Devices,
}

//IMPLEMENTATIONS
impl Default for Settings
{
    fn default() -> Self { Self::new() }
}

impl Item
{
    fn client(label: &str, key: &str, value: Value) -> Self //A ROW BACKED BY client.toml
    {
        Self { label: label.to_string(), key: key.to_string(), value, hint: String::new(), changed: false }
    }
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
            server: false,
            edit: None,
            saving: false,
            save: None,

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
            rows.push(Row::Header(String::from("Audio")));

            rows.push(Row::Item(Item::client("Input device", "input_device",
                Value::Device { id: config::read_config::<String>("input_device"), input: true })));

            rows.push(Row::Item(Item::client("Output device", "output_device",
                Value::Device { id: config::read_config::<String>("output_device"), input: false })));

            rows.push(Row::Item(Item::client("Input volume", "input_volume",
                Value::Volume(voice_options::clamp_volume(config::read_config::<u32>("input_volume"))))));

            rows.push(Row::Item(Item::client("Output volume", "output_volume",
                Value::Volume(voice_options::clamp_volume(config::read_config::<u32>("output_volume"))))));

            rows.push(Row::Item(Item::client("Noise suppression", "noise_suppression",
                toggle_value("noise_suppression", false))));

            rows.push(Row::Item(Item::client("Automatic gain", "automatic_gain",
                toggle_value("automatic_gain", false))));
        }

        rows.push(Row::Header(String::from("Interface")));

        rows.push(Row::Item(Item::client("Message colors", "disable_colors", toggle_value("disable_colors", true))));
        rows.push(Row::Item(Item::client("Background logo", "disable_logo", toggle_value("disable_logo", true))));
        rows.push(Row::Item(Item::client("Show client IDs", "show_id", toggle_value("show_id", false))));

        self.rows = rows;
        self.picker = None;
        self.open = true;
        self.server = false;
        self.edit = None;
        self.saving = false;
        self.selected = 0;

        #[cfg(feature = "client_voice")]
        {
            self.devices = devices;
        }

        #[cfg(not(feature = "client_voice"))]
        let _ = devices;

        self.step(1); //LAND ON THE FIRST ITEM, NOT ON THE HEADER ABOVE IT
    }

    //THE SERVER'S OWN CONFIG. NOTHING HERE NAMES A KEY - THE ROWS, THE HEADINGS AND THE HINTS ARE ALL
    //WHATEVER server.toml TURNED OUT TO HOLD, SO A KEY ADDED THERE NEEDS NO CLIENT CHANGE AT ALL
    pub fn open_server(&mut self, settings: Vec<ServerSetting>)
    {
        let mut rows: Vec<Row> = Vec::new();
        let mut section = String::new();

        for setting in settings
        {
            if setting.section != section
            {
                section = setting.section.clone();

                if !section.is_empty() { rows.push(Row::Header(section.clone())); }
            }

            rows.push(Row::Item(Item
            {
                label: setting.key.replace('_', " "),
                key: setting.key,
                value: match setting.value
                {
                    SettingValue::Toggle(on) => Value::Toggle { on, invert: false },
                    SettingValue::Number(number) => Value::Number(number),
                    SettingValue::Text(text) => Value::Text(text),
                },
                hint: setting.description,
                changed: false,
            }));
        }

        rows.push(Row::Action(SAVE_LABEL)); //NOTHING LEAVES THIS BOX UNTIL THIS IS PRESSED

        self.rows = rows;
        self.picker = None;
        self.open = true;
        self.server = true;
        self.edit = None;
        self.saving = false;
        self.selected = 0;

        self.step(1);
    }

    pub fn close(&mut self)
    {
        self.open = false;
        self.picker = None;
        self.edit = None;
        self.server = false;
        self.saving = false;
        self.rows = Vec::new();
    }

    pub fn title(&self) -> String //WHAT THE BOX CALLS ITSELF - AN UNSAVED SERVER ROW IS SAID SO IN THE TITLE
    {
        if !self.server { return String::from(" Settings "); }

        if self.saving { return String::from(" Server settings · saving… "); }

        if self.unsaved() { String::from(" Server settings · unsaved ") } else { String::from(" Server settings ") }
    }

    pub fn unsaved(&self) -> bool //A ROW HAS BEEN EDITED AND NOT SENT BACK YET
    {
        self.rows.iter().any(|row| matches!(row, Row::Item(item) if item.changed))
    }

    //THE ROWS THE EVENT LOOP STILL HAS TO SEND - IT OWNS THE SOCKET, THIS OVERLAY DOES NOT
    pub fn take_save(&mut self) -> Option<Vec<ServerSetting>> { self.save.take() }

    //WHAT THE SERVER ANSWERED A SAVE WITH: THE CONFIG AS IT ACTUALLY STANDS NOW, SO A ROW IT REFUSED
    //SNAPS BACK INSTEAD OF SITTING THERE LOOKING APPLIED. THE SELECTION IS KEPT WHERE THE USER LEFT IT
    pub fn stored(&mut self, settings: Vec<ServerSetting>)
    {
        let selected = self.selected;

        self.open_server(settings);
        self.selected = selected.min(self.rows.len().saturating_sub(1));

        //THE ROW THE SELECTION LANDED ON MAY BE A HEADING NOW
        if matches!(self.rows.get(self.selected), Some(Row::Header(_))) { self.step(1); }
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

            if !matches!(self.rows[index as usize], Row::Header(_))
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
                item.value = Value::Device { id: config::read_config::<String>(&item.key), input };
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
//ONE KEYPRESS WHILE THE OVERLAY IS UP. A CLIENT ROW IS WRITTEN THROUGH IMMEDIATELY - A SERVER ROW IS NOT
//OURS TO WRITE, SO IT IS HELD UNTIL Save AND SENT IN ONE GO.
pub fn handle_key(app: &mut App, key: KeyEvent)
{
    //THE DEVICE PICKER OWNS THE KEYBOARD WHILE IT IS OPEN
    if app.settings.picker.is_some()
    {
        handle_picker_key(app, key);
        return;
    }

    //SO DOES A ROW THAT IS BEING TYPED INTO
    if app.settings.edit.is_some()
    {
        handle_edit_key(app, key);
        return;
    }

    //Ctrl+S SAVES FROM WHEREVER THE SELECTION IS - THE BUTTON IS AT THE BOTTOM OF A LONG LIST
    if app.settings.server && key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s')
    {
        save(app);
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
    if app.settings.edit.is_some() { return; } //A ROW BEING TYPED INTO IS NOT SCROLLED AWAY FROM

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

//TYPING INTO A Number/Text ROW. Esc PUTS THE OLD VALUE BACK, ⏎ KEEPS WHAT WAS TYPED
fn handle_edit_key(app: &mut App, key: KeyEvent)
{
    match key.code
    {
        KeyCode::Esc => app.settings.edit = None,

        KeyCode::Enter => commit_edit(app),

        KeyCode::Backspace => { if let Some(edit) = app.settings.edit.as_mut() { edit.pop(); } },

        KeyCode::Char(c) =>
        {
            //A NUMBER ROW ONLY TAKES A NUMBER - THE MINUS SIGN ONLY AS THE FIRST CHARACTER
            let numeric = matches!(app.settings.rows.get(app.settings.selected), Some(Row::Item(item))
                if matches!(item.value, Value::Number(_)));

            if let Some(edit) = app.settings.edit.as_mut()
                && (!numeric || c.is_ascii_digit() || (c == '-' && edit.is_empty()))
            {
                edit.push(c);
            }
        },

        _ => {},
    }
}

fn commit_edit(app: &mut App) //KEEP WHAT WAS TYPED, IF THE ROW CAN HOLD IT
{
    let Some(edit) = app.settings.edit.take() else { return };

    let Some(Row::Item(item)) = app.settings.rows.get_mut(app.settings.selected) else { return };

    match &item.value
    {
        //AN UNPARSEABLE NUMBER IS NOT A CHANGE - THE ROW KEEPS WHAT IT HAD
        Value::Number(current) => match edit.trim().parse::<i64>()
        {
            Ok(number) if number != *current =>
            {
                item.value = Value::Number(number);
                item.changed = true;
            },

            _ => {},
        },

        Value::Text(current) => if edit != *current
        {
            item.value = Value::Text(edit);
            item.changed = true;
        },

        _ => {},
    }
}

//WHAT THE SELECTED ROW HOLDS, COPIED OUT SO THE ACTIONS BELOW CAN TOUCH App AGAIN
enum Selected
{
    Toggle(bool),
    Number(i64),
    Text(String),
    Action,

    #[cfg(feature = "client_voice")]
    Volume(String, u32),

    #[cfg(feature = "client_voice")]
    Device(String, bool),
}

fn selected(app: &App) -> Option<Selected>
{
    match app.settings.rows.get(app.settings.selected)?
    {
        Row::Header(_) => None,
        Row::Action(_) => Some(Selected::Action),

        Row::Item(item) => Some(match &item.value
        {
            Value::Toggle { on, .. } => Selected::Toggle(*on),
            Value::Number(number) => Selected::Number(*number),
            Value::Text(text) => Selected::Text(text.clone()),

            #[cfg(feature = "client_voice")]
            Value::Volume(percent) => Selected::Volume(item.key.clone(), *percent),

            #[cfg(feature = "client_voice")]
            Value::Device { id, input } => Selected::Device(id.clone(), *input),
        }),
    }
}

//LEFT/RIGHT: SLIDE A VOLUME, FLIP A TOGGLE, STEP A NUMBER, OR CYCLE A DEVICE WITHOUT OPENING THE PICKER
fn adjust(app: &mut App, direction: i32)
{
    let row = app.settings.selected;

    match selected(app)
    {
        //A TOGGLE ONLY HAS TWO STATES, SO EITHER DIRECTION MEANS THE OTHER ONE
        Some(Selected::Toggle(on)) => if (direction > 0) != on { toggle(app) },

        Some(Selected::Number(number)) =>
        {
            let next = number.saturating_add(direction as i64);

            if let Some(Row::Item(item)) = app.settings.rows.get_mut(row)
            {
                item.value = Value::Number(next);
                item.changed = true;
            }
        },

        //A FREE-FORM STRING HAS NO NEXT VALUE TO STEP TO - IT IS TYPED
        Some(Selected::Text(_)) | Some(Selected::Action) => {},

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

            if let Some(Row::Item(item)) = app.settings.rows.get_mut(row) { item.value = Value::Volume(next); }

            config::client_write_int(&key, next as i64);
            apply_volume(&key, next);
        },

        #[cfg(feature = "client_voice")]
        Some(Selected::Device(id, input)) =>
        {
            let entries = device_entries(app, input);
            let current = entries.iter().position(|entry| entry.id == id).unwrap_or(0);

            let next = (current as isize + direction as isize).rem_euclid(entries.len() as isize) as usize;
            let chosen = entries[next].id.clone();

            set_device(app, row, chosen);
        },

        None => {},
    }
}

//ENTER/SPACE: FLIP A TOGGLE, START TYPING INTO A VALUE, PRESS THE BUTTON, OR OPEN THE DEVICE PICKER
fn activate(app: &mut App)
{
    let _row = app.settings.selected; //ONLY THE AUDIO ROWS NEED TO KNOW WHICH ROW THEY ARE

    match selected(app)
    {
        Some(Selected::Toggle(_)) => toggle(app),

        Some(Selected::Number(number)) => app.settings.edit = Some(number.to_string()),
        Some(Selected::Text(text)) => app.settings.edit = Some(text),

        Some(Selected::Action) => save(app),

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

//HAND THE EDITED ROWS TO THE EVENT LOOP, WHICH IS WHERE THE SOCKET IS. THE ROWS STAY MARKED UNTIL THE
//SERVER SAYS WHAT IT STORED - Settings::stored REBUILDS THEM FROM ITS ANSWER
fn save(app: &mut App)
{
    if !app.settings.server { return; }

    let changed: Vec<ServerSetting> = app.settings.rows.iter().filter_map(|row|
    {
        let Row::Item(item) = row else { return None };

        if !item.changed { return None; }

        Some(ServerSetting
        {
            key: item.key.clone(),
            value: match &item.value
            {
                Value::Toggle { on, .. } => SettingValue::Toggle(*on),
                Value::Number(number) => SettingValue::Number(*number),
                Value::Text(text) => SettingValue::Text(text.clone()),

                #[allow(unreachable_patterns)] //THE OTHER VARIANTS ONLY EXIST ON A CLIENT ROW
                _ => return None,
            },

            //THE SERVER IS THE ONE WHO KNOWS THESE - SENDING THEM BACK WOULD ONLY BE US QUOTING IT
            section: String::new(),
            description: String::new(),
        })
    }).collect();

    if changed.is_empty() { return; }

    app.settings.saving = true;
    app.settings.save = Some(changed);
}

fn toggle(app: &mut App)
{
    let server = app.settings.server;

    //FLIP THE ROW FIRST, THEN LET GO OF IT - THE FOLLOW-UP TOUCHES App AS A WHOLE
    let changed = match app.settings.rows.get_mut(app.settings.selected)
    {
        Some(Row::Item(item)) => match &item.value
        {
            Value::Toggle { on, invert } =>
            {
                let (next, invert) = (!*on, *invert);
                item.value = Value::Toggle { on: next, invert };
                item.changed = item.changed || server;

                Some((item.key.clone(), next, invert))
            },

            #[allow(unreachable_patterns)] //THE OTHER VARIANTS ARE NOT TOGGLES
            _ => None,
        },

        _ => None,
    };

    let Some((key, next, invert)) = changed else { return };

    //A SERVER ROW IS NOT OURS TO WRITE ANYWHERE - IT GOES BACK OVER THE WIRE ON Save
    if server { return; }

    config::client_write_bool(&key, if invert { !next } else { next });

    //THE INTERFACE ROWS ARE READ THROUGH App::theme, AND APPLY TO THE WHOLE PANE AT ONCE
    app.reload_theme();

    #[cfg(feature = "client_voice")]
    match key.as_str()
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

    let (key, input) = (item.key.clone(), *input);
    item.value = Value::Device { id: chosen.clone(), input };

    config::client_write(&key, &chosen);

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
