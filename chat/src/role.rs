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

use std::
{
    result,
    str::FromStr,
    fmt::
    {
        Display,
        Formatter,
        Result,
    },
};

use wincode::{ SchemaWrite, SchemaRead };

//MACROS
macro_rules! roles
{
    ($($variant:ident => $name:literal,)+) =>
    {
        //THE ORDER *IS* THE PERMISSION CHECK: EVERY GATE IN THE PROTOCOL IS `role >= Role::Something`,
        //SO Ord IS DERIVED FROM THE DECLARATION ORDER AND THE VARIANTS RUN LOWEST RANK FIRST
        #[derive(SchemaWrite, SchemaRead, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
        pub enum Role
        {
            #[default] $($variant),+
        }

        impl Role
        {
            pub const ALL: &'static [Role] = &[$(Role::$variant),+]; //EVERY RANK, LOWEST FIRST

            pub fn name(&self) -> &'static str //THE NAME THIS RANK IS TYPED, STORED AND SHOWN BY
            {
                match self
                {
                    $(Role::$variant => $name),+
                }
            }
        }
    };
}

//THE RANKS THEMSELVES, LOWEST FIRST
roles!
{
    User      => "user",      //WHAT REGISTERING GETS YOU
    Moderator => "moderator", //MUTE AND KICK
    Owner     => "owner",     //BANS, THE SERVER CONFIG, AND HANDING OUT RANKS
}

//IMPLEMENTATIONS
impl Display for Role
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result { write!(f, "{}", self.name()) }
}

impl FromStr for Role
{
    type Err = ();

    //A RANK IS ITS NAME EVERYWHERE IT IS READ - TYPED INTO /server role, AND STORED IN server_users.toml
    fn from_str(text: &str) -> result::Result<Self, Self::Err>
    {
        Self::ALL.iter().find(|role| role.name().eq_ignore_ascii_case(text.trim())).copied().ok_or(())
    }
}
