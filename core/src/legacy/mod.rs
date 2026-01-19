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

//! # WHY2 Legacy
//!
//! ## Deprecation Notice
//! This module is deprecated and retained only for refence and legacy compatibility.
//! These parts represent early versions of the WHY2 encryption routines that are now considered insecure.
//!
//! Due to identified security concerns, these legacy modules should **not be used in production**.
//!
//! For secure applications, always use [`rex`](crate)—the current and trusted implementation of the WHY2 encryption engine.

pub mod crypto;
pub mod decrypter;
pub mod encrypter;
pub mod options;
