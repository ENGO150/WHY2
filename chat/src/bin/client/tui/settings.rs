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

//! # Settings Overlay
//!
//! One modal box in three modes ([`Mode`]):
//!
//! - **Client** — `client.toml` and the audio devices. Every row writes straight through to the
//!   file and to the atomics the running audio callbacks read; there is no save step.
//! - **Server** — `server.toml`, as the server sent it. Nothing here names a server key: rows
//!   are built from whatever the server sends, with its section headings and comments. Edits are
//!   held, marked, until Save, and a startup-only key is marked as needing a restart; a second
//!   button restarts the server, refused while anything is unsaved and confirmed by a second
//!   press.
//! - **Profile** — somebody's profile: ours to edit and save, or theirs to read (where Enter on a
//!   website opens it).
//!
//! The selected row's description — a server key's comment, a profile field's full text — is
//! wrapped in the foot of the box, sized for the longest so the box does not jump around. The
//! overlay never touches the socket: a save is handed to the event loop through
//! [`Settings::take_save`] and friends.
//!
//! # Inverted Keys
//! A row whose config key is phrased negatively (`disable_colors`) carries `invert`, applied in
//! exactly one place per direction ([`toggle_value`] reading, `toggle` writing).

use ratatui::layout::Rect;

use crossterm::event::
{
    KeyCode,
    KeyEvent,
    KeyModifiers,
};

use why2_chat::
{
    t,
    i18n,
    config,
    misc,
    role::Role,
    network::codes::{ ServerSetting, SettingValue, UserProfile },
};

#[cfg(feature = "client_voice")]
use why2_chat::network::voice::client::options as voice_options;

#[cfg(feature = "client_screen")]
use why2_chat::network::screen::client::options as screen_options;

use super::
{
    consts,
    state::
    {
        App,
        Fitted,
    },
    theme,
};

//ENUMS
/// What the box is showing.
pub enum Mode //WHAT THE BOX IS SHOWING
{
    /// `client.toml`, written through on every edit.
    Client,                  //client.toml, WRITTEN THROUGH ON EVERY EDIT
    /// `server.toml`, held until Save.
    Server,                  //server.toml, HELD UNTIL Save
    /// A user profile; `own` means ours, and so editable.
    Profile { own: bool },   //A USER PROFILE | own = OURS, SO EDITABLE
}

/// The value of one row.
pub enum Value
{
    //A CONFIG KEY, invert FOR A NEGATIVE ONE
    /// A boolean key; `invert` for a key phrased negatively.
    Toggle { on: bool, invert: bool },

    //THE TWO SERVER-ONLY DATATYPES, EDITED BY TYPING
    /// An integer (server rows), edited by typing.
    Number(i64),
    /// A string (server rows and profile fields), edited by typing.
    Text(String),

    //A PATH TO UPLOAD | None = UNTOUCHED, EMPTY = DROP IT
    /// The avatar path to upload: `None` is untouched, empty drops the avatar.
    Avatar(Option<String>),

    /// A palette, by index into [`PALETTES`](super::theme::PALETTES).
    Theme(usize), //INDEX INTO theme::PALETTES
    /// A language, by locale code.
    Language(String), //A LOCALE CODE

    /// A volume, in percent.
    #[cfg(feature = "client_voice")]
    Volume(u32), //PERCENT

    //THE cpal DEVICE ID; THE LABEL IS DISPLAY ONLY
    /// An audio device by cpal id; empty is the system default.
    #[cfg(feature = "client_voice")]
    Device { id: String, input: bool }, //EMPTY ID = SYSTEM DEFAULT
}

/// One row of the box.
pub enum Row
{
    /// A section heading.
    Header(String),
    /// A setting.
    Item(Item),
    /// A button.
    Action(&'static str), //A BUTTON - THE SERVER ROWS ARE THE ONLY THING THAT NEEDS ONE
}

//STRUCTS
/// One setting row.
pub struct Item
{
    /// The label shown.
    pub label: String,
    /// The config key the row owns.
    pub key: String, //THE CONFIG KEY THIS ROW OWNS - client.toml's OR THE SERVER'S
    /// Its value.
    pub value: Value,
    /// The comment the server sent along; empty on a client row.
    pub hint: String,   //THE COMMENT THE SERVER SENT ALONG (EMPTY ON A CLIENT ROW)
    /// Whether it was edited and not saved yet.
    pub changed: bool,  //EDITED AND NOT SAVED YET
    /// Whether it is only read at startup.
    pub restart: bool,  //STORED ON SAVE, BUT ONLY READ AT STARTUP
}

//ONE DEVICE AS THE PICKER SHOWS IT
/// One audio device, as the picker shows it.
#[derive(Clone, Default)]
pub struct DeviceEntry
{
    /// The cpal device id.
    pub id: String,
    /// The description shown.
    pub label: String,
}

/// The device list opened on top of the rows.
pub struct Picker //DEVICE LIST OPENED ON TOP OF THE SETTINGS ROWS
{
    /// The list's title.
    pub title: &'static str,
    /// The devices; the first is always the system default.
    pub entries: Vec<DeviceEntry>, //ENTRY 0 IS ALWAYS THE SYSTEM DEFAULT
    /// The selected entry.
    pub selected: usize,
    /// The row that opened it.
    pub row: usize,    //THE SETTINGS ROW THAT OPENED IT
    /// The first visible entry.
    pub offset: usize, //FIRST VISIBLE ENTRY
}

/// The audio devices, enumerated once when the box is opened.
#[derive(Default)]
pub struct Devices //WHAT cpal REPORTED, ENUMERATED ONCE WHEN /settings IS TYPED
{
    /// Capture devices.
    #[cfg(feature = "client_voice")]
    pub input: Vec<DeviceEntry>,

