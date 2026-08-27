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
    consts,
    options,
    network::
    {
        self,
        codes::PacketCode,
    },
};

#[cfg(feature = "client_screen")]
use crate::network::screen::client::
{
    capture as screen_capture,
    options as screen_options,
};

//ENUMS
#[derive(Clone, PartialEq)]
pub enum Command
{
    Exit,                                       //DISCONNECT FROM SERVER
    Logout,                                     //DISCONNECT FROM SERVER, BACK TO THE CONNECT BOX
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
    Ban,      //BAN A USER
    BanIp,    //IP BAN A USER
    Bans,     //LIST EVERY BAN
    Pardon,   //LIFT A USERNAME BAN
    PardonIp, //LIFT AN IP BAN
    Say,      //SAY AS SERVER
    Settings, //SERVER CONFIGURATION
}

//A PARAMETER WITH A CLOSED SET OF ANSWERS - THE PALETTE OFFERS THEM INSTEAD OF LEAVING THE USER GUESSING.
//THE VARIANT ONLY NAMES THE SET; THE VALUES THEMSELVES LIVE WHERE THEY ARE ALREADY DEFINED (colors::COLORS)
#[derive(Clone, Copy, PartialEq)]
pub enum ArgValues
{
    Free,     //ANYTHING - A NAME, A MESSAGE, AN ID
    Colors,   //A crossterm COLOR NAME
    Monitors, //A MONITOR OF THIS MACHINE, AS THE DISPLAY SERVER NAMES IT
}

