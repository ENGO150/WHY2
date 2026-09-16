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

use wincode::{ SchemaWrite, SchemaRead };

use crate::
{
    role::Role,
    network::schema::
    {
        Offer,
        BoxedOffer,
        Reply,
        BoxedReply,
    },
};

//ENUMS
#[derive(SchemaWrite, SchemaRead, Clone)]
pub enum PacketCode //CONTROL CODES
{
    //CLIENT -> SERVER | TEXT MESSAGE REQUEST
    MessageRequest
    {
        text: String,
    },

    //SERVER -> CLIENT | TEXT MESSAGE
    Message
    {
        text: String,
        username: String,
        id: usize,
        colors: MessageColors,
    },

    //SERVER -> CLIENT | KEY EXCHANGE OFFER
    KeyExchangeOffer
    {
        #[wincode(with = "BoxedOffer")]
        offer: Box<Offer>,
    },

    //CLIENT -> SERVER | KEY EXCHANGE REPLY
    KeyExchangeReply
    {
        #[wincode(with = "BoxedReply")]
        reply: Box<Reply>,
    },

    //SERVER -> CLIENT | INFORMATIONS
    Welcome
    {
        min_pass: u64,
        max_uname: u64,
        min_uname: u64,
        server_name: String,
        server_uname: String,
        git_hash: String,
    },

    //CLIENT -> SERVER | PICK USERNAME
    Username
    {
        username: String,
        device: Option<Device>,
    },

    //SERVER -> CLIENT | START CHATTING
    Accept
    {
        id: usize,
        role: Role,
    },

    //SERVER -> CLIENT | CLIENT JOIN MESSAGE
    Join
    {
        username: String,
        username_color: Option<u8>,
        id: usize,
        device: Option<Device>,
    },

    //SERVER -> CLIENT | CLIENT LEAVE MESSAGE
    Leave
    {
        username: String,
        id: usize,
    },

    //CLIENT -> SERVER | SEND MESSAGE ONLY TO ONE CLIENT
    PrivateMessageRequest
    {
        text: String,
        id: usize,
    },

    //SERVER -> CLIENT | SEND MESSAGE ONLY TO ONE CLIENT
    PrivateMessage
    {
        text: String,
        username: String,
        id: usize,
    },

    //SERVER -> CLIENT | SEND MESSAGE BACK TO SENDER
    PrivateMessageBack
    {
        text: String,
        username: String,
        id: usize,
    },

    //SERVER -> CLIENT | CLIENT JOINED VOICE
    VoiceJoin
    {
        username: String,
        id: usize,
    },

    //SERVER -> CLIENT | FILE UPLOAD APPROVAL
    Upload
    {
        hash: [u8; 32],
        token: [u8; 32],
        uid: u64,
    },

    //CLIENT -> SERVER | DOWNLOAD FILE FROM SERVER
    DownloadRequest
    {
        id: usize,
        file_id: usize,
    },

    //CLIENT -> SERVER | REQUEST IMAGE UPLOAD
    ImageRequest
    {
        hash: [u8; 32],
        filename: String,
    },

    //SERVER -> CLIENT | IMAGE UPLOAD APPROVAL
    Image
    {
        hash: [u8; 32],
        token: [u8; 32],
        uid: u64,
    },

    //SERVER -> CLIENT | A STORED IMAGE, AS IT WAS UPLOADED
    ImageDisplay
    {
        username: String,
        filename: String,
        hash: [u8; 32],
        data: Option<Vec<u8>>,
        username_color: Option<u8>,
    },

    //SERVER -> CLIENT | ASK FOR A STORED PICTURE
    ImageData
    {
        hash: [u8; 32],
        data: Vec<u8>,
    },

    //SERVER -> CLIENT | ANNOUNCE NEW UPLOADED FILE
    Uploaded
    {
        filename: String,
        username: String,
    },

    //SERVER -> CLIENT | SCREENSHARE ATTACH APPROVAL
    Attach
    {
        username: String,
        token: [u8; 32],
    },

    //SERVER -> CLIENT | PRINT CONNECTED USERS
    List
    {
        online: Vec<OnlineUser>,
        offline: Option<Vec<OfflineUser>>,
    },

    //SERVER -> CLIENT | THE WHOLE BAN LIST
    ServerBans
    {
        users: Vec<BanEntry>,
        ips: Vec<BanEntry>,
    },

    //CLIENT -> SERVER | SET A USER'S ROLE
    ServerRoleRequest
    {
        id: usize,  //TARGET USER
        role: Role, //THE ROLE THEY ARE BEING GIVEN
    },