    /// Playback devices.
    #[cfg(feature = "client_voice")]
    pub output: Vec<DeviceEntry>,
}

/// The settings overlay, in any of its modes.
pub struct Settings //THE /settings OVERLAY, IN ANY OF ITS MODES
{
    /// Whether the box is up.
    pub open: bool,
    /// Its rows.
    pub rows: Vec<Row>,
    /// The selected row.
    pub selected: usize,
    /// The device picker, if open.
    pub picker: Option<Picker>,

    //WHERE THE VIEW STANDS, WRITTEN BY THE DRAW PATH
    /// The first visible row, written by the draw path.
    pub offset: usize,
    /// Rows the last frame fit, for paging.
    pub page: usize, //ROWS THE LAST FRAME FIT

    /// What the box is showing.
    pub mode: Mode,
    /// Whose profile it is, in profile mode.
    pub subject: String,      //WHOSE PROFILE, IN Profile MODE
    /// Their role.
    pub role: Role,           //AND THEIR RANK

    //THE PICTURE A PROFILE NAMES, THE ONE THAT IS BUILT, AND WHERE THE BOX PUT IT
    /// The avatar the profile names.
    pub avatar: Option<[u8; 32]>,
    /// The avatar, decoded and fitted.
    pub picture: Option<Box<Fitted>>,
    /// Which hash the fitted avatar is.
    pub picture_of: Option<[u8; 32]>,
    /// Where the draw path put the avatar.
    pub picture_area: Rect,

    //THE MODES THAT SAVE IN ONE GO
    /// What is being typed into the selected row.
    pub edit: Option<String>, //WHAT IS BEING TYPED INTO THE SELECTED ROW
    /// Paths offered for the avatar row.
    pub paths: Vec<String>,   //PATHS OFFERED FOR THE AVATAR ROW
    /// The selected path.
    pub path: usize,          //THE ONE SELECTED
    /// Whether a save is in flight.
    pub saving: bool,         //A SAVE IS ON THE WIRE

    /// Server rows waiting for the event loop to send.
    save: Option<Vec<ServerSetting>>, //ROWS THE EVENT LOOP STILL HAS TO PUT ON THE WIRE
    /// A profile waiting for the event loop to send.
    profile: Option<UserProfile>,     //AND THE PROFILE, THE SAME WAY
    /// An avatar path waiting for the event loop to upload.
    avatar_save: Option<String>,      //AND THE AVATAR

    //THE STARTUP-ONLY KEYS IN THAT SAVE
    /// The startup-only keys in the last save.
    pub restart_note: Option<String>,

    //ONE PRESS ARMS THE RESTART, THE NEXT FIRES IT
    /// Whether the restart button is armed.
    pub confirm: bool,
    /// Whether a confirmed restart waits for the event loop.
    restart: bool, //ARMED, CONFIRMED

    /// The enumerated audio devices.
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
    /// Creates a client row backed by `client.toml`.
    fn client(label: &str, key: &str, value: Value) -> Self //A ROW BACKED BY client.toml
    {
        Self { label: label.to_string(), key: key.to_string(), value, hint: String::new(), changed: false, restart: false }
    }
}

impl Settings
{
    /// Creates a closed overlay.
    pub fn new() -> Self
    {
        Self
        {
            open: false,
            rows: Vec::new(),
            selected: 0,
            picker: None,
            offset: 0,
            page: 0,
            mode: Mode::Client,
            subject: String::new(),
            role: Role::default(),
            avatar: None,
            picture: None,
            picture_of: None,
            picture_area: Rect::ZERO,
            edit: None,
            paths: Vec::new(),
            path: 0,
            saving: false,
            save: None,
            profile: None,
            avatar_save: None,
            restart_note: None,
            confirm: false,
            restart: false,

            #[cfg(feature = "client_voice")]
            devices: Devices::default(),
        }
    }

