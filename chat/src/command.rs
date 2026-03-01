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

use std::fmt::
{
    Display,
    Formatter,
    Result,
};

use crate::network::MessageCode;

#[cfg(feature = "client")]
use std::net::TcpStream;

#[cfg(feature = "client")]
use crate::
{
    options,
    network::{ self, MessagePacket },
};

//ENUMS
#[derive(Clone, PartialEq)]
pub enum Command
{
    Exit,           //DISCONNECT FROM SERVER
    Voice,          //ENABLE VOICE CHAT
    Mute,           //TOGGLE-MUTE USER/YOURSELF
    Channel,        //SWITCH CHANNEL
    Help,           //PRINT COMMANDS
    Info,           //COMMAND INFO
    List,           //LIST USERS
    Files,          //LIST FILES
    Upload,         //UPLOAD FILE TO SERVER
    Download,       //DOWNLOAD FILE FROM SERVER
    PrivateMessage, //ONE TO ONE MESSAGE
    UsernameColor,  //SET COLOR OF USERNAME
    MessageColor,   //SET COLOR OF MESSAGE
    Invalid,        //INVALID COMMAND
}

//STRUCTS
pub struct CommandArg //COMMAND PARAMETER
{
    pub name: &'static str,
    pub required: bool,
}

pub struct CommandInfo //COMMAND INFO
{
    pub command: Command,
    pub triggers: &'static [&'static str],
    pub shortcut: Option<char>,
    pub args: &'static [CommandArg],
    pub description: &'static str,
}

pub const COMMAND_LIST: &[CommandInfo] =
&[
    CommandInfo
    {
        command: Command::Help,
        triggers: &[ "HELP", "H", "COMMANDS", "USAGE", "GUIDE" ],
        shortcut: Some('h'),
        args: &[],
        description: "Prints all available commands",
    },

    CommandInfo
    {
        command: Command::Info,
        triggers: &[ "INFO", "COMMAND", "MAN" ],
        shortcut: None,
        args: &[CommandArg { name: "COMMAND", required: true }],
        description: "Shows command info",
    },

    CommandInfo
    {
        command: Command::Voice,
        triggers: &[ "VOICE", "VOIP", "CALL" ],
        shortcut: None,
        args: &[],
        description: "Toggles voice chat",
    },

    CommandInfo
    {
        command: Command::Mute,
        triggers: &[ "MUTE", "UNMUTE", "SILENCE", "STFU" ],
        shortcut: Some('s'),
        args: &[CommandArg { name: "ID", required: false }],
        description: "Toggle-mutes user/yourself",
    },

    CommandInfo
    {
        command: Command::Channel,
        triggers: &[ "CHANNEL", "SWITCH", "CHECKOUT", "AREA" ],
        shortcut: None,
        args: &[CommandArg { name: "NAME", required: false }],
        description: "Switches to channel/lobby if NAME is omitted",
    },

    CommandInfo
    {
        command: Command::Upload,
        triggers: &[ "UPLOAD", "FILEUP", "PUSH", "UP" ],
        shortcut: None,
        args: &[CommandArg { name: "PATH", required: true }],
        description: "Uploads file to server",
    },

    CommandInfo
    {
        command: Command::Download,
        triggers: &[ "DOWNLOAD", "FILEDOWN", "PULL", "DOWN", "FETCH" ],
        shortcut: None,
        args:
        &[
            CommandArg { name: "USER ID", required: true },
            CommandArg { name: "FILE ID", required: true },
        ],
        description: "Downloads file from server",
    },

    CommandInfo
    {
        command: Command::List,
        triggers: &[ "LIST", "USERS", "CLIENTS", "CHANNELS", "IDS", "ID" ],
        shortcut: Some('l'),
        args: &[],
        description: "Shows connected users and their IDs",
    },

    CommandInfo
    {
        command: Command::Files,
        triggers: &[ "FILES", "LISTFILES", "UPLOADS", "DOWNLOADS", "SHARES" ],
        shortcut: Some('u'),
        args: &[],
        description: "Shows available files and their IDs",
    },

    CommandInfo
    {
        command: Command::PrivateMessage,
        triggers: &[ "PM", "DM", "MSG", "TELL" ],
        shortcut: None,
        args:
        &[
            CommandArg { name: "ID", required: true },
            CommandArg { name: "MESSAGE", required: true },
        ],
        description: "Sends private message",
    },

    CommandInfo
    {
        command: Command::UsernameColor,
        triggers: &[ "UCOLOR", "USERNAME" ],
        shortcut: None,
        args: &[CommandArg { name: "COLOR", required: true }],
        description: "Sets color of username",
    },

    CommandInfo
    {
        command: Command::MessageColor,
        triggers: &[ "COLOR", "MESSAGE" ],
        shortcut: None,
        args: &[CommandArg { name: "COLOR", required: true }],
        description: "Sets color of message",
    },

    CommandInfo
    {
        command: Command::Exit,
        triggers: &[ "EXIT", "LEAVE", "QUIT", "DISCONNECT" ],
        shortcut: Some('c'),
        args: &[],
        description: "Disconnects from the server",
    },
];

//CONSTS
pub const COMMAND_PREFIX: &str = "/"; //PREFIX FOR COMMANDS

//IMPLEMENTATIONS
impl Command
{
    //GET CODE MATCHING TO COMMAND
    pub fn to_code(&self) -> Option<MessageCode>
    {
        match self
        {
            Command::Exit => Some(MessageCode::Disconnect),
            Command::Voice => Some(MessageCode::Voice),
            Command::Channel => Some(MessageCode::Channel),
            Command::List => Some(MessageCode::List),
            Command::PrivateMessage => Some(MessageCode::PrivateMessage),
            Command::Upload => Some(MessageCode::Upload),
            Command::Download => Some(MessageCode::Download),
            Command::Files => Some(MessageCode::Files),

            _ => None,
        }
    }
}

impl Display for Command
{
    //Command TO STRING
    fn fmt(&self, f: &mut Formatter<'_>) -> Result
    {
        let name = COMMAND_LIST.iter()
            .find(|info| info.command == *self)
            .map(|info| info.triggers[0].to_lowercase())
            .unwrap_or(String::new()); //HANDLE INVALID

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

    //SEARCH FOR COMMAND
    for info in COMMAND_LIST
    {
        if info.triggers.contains(&command.as_str())
        {
            return (Some(info.command.clone()), parameters);
        }
    }

    (Some(Command::Invalid), None)
}

#[cfg(feature = "client")]
pub fn send_command_code(stream: &mut TcpStream, command: &Command, parameters: &Option<String>) -> bool //SEND CODE FROM COMMAND IF POSSIBLE
{
    //CODE COMMAND
    if let Some(code) = command.to_code() && command != &Command::Upload
    {
        network::send(stream, MessagePacket
        {
            text: parameters.clone(),
            code: Some(code),
            ..Default::default()
        }, options::get_keys().as_ref());
        true
    } else { false }
}
