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

//! # Server Roles
//!
//! [`Role`] is a user's rank on a server. The variants are declared lowest first and derive
//! [`Ord`] from that order, so every permission check is a plain comparison
//! (`role >= Role::Moderator`).
//!
//! The variants, their names and [`Role::ALL`] are generated together by the `roles!` macro,
//! so adding a rank is one line and the three cannot drift apart.

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
/// Generates [`Role`], its names and [`Role::ALL`] from one list.
macro_rules! roles
{
    ($($variant:ident => $name:literal,)+) =>
    {
        //THE ORDER IS THE PERMISSION CHECK
        /// A user's rank on the server, ordered lowest first.
        ///
        /// - `User` — what registering gets you.
        /// - `Moderator` — may mute and kick.
        /// - `Owner` — may ban, edit the server configuration and hand out ranks.
        ///
        /// A role crosses the wire as itself and is typed, stored and shown by [`Role::name`].
        #[derive(SchemaWrite, SchemaRead, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
        pub enum Role
        {
            #[default] $($variant),+
        }

        impl Role
        {
            /// Every rank, lowest first.
            pub const ALL: &'static [Role] = &[$(Role::$variant),+]; //EVERY RANK, LOWEST FIRST

            /// Returns the name this rank is typed, stored and shown by.
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

    //A RANK IS ITS NAME EVERYWHERE IT IS READ
    fn from_str(text: &str) -> result::Result<Self, Self::Err>
    {
        Self::ALL.iter().find(|role| role.name().eq_ignore_ascii_case(text.trim())).copied().ok_or(())
    }
}