    //OPEN THE OVERLAY, READING THE CONFIG ONCE
    /// Opens the client settings, reading the config once.
    pub fn open(&mut self, devices: Devices)
    {
        let mut rows: Vec<Row> = Vec::new();

        #[cfg(feature = "client_voice")]
        {
            rows.push(Row::Header(t!("settings.section.audio").to_owned()));

            rows.push(Row::Item(Item::client(t!("settings.row.input_device"), "input_device",
                Value::Device { id: config::read_config::<String>("input_device"), input: true })));

            rows.push(Row::Item(Item::client(t!("settings.row.output_device"), "output_device",
                Value::Device { id: config::read_config::<String>("output_device"), input: false })));

            rows.push(Row::Item(Item::client(t!("settings.row.input_volume"), "input_volume",
                Value::Volume(voice_options::clamp_volume(config::read_config::<u32>("input_volume"))))));

            rows.push(Row::Item(Item::client(t!("settings.row.output_volume"), "output_volume",
                Value::Volume(voice_options::clamp_volume(config::read_config::<u32>("output_volume"))))));

            #[cfg(feature = "client_screen")]
            rows.push(Row::Item(Item::client(t!("settings.row.screen_volume"), "screen_volume",
                Value::Volume(voice_options::clamp_volume(config::read_config::<u32>("screen_volume"))))));

            rows.push(Row::Item(Item::client(t!("settings.row.noise_suppression"), "noise_suppression",
                toggle_value("noise_suppression", false))));

            rows.push(Row::Item(Item::client(t!("settings.row.automatic_gain"), "automatic_gain",
                toggle_value("automatic_gain", false))));
        }

        rows.push(Row::Header(t!("settings.section.interface").to_owned()));

        rows.push(Row::Item(Item::client(t!("settings.row.language"), "language",
            Value::Language(config::read_config::<String>("language").trim().to_owned()))));

        rows.push(Row::Item(Item::client(t!("settings.row.theme"), "theme",
            Value::Theme(theme::palette_index(&config::read_config::<String>("theme"))))));

        rows.push(Row::Item(Item::client(t!("settings.row.disable_colors"), "disable_colors", toggle_value("disable_colors", true))));
        rows.push(Row::Item(Item::client(t!("settings.row.disable_logo"), "disable_logo", toggle_value("disable_logo", true))));
        rows.push(Row::Item(Item::client(t!("settings.row.auto_show_images"), "auto_show_images",
            toggle_value("auto_show_images", false))));

        rows.push(Row::Item(Item::client(t!("settings.row.render_math"), "render_math", toggle_value("render_math", false))));

        rows.push(Row::Item(Item::client(t!("settings.row.show_id"), "show_id", toggle_value("show_id", false))));
        rows.push(Row::Item(Item::client(t!("settings.row.show_message_ids"), "show_message_ids", toggle_value("show_message_ids", false))));
        rows.push(Row::Item(Item::client(t!("settings.row.show_timestamps"), "show_timestamps", toggle_value("show_timestamps", false))));
        rows.push(Row::Item(Item::client(t!("settings.row.message_stripes"), "message_stripes", toggle_value("message_stripes", false))));

        rows.push(Row::Header(t!("settings.section.privacy").to_owned()));

        rows.push(Row::Item(Item::client(t!("settings.row.share_device"), "share_device", toggle_value("share_device", false))));

        rows.push(Row::Item(Item::client(t!("settings.row.typing_indicator"), "typing_indicator",
            toggle_value("typing_indicator", false))));

        self.rows = rows;
        self.picker = None;
        self.open = true;
        self.mode = Mode::Client;
        self.edit = None;
        self.saving = false;
        self.selected = 0;
        self.offset = 0;

        #[cfg(feature = "client_voice")]
        {
            self.devices = devices;
        }

        #[cfg(not(feature = "client_voice"))]
        let _ = devices;

        self.step(1); //LAND ON THE FIRST ITEM, NOT ON THE HEADER ABOVE IT
    }

    //REBUILD THE CLIENT ROWS IN THE CURRENT LANGUAGE
    /// Rebuilds the client rows in the current language, keeping the selection.
    pub fn relabel(&mut self)
    {
        let (selected, offset) = (self.selected, self.offset);

        #[cfg(feature = "client_voice")]
        let devices = std::mem::take(&mut self.devices);

        #[cfg(not(feature = "client_voice"))]
        let devices = Devices::default();

        self.open(devices);
        self.selected = selected;
        self.offset = offset;
    }

