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
    Table,
    Value,
};

use crate::
{
    colors,
    consts,
    misc,
    role::Role,
    network::codes::
    {
        MessageColors,
        UserProfile,
    },
};

const COLOR_KEYS: [&str; 2] = ["username_color", "message_color"]; //THE COLORS AS server_users.toml SPELLS THEM
const PROFILE_TABLE: &str = "profile";        //THE SUBTABLE A PROFILE SITS IN, APART FROM THE CREDENTIALS
const PROFILE_KEYS: [&str; 4] = UserProfile::KEYS; //THE PROFILE AS server_users.toml SPELLS IT
const AVATAR_KEY: &str = "avatar";            //THE PICTURE'S HASH, WHICH IS NOT A TYPED FIELD

fn user_field(username: &str, key: &str) -> Option<String> //READ ONE FIELD OF username
{
    super::with_cached(&super::config_path(consts::SERVER_USERS_CONFIG), |users| users.get(username)?
        .as_table_like()?.get(key)?.as_str().map(str::to_string))
}

fn profile_field(username: &str, key: &str) -> Option<String> //READ ONE PROFILE FIELD OF username
{
    super::with_cached(&super::config_path(consts::SERVER_USERS_CONFIG), |users| users.get(username)?
        .as_table_like()?.get(PROFILE_TABLE)?.as_table_like()?.get(key)?.as_str().map(str::to_string))
}

fn write_user_field(username: &str, key: &str, value: Value) //WRITE ONE FIELD OF username TO server_users.toml
{
    super::with_cached_mut(&super::config_path(consts::SERVER_USERS_CONFIG), |doc|
    {
        let users = doc.as_table_mut();

        //A MISSING ENTRY BECOMES A SUBTABLE
        if users.get(username).and_then(Item::as_table_like).is_none()
        {
            users.insert(username, Item::Table(Table::new()));
        }

        users.get_mut(username).and_then(Item::as_table_like_mut)
            .expect("User entry is not a table").insert(key, Item::Value(value));
    });
}

fn write_profile_field(username: &str, key: &str, value: Value) //WRITE ONE PROFILE FIELD TO server_users.toml
{
    super::with_cached_mut(&super::config_path(consts::SERVER_USERS_CONFIG), |doc|
    {
        let users = doc.as_table_mut();

        //A MISSING ENTRY BECOMES A SUBTABLE
        if users.get(username).and_then(Item::as_table_like).is_none()
        {
            users.insert(username, Item::Table(Table::new()));
        }

        let user = users.get_mut(username).and_then(Item::as_table_like_mut).expect("User entry is not a table");

        //AND SO DOES A MISSING PROFILE
        if user.get(PROFILE_TABLE).and_then(Item::as_table_like).is_none()
        {
            user.insert(PROFILE_TABLE, Item::Table(Table::new()));
        }

        user.get_mut(PROFILE_TABLE).and_then(Item::as_table_like_mut)
            .expect("Profile entry is not a table").insert(key, Item::Value(value));
    });
}

pub fn len() -> usize //COUNT USERS
{
    super::with_cached(&super::config_path(consts::SERVER_USERS_CONFIG), |users| users.len())
}

pub fn all() -> Vec<String> //RETURN EVERY REGISTERED USERNAME
{
    super::with_cached(&super::config_path(consts::SERVER_USERS_CONFIG), |users| users.iter()
        .map(|(username, _)| username.to_string()).collect())
}

pub fn password(username: &str) -> Option<String> //RETURN PASSWORD HASH OF username
{
    user_field(username, "password")
}

pub fn role(username: &str) -> Option<Role> //RETURN ROLE OF username
{
    user_field(username, "role")?.parse().ok()
}

pub fn colors(username: &str) -> MessageColors //RETURN COLORS OF username
{
    let mut codes = COLOR_KEYS.iter().map(|key| user_field(username, key).as_deref().and_then(colors::code));

    MessageColors
    {
        username_color: codes.next().flatten(),
        message_color: codes.next().flatten(),
    }
}

pub fn profile(username: &str) -> UserProfile //RETURN PROFILE OF username
{
    let mut fields = PROFILE_KEYS.iter().map(|key| profile_field(username, key).unwrap_or_default());

    UserProfile
    {
        bio: fields.next().unwrap_or_default(),
        pronouns: fields.next().unwrap_or_default(),
        website: fields.next().unwrap_or_default(),
        status: fields.next().unwrap_or_default(),
        avatar: avatar(username),
        role: role(username).unwrap_or_default(),
    }
}

pub fn avatar(username: &str) -> Option<[u8; 32]> //RETURN username's PICTURE
{
    misc::unhex(&profile_field(username, AVATAR_KEY)?)
}

pub fn set_avatar(username: &str, hash: Option<&[u8; 32]>) //STORE username's PICTURE
{
    write_profile_field(username, AVATAR_KEY, hash.map(|hash| misc::hex(hash)).unwrap_or_default().into());
}

pub fn names_avatar(hash: &[u8; 32]) -> bool //DOES ANY ACCOUNT NAME THIS PICTURE?
{
    all().iter().any(|username| avatar(username).as_ref() == Some(hash))
}

pub fn avatars() -> Vec<[u8; 32]> //EVERY PICTURE THE ACCOUNTS NAME
{
    all().iter().filter_map(|username| avatar(username)).collect()
}

//STORE username's TYPED FIELDS - THE PICTURE IS SET BY AN UPLOAD, NOT BY A SAVE
pub fn set_profile(username: &str, profile: &UserProfile)
{
    for (key, value) in PROFILE_KEYS.iter().zip(profile.fields())
    {
        write_profile_field(username, key, value.into());
    }
}

//STORE ONE OF username's COLORS, BY NAME
pub fn set_color(username: &str, username_color: bool, code: u8)
{
    let key = COLOR_KEYS[usize::from(!username_color)];

    write_user_field(username, key, colors::name(Some(code)).into());
}

pub fn set_role(username: &str, role: Role) //STORE A NEW ROLE FOR username
{
    write_user_field(username, "role", role.name().into());
}

pub fn set_password(username: &str, hash: &str) //SET USER PASSWORD
{
    write_user_field(username, "password", hash.into());
}

pub fn add(username: &str, hash: &str) -> bool //CREATE NEW USER, RETURN TRUE ON FIRST USER
{
    let first_user = len() == 0; //SELF-EXPLANATORY, INNIT?

    set_password(username, hash); //PASSWORD
    set_role(username, if first_user { Role::Owner } else { Role::User }); //ROLE (OWNER IF THIS IS THE FIRST USER)

    //NO COLORS OR PROFILE YET, BUT THE KEYS ARE THERE
    for key in COLOR_KEYS { write_user_field(username, key, colors::NONE.into()); }
    for key in PROFILE_KEYS { write_profile_field(username, key, "".into()); }
    write_profile_field(username, AVATAR_KEY, "".into());

    first_user
}

pub fn remove(username: &str) //DELETE username's ENTRY
{
    super::with_cached_mut(&super::config_path(consts::SERVER_USERS_CONFIG), |doc| { doc.as_table_mut().remove(username); });
}

pub fn contains(key: &str) -> bool //CHECK IF server_users.toml contains
{
    super::with_cached(&super::config_path(consts::SERVER_USERS_CONFIG), |users| users.get(key).is_some())
}