    //SERVER -> CLIENT | A ROLE WAS SET
    ServerRole
    {
        id: usize,                //WHO WAS RETITLED
        role: Role,               //THE ROLE THEY WERE GIVEN
        username: Option<String>, //THE TARGET | None = THE RECIPIENT THEMSELVES
    },

    //CLIENT <> SERVER | SET ONE CHAT COLOR
    Colors
    {
        username: bool, //TRUE = THE USERNAME'S COLOR, FALSE = THE MESSAGE'S
        color: u8,
    },

    //SERVER -> CLIENT | THE WHOLE server.toml
    ServerSettings
    {
        settings: Vec<ServerSetting>,
        save: bool, //FALSE = READ ANSWER, TRUE = SAVE ACK
    },

    Version { version: String },                    //SERVER <> CLIENT | THE SENDER'S PKG VERSION
    UsernameRequest,                                //SERVER -> CLIENT | PICK USERNAME
    LoginRequest,                                   //SERVER -> CLIENT | LOGIN
    RegisterRequest,                                //SERVER -> CLIENT | REGISTER
    Login { password: String },                     //CLIENT -> SERVER | LOGIN
    Register { password: String },                  //CLIENT -> SERVER | REGISTER
    History { messages: Vec<StoredMessage> },       //SERVER -> CLIENT | THE LOBBY'S STORED MESSAGES
    Re { message: String },                         //CLIENT -> SERVER | REPLY TO LAST PM
    Channel { channel: Option<String> },            //SERVER <> CLIENT | CHANNEL CHANGE
    ChannelCreated { name: String },                //SERVER -> CLIENT | CHANNEL CREATED
    ChannelDestroyed { name: String },              //SERVER -> CLIENT | CHANNEL ABANDONED
    VoiceClients { clients: Vec<(usize, String)> }, //SERVER -> CLIENT | THE CHANNEL'S VOICE ROSTER
    VoiceLeave { id: usize },                       //SERVER -> CLIENT | CLIENT LEFT VOICE
    UploadRequest { hash: [u8; 32] },               //CLIENT -> SERVER | REQUEST FILE UPLOAD
    Download { token: [u8; 32] },                   //SERVER -> CLIENT | DOWNLOAD FILE FROM SERVER
    ImageDuplicate { hash: [u8; 32] },              //SERVER -> CLIENT | IMAGE ALREADY UPLOADED
    ImageDataRequest { hash: [u8; 32] },            //CLIENT -> SERVER | ASK FOR A STORED PICTURE
    FilesRequest,                                   //CLIENT -> SERVER | REQUEST FILE LIST
    ListRequest,                                    //CLIENT -> SERVER | REQUEST CONNECTED USERS
    ScreensRequest,                                 //CLIENT -> SERVER | REQUEST SCREENSHARE LIST
    DeattachRequest,                                //CLIENT -> SERVER | DEATTACH CLIENT SCREENSHARE
    ScreenRequest,                                  //CLIENT -> SERVER | TOGGLE SCREENSHARE
    AttachRequest { id: usize },                    //CLIENT -> SERVER | ATTACH CLIENT SCREENSHARE
    VoiceRequest,                                   //CLIENT -> SERVER | ESTABLISH VOICE CONNECTION
    ServerBansRequest,                              //CLIENT -> SERVER | READ server_bans.toml
    ServerSettingsRequest,                          //CLIENT -> SERVER | READ server.toml
    Files { users: Vec<UserFile> },                 //SERVER -> CLIENT | LIST UPLOADED FILES
    Screens { users: Vec<UserScreen> },             //SERVER -> CLIENT | LIST SCREENSHARES
    Deattach { username: String },                  //SERVER -> CLIENT | DEATTACH CLIENT SCREENSHARE
    Attached { username: String },                  //SERVER -> CLIENT | CLIENT ATTACHED LOCAL CLIENT SHARE
    Deattached { username: String },                //SERVER -> CLIENT | CLIENT DEATTACHED LOCAL CLIENT SHARE
    Screen { token: Option<[u8; 32]> },             //SERVER -> CLIENT | SCREENSHARE APPROVAL | None = SHARE STOPPED
    Screenshare { username: String },               //SERVER -> CLIENT | CLIENT STARTED SCREENSHARING
    ScreenshareEnd { username: String },            //SERVER -> CLIENT | CLIENT STOPPED SCREENSHARING
    Voice { token: Option<[u8; 32]> },              //SERVER -> CLIENT | VOICE APPROVAL | None = VOICE LEFT