    //THE SERVER'S OWN CONFIG ROWS
    /// Opens the server's configuration rows.
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
                restart: setting.restart, //THE SERVER NAMES ITS STARTUP-ONLY KEYS
            }));
        }

        rows.push(Row::Action(consts::SAVE_LABEL)); //NOTHING LEAVES THIS BOX UNTIL THIS IS PRESSED
        rows.push(Row::Action(consts::RESTART_LABEL));

        self.rows = rows;
        self.picker = None;
        self.open = true;
        self.mode = Mode::Server;
        self.edit = None;
        self.saving = false;
        self.confirm = false;
        self.selected = 0;
        self.offset = 0;

        self.step(1);
    }

    //SOMEBODY'S PROFILE - OURS TO EDIT, OR THEIRS TO READ
    /// Opens a profile: ours to edit, or somebody else's to read.
    pub fn open_profile(&mut self, username: String, mut profile: UserProfile, own: bool)
    {
        //THE AVATAR IS TYPED AS A PATH
        let mut rows: Vec<Row> = match own
        {
            true => vec![Row::Item(Item
            {
                label: t!("settings.avatar").to_owned(),
                key: String::from(consts::AVATAR_KEY),
                value: Value::Avatar(None),
                hint: String::new(),
                changed: false,
                restart: false,
            })],

            false => Vec::new(),
        };

        //ONE ROW PER FIELD, IN THE ORDER THE PROFILE ITSELF KEEPS THEM
        rows.extend(UserProfile::KEYS.iter().map(|key| Row::Item(Item
        {
            label: field_label(key),
            key: (*key).to_string(),
            value: Value::Text(profile.field_mut(key).map(std::mem::take).unwrap_or_default()),
            hint: String::new(),
            changed: false,
            restart: false,
        })));

        //OUR OWN VISIBILITY IS A TOGGLE
        if own
        {
            rows.push(Row::Item(Item
            {
                label: field_label(UserProfile::ONLINE_STATUS_KEY),
                key: String::from(UserProfile::ONLINE_STATUS_KEY),
                value: Value::Toggle { on: profile.online_status, invert: false },
                hint: String::new(),
                changed: false,
                restart: false,
            }));
        }

        //NOTHING LEAVES THIS BOX UNTIL THIS IS PRESSED
        if own { rows.push(Row::Action(consts::SAVE_LABEL)); }

        //A PICTURE THAT IS NOT THE ONE ALREADY BUILT IS PUT DOWN
        self.avatar = profile.avatar;

        if self.picture_of != self.avatar
        {
            self.picture = None;
            self.picture_of = None;
        }

        self.rows = rows;
        self.picker = None;
        self.open = true;
        self.mode = Mode::Profile { own };
        self.subject = username;
        self.role = profile.role;
        self.edit = None;
        self.saving = false;
        self.confirm = false;
        self.selected = 0;
        self.offset = 0;

        self.land(0, 1);
    }

    /// Closes the box, discarding anything unsaved.
    pub fn close(&mut self)
    {
        self.open = false;
        self.picker = None;
        self.avatar = None;
        self.picture = None;
        self.picture_of = None;
        self.picture_area = Rect::ZERO;
        self.edit = None;
        self.mode = Mode::Client;
        self.saving = false;
        self.confirm = false;
        self.rows = Vec::new();
        self.offset = 0;
        self.page = 0;
    }

    //ROWS THE PICTURE CLAIMS AT THE TOP OF THE BOX, THE LINE OF AIR UNDER IT INCLUDED
    /// Returns the rows the avatar claims at the top of the box, the gap under it included.
    pub fn picture_rows(&self) -> u16
    {
        match self.picture.is_some()
        {
            true => consts::AVATAR_ROWS + 1,
            false => 0,
        }
    }

    //THE AVATAR ROW IS BEING TYPED INTO
    /// Returns whether the avatar row is being typed into.
    pub fn editing_avatar(&self) -> bool
    {
        self.edit.is_some() && matches!(self.rows.get(self.selected), Some(Row::Item(item)) if item.key == consts::AVATAR_KEY)
    }

    //RE-READ THE PATHS BESIDE WHAT IS TYPED
    /// Rereads the paths beside what is typed into the avatar row.
    fn refresh_paths(&mut self)
    {
        self.paths = match (self.editing_avatar(), self.edit.as_deref())
        {
            (true, Some(edit)) => super::palette::paths(edit, true),
            _ => Vec::new(),
        };

        self.path = 0;
    }

    /// Returns whether the box shows the server's configuration.
    pub fn server(&self) -> bool { matches!(self.mode, Mode::Server) } //server.toml's ROWS

    /// Returns whether the box shows a profile.
    pub fn profile(&self) -> bool { matches!(self.mode, Mode::Profile { .. }) } //SOMEBODY'S PROFILE

    /// Returns whether the box shows somebody else's profile.
    pub fn readonly(&self) -> bool { matches!(self.mode, Mode::Profile { own: false }) } //A PROFILE THAT IS NOT OURS

    //A MODE WHOSE ROWS ARE HELD UNTIL Save
    /// Returns whether edits are held until Save.
    pub fn deferred(&self) -> bool { matches!(self.mode, Mode::Server | Mode::Profile { own: true }) }

    //THE SELECTED ROW'S LINK, WHEN IT IS ONE
    /// Returns the selected row's website, in somebody else's profile.
    pub fn link(&self) -> Option<&str>
    {
        if !self.profile() { return None; }

        let Some(Row::Item(item)) = self.rows.get(self.selected) else { return None };
        let Value::Text(text) = &item.value else { return None };

        (item.key == "website" && misc::is_web_url(text)).then_some(text.as_str())
    }

    /// Returns the box's title.
    pub fn title(&self) -> String //WHAT THE BOX CALLS ITSELF
    {
        let name = match self.mode
        {
            Mode::Client => return t!("settings.title.client").to_owned(),
            Mode::Server => t!("settings.title.server").to_owned(),
            Mode::Profile { own: true } => t!("settings.title.own_profile").to_owned(),
            Mode::Profile { own: false } => t!("settings.title.profile", username = self.subject),
        };

        if self.saving { return t!("settings.title.saving", name); }

        if self.unsaved() { t!("settings.title.unsaved", name) } else { name }
    }

    /// Returns whether a row was edited and not saved.
    pub fn unsaved(&self) -> bool //A ROW HAS BEEN EDITED AND NOT SENT BACK YET
    {
        self.rows.iter().any(|row| matches!(row, Row::Item(item) if item.changed))
    }

    //THE ROWS THE EVENT LOOP STILL HAS TO SEND
    /// Takes the server rows waiting to be sent.
    pub fn take_save(&mut self) -> Option<Vec<ServerSetting>> { self.save.take() }

    //AND THE PROFILE THE SAME
    /// Takes the profile waiting to be sent.
    pub fn take_profile_save(&mut self) -> Option<UserProfile> { self.profile.take() }

    //AND THE AVATAR
    /// Takes the avatar path waiting to be uploaded.
    pub fn take_avatar_save(&mut self) -> Option<String> { self.avatar_save.take() }

    //AND A CONFIRMED RESTART
    /// Takes a confirmed restart.
    pub fn take_restart(&mut self) -> bool { std::mem::take(&mut self.restart) }

    //TAKE THE SAVED CONFIG, KEEPING THE SELECTION
    /// Takes the server's acknowledged configuration, keeping the selection.
    pub fn stored(&mut self, settings: Vec<ServerSetting>)
    {
        let selected = self.selected;
        let offset = self.offset; //THE VIEW STAYS WHERE IT WAS TOO

        self.open_server(settings);
        self.selected = selected.min(self.rows.len().saturating_sub(1));
        self.offset = offset;

        //THE ROW LANDED ON MAY BE A HEADING NOW
        if matches!(self.rows.get(self.selected), Some(Row::Header(_))) { self.step(1); }
    }

    //TAKE THE SAVED PROFILE, KEEPING THE SELECTION
    /// Takes the server's acknowledged profile, keeping the selection.
    pub fn stored_profile(&mut self, username: String, profile: UserProfile)
    {
        let selected = self.selected;

        self.open_profile(username, profile, true);
        self.selected = selected.min(self.rows.len().saturating_sub(1));
    }

    //MOVE THE SELECTION, SKIPPING HEADERS
    /// Moves the selection, skipping headings.
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

    //PageUp/PageDown MOVE BY WHAT THE FRAME FIT
    /// Moves the selection by a page.
    fn page_move(&mut self, direction: isize)
    {
        if self.rows.is_empty() { return; }

        let page = self.page.max(1) as isize;

        self.offset = (self.offset as isize + direction * page).max(0) as usize;

        let target = (self.selected as isize + direction * page).clamp(0, self.rows.len() as isize - 1);

        self.land(target as usize, direction);
    }

    //LAND ON index, OR THE NEAREST NON-HEADING ROW
    /// Selects `index`, or the nearest row that is not a heading in `direction`.
    fn land(&mut self, index: usize, direction: isize)
    {
        if self.rows.is_empty() { return; }

        self.selected = index.min(self.rows.len() - 1);

        if !matches!(self.rows[self.selected], Row::Header(_)) { return; }

        let before = self.selected;

        self.step(direction);

        if self.selected == before { self.step(-direction); }
    }

    //Home/End GO TO THE ENDS OF THE LIST
    /// Selects the first row.
    fn first_row(&mut self)
    {
        self.offset = 0;
        self.land(0, 1);
    }

    /// Selects the last row.
    fn last_row(&mut self)
    {
        self.offset = self.rows.len();
        self.land(self.rows.len().saturating_sub(1), -1);
    }

    //WHAT A STORED DEVICE ID IS CALLED
    /// Returns what a stored device id is called.
    #[cfg(feature = "client_voice")]
    pub fn device_label(&self, id: &str, input: bool) -> String
    {
        if id.is_empty() { return t!("settings.default_device").to_owned(); }

        let devices = if input { &self.devices.input } else { &self.devices.output };

        devices.iter().find(|device| device.id == id).map(|device| device.label.clone()).unwrap_or_else(|| String::from(id))
    }

    //RE-READ THE DEVICE ROWS OUT OF THE CONFIG
    /// Rereads the device rows from the config, after a failed switch put the old devices back.
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

