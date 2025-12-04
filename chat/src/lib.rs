/*
This is part of WHY2
Copyright (C) 2022-2025 Václav Šmejkal

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

pub use why2_core;

#[cfg(feature = "chat")]
#[doc(hidden)]
pub mod command;

#[cfg(feature = "chat")]
#[doc(hidden)]
pub mod config;

#[cfg(feature = "chat")]
#[doc(hidden)]
pub mod crypto;

#[cfg(feature = "chat")]
#[doc(hidden)]
pub mod misc;

#[cfg(feature = "chat")]
#[doc(hidden)]
pub mod network;

#[cfg(feature = "chat")]
#[doc(hidden)]
pub mod options;

#[cfg(feature = "chat")]
pub mod chat
{
    pub use super::command;
    pub use super::config;
    pub use super::crypto;
    pub use super::misc;
    pub use super::network;
    pub use super::options;
}