    ServerKick { id: usize },                            //CLIENT -> SERVER | KICK USER
    ServerMute { id: usize },                            //CLIENT -> SERVER | MUTE USER
    ServerBan { id: usize },                             //CLIENT -> SERVER | BAN USER
    ServerBanIp { id: usize },                           //CLIENT -> SERVER | BAN USER'S IP
    ServerPardon { id: usize },                          //CLIENT -> SERVER | LIFT A USERNAME BAN
    ServerPardonIp { id: usize },                        //CLIENT -> SERVER | LIFT AN IP BAN
    ServerSay { message: String },                       //CLIENT <> SERVER | SAY AS SERVER
    ServerSettingsSave { settings: Vec<ServerSetting> }, //CLIENT -> SERVER | WRITE server.toml

    FirstUser,        //SERVER -> CLIENT | FIRST ONE TO REGISTER, OWNER ROLE ADDED
    Rekey,            //SERVER -> CLIENT | TRIGGER KEY EXCHANGE (USED FOR RE-KEYING)
    Disconnect,       //SERVER <> CLIENT | QUIT COMMUNICATION
    SpamWarning,      //SERVER -> CLIENT | TELL CLIENT TO CALM TF DOWN
    RegisterDisabled, //SERVER -> CLIENT | REGISTRATION IS DISABLED
    UploadLimit,      //SERVER -> CLIENT | MAX CONCURRENT UPLOADS REACHED
    Muted,            //SERVER -> CLIENT | TELL CLIENT TO STFU
    InvalidUsage,     //SERVER -> CLIENT | INVALID PARAMETERS TO A COMMAND
    InvalidFeature,   //SERVER -> CLIENT | CLIENT REQUESTED DISABLED FEATURE
    KeepAlive,        //SERVER <> CLIENT | A BIT LESS STUPID KEEP-ALIVE
    ServerRestart,    //CLIENT -> SERVER | RESTART THE SERVER PROCESS
}

//IMPLEMENTATIONS
impl PacketCode
{
    //THE VARIANT'S NAME, FOR THE SERVER LOG
    pub fn name(&self) -> &'static str
    {
        match self
        {
            Self::MessageRequest { .. }        => "MessageRequest",
            Self::Message { .. }               => "Message",
            Self::KeyExchangeOffer { .. }      => "KeyExchangeOffer",
            Self::KeyExchangeReply { .. }      => "KeyExchangeReply",
            Self::Welcome { .. }               => "Welcome",
            Self::Accept { .. }                => "Accept",
            Self::Leave { .. }                 => "Leave",
            Self::PrivateMessageRequest { .. } => "PrivateMessageRequest",
            Self::PrivateMessage { .. }        => "PrivateMessage",
            Self::PrivateMessageBack { .. }    => "PrivateMessageBack",
            Self::Re { .. }                    => "Re",
            Self::VoiceJoin { .. }             => "VoiceJoin",
            Self::VoiceLeave { .. }            => "VoiceLeave",
            Self::UploadRequest { .. }         => "UploadRequest",
            Self::Upload { .. }                => "Upload",
            Self::DownloadRequest { .. }       => "DownloadRequest",
            Self::Download { .. }              => "Download",
            Self::ImageRequest { .. }          => "ImageRequest",
            Self::ImageDuplicate { .. }        => "ImageDuplicate",
            Self::Image { .. }                 => "Image",
            Self::ImageDisplay { .. }          => "ImageDisplay",
            Self::ImageDataRequest { .. }      => "ImageDataRequest",
            Self::ImageData { .. }             => "ImageData",
            Self::Uploaded { .. }              => "Uploaded",
            Self::AttachRequest { .. }         => "AttachRequest",
            Self::Attach { .. }                => "Attach",
            Self::ServerBansRequest { .. }     => "ServerBansRequest",
            Self::ServerBans { .. }            => "ServerBans",
            Self::ServerRoleRequest { .. }     => "ServerRoleRequest",
            Self::ServerRole { .. }            => "ServerRole",
            Self::ServerSettingsRequest { .. } => "ServerSettingsRequest",
            Self::ServerSettingsSave { .. }    => "ServerSettingsSave",
            Self::ServerSettings { .. }        => "ServerSettings",
            Self::Colors { .. }                => "Colors",
            Self::Version { .. }               => "Version",
            Self::UsernameRequest { .. }       => "UsernameRequest",
            Self::Username { .. }              => "Username",
            Self::LoginRequest { .. }          => "LoginRequest",
            Self::Login { .. }                 => "Login",
            Self::RegisterRequest { .. }       => "RegisterRequest",
            Self::Register { .. }              => "Register",
            Self::History { .. }               => "History",
            Self::Channel { .. }               => "Channel",
            Self::ChannelCreated { .. }        => "ChannelCreated",
            Self::ChannelDestroyed { .. }      => "ChannelDestroyed",
            Self::VoiceClients { .. }          => "VoiceClients",
            Self::FilesRequest { .. }          => "FilesRequest",
            Self::Files { .. }                 => "Files",
            Self::ScreensRequest { .. }        => "ScreensRequest",
            Self::Screens { .. }               => "Screens",
            Self::DeattachRequest { .. }       => "DeattachRequest",
            Self::Deattach { .. }              => "Deattach",
            Self::Attached { .. }              => "Attached",
            Self::Deattached { .. }            => "Deattached",
            Self::ScreenRequest { .. }         => "ScreenRequest",
            Self::Screen { .. }                => "Screen",
            Self::Screenshare { .. }           => "Screenshare",
            Self::ScreenshareEnd { .. }        => "ScreenshareEnd",
            Self::VoiceRequest { .. }          => "VoiceRequest",
            Self::Voice { .. }                 => "Voice",
            Self::Join { .. }                  => "Join",
            Self::ListRequest { .. }           => "ListRequest",
            Self::List { .. }                  => "List",
            Self::ServerKick { .. }            => "ServerKick",
            Self::ServerMute { .. }            => "ServerMute",
            Self::ServerBan { .. }             => "ServerBan",
            Self::ServerBanIp { .. }           => "ServerBanIp",
            Self::ServerPardon { .. }          => "ServerPardon",
            Self::ServerPardonIp { .. }        => "ServerPardonIp",
            Self::ServerSay { .. }             => "ServerSay",
            Self::FirstUser { .. }             => "FirstUser",
            Self::Rekey { .. }                 => "Rekey",
            Self::Disconnect { .. }            => "Disconnect",
            Self::SpamWarning { .. }           => "SpamWarning",
            Self::RegisterDisabled { .. }      => "RegisterDisabled",
            Self::UploadLimit { .. }           => "UploadLimit",
            Self::Muted { .. }                 => "Muted",
            Self::InvalidUsage { .. }          => "InvalidUsage",
            Self::InvalidFeature { .. }        => "InvalidFeature",
            Self::KeepAlive { .. }             => "KeepAlive",
            Self::ServerRestart { .. }         => "ServerRestart",
        }
    }
}