//WHAT A PROFILE FIELD IS CALLED IN THE BOX
/// Returns what a profile field is called in the box.
fn field_label(key: &str) -> String
{
    i18n::get(&format!("profile.{key}")).map(str::to_owned).unwrap_or_else(|| key[..1].to_uppercase() + &key[1..])
}

//READ A BOOLEAN AS THE ROW SHOWS IT
/// Reads a boolean key as the row shows it, applying the inversion.
fn toggle_value(key: &str, invert: bool) -> Value
{
    let stored = config::read_config::<bool>(key);

    Value::Toggle { on: if invert { !stored } else { stored }, invert }
}

//FUNCTIONS
//PUBLIC
//ONE KEYPRESS WHILE THE OVERLAY IS UP
/// Handles a key while the box is up.
pub fn handle_key(app: &mut App, key: KeyEvent)
{
    //THE DEVICE PICKER OWNS THE KEYBOARD
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

    //ANYTHING BUT THE CONFIRMING KEY DISARMS IT
    if !matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) { app.settings.confirm = false; }

    //Ctrl+S SAVES FROM ANY ROW
    if app.settings.deferred() && key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s')
    {
        save(app);
        return;
    }

    match key.code
    {
        KeyCode::Esc => app.settings.close(),

        KeyCode::Up => app.settings.step(-1),
        KeyCode::Down => app.settings.step(1),

        KeyCode::PageUp => app.settings.page_move(-1),
        KeyCode::PageDown => app.settings.page_move(1),

        KeyCode::Home => app.settings.first_row(),
        KeyCode::End => app.settings.last_row(),

        KeyCode::Left => adjust(app, -1),
        KeyCode::Right => adjust(app, 1),

        KeyCode::Enter | KeyCode::Char(' ') => activate(app),

        _ => {},
    }
}

