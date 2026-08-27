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

//THE HEADING A KEY SITS UNDER - THE LAST COMMENT BLOCK ABOVE IT. THE LICENSE BLOCK AT THE TOP OF THE FILE
//IS NOT ONE: IT IS SEPARATED FROM THE FIRST KEY BY A BLANK LINE, WHICH IS WHAT STARTS THE BLOCK OVER
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

//EVERY KEY OF server.toml AS THE CLIENT EDITS IT. THE FILE ITSELF IS THE LIST - NOTHING HERE NAMES A KEY,
//SO A KEY ADDED TO THE DEFAULT CONFIG SHOWS UP IN THE OVERLAY WITHOUT ANY FURTHER WORK
pub fn all() -> Vec<ServerSetting>
{
    let data = super::get_data(&super::config_path(consts::SERVER_CONFIG));
    let table = data.as_table();

    let mut settings = Vec::new();
    let mut section = String::new();

    for (key, item) in table.iter()
    {
        //A KEY OF A DATATYPE THE CONFIG READER DOES NOT UNDERSTAND HAS NO ROW TO BE EDITED IN
        let Some(value) = item.as_value() else { continue };

        //THE HEADING CARRIES DOWN THE FILE UNTIL THE NEXT ONE
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

            //SAVING ONE OF THESE STORES IT, AND THE RUNNING SERVER GOES ON USING WHAT IT READ AT STARTUP
            restart: consts::SERVER_RESTART_SETTINGS.contains(&key),
        });
    }

    settings
}

//STORE WHAT THE CLIENT SENT BACK, RETURNING HOW MANY ROWS WERE ACCEPTED. A KEY THE CONFIG DOES NOT ALREADY
//HAVE, OR ONE THAT COMES BACK AS A DIFFERENT DATATYPE, IS DROPPED - THE CLIENT DOES NOT GET TO INVENT KEYS
pub fn write(settings: &[ServerSetting]) -> usize
{
    let data = super::get_data(&super::config_path(consts::SERVER_CONFIG));

    let accepted: Vec<(&str, Value)> = settings.iter().filter_map(|setting|
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
    }).collect();

    //ONE PASS OVER THE DOCUMENT, SO THE FILE IS REWRITTEN ONCE NO MATTER HOW MANY ROWS CHANGED
    super::with_cached_mut(&super::config_path(consts::SERVER_CONFIG), |doc|
    {
        for (key, value) in &accepted { super::set_value(doc.as_table_mut(), key, value.clone()); }
    });

    accepted.len()
}
