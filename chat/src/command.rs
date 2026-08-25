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
    fmt::
    {
        Display,
        Formatter,
        Result,
    }
};

use tokio::net::tcp::OwnedWriteHalf;

use crate::
{
    options,
    network::
    {
        self,
        codes::PacketCode,
    },
};

//ENUMS
#[derive(Clone, PartialEq)]
pub enum Command
{
    Exit,                                       //DISCONNECT FROM SERVER
    #[cfg(feature = "client_voice")] Voice,     //ENABLE VOICE CHAT
    #[cfg(feature = "client_voice")] Mute,      //TOGGLE-MUTE USER/YOURSELF
    Channel,                                    //SWITCH CHANNEL
    Help,                                       //PRINT COMMANDS
    Info,                                       //COMMAND INFO
    List,                                       //LIST USERS
    Files,                                      //LIST FILES
    #[cfg(feature = "client_screen")] Screens,  //LIST SCREENSHARES
    Upload,                                     //UPLOAD FILE TO SERVER
    Download,                                   //DOWNLOAD FILE FROM SERVER
    #[cfg(feature = "client_screen")] Screen,   //TOGGLE SCREEN SHARING
    #[cfg(feature = "client_screen")] Attach,   //ATTACH SCREEN SHARE
    #[cfg(feature = "client_screen")] Deattach, //DEATTACH SCREEN SHARE
    PrivateMessage,                             //ONE TO ONE MESSAGE
    Settings,                                   //OPEN THE SETTINGS OVERLAY
    Server,                                     //MODERATION ACTIONS (TAKES A SUBCOMMAND)
    UsernameColor,                              //SET COLOR OF USERNAME
    MessageColor,                               //SET COLOR OF MESSAGE
    Invalid,                                    //INVALID COMMAND
}

//ONE ACTION OF A COMMAND THAT TAKES ONE - THE COMMAND WORD ALONE DOES NOTHING (/server mute <id>)
#[derive(Clone, PartialEq)]
pub enum Subcommand
{
    Mute,     //MUTE A USER SERVER-SIDE
    Kick,     //DISCONNECT A USER
    Settings, //SERVER CONFIGURATION
}

//STRUCTS
pub struct CommandArg //COMMAND PARAMETER
{
    pub name: &'static str,
    pub description: &'static str,
    pub required: bool,
}

pub struct SubcommandInfo //SUBCOMMAND INFO - CARRIES ITS OWN ROLE, SO ONE COMMAND CAN HOLD ACTIONS OF DIFFERENT RANKS
{
    pub subcommand: Subcommand,
    pub triggers: &'static [&'static str],
    pub minimal_role: usize,
    pub args: &'static [CommandArg],
    pub description: &'static str,
}

pub struct CommandInfo //COMMAND INFO
{
    pub command: Command,
    pub triggers: &'static [&'static str],
    pub shortcut: Option<char>,
    pub minimal_role: usize,
    pub subcommands: &'static [SubcommandInfo], //EMPTY UNLESS THE COMMAND IS A DOORWAY TO ACTIONS
    pub args: &'static [CommandArg],
    pub description: &'static str,
}

pub const SERVER_SUBCOMMANDS: &[SubcommandInfo] =
&[
    SubcommandInfo
    {
        subcommand: Subcommand::Mute,
        triggers: &[ "MUTE", "SILENCE", "STFU" ],
        minimal_role: 1,
        args:
        &[
            CommandArg
            {
                name: "ID",
                description: "Target user",
                required: true,
            },
        ],
        description: "Mutes a user server-side",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::Kick,
        triggers: &[ "KICK", "BOOT", "REMOVE" ],
        minimal_role: 1,
        args:
        &[
            CommandArg
            {
                name: "ID",
                description: "Target user",
                required: true,
            },
        ],
        description: "Disconnects a user from the server",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::Settings,
        triggers: &[ "SETTINGS", "CONFIG", "SETUP" ],
        minimal_role: 2,
        args: &[],
        description: "Opens the server configuration",
    },
];