//STRUCTS
pub struct CommandArg //COMMAND PARAMETER
{
    pub name: &'static str,
    pub description: &'static str,
    pub required: bool,
    pub values: ArgValues, //WHAT MAY BE TYPED HERE, WHEN THAT IS A KNOWN, SHORT LIST
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
        minimal_role: consts::SERVER_MODERATOR_ROLE,
        args:
        &[
            CommandArg
            {
                name: "ID",
                description: "Target user",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "Mutes a user server-side",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::Kick,
        triggers: &[ "KICK", "BOOT", "REMOVE" ],
        minimal_role: consts::SERVER_MODERATOR_ROLE,
        args:
        &[
            CommandArg
            {
                name: "ID",
                description: "Target user",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "Disconnects a user from the server",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::Ban,
        triggers: &[ "BAN", "DISABLE", "KILL" ],
        minimal_role: consts::SERVER_OWNER_ROLE,
        args:
        &[
            CommandArg
            {
                name: "ID",
                description: "Target user",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "Bans a user from the server",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::BanIp,
        triggers: &[ "BANIP", "DISABLEIP", "BLOCKIP" ],
        minimal_role: consts::SERVER_OWNER_ROLE,
        args:
        &[
            CommandArg
            {
                name: "ID",
                description: "Target user",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "Bans a user's IP from the server",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::Bans,
        triggers: &[ "BANLIST", "BANS", "BANNED", "DISABLED", "BLOCKED" ],
        minimal_role: consts::SERVER_OWNER_ROLE,
        args: &[],
        description: "Lists every ban",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::Pardon,
        triggers: &[ "PARDON", "UNBAN", "FORGIVE", "UNBLOCK" ],
        minimal_role: consts::SERVER_OWNER_ROLE,
        args:
        &[
            CommandArg
            {
                name: "ID",
                description: "Banned user ID",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "Removes a user ban",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::PardonIp,
        triggers: &[ "PARDONIP", "UNBANIP", "FORGIVEIP", "UNBLOCKIP" ],
        minimal_role: consts::SERVER_OWNER_ROLE,
        args:
        &[
            CommandArg
            {
                name: "ID",
                description: "Banned address ID",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "Removes an IP ban",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::Say,
        triggers: &[ "SAY", "ECHO", "BROADCAST", "NOTICE", "MESSAGE" ],
        minimal_role: consts::SERVER_OWNER_ROLE,
        args:
        &[
            CommandArg
            {
                name: "MESSAGE",
                description: "Message to broadcast",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "Broadcasts message as server",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::Settings,
        triggers: &[ "SETTINGS", "CONFIG", "SETUP" ],
        minimal_role: consts::SERVER_OWNER_ROLE,
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
        minimal_role: consts::SERVER_USER_ROLE,
        subcommands: &[],
        args: &[],
        description: "Prints all available commands",
    },

    CommandInfo
    {
        command: Command::Info,
        triggers: &[ "INFO", "COMMAND", "MAN" ],
        shortcut: None,
        minimal_role: consts::SERVER_USER_ROLE,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "COMMAND",
                description: "Target command",
                required: true,
                values: ArgValues::Free,
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
        minimal_role: consts::SERVER_USER_ROLE,
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
        minimal_role: consts::SERVER_USER_ROLE,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "ID",
                description: "ID of target user",
                required: false,
                values: ArgValues::Free,
            },
        ],
        description: "Toggle-mutes user/yourself",
    },

    CommandInfo
    {
        command: Command::Channel,
        triggers: &[ "CHANNEL", "SWITCH", "CHECKOUT", "AREA" ],
        shortcut: None,
        minimal_role: consts::SERVER_USER_ROLE,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "NAME",
                description: "Name of channel",
                required: false,
                values: ArgValues::Free,
            },
        ],
        description: "Switches to channel/lobby if NAME is omitted",
    },

    CommandInfo
    {
        command: Command::Upload,
        triggers: &[ "UPLOAD", "FILEUP", "PUSH", "UP" ],
        shortcut: None,
        minimal_role: consts::SERVER_USER_ROLE,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "PATH",
                description: "Path of target file",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "Uploads file to server",
    },

    CommandInfo
    {
        command: Command::Download,
        triggers: &[ "DOWNLOAD", "FILEDOWN", "PULL", "DOWN", "FETCH" ],
        shortcut: None,
        minimal_role: consts::SERVER_USER_ROLE,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "USER ID",
                description: "ID of uploader",
                required: true,
                values: ArgValues::Free,
            },
            CommandArg
            {
                name: "FILE ID",
                description: "ID of target file",
                required: true,
                values: ArgValues::Free,
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
        minimal_role: consts::SERVER_USER_ROLE,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "MONITOR",
                description: "Index or name of the monitor to share",
                required: false,
                values: ArgValues::Monitors,
            },
        ],
        description: "Toggles screensharing, or swaps the shared monitor while it runs",
    },

    #[cfg(feature = "client_screen")]
    CommandInfo
    {
        command: Command::Attach,
        triggers: &[ "ATTACH", "WATCH", "DISPLAY", "JOIN" ],
        shortcut: None,
        minimal_role: consts::SERVER_USER_ROLE,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "ID",
                description: "ID of screensharing user",
                required: true,
                values: ArgValues::Free,
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
        minimal_role: consts::SERVER_USER_ROLE,
        subcommands: &[],
        args: &[],
        description: "Dettaches client screenshare.",
    },

    CommandInfo
    {
        command: Command::List,
        triggers: &[ "LIST", "USERS", "CLIENTS", "CHANNELS", "IDS", "ID" ],
        shortcut: Some('l'),
        minimal_role: consts::SERVER_USER_ROLE,
        subcommands: &[],
        args: &[],
        description: "Shows connected users and their IDs",
    },

    CommandInfo
    {
        command: Command::Files,
        triggers: &[ "FILES", "LISTFILES", "UPLOADS", "DOWNLOADS" ],
        shortcut: Some('u'),
        minimal_role: consts::SERVER_USER_ROLE,
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
        minimal_role: consts::SERVER_USER_ROLE,
        subcommands: &[],
        args: &[],
        description: "Shows all screensharing clients.",
    },

    CommandInfo
    {
        command: Command::PrivateMessage,
        triggers: &[ "PM", "DM", "MSG", "TELL" ],
        shortcut: None,
        minimal_role: consts::SERVER_USER_ROLE,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "ID",
                description: "ID of target user",
                required: true,
                values: ArgValues::Free,
            },
            CommandArg
            {
                name: "MESSAGE",
                description: "Message content",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "Sends private message",
    },

    CommandInfo
    {
        command: Command::Settings,
        triggers: &[ "SETTINGS", "SETUP", "CONFIG", "PREFERENCES", "AUDIO" ],
        shortcut: Some(','),
        minimal_role: consts::SERVER_USER_ROLE,
        subcommands: &[],
        args: &[],
        description: "Opens audio and interface settings",
    },

    CommandInfo
    {
        command: Command::UsernameColor,
        triggers: &[ "UCOLOR", "USERNAME" ],
        shortcut: None,
        minimal_role: consts::SERVER_USER_ROLE,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "COLOR",
                description: "Target color",
                required: true,
                values: ArgValues::Colors,
            },
        ],
        description: "Sets color of username",
    },

    CommandInfo
    {
        command: Command::MessageColor,
        triggers: &[ "COLOR", "MESSAGE" ],
        shortcut: None,
        minimal_role: consts::SERVER_USER_ROLE,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "COLOR",
                description: "Target color",
                required: true,
                values: ArgValues::Colors,
            },
        ],
        description: "Sets color of message",
    },

    CommandInfo
    {
        command: Command::Server,
        triggers: &[ "SERVER", "ADMIN", "MOD" ],
        shortcut: None,
        minimal_role: consts::SERVER_MODERATOR_ROLE,
        subcommands: SERVER_SUBCOMMANDS,
        args:
        &[
            CommandArg
            {
                name: "ACTION",
                description: "Moderation action",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "Moderation actions",
    },

    CommandInfo
    {
        command: Command::Logout,
        triggers: &[ "LOGOUT", "SIGNOUT", "SWITCH" ],
        shortcut: Some('o'),
        minimal_role: consts::SERVER_USER_ROLE,
        subcommands: &[],
        args: &[],
        description: "Disconnects from the server and returns to the login screen",
    },

    CommandInfo
    {
        command: Command::Exit,
        triggers: &[ "EXIT", "LEAVE", "QUIT", "DISCONNECT" ],
        shortcut: Some('c'),
        minimal_role: consts::SERVER_USER_ROLE,
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

    //WHETHER THE PARAMETER IS A TARGET ID - EVERY ACTION AIMED AT A USER TAKES ONE, THE REST TAKE THEIR PARAMETER AS TEXT
    pub fn takes_id(&self) -> bool
    {
        matches!(self.subcommand, Subcommand::Mute
            | Subcommand::Kick
            | Subcommand::Ban
            | Subcommand::BanIp
            | Subcommand::Pardon
            | Subcommand::PardonIp)
    }
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

            //THE MONITOR IS PICKED ON THIS MACHINE AND NEVER LEAVES IT - THE SERVER ONLY EVER TOGGLES THE
            //SHARE. THE PICK LASTS EXACTLY AS LONG AS THE SHARE DOES (THE STOP CLEARS IT), SO A BARE
            //COMMAND ALWAYS STARTS ON THE DEFAULT MONITOR, AND NAMING ANOTHER ONE MID-SHARE SWAPS TO IT.
            #[cfg(feature = "client_screen")]
            Command::Screen =>
            {
                let sharing = screen_options::get_use_screen();

                let Some(selection) = parameters.map(str::trim).filter(|m| !m.is_empty()) else
                {
                    //NO MONITOR NAMED: STOP THE SHARE, OR START ONE ON THE DEFAULT MONITOR
                    if !sharing { screen_options::set_monitor(None); }

                    return Some(Ok(PacketCode::Screen { token: None }));
                };

                //RESOLVED BEFORE IT IS STORED, SO A MONITOR THAT DOES NOT EXIST IS REFUSED ON THE SPOT
                //RATHER THAN STARTING A SHARE THAT DIES, AND SO WHAT IS COMPARED BELOW IS THE MONITOR
                //ITSELF RATHER THAN WHICHEVER OF ITS TWO SPELLINGS WAS TYPED
                let Ok(monitor) = screen_capture::resolve_monitor(selection) else { return Some(Err(())) };

                //THE MONITOR WE ARE ALREADY ON ENDS THE SHARE, LIKE A BARE /screen: ASKING FOR WHAT IS
                //ALREADY ON THE WIRE IS THE ONE CASE WHERE A SWAP WOULD MEAN NOTHING. THE PICK IS LEFT
                //ALONE HERE - IT IS THE SHARE STOPPING THAT CLEARS IT, AND SWAPPING TO THE MONITOR WE
                //ARE ABOUT TO STOP CAPTURING WOULD ONLY MAKE THE CAPTURE RESTART ON ITS WAY OUT
                if sharing && screen_capture::current_monitor().is_some_and(|current| current == monitor)
                {
                    return Some(Ok(PacketCode::Screen { token: None }));
                }

                screen_options::set_monitor(Some(monitor));

                //SWAP THE CAPTURE OVER INSTEAD OF TOGGLING: THE SERVER ONLY EVER KNOWS *THAT* WE ARE
                //SHARING, SO THE RUNNING CAPTURE PICKS THE NEW MONITOR UP AND NOTHING IS SENT AT ALL
                if sharing { return None; }

                Some(Ok(PacketCode::Screen { token: None }))
            },

            #[cfg(feature = "client_screen")] Command::Deattach => Some(Ok(PacketCode::Deattach { username: None } )),
            #[cfg(feature = "client_screen")] Command::Screens => Some(Ok(PacketCode::Screens { users: None })),

            //THE SAME PACKET AS /exit - THE TWO DIFFER ONLY IN WHAT THE CLIENT DOES WITH THE DISCONNECT
            //THAT COMES BACK: ONE ENDS THE PROCESS, THE OTHER LANDS IN THE CONNECT BOX
            Command::Exit | Command::Logout => Some(Ok(PacketCode::Disconnect)),
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
