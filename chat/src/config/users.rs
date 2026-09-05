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
    consts,
    role::Role,
};

fn write_user_field(username: &str, key: &str, value: Value) //WRITE ONE FIELD OF username TO server_users.toml
{
    super::with_cached_mut(&super::config_path(consts::SERVER_USERS_CONFIG), |doc|
    {
        let users = doc.as_table_mut();

        //A MISSING OR LEGACY FLAT ENTRY BECOMES AN EMPTY SUBTABLE FIRST
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
    super::get_data(&super::config_path(consts::SERVER_USERS_CONFIG)).get(username)?
        .as_table_like()?.get("password")?.as_str().map(str::to_string)
}

pub fn role(username: &str) -> Option<Role> //RETURN ROLE OF username
{
    super::get_data(&super::config_path(consts::SERVER_USERS_CONFIG)).get(username)?
        .as_table_like()?.get("role")?.as_str()?.parse().ok()
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

    first_user
}

pub fn contains(key: &str) -> bool //CHECK IF server_users.toml contains
{
    super::get_data(&super::config_path(consts::SERVER_USERS_CONFIG)).get(key).is_some()
}
