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

use std::fmt::
{
    self,
    Display,
    Result,
};

//CONSTS
const COMMAND_PREFIX: &str = "/";  //PREFIX FOR COMMANDS

//ENUMS
pub enum Command
{
    Exit,
    Help,
}

//IMPLEMENTATIONS
impl Display for Command
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result
    {
        let name = match self
        {
            Command::Help => "help",
            Command::Exit => "exit",
        };

        write!(f, "{}{}", COMMAND_PREFIX, name)
    }
}

pub fn get_command(input: &str) -> (Option<Command>, Option<String>)
{
    //input DOESN'T START WITH PREFIX, NO COMMAND
    if !input.starts_with(COMMAND_PREFIX) { return (None, None); }

    //COMPARE COMMANDS
    match &input[1..]
    {
        "EXIT" | "QUIT" | "LEAVE" => (Some(Command::Exit), None),
        //Command::Help | "H" | "COMMANDS" | "USAGE" | "GUIDE" => (None, Some(Command::Help)),

        _ => (None, None)
    }
}