//STRUCTS
//ONE server.toml KEY AS THE CLIENT EDITS IT
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct ServerSetting
{
    pub key: String,
    pub value: SettingValue,
    pub section: String,     //THE '# Network' HEADING THE KEY SITS UNDER
    pub description: String, //THE TRAILING COMMENT ON THE KEY'S OWN LINE
    pub restart: bool,       //READ ONLY WHILE THE SERVER IS STARTING UP
}

//THE THREE DATATYPES config_read UNDERSTANDS
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub enum SettingValue
{
    Toggle(bool),
    Number(i64),
    Text(String),
}

//ONE MESSAGE AS server_messages.bin KEEPS IT
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct StoredMessage
{
    pub username: String,
    pub text: String,            //THE MESSAGE - OR THE FILENAME, WHEN THIS LINE IS AN IMAGE
    pub colors: MessageColors,
    pub image: Option<[u8; 32]>, //CONTENT HASH OF THE PICTURE
}

#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct MessageColors //COLORS OF MESSAGE
{
    pub username_color: Option<u8>, //COLOR OF USERNAME
    pub message_color: Option<u8>,  //COLOR OF MESSAGE
}

#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct UserFile //USER FILE LIST ITEM
{
    pub username: String,
    pub id: usize,
    pub upload: Vec<(String, usize)>,
}

#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct UserScreen //USER SCREEN SHARE LIST ITEM
{
    pub username: String,
    pub id: usize,
}

#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct BanEntry //ONE BANNED SUBJECT
{
    pub id: usize,
    pub subject: String,
}

#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct OnlineUser //USER CONNECTED TO THE SERVER
{
    pub username: String,
    pub username_color: Option<u8>,
    pub id: usize,
    pub channel: Option<String>,
    pub device: Option<Device>,
}

#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct OfflineUser //OFFLINE USER REGISTERED ON THE SERVER
{
    pub username: String,
    pub username_color: Option<u8>,
}

//ENUMS
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub enum Device
{
    TUI,
    Desktop,
    Phone,
}
