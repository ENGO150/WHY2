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
    Display,
    Formatter,
    Result,
};

use crate::chat::network::MessageCode;

//CONSTS
const COMMAND_PREFIX: &str = "/"; //PREFIX FOR COMMANDS

//ENUMS
pub enum Command
{
    Exit,           //DISCONNECT FROM SERVER
    Channel,        //SWITCH CHANNEL
    Help,           //PRINT COMMANDS
    List,           //LIST USERS
    PrivateMessage, //ONE TO ONE MESSAGE
    UsernameColor,  //SET COLOR OF USERNAME
    MessageColor,   //SET COLOR OF MESSAGE
    Invalid,        //INVALID COMMAND
}

//IMPLEMENTATIONS
impl Command
{
    //GET CODE MATCHING TO COMMAND
    pub fn to_code(&self) -> Option<MessageCode>
    {
        match self
        {
            Command::Exit => Some(MessageCode::Disconnect),
            Command::Channel => Some(MessageCode::Channel),
            Command::List => Some(MessageCode::List),
            Command::PrivateMessage => Some(MessageCode::PrivateMessage),

            _ => None,
        }
    }
}

impl Display for Command
{
    //Command TO STRING
    fn fmt(&self, f: &mut Formatter<'_>) -> Result
    {
        let name = match self
        {
            Command::Help           => "help",
            Command::Channel        => "channel",
            Command::Exit           => "exit",
            Command::List           => "list",
            Command::PrivateMessage => "pm",
            Command::UsernameColor  => "ucolor",
            Command::MessageColor   => "color",
            Command::Invalid        => "",
        };

        write!(f, "{}{}", COMMAND_PREFIX, name)
    }
}

pub fn get_command(input: &str) -> (Option<Command>, Option<String>) //GET COMMAND + PARAMETERS FROM STRING
{
    //input DOESN'T START WITH PREFIX, NO COMMAND
    if !input.starts_with(COMMAND_PREFIX) { return (None, None); }

    //SPLIT input TO COMMAND AND PARAMETERS
    let no_prefix = &input[COMMAND_PREFIX.len()..]; //EXTRACT COMMAND WITHOUT PREFIX (IN UPPERCASE)
    let (command, parameters) = match no_prefix.split_once(' ') //EXTRACT POSSIBLE PARAMETERS
    {
        Some((command, parameters)) => (command.to_ascii_uppercase(), Some(parameters.trim().to_string())),
        None => (no_prefix.to_ascii_uppercase(), None)
    };

    //COMPARE COMMANDS
    match command.as_str()
    {
        //NON PARAMETRIC
        "EXIT" | "QUIT" | "LEAVE" | "DISCONNECT"      => (Some(Command::Exit), None),
        "HELP" | "H" | "COMMANDS" | "USAGE" | "GUIDE" => (Some(Command::Help), None),
        "LIST" | "USERS" | "CLIENTS" | "SHOW"         => (Some(Command::List), None),

        //PARAMETRIC
        "CHANNEL" | "SWITCH" | "CHECKOUT" | "AREA"    => (Some(Command::Channel), parameters),
        "PM" | "DM" | "MSG" | "TELL"                  => (Some(Command::PrivateMessage), parameters),
        "UCOLOR" | "USERNAME"                         => (Some(Command::UsernameColor), parameters),
        "COLOR" | "MESSAGE"                           => (Some(Command::MessageColor), parameters),

        _ => (Some(Command::Invalid), None)
    }
}
