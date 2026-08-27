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

use std::net::IpAddr;

use toml_edit::
{
    Array,
    DocumentMut,
    Item,
    Value,
};

use crate::
{
    consts,
    network::codes::BanEntry,
};

//ADD ONE SUBJECT TO A LIST OF server_bans.toml. USERNAMES AND ADDRESSES GET A LIST OF THEIR OWN SO A
//USERNAME THAT LOOKS LIKE AN ADDRESS - OR THE OTHER WAY AROUND - CANNOT BAN THE WRONG SUBJECT
fn set_ban(doc: &mut DocumentMut, section: &str, key: &str)
{
    //A MISSING LIST BECOMES AN EMPTY ONE FIRST
    if doc.get(section).and_then(Item::as_array).is_none()
    {
        doc.insert(section, Item::Value(Value::Array(Array::new())));
    }

    let bans = doc.get_mut(section).and_then(Item::as_array_mut).expect("Ban list is not an array");

    //BEING ON THE LIST IS THE WHOLE BAN, SO BANNING TWICE MUST NOT LIST THE SUBJECT TWICE
    if !bans.iter().any(|ban| ban.as_str() == Some(key)) { bans.push(key); }
}

fn unset_ban(doc: &mut DocumentMut, section: &str, id: usize) -> bool //REMOVES BAN
{
    let Some(bans) = doc.get_mut(section).and_then(Item::as_array_mut) else { return false };
    if id >= bans.len() { return false; }

    bans.remove(id);
    true
}

fn ban_list(section: &str) -> Vec<BanEntry> //EVERY SUBJECT ON A LIST OF server_bans.toml, NUMBERED
{
    super::get_data(&super::config_path(consts::SERVER_BANS_CONFIG)).get(section)
        .and_then(Item::as_array)
        .map(|bans| bans.iter().enumerate()
            .filter_map(|(id, ban)| Some(BanEntry { id, subject: ban.as_str()?.to_string() }))
            .collect())
        .unwrap_or_default()
}

fn listed(section: &str, key: &str) -> bool //IS key ON A LIST OF server_bans.toml?
{
    super::get_data(&super::config_path(consts::SERVER_BANS_CONFIG)).get(section)
        .and_then(Item::as_array)
        .is_some_and(|bans| bans.iter().any(|ban| ban.as_str() == Some(key)))
}

pub fn banned(username: &str) -> bool //IS username BANNED?
{
    listed("user", username)
}

pub fn banned_ip(ip: &IpAddr) -> bool //IS ip BANNED?
{
    listed("ip", &ip.to_string())
}

pub fn ban(username: &str) //BAN username
{
    super::with_cached_mut(&super::config_path(consts::SERVER_BANS_CONFIG), |doc| set_ban(doc, "user", username));
}

pub fn ban_ip(ip: &IpAddr) //BAN ip
{
    super::with_cached_mut(&super::config_path(consts::SERVER_BANS_CONFIG), |doc| set_ban(doc, "ip", &ip.to_string()));
}

pub fn users() -> Vec<BanEntry> //EVERY BANNED USERNAME
{
    ban_list("user")
}

pub fn ips() -> Vec<BanEntry> //EVERY BANNED ADDRESS
{
    ban_list("ip")
}

pub fn pardon(id: usize) -> bool //LIFT THE USERNAME BAN NUMBERED id
{
    let mut pardoned = false;
    super::with_cached_mut(&super::config_path(consts::SERVER_BANS_CONFIG), |doc| pardoned = unset_ban(doc, "user", id));

    pardoned
}

pub fn pardon_ip(id: usize) -> bool //LIFT THE ADDRESS BAN NUMBERED id
{
    let mut pardoned = false;
    super::with_cached_mut(&super::config_path(consts::SERVER_BANS_CONFIG), |doc| pardoned = unset_ban(doc, "ip", id));

    pardoned
}