//MOUSE WHEEL - MOVES WHICHEVER LIST IS IN FRONT
/// Scrolls whichever list is in front.
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
/// Handles a key while the device picker is open.
fn handle_picker_key(app: &mut App, key: KeyEvent)
{
    //CLOSING THE PICKER TAKES IT OUT OF App
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

            //ENTRY 0 IS THE SYSTEM DEFAULT
            let chosen = picker.entries.get(picker.selected).map(|entry| entry.id.clone()).unwrap_or_default();

            set_device(app, picker.row, chosen);
            return;
        },

        _ => {},
    }

    let page = app.settings.page.max(1) as isize; //READ BEFORE THE BORROW BELOW TAKES app.settings

    let Some(picker) = app.settings.picker.as_mut() else { return };

    match key.code
    {
        KeyCode::Up => picker.selected = if picker.selected == 0 { picker.entries.len() - 1 } else { picker.selected - 1 },
        KeyCode::Down => picker.selected = (picker.selected + 1) % picker.entries.len(),

        //THE PICKER HAS NO HEADINGS, SO A PAGE IS A JUMP
        KeyCode::PageUp | KeyCode::PageDown =>
        {
            let direction: isize = if key.code == KeyCode::PageUp { -1 } else { 1 };

            picker.offset = (picker.offset as isize + direction * page).max(0) as usize;

            picker.selected = (picker.selected as isize + direction * page)
                .clamp(0, picker.entries.len() as isize - 1) as usize;
        },

        KeyCode::Home => { picker.selected = 0; picker.offset = 0; },
        KeyCode::End => { picker.selected = picker.entries.len() - 1; picker.offset = picker.entries.len(); },

        _ => {},
    }
}

