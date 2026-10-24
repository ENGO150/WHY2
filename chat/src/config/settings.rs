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

//! # Remote Server Settings
//!
//! Reads and writes `server.toml` on behalf of a client editing it remotely.
//!
//! Nothing here names a setting: every key is read straight out of the file, together with the
//! `#` heading above it and its trailing comment, so a key added to the default configuration
//! shows up in the client with no code change. Writes accept only keys the file already has,
//! with the datatype they already have.

use toml_edit::
{
    Item,
    RawString,
    Value,
};

use crate::
{
    consts,
    network::codes::
    {
        ServerSetting,
        SettingValue,
    },
};

//THE HEADING A KEY SITS UNDER
/// Returns the last `#` heading in a key's leading decoration, if the block was not interrupted
/// by a blank line.
fn heading(prefix: &str) -> Option<String>
{
    let mut heading = None;

    for line in prefix.lines()
    {
        let line = line.trim();

        if line.is_empty() { heading = None; }
        else if let Some(comment) = line.strip_prefix('#') { heading = Some(comment.trim().to_string()); }
    }

    heading
}

//EVERY KEY OF server.toml AS THE CLIENT EDITS IT
/// Returns every key of `server.toml` as the client edits it.
///
/// Each entry carries its value, the section heading it sits under, its trailing comment as a
/// description, and whether it only applies after a restart
/// ([`SERVER_RESTART_SETTINGS`](consts::SERVER_RESTART_SETTINGS)).
pub fn all() -> Vec<ServerSetting>
{
    super::with_cached(&super::config_path(consts::SERVER_CONFIG), |data|
    {
        let table = data.as_table();

        let mut settings = Vec::new();
        let mut section = String::new();

        for (key, item) in table.iter()
        {
            //A DATATYPE THE CONFIG READER DOES NOT UNDERSTAND
            let Some(value) = item.as_value() else { continue };

            //THE HEADING CARRIES DOWN TO THE NEXT ONE
            if let Some(prefix) = table.key(key).and_then(|key| key.leaf_decor().prefix()).and_then(RawString::as_str)
                && let Some(found) = heading(prefix)
            {
                section = found;
            }

            let description = value.decor().suffix().and_then(RawString::as_str)
                .map(|comment| comment.trim().trim_start_matches('#').trim().to_string()).unwrap_or_default();

            settings.push(ServerSetting
            {
                key: key.to_string(),
                value: match value
                {
                    Value::Boolean(on) => SettingValue::Toggle(*on.value()),
                    Value::Integer(number) => SettingValue::Number(*number.value()),
                    Value::String(text) => SettingValue::Text(text.value().clone()),

                    _ => continue,
                },
                section: section.clone(),
                description,

                //THESE ARE STORED BUT NOT USED UNTIL A RESTART
                restart: consts::SERVER_RESTART_SETTINGS.contains(&key),
            });
        }

        settings
    })
}

//STORE THE CLIENT'S ROWS, DROPPING UNKNOWN ONES
/// Stores the client's edited settings.
///
/// A key the file does not have, or a value of a different datatype than the current one, is
/// dropped. All accepted values are written in a single pass.
///
/// # Returns
/// The number of settings that were stored.
pub fn write(settings: &[ServerSetting]) -> usize
{
    let accepted: Vec<(&str, Value)> = super::with_cached(&super::config_path(consts::SERVER_CONFIG),
        |data| settings.iter().filter_map(|setting|
    {
        let current = data.get(&setting.key).and_then(Item::as_value)?;

        let value: Value = match (&setting.value, current)
        {
            (SettingValue::Toggle(on), Value::Boolean(_)) => (*on).into(),
            (SettingValue::Number(number), Value::Integer(_)) => (*number).into(),
            (SettingValue::Text(text), Value::String(_)) => text.as_str().into(),

            _ => return None,
        };

        Some((setting.key.as_str(), value))
    }).collect());

    //ONE PASS, SO THE FILE IS REWRITTEN ONCE
    super::with_cached_mut(&super::config_path(consts::SERVER_CONFIG), |doc|
    {
        for (key, value) in &accepted { super::set_value(doc.as_table_mut(), key, value.clone()); }
    });

    accepted.len()
}
