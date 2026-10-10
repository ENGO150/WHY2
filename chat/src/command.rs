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
    role::Role,
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
    Exit,                                         //DISCONNECT FROM SERVER
    Logout,                                       //DISCONNECT FROM SERVER, BACK TO THE CONNECT BOX
    #[cfg(feature = "client_voice")] Voice,       //ENABLE VOICE CHAT
    #[cfg(feature = "client_voice")] Mute,        //TOGGLE-MUTE USER/YOURSELF
    #[cfg(feature = "client_voice")] Record,      //RECORD A VOICE MESSAGE
    #[cfg(feature = "client_voice")] Play,        //PLAY A VOICE MESSAGE
    Channel,                                      //SWITCH CHANNEL
    Help,                                         //PRINT COMMANDS
    Info,                                         //COMMAND INFO
    List,                                         //LIST USERS
    Files,                                        //LIST FILES
    #[cfg(feature = "client_screen")] Screens,    //LIST SCREENSHARES
    Upload,                                       //UPLOAD FILE TO SERVER
    Download,                                     //DOWNLOAD FILE FROM SERVER
    Image,                                        //PERSISTENT IMAGE UPLOAD
    #[cfg(feature = "client_screen")] Screen,     //TOGGLE SCREEN SHARING
    #[cfg(feature = "client_screen")] Attach,     //ATTACH SCREEN SHARE
    #[cfg(feature = "client_screen")] Deattach,   //DEATTACH SCREEN SHARE
    #[cfg(feature = "client_screen")] MuteScreen, //MUTE SCREEN SHARE
    Delete,                                       //DELETE A STORED MESSAGE
    Edit,                                         //REWORD A STORED MESSAGE
    PrivateMessage,                               //ONE TO ONE MESSAGE
    Heart,                                        //TOGGLE MESSAGE HEART REACTION
    Hearts,                                       //LIST WHO HEARTED A MESSAGE
    Reply,                                        //REPLY TO MESSAGE
    Re,                                           //REPLY TO PRIVATE MESSAGE
    Settings,                                     //OPEN THE SETTINGS OVERLAY
    Profile,                                      //OPEN A USER PROFILE
    Server,                                       //MODERATION ACTIONS (TAKES A SUBCOMMAND)
    Account,                                      //ACCOUNT ACTIONS (TAKES A SUBCOMMAND)
    UsernameColor,                                //SET COLOR OF USERNAME
    MessageColor,                                 //SET COLOR OF MESSAGE
    Invalid,                                      //INVALID COMMAND
}

//ONE ACTION OF A COMMAND (/server mute <id>)
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
    Role,     //SET A USER'S ROLE
    Settings, //SERVER CONFIGURATION
    Icon,     //SERVER PICTURE
    Delete,   //DELETE OWN ACCOUNT
    Passwd,   //CHANGE OWN PASSWORD
}

//A PARAMETER THE PALETTE CAN OFFER ANSWERS FOR
#[derive(Clone, Copy, PartialEq)]
pub enum ArgValues
{
    Free,     //ANYTHING - A NAME, A MESSAGE, AN ID
    Colors,   //A crossterm COLOR NAME
    Paths,    //A FILE OR DIRECTORY BESIDE THE ONE BEING TYPED
    Images,   //THE SAME AS Paths BUT DECODABLE PICTURES ONLY
    Monitors, //A MONITOR OF THIS MACHINE, AS THE DISPLAY SERVER NAMES IT
    Roles,    //A SERVER ROLE, BY THE NAME BOTH SIDES KNOW IT BY (role::Role)
    Bools,    //true OR false
}

//STRUCTS
pub struct CommandArg //COMMAND PARAMETER
{
    pub name: &'static str,        //TRANSLATION KEYS
    pub description: &'static str,
    pub required: bool,
    pub values: ArgValues, //WHAT MAY BE TYPED HERE, WHEN THAT IS A KNOWN, SHORT LIST
}