pub const COMMAND_LIST: &[CommandInfo] =
&[
    CommandInfo
    {
        command: Command::Help,
        triggers: &[ "HELP", "H", "COMMANDS", "USAGE", "GUIDE" ],
        shortcut: Some('h'),
        minimal_role: 0,
        subcommands: &[],
        args: &[],
        description: "Prints all available commands",
    },

    CommandInfo
    {
        command: Command::Info,
        triggers: &[ "INFO", "COMMAND", "MAN" ],
        shortcut: None,
        minimal_role: 0,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "COMMAND",
                description: "Target command",
                required: true,
            },
        ],
        description: "Shows command info",
    },

    #[cfg(feature = "client_voice")]
    CommandInfo
    {
        command: Command::Voice,
        triggers: &[ "VOICE", "VOIP", "CALL" ],
        shortcut: None,
        minimal_role: 0,
        subcommands: &[],
        args: &[],
        description: "Toggles voice chat",
    },

    #[cfg(feature = "client_voice")]
    CommandInfo
    {
        command: Command::Mute,
        triggers: &[ "MUTE", "UNMUTE", "SILENCE", "STFU" ],
        shortcut: Some('s'),
        minimal_role: 0,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "ID",
                description: "ID of target user",
                required: false,
            },
        ],
        description: "Toggle-mutes user/yourself",
    },

    CommandInfo
    {
        command: Command::Channel,
        triggers: &[ "CHANNEL", "SWITCH", "CHECKOUT", "AREA" ],
        shortcut: None,
        minimal_role: 0,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "NAME",
                description: "Name of channel",
                required: false,
            },
        ],
        description: "Switches to channel/lobby if NAME is omitted",
    },

    CommandInfo
    {
        command: Command::Upload,
        triggers: &[ "UPLOAD", "FILEUP", "PUSH", "UP" ],
        shortcut: None,
        minimal_role: 0,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "PATH",
                description: "Path of target file",
                required: true,
            },
        ],
        description: "Uploads file to server",
    },

    CommandInfo
    {
        command: Command::Download,
        triggers: &[ "DOWNLOAD", "FILEDOWN", "PULL", "DOWN", "FETCH" ],
        shortcut: None,
        minimal_role: 0,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "USER ID",
                description: "ID of uploader",
                required: true,
            },
            CommandArg
            {
                name: "FILE ID",
                description: "ID of target file",
                required: true,
            },
        ],
        description: "Downloads file from server",
    },

    #[cfg(feature = "client_screen")]
    CommandInfo
    {
        command: Command::Screen,
        triggers: &[ "SCREEN", "SCREENSHARE", "PRESENTATION", "SHARE" ],
        shortcut: None,
        minimal_role: 0,
        subcommands: &[],
        args: &[],
        description: "Toggles screensharing",
    },

    #[cfg(feature = "client_screen")]
    CommandInfo
    {
        command: Command::Attach,
        triggers: &[ "ATTACH", "WATCH", "DISPLAY", "JOIN" ],
        shortcut: None,
        minimal_role: 0,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "ID",
                description: "ID of screensharing user",
                required: true,
            },
        ],
        description: "Attaches client screenshare.",
    },

    #[cfg(feature = "client_screen")]
    CommandInfo
    {
        command: Command::Deattach,
        triggers: &[ "DEATTACH", "STOP", "CLOSE" ],
        shortcut: None,
        minimal_role: 0,
        subcommands: &[],
        args: &[],
        description: "Dettaches client screenshare.",
    },

    CommandInfo
    {
        command: Command::List,
        triggers: &[ "LIST", "USERS", "CLIENTS", "CHANNELS", "IDS", "ID" ],
        shortcut: Some('l'),
        minimal_role: 0,
        subcommands: &[],
        args: &[],
        description: "Shows connected users and their IDs",
    },

    CommandInfo
    {
        command: Command::Files,
        triggers: &[ "FILES", "LISTFILES", "UPLOADS", "DOWNLOADS" ],
        shortcut: Some('u'),
        minimal_role: 0,
        subcommands: &[],
        args: &[],
        description: "Shows available files and their IDs",
    },

    #[cfg(feature = "client_screen")]
    CommandInfo
    {
        command: Command::Screens,
        triggers: &[ "SCREENS", "LISTSCREENS", "SCREENSHARES", "SHARES" ],
        shortcut: None,
        minimal_role: 0,
        subcommands: &[],
        args: &[],
        description: "Shows all screensharing clients.",
    },

    CommandInfo
    {
        command: Command::PrivateMessage,
        triggers: &[ "PM", "DM", "MSG", "TELL" ],
        shortcut: None,
        minimal_role: 0,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "ID",
                description: "ID of target user",
                required: true,
            },
            CommandArg
            {
                name: "MESSAGE",
                description: "Message content",
                required: true,
            },
        ],
        description: "Sends private message",
    },

    CommandInfo
    {
        command: Command::Settings,
        triggers: &[ "SETTINGS", "SETUP", "CONFIG", "PREFERENCES", "AUDIO" ],
        shortcut: Some(','),
        minimal_role: 0,
        subcommands: &[],
        args: &[],
        description: "Opens audio and interface settings",
    },

    CommandInfo
    {
        command: Command::UsernameColor,
        triggers: &[ "UCOLOR", "USERNAME" ],
        shortcut: None,
        minimal_role: 0,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "COLOR",
                description: "Target color",
                required: true,
            },
        ],
        description: "Sets color of username",
    },

    CommandInfo
    {
        command: Command::MessageColor,
        triggers: &[ "COLOR", "MESSAGE" ],
        shortcut: None,
        minimal_role: 0,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "COLOR",
                description: "Target color",
                required: true,
            },
        ],
        description: "Sets color of message",
    },

    CommandInfo
    {
        command: Command::Server,
        triggers: &[ "SERVER", "ADMIN", "MOD" ],
        shortcut: None,
        minimal_role: 1,
        subcommands: SERVER_SUBCOMMANDS,
        args:
        &[
            CommandArg
            {
                name: "ACTION",
                description: "Moderation action",
                required: true,
            },
        ],
        description: "Moderation actions",
    },

    CommandInfo
    {
        command: Command::Exit,
        triggers: &[ "EXIT", "LEAVE", "QUIT", "DISCONNECT" ],
        shortcut: Some('c'),
        minimal_role: 0,
        subcommands: &[],
        args: &[],
        description: "Disconnects from the server",
    },
];