//TYPING INTO A Number/Text ROW
/// Handles a key while a row is being typed into.
fn handle_edit_key(app: &mut App, key: KeyEvent)
{
    let count = app.settings.paths.len();

    match key.code
    {
        KeyCode::Esc => app.settings.edit = None,

        KeyCode::Enter => commit_edit(app),

        KeyCode::Backspace => { if let Some(edit) = app.settings.edit.as_mut() { edit.pop(); } },

        //WALK THE OFFERED PATHS
        KeyCode::Up if count > 0 => app.settings.path = (app.settings.path + count - 1) % count,
        KeyCode::Down if count > 0 => app.settings.path = (app.settings.path + 1) % count,

        //TAKE THE SELECTED ONE
        KeyCode::Tab if count > 0 =>
        {
            let chosen = app.settings.paths[app.settings.path].clone();
            app.settings.edit = Some(chosen);
        },

        KeyCode::Char(c) =>
        {
            //A NUMBER ROW ONLY TAKES DIGITS AND A LEADING -
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

    //THE LIST FOLLOWS THE TEXT
    if matches!(key.code, KeyCode::Backspace | KeyCode::Tab | KeyCode::Char(_)) { app.settings.refresh_paths(); }

    if app.settings.edit.is_none() { app.settings.paths.clear(); }
}

/// Keeps what was typed, if the row can hold it: a number must parse, and a website must be
/// `http` or `https`.
fn commit_edit(app: &mut App) //KEEP WHAT WAS TYPED, IF THE ROW CAN HOLD IT
{
    let Some(edit) = app.settings.edit.take() else { return };

    //A WEBSITE IS REFUSED WHERE IT IS TYPED, NOT AFTER THE SERVER HAS SEEN IT
    if selected_key(&app.settings) == Some("website") && !edit.trim().is_empty()
        && !misc::is_web_url(edit.trim())
    {
        app.notify(t!("profile.bad_website"));
        return;
    }

    //AN AVATAR IS CHECKED WHERE IT IS TYPED
    if selected_key(&app.settings) == Some(consts::AVATAR_KEY)
    {
        let path = edit.trim().to_string();

        //NOTHING TO DROP
        if path.is_empty() && app.settings.avatar.is_none() { return; }

        if !path.is_empty() && let Err(error) = crate::check_upload(&path, crate::Upload::Avatar)
        {
            app.notify(error);
            return;
        }

        if let Some(Row::Item(item)) = app.settings.rows.get_mut(app.settings.selected)
        {
            item.value = Value::Avatar(Some(path));
            item.changed = true;
        }

        return;
    }

    let Some(Row::Item(item)) = app.settings.rows.get_mut(app.settings.selected) else { return };

    match &item.value
    {
        //AN UNPARSEABLE NUMBER IS NOT A CHANGE
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

//THE CONFIG KEY THE SELECTED ROW OWNS
/// Returns the config key of the selected row.
fn selected_key(settings: &Settings) -> Option<&str>
{
    match settings.rows.get(settings.selected)?
    {
        Row::Item(item) => Some(item.key.as_str()),
        Row::Header(_) | Row::Action(_) => None,
    }
}

//WHAT THE SELECTED ROW HOLDS, COPIED OUT
/// What the selected row holds, copied out.
enum Selected
{
    Toggle(bool),
    Number(i64),
    Text(String),
    Action(&'static str), //WHICH BUTTON - THE SERVER ROWS HAVE TWO OF THEM
    Theme(usize),
    Language(String),

    #[cfg(feature = "client_voice")]
    Volume(String, u32),

    #[cfg(feature = "client_voice")]
    Device(String, bool),
}

/// Returns what the selected row holds.
fn selected(app: &App) -> Option<Selected>
{
    match app.settings.rows.get(app.settings.selected)?
    {
        Row::Header(_) => None,
        Row::Action(label) => Some(Selected::Action(label)),

        Row::Item(item) => Some(match &item.value
        {
            Value::Toggle { on, .. } => Selected::Toggle(*on),
            Value::Number(number) => Selected::Number(*number),
            Value::Text(text) => Selected::Text(text.clone()),
            Value::Avatar(path) => Selected::Text(path.clone().unwrap_or_default()),
            Value::Theme(index) => Selected::Theme(*index),
            Value::Language(code) => Selected::Language(code.clone()),

            #[cfg(feature = "client_voice")]
            Value::Volume(percent) => Selected::Volume(item.key.clone(), *percent),

            #[cfg(feature = "client_voice")]
            Value::Device { id, input } => Selected::Device(id.clone(), *input),
        }),
    }
}

//LEFT/RIGHT: SLIDE, FLIP, STEP OR CYCLE A ROW
/// Slides, flips, steps or cycles the selected row (Left and Right).
fn adjust(app: &mut App, direction: i32)
{
    if app.settings.readonly() { return; }

    let row = app.settings.selected;

    match selected(app)
    {
        //EITHER DIRECTION FLIPS A TOGGLE
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

        Some(Selected::Theme(index)) =>
        {
            let next = (index as isize + direction as isize).rem_euclid(theme::PALETTES.len() as isize) as usize;

            if let Some(Row::Item(item)) = app.settings.rows.get_mut(row) { item.value = Value::Theme(next); }

            config::client_write("theme", theme::PALETTES[next].id);
            app.reload_theme();
        },

        Some(Selected::Language(code)) =>
        {
            let languages = i18n::languages();

            if languages.is_empty() { return; }

            let current = languages.iter().position(|language| *language == code).unwrap_or(0);
            let next = &languages[(current as isize + direction as isize).rem_euclid(languages.len() as isize) as usize];

            config::client_write("language", next);
            i18n::set_language(next);

            //EVERY LABEL IN THE BOX AND THE PANE
            app.settings.relabel();
            app.reload_theme();
        },

        //A FREE-FORM STRING IS TYPED, NOT STEPPED
        Some(Selected::Text(_)) | Some(Selected::Action(_)) => {},

        #[cfg(feature = "client_voice")]
        Some(Selected::Volume(key, percent)) =>
        {
            let next = if direction > 0
            {
                voice_options::clamp_volume(percent.saturating_add(consts::VOLUME_STEP))
            } else
            {
                percent.saturating_sub(consts::VOLUME_STEP)
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

//ENTER/SPACE: FLIP, TYPE, PRESS OR OPEN A ROW
/// Flips, types into, presses or opens the selected row (Enter and Space).
fn activate(app: &mut App)
{
    //A PROFILE THAT IS NOT OURS CHANGES NOTHING, BUT ITS LINK STILL OPENS
    if app.settings.readonly()
    {
        if let Some(url) = app.settings.link().map(str::to_owned)
        {
            super::open_link(&url);
            app.notify(t!("tui.opening", url));
        }

        return;
    }

    let _row = app.settings.selected; //ONLY THE AUDIO ROWS NEED TO KNOW WHICH ROW THEY ARE

    match selected(app)
    {
        Some(Selected::Toggle(_)) => toggle(app),

        Some(Selected::Number(number)) => app.settings.edit = Some(number.to_string()),
        Some(Selected::Text(text)) =>
        {
            app.settings.edit = Some(text);
            app.settings.refresh_paths();
        },

        Some(Selected::Action(consts::RESTART_LABEL)) => restart(app),
        Some(Selected::Action(_)) => save(app),

        Some(Selected::Theme(_)) | Some(Selected::Language(_)) => adjust(app, 1),

        #[cfg(feature = "client_voice")]
        Some(Selected::Volume(..)) => adjust(app, 1),

        #[cfg(feature = "client_voice")]
        Some(Selected::Device(id, input)) =>
        {
            let entries = device_entries(app, input);

            app.settings.picker = Some(Picker
            {
                title: if input { t!("settings.row.input_device") } else { t!("settings.row.output_device") },
                selected: entries.iter().position(|entry| entry.id == id).unwrap_or(0),
                entries,
                row: _row,
                offset: 0,
            });
        },

        None => {},
    }
}

//ARM THE RESTART, THEN FIRE IT
/// Arms the restart, then fires it; refused while anything is unsaved.
fn restart(app: &mut App)
{
    if !app.settings.server() || app.settings.saving || app.settings.unsaved() { return; }

    //THE FIRST PRESS ONLY ARMS IT
    if !app.settings.confirm
    {
        app.settings.confirm = true;
        return;
    }

    app.settings.restart = true;

    //THE SERVER GOES DOWN WITH THIS
    app.settings.close();
}

//HAND THE EDITED ROWS TO THE EVENT LOOP
/// Hands the edited server rows to the event loop.
fn save(app: &mut App)
{
    if app.settings.profile() { return save_profile(app); }

    if !app.settings.server() { return; }

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

            //THE SERVER ALREADY KNOWS THESE
            section: String::new(),
            description: String::new(),
            restart: false,
        })
    }).collect();

    if changed.is_empty() { return; }

    //A STARTUP-ONLY KEY IS STORED LIKE ANY OTHER
    let restart: Vec<&str> = app.settings.rows.iter().filter_map(|row| match row
    {
        Row::Item(item) if item.changed && item.restart => Some(item.key.as_str()),
        _ => None,
    }).collect();

    app.settings.restart_note = (!restart.is_empty()).then(|| restart.join(", "));

    app.settings.saving = true;
    app.settings.save = Some(changed);
}

//HAND THE EDITED FIELDS TO THE EVENT LOOP
/// Hands the edited profile fields and avatar to the event loop.
fn save_profile(app: &mut App)
{
    if !app.settings.unsaved() { return; }

    let mut profile = UserProfile::default();
    let mut fields = false;

    //EVERY ROW GOES, CHANGED OR NOT - THE SERVER STORES THE WHOLE PROFILE
    for row in &app.settings.rows
    {
        let Row::Item(item) = row else { continue };

        match &item.value
        {
            Value::Text(text) =>
            {
                if let Some(field) = profile.field_mut(&item.key) { *field = text.clone(); }

                fields |= item.changed;
            },

            //THE VISIBILITY TOGGLE RIDES WITH THE PROFILE
            Value::Toggle { on, .. } if item.key == UserProfile::ONLINE_STATUS_KEY =>
            {
                profile.online_status = *on;
                fields |= item.changed;
            },

            //THE AVATAR GOES ON ITS OWN
            Value::Avatar(Some(path)) if item.changed => app.settings.avatar_save = Some(path.clone()),

            _ => {},
        }
    }

    app.settings.saving = true;

    if fields { app.settings.profile = Some(profile); }
}

/// Flips the selected toggle, writing it through.
fn toggle(app: &mut App)
{
    let deferred = app.settings.deferred();

    //FLIP THE ROW, THEN LET GO OF IT
    let changed = match app.settings.rows.get_mut(app.settings.selected)
    {
        Some(Row::Item(item)) => match &item.value
        {
            Value::Toggle { on, invert } =>
            {
                let (next, invert) = (!*on, *invert);
                item.value = Value::Toggle { on: next, invert };
                item.changed = item.changed || deferred;

                Some((item.key.clone(), next, invert))
            },

            #[allow(unreachable_patterns)] //THE OTHER VARIANTS ARE NOT TOGGLES
            _ => None,
        },

        _ => None,
    };

    let Some((key, next, invert)) = changed else { return };

    //A SERVER ROW GOES BACK OVER THE WIRE ON Save
    if deferred { return; }

    config::client_write_bool(&key, if invert { !next } else { next });

    //THE INTERFACE ROWS ARE READ THROUGH App::theme
    app.reload_theme();

    #[cfg(feature = "client_voice")]
    match key.as_str()
    {
        "noise_suppression" => voice_options::set_noise_suppression(next),
        "automatic_gain" => voice_options::set_automatic_gain(next),
        _ => {},
    }
}

/// Stores a device choice and signals the voice session to rebuild its streams.
#[cfg(feature = "client_voice")]
fn set_device(app: &mut App, row: usize, chosen: String)
{
    let Some(Row::Item(item)) = app.settings.rows.get_mut(row) else { return };

    let Value::Device { id, input } = &item.value else { return };

    if *id == chosen { return; }

    let (key, input) = (item.key.clone(), *input);
    item.value = Value::Device { id: chosen.clone(), input };

    config::client_write(&key, &chosen);

    //A RUNNING VOICE SESSION REBUILDS ITS STREAMS
    voice_options::mark_devices_changed();
}

/// Stores a device choice; nothing to do without voice.
#[cfg(not(feature = "client_voice"))]
fn set_device(_app: &mut App, _row: usize, _chosen: String) {}

//THE SYSTEM DEFAULT PLUS EVERY REPORTED DEVICE
/// Returns the system default plus every reported device.
#[cfg(feature = "client_voice")]
fn device_entries(app: &App, input: bool) -> Vec<DeviceEntry>
{
    let devices = if input { &app.settings.devices.input } else { &app.settings.devices.output };

    let mut entries = vec![DeviceEntry { id: String::new(), label: t!("settings.default_device").to_owned() }];
    entries.extend(devices.iter().cloned());

    //A CONFIGURED BUT UNPLUGGED DEVICE KEEPS ITS ROW
    let configured = config::read_config::<String>(if input { "input_device" } else { "output_device" });
    if !configured.is_empty() && !entries.iter().any(|entry| entry.id == configured)
    {
        entries.push(DeviceEntry { label: configured.clone(), id: configured });
    }

    entries
}

/// Applies a volume to the running audio streams.
#[cfg(feature = "client_voice")]
fn apply_volume(key: &str, percent: u32) //LIVE-UPDATE THE RUNNING AUDIO STREAMS
{
    match key
    {
        "input_volume" => voice_options::set_input_volume(percent),
        "output_volume" => voice_options::set_output_volume(percent),

        #[cfg(feature = "client_screen")]
        "screen_volume" => screen_options::set_screen_volume(percent),
        _ => {},
    }
}