pub struct SubcommandInfo //SUBCOMMAND INFO
{
    pub subcommand: Subcommand,
    pub triggers: &'static [&'static str],
    pub minimal_role: Role,
    pub args: &'static [CommandArg],
    pub description: &'static str,
}

pub struct CommandInfo //COMMAND INFO
{
    pub command: Command,
    pub triggers: &'static [&'static str],
    pub shortcut: Option<char>,
    pub minimal_role: Role,
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
        minimal_role: Role::Moderator,
        args:
        &[
            CommandArg
            {
                name: "arg.id",
                description: "command.server.mute.args.id",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.server.mute.description",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::Kick,
        triggers: &[ "KICK", "BOOT", "REMOVE" ],
        minimal_role: Role::Moderator,
        args:
        &[
            CommandArg
            {
                name: "arg.id",
                description: "command.server.kick.args.id",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.server.kick.description",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::Ban,
        triggers: &[ "BAN", "DISABLE", "KILL" ],
        minimal_role: Role::Owner,
        args:
        &[
            CommandArg
            {
                name: "arg.user",
                description: "command.server.ban.args.user",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.server.ban.description",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::BanIp,
        triggers: &[ "BANIP", "DISABLEIP", "BLOCKIP" ],
        minimal_role: Role::Owner,
        args:
        &[
            CommandArg
            {
                name: "arg.id",
                description: "command.server.banip.args.id",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.server.banip.description",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::Bans,
        triggers: &[ "BANLIST", "BANS", "BANNED", "DISABLED", "BLOCKED" ],
        minimal_role: Role::Owner,
        args: &[],
        description: "command.server.banlist.description",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::Pardon,
        triggers: &[ "PARDON", "UNBAN", "FORGIVE", "UNBLOCK" ],
        minimal_role: Role::Owner,
        args:
        &[
            CommandArg
            {
                name: "arg.id",
                description: "command.server.pardon.args.id",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.server.pardon.description",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::PardonIp,
        triggers: &[ "PARDONIP", "UNBANIP", "FORGIVEIP", "UNBLOCKIP" ],
        minimal_role: Role::Owner,
        args:
        &[
            CommandArg
            {
                name: "arg.id",
                description: "command.server.pardonip.args.id",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.server.pardonip.description",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::Say,
        triggers: &[ "SAY", "ECHO", "BROADCAST", "NOTICE", "MESSAGE" ],
        minimal_role: Role::Owner,
        args:
        &[
            CommandArg
            {
                name: "arg.message",
                description: "command.server.say.args.message",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.server.say.description",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::Role,
        triggers: &[ "ROLE", "RANK", "PROMOTE", "DEMOTE" ],
        minimal_role: Role::Owner,
        args:
        &[
            CommandArg
            {
                name: "arg.user",
                description: "command.server.role.args.user",
                required: true,
                values: ArgValues::Free,
            },

            CommandArg
            {
                name: "arg.role",
                description: "command.server.role.args.role",
                required: true,
                values: ArgValues::Roles,
            },
        ],
        description: "command.server.role.description",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::Settings,
        triggers: &[ "SETTINGS", "CONFIG", "SETUP" ],
        minimal_role: Role::Owner,
        args: &[],
        description: "command.server.settings.description",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::Icon,
        triggers: &[ "ICON", "PICTURE", "LOGO" ],
        minimal_role: Role::Owner,
        args:
        &[
            CommandArg
            {
                name: "arg.path",
                description: "command.server.icon.args.path",
                required: false,
                values: ArgValues::Images,
            },
        ],
        description: "command.server.icon.description",
    },
];

pub const ACCOUNT_SUBCOMMANDS: &[SubcommandInfo] =
&[
    SubcommandInfo
    {
        subcommand: Subcommand::Delete,
        triggers: &[ "DELETE", "REMOVE", "CLOSE" ],
        minimal_role: Role::User,
        args: &[],
        description: "command.account.delete.description",
    },

    SubcommandInfo
    {
        subcommand: Subcommand::Passwd,
        triggers: &[ "PASSWD", "PASSWORD", "CHANGEPASS" ],
        minimal_role: Role::User,
        args: &[],
        description: "command.account.passwd.description",
    },
];

pub const COMMAND_LIST: &[CommandInfo] =
&[
    CommandInfo
    {
        command: Command::Help,
        triggers: &[ "HELP", "H", "COMMANDS", "USAGE", "GUIDE" ],
        shortcut: Some('h'),
        minimal_role: Role::User,
        subcommands: &[],
        args: &[],
        description: "command.help.description",
    },

    CommandInfo
    {
        command: Command::Info,
        triggers: &[ "INFO", "COMMAND", "MAN" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.command",
                description: "command.info.args.command",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.info.description",
    },

    #[cfg(feature = "client_voice")]
    CommandInfo
    {
        command: Command::Voice,
        triggers: &[ "VOICE", "VOIP", "CALL" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args: &[],
        description: "command.voice.description",
    },

    #[cfg(feature = "client_voice")]
    CommandInfo
    {
        command: Command::Mute,
        triggers: &[ "MUTE", "UNMUTE", "SILENCE", "STFU" ],
        shortcut: Some('s'),
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.id",
                description: "command.mute.args.id",
                required: false,
                values: ArgValues::Free,
            },
        ],
        description: "command.mute.description",
    },

    #[cfg(feature = "client_voice")]
    CommandInfo
    {
        command: Command::Record,
        triggers: &[ "RECORD", "REC", "VOICEMESSAGE", "VM" ],
        shortcut: Some('r'),
        minimal_role: Role::User,
        subcommands: &[],
        args: &[],
        description: "command.record.description",
    },

    #[cfg(feature = "client_voice")]
    CommandInfo
    {
        command: Command::Play,
        triggers: &[ "PLAY", "LISTEN" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.id",
                description: "command.play.args.id",
                required: false,
                values: ArgValues::Free,
            },
        ],
        description: "command.play.description",
    },

    CommandInfo
    {
        command: Command::Channel,
        triggers: &[ "CHANNEL", "SWITCH", "CHECKOUT", "AREA" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.name",
                description: "command.channel.args.name",
                required: false,
                values: ArgValues::Free,
            },
        ],
        description: "command.channel.description",
    },

    CommandInfo
    {
        command: Command::Upload,
        triggers: &[ "UPLOAD", "FILEUP", "PUSH", "UP" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.path",
                description: "command.upload.args.path",
                required: true,
                values: ArgValues::Paths,
            },
        ],
        description: "command.upload.description",
    },

    CommandInfo
    {
        command: Command::Download,
        triggers: &[ "DOWNLOAD", "FILEDOWN", "PULL", "DOWN", "FETCH" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.user_id",
                description: "command.download.args.user_id",
                required: true,
                values: ArgValues::Free,
            },
            CommandArg
            {
                name: "arg.file_id",
                description: "command.download.args.file_id",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.download.description",
    },

    CommandInfo
    {
        command: Command::Image,
        triggers: &[ "IMAGE", "PICTURE", "PIC" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.path",
                description: "command.image.args.path",
                required: true,
                values: ArgValues::Images,
            },
        ],
        description: "command.image.description",
    },

    #[cfg(feature = "client_screen")]
    CommandInfo
    {
        command: Command::Screen,
        triggers: &[ "SCREEN", "SCREENSHARE", "PRESENTATION", "SHARE" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.monitor",
                description: "command.screen.args.monitor",
                required: false,
                values: ArgValues::Monitors,
            },
            CommandArg
            {
                name: "arg.sound",
                description: "command.screen.args.sound",
                required: false,
                values: ArgValues::Bools,
            },
        ],
        description: "command.screen.description",
    },

    #[cfg(feature = "client_screen")]
    CommandInfo
    {
        command: Command::Attach,
        triggers: &[ "ATTACH", "WATCH", "DISPLAY", "JOIN" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.id",
                description: "command.attach.args.id",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.attach.description",
    },

    #[cfg(feature = "client_screen")]
    CommandInfo
    {
        command: Command::Deattach,
        triggers: &[ "DEATTACH", "STOP", "CLOSE" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args: &[],
        description: "command.deattach.description",
    },

    #[cfg(feature = "client_screen")]
    CommandInfo
    {
        command: Command::MuteScreen,
        triggers: &[ "MUTESCREEN", "SILENCESTREAM" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args: &[],
        description: "command.mutescreen.description",
    },

    CommandInfo
    {
        command: Command::List,
        triggers: &[ "LIST", "USERS", "CLIENTS", "CHANNELS", "IDS", "ID" ],
        shortcut: Some('l'),
        minimal_role: Role::User,
        subcommands: &[],
        args: &[],
        description: "command.list.description",
    },

    CommandInfo
    {
        command: Command::Files,
        triggers: &[ "FILES", "LISTFILES", "UPLOADS", "DOWNLOADS" ],
        shortcut: Some('u'),
        minimal_role: Role::User,
        subcommands: &[],
        args: &[],
        description: "command.files.description",
    },

    #[cfg(feature = "client_screen")]
    CommandInfo
    {
        command: Command::Screens,
        triggers: &[ "SCREENS", "LISTSCREENS", "SCREENSHARES", "SHARES" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args: &[],
        description: "command.screens.description",
    },

    CommandInfo
    {
        command: Command::PrivateMessage,
        triggers: &[ "PM", "DM", "MSG", "TELL" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.id",
                description: "command.pm.args.id",
                required: true,
                values: ArgValues::Free,
            },
            CommandArg
            {
                name: "arg.message",
                description: "command.pm.args.message",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.pm.description",
    },

    CommandInfo
    {
        command: Command::Heart,
        triggers: &[ "HEART", "UNHEART", "LIKE", "REACT", "STAR" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.id",
                description: "command.heart.args.id",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.heart.description",
    },

    CommandInfo
    {
        command: Command::Hearts,
        triggers: &[ "HEARTS", "LIKES", "STARS", "REACTIONS" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.id",
                description: "command.hearts.args.id",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.hearts.description",
    },

    CommandInfo
    {
        command: Command::Reply,
        triggers: &[ "REPLY", "RESPOND" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.id",
                description: "command.reply.args.id",
                required: true,
                values: ArgValues::Free,
            },
            CommandArg
            {
                name: "arg.message",
                description: "command.reply.args.message",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.reply.description",
    },

    CommandInfo
    {
        command: Command::Re,
        triggers: &[ "RE", "ANSWER" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.message",
                description: "command.re.args.message",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.re.description",
    },

    CommandInfo
    {
        command: Command::Delete,
        triggers: &[ "DELETE", "DEL", "RM" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.id",
                description: "command.delete.args.id",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.delete.description",
    },

    CommandInfo
    {
        command: Command::Edit,
        triggers: &[ "EDIT", "E" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.id",
                description: "command.edit.args.id",
                required: true,
                values: ArgValues::Free,
            },
            CommandArg
            {
                name: "arg.message",
                description: "command.edit.args.message",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.edit.description",
    },

    CommandInfo
    {
        command: Command::Settings,
        triggers: &[ "SETTINGS", "SETUP", "CONFIG", "PREFERENCES", "AUDIO" ],
        shortcut: Some(','),
        minimal_role: Role::User,
        subcommands: &[],
        args: &[],
        description: "command.settings.description",
    },

    CommandInfo
    {
        command: Command::Profile,
        triggers: &[ "PROFILE", "BIO", "ABOUT" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.user",
                description: "command.profile.args.user",
                required: false,
                values: ArgValues::Free,
            },
        ],
        description: "command.profile.description",
    },

    CommandInfo
    {
        command: Command::UsernameColor,
        triggers: &[ "UCOLOR", "USERNAME" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.color",
                description: "command.ucolor.args.color",
                required: true,
                values: ArgValues::Colors,
            },
        ],
        description: "command.ucolor.description",
    },

    CommandInfo
    {
        command: Command::MessageColor,
        triggers: &[ "COLOR", "MESSAGE" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: &[],
        args:
        &[
            CommandArg
            {
                name: "arg.color",
                description: "command.color.args.color",
                required: true,
                values: ArgValues::Colors,
            },
        ],
        description: "command.color.description",
    },

    CommandInfo
    {
        command: Command::Server,
        triggers: &[ "SERVER", "ADMIN", "MOD" ],
        shortcut: None,
        minimal_role: Role::Moderator,
        subcommands: SERVER_SUBCOMMANDS,
        args:
        &[
            CommandArg
            {
                name: "arg.action",
                description: "command.server.args.action",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.server.description",
    },

    CommandInfo
    {
        command: Command::Account,
        triggers: &[ "ACCOUNT", "MANAGEMENT", "CLIENT" ],
        shortcut: None,
        minimal_role: Role::User,
        subcommands: ACCOUNT_SUBCOMMANDS,
        args:
        &[
            CommandArg
            {
                name: "arg.action",
                description: "command.account.args.action",
                required: true,
                values: ArgValues::Free,
            },
        ],
        description: "command.account.description",
    },

    CommandInfo
    {
        command: Command::Logout,
        triggers: &[ "LOGOUT", "SIGNOUT" ],
        shortcut: Some('o'),
        minimal_role: Role::User,
        subcommands: &[],
        args: &[],
        description: "command.logout.description",
    },

    CommandInfo
    {
        command: Command::Exit,
        triggers: &[ "EXIT", "LEAVE", "QUIT", "DISCONNECT" ],
        shortcut: Some('c'),
        minimal_role: Role::User,
        subcommands: &[],
        args: &[],
        description: "command.exit.description",
    },
];

//CONSTS
pub const COMMAND_PREFIX: &str = "/"; //PREFIX FOR COMMANDS

//IMPLEMENTATIONS
impl CommandInfo
{
    //WHETHER role IS OFFERED THE COMMAND
    pub fn available(&self, role: Role) -> bool
    {
        //HIDE A DOORWAY COMMAND WITH NO ACTIONS LEFT
        role >= self.minimal_role && (self.subcommands.is_empty() || self.actions(role).next().is_some())
    }

    pub fn actions(&self, role: Role) -> impl Iterator<Item = &'static SubcommandInfo> //ACTIONS role MAY RUN
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
    pub fn available(&self, role: Role) -> bool { role >= self.minimal_role }

    //WHETHER THE WHOLE PARAMETER IS A TARGET ID
    pub fn takes_id(&self) -> bool
    {
        matches!(self.subcommand, Subcommand::Mute
            | Subcommand::Kick
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
                    Some((id, text)) => Ok(PacketCode::PrivateMessageRequest { id, text }),
                    None => Err(()),
                })
            },

            Command::Heart => Some(match parameters.and_then(|p| p.parse::<u64>().ok())
            {
                Some(message_id) => Ok(PacketCode::HeartRequest { message_id }),
                None => Err(()),
            }),

            Command::Reply =>
            {
                let parsed = parameters
                    .and_then(|p| p.split_once(' '))
                    .and_then(|(reply, text)| Some((reply.parse::<u64>().ok()?, text.to_string())));

                Some(match parsed
                {
                    Some((reply, text)) => Ok(PacketCode::MessageRequest { text, reply: Some(reply) }),
                    None => Err(()),
                })
            },

            Command::Re =>
            {
                Some(match parameters
                {
                    Some(parameters) => Ok(PacketCode::Re { message: parameters.to_string() }),
                    None => Err(()),
                })
            }

            Command::Download =>
            {
                let parsed = parameters
                    .and_then(|p| p.split_once(' '))
                    .and_then(|(uid, fid)| Some((uid.parse::<usize>().ok()?, fid.parse::<usize>().ok()?)));

                Some(match parsed
                {
                    Some((id, file_id)) => Ok(PacketCode::DownloadRequest { id, file_id }),
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
                    Some(id) => Ok(PacketCode::AttachRequest { id }),
                    None => Err(()),
                })
            },

            Command::Delete => Some(match parameters.and_then(|p| p.parse::<u64>().ok())
            {
                Some(message_id) => Ok(PacketCode::DeleteRequest { message_id }),
                None => Err(()),
            }),

            Command::Edit =>
            {
                let parsed = parameters
                    .and_then(|p| p.split_once(' '))
                    .and_then(|(message_id, text)| Some((message_id.parse::<u64>().ok()?, text.to_string())));

                Some(match parsed
                {
                    Some((message_id, text)) => Ok(PacketCode::EditRequest { message_id, text }),
                    None => Err(()),
                })
            },

            Command::Channel => Some(Ok(PacketCode::Channel { channel: parameters.map(str::to_string) })),

            //THE SERVER OPENS THE BOX, SINCE IT DECIDES WHAT WE MAY SEE
            Command::Profile => Some(Ok(PacketCode::ProfileRequest
            {
                target: parameters.map(str::trim).filter(|target| !target.is_empty()).map(str::to_string),
            })),

            Command::List => Some(Ok(PacketCode::ListRequest)),
            Command::Files => Some(Ok(PacketCode::FilesRequest)),

            //RESOLVE THE MONITOR LOCALLY
            #[cfg(feature = "client_screen")]
            Command::Screen =>
            {
                let sharing = screen_options::get_use_screen();
                let (selection, sound) = screen_parameters(parameters);

                //RESOLVE BEFORE STORING, SO AN UNKNOWN ONE FAILS
                let monitor = match selection.map(screen_capture::resolve_monitor)
                {
                    Some(Ok(monitor)) => Some(monitor),
                    Some(Err(_)) => return Some(Err(())),
                    None => None,
                };

                //A NEW SHARE, WITH SOUND UNLESS TOLD OTHERWISE
                if !sharing
                {
                    screen_options::set_monitor(monitor);
                    screen_options::set_share_audio(sound.unwrap_or(true));

                    return Some(Ok(PacketCode::ScreenRequest));
                }

                //SOUND NAMED: CHANGE THE RUNNING SHARE, SENDING NOTHING
                if let Some(sound) = sound
                {
                    screen_options::set_share_audio(sound);
                    if monitor.is_some() { screen_options::set_monitor(monitor); }

                    return None;
                }

                //NO MONITOR, OR THE CAPTURED ONE, ENDS THE SHARE
                let Some(monitor) = monitor.filter(|monitor| screen_capture::current_monitor().is_none_or(|current| current != *monitor)) else
                {
                    return Some(Ok(PacketCode::ScreenRequest));
                };

                //SWAP THE RUNNING CAPTURE OVER, SENDING NOTHING
                screen_options::set_monitor(Some(monitor));

                None
            },

            #[cfg(feature = "client_screen")] Command::Deattach => Some(Ok(PacketCode::DeattachRequest)),
            #[cfg(feature = "client_screen")] Command::Screens => Some(Ok(PacketCode::ScreensRequest)),
            #[cfg(feature = "client_screen")] Command::MuteScreen => Some(Ok(PacketCode::MuteScreenRequest)),

            //SAME PACKET AS /exit
            Command::Exit | Command::Logout => Some(Ok(PacketCode::Disconnect)),
            #[cfg(feature = "client_voice")] Command::Voice => Some(Ok(PacketCode::VoiceRequest)),

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

#[cfg(feature = "client_screen")]
pub fn screen_parameters(parameters: Option<&str>) -> (Option<&str>, Option<bool>) //MONITOR AND SOUND OF /screen
{
    let Some(parameters) = parameters.map(str::trim).filter(|p| !p.is_empty()) else { return (None, None) };

    //SOUND IS THE LAST WORD
    let (monitor, last) = match parameters.rsplit_once(char::is_whitespace)
    {
        Some((monitor, last)) => (Some(monitor.trim_end()), last),
        None => (None, parameters),
    };

    match last.to_ascii_lowercase().parse::<bool>()
    {
        Ok(sound) => (monitor, Some(sound)),
        Err(_) => (Some(parameters), None),
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