//CONSTS
pub const COMMAND_PREFIX: &str = "/"; //PREFIX FOR COMMANDS

//IMPLEMENTATIONS
impl CommandInfo
{
    //THE COMMAND IS OFFERED TO role - HIDING IT IS COSMETIC, THE SERVER STILL CHECKS EVERY PRIVILEGED PACKET ITSELF
    pub fn available(&self, role: usize) -> bool
    {
        //A COMMAND THAT IS NOTHING BUT A DOORWAY TO ITS ACTIONS IS WORTH SHOWING ONLY WHILE ONE OF THEM IS LEFT
        role >= self.minimal_role && (self.subcommands.is_empty() || self.actions(role).next().is_some())
    }

    pub fn actions(&self, role: usize) -> impl Iterator<Item = &'static SubcommandInfo> //ACTIONS role MAY RUN
    {
        self.subcommands.iter().filter(move |sub| sub.available(role))
    }

    pub fn action(&self, word: &str) -> Option<&'static SubcommandInfo> //ACTION BY TRIGGER (ROLE IS NOT CHECKED HERE)
    {
        self.subcommands.iter().find(|sub| sub.triggers.iter().any(|t| t.eq_ignore_ascii_case(word)))
    }
}

impl SubcommandInfo
{
    pub fn available(&self, role: usize) -> bool { role >= self.minimal_role }
}

impl Command
{
    //GET CODE MATCHING TO COMMAND
    pub fn build_message(&self, parameters: Option<&str>) -> Option<result::Result<PacketCode, ()>>
    {
        match self
        {
            Command::PrivateMessage =>
            {
                let parsed = parameters
                    .and_then(|p| p.split_once(' '))
                    .and_then(|(id, text)| Some((id.parse::<usize>().ok()?, text.to_string())));

                Some(match parsed
                {
                    Some((id, text)) => Ok(PacketCode::PrivateMessage { id, text, username: None }),
                    None => Err(()),
                })
            },

            Command::Download =>
            {
                let parsed = parameters
                    .and_then(|p| p.split_once(' '))
                    .and_then(|(uid, fid)| Some((uid.parse::<usize>().ok()?, fid.parse::<usize>().ok()?)));

                Some(match parsed
                {
                    Some((id, file_id)) => Ok(PacketCode::Download { id: Some(id), file_id: Some(file_id), token: None }),
                    None => Err(()),
                })
            },

            #[cfg(feature = "client_screen")]
            Command::Attach =>
            {
                //PARSE TARGET ID
                let target_id = parameters.and_then(|p| p.parse::<usize>().ok());

                Some(match target_id
                {
                    Some(id) => Ok(PacketCode::Attach { id: Some(id), token: None, username: None }), //NOVÁ VARIANTA
                    None => Err(()),
                })
            },

            Command::Channel => Some(Ok(PacketCode::Channel { channel: parameters.map(str::to_string) })),
            Command::List => Some(Ok(PacketCode::List { users: None })),
            Command::Files => Some(Ok(PacketCode::Files { users: None })),
            #[cfg(feature = "client_screen")] Command::Screen => Some(Ok(PacketCode::Screen { token: None })),
            #[cfg(feature = "client_screen")] Command::Deattach => Some(Ok(PacketCode::Deattach { username: None } )),
            #[cfg(feature = "client_screen")] Command::Screens => Some(Ok(PacketCode::Screens { users: None })),

            Command::Exit => Some(Ok(PacketCode::Disconnect)),
            #[cfg(feature = "client_voice")] Command::Voice => Some(Ok(PacketCode::Voice { token: None })),

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
            .unwrap_or_default(); //HANDLE INVALID

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

pub async fn send_command_code(write_stream: &mut OwnedWriteHalf, command: &Command, parameters: &Option<String>) -> Option<bool> //SEND CODE FROM COMMAND IF POSSIBLE
{
    //CODE COMMAND
    match command.build_message(parameters.as_deref())?
    {
        Ok(message) =>
        {
            network::send(write_stream, message, options::get_keys().as_ref()).await;
            Some(true)
        },

        Err(()) => Some(false),
    }
}
