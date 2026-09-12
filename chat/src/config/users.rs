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
    role::Role,
    network::codes::MessageColors,
};

const COLOR_KEYS: [&str; 2] = ["username_color", "message_color"]; //THE COLORS AS server_users.toml SPELLS THEM

fn user_field(username: &str, key: &str) -> Option<String> //READ ONE FIELD OF username
{
    super::get_data(&super::config_path(consts::SERVER_USERS_CONFIG)).get(username)?
        .as_table_like()?.get(key)?.as_str().map(str::to_string)
}

fn write_user_field(username: &str, key: &str, value: Value) //WRITE ONE FIELD OF username TO server_users.toml
{
    super::with_cached_mut(&super::config_path(consts::SERVER_USERS_CONFIG), |doc|
    {
        let users = doc.as_table_mut();

        //A MISSING OR FLAT ENTRY BECOMES A SUBTABLE
        if users.get(username).and_then(Item::as_table_like).is_none()
        {
            users.insert(username, Item::Table(Table::new()));
        }

        users.get_mut(username).and_then(Item::as_table_like_mut)
            .expect("User entry is not a table").insert(key, Item::Value(value));
    });
}

pub fn len() -> usize //COUNT USERS
{
    super::get_data(&super::config_path(consts::SERVER_USERS_CONFIG)).len()
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

pub fn add(username: &str, hash: &str) -> bool //CREATE NEW USER, RETURN TRUE ON FIRST USER
{
    let first_user = len() == 0; //SELF-EXPLANATORY, INNIT?

    write_user_field(username, "password", hash.into()); //PASSWORD
    set_role(username, if first_user { Role::Owner } else { Role::User }); //ROLE (OWNER IF THIS IS THE FIRST USER)

    //NO COLORS YET, BUT THE KEYS ARE THERE
    for key in COLOR_KEYS { write_user_field(username, key, colors::NONE.into()); }

    first_user
}

pub fn contains(key: &str) -> bool //CHECK IF server_users.toml contains
{
    super::get_data(&super::config_path(consts::SERVER_USERS_CONFIG)).get(key).is_some()
}

pub fn migrate() //MIGRATE COLORS (will be removed with next version bump)
{
    super::with_cached_mut(&super::config_path(consts::SERVER_USERS_CONFIG), |doc|
    {
        for (_, entry) in doc.as_table_mut().iter_mut()
        {
            let Some(user) = entry.as_table_like_mut() else { continue };

            for key in COLOR_KEYS
            {
                if user.get(key).is_none() { user.insert(key, Item::Value(colors::NONE.into())); }
            }
        }
    });
}
