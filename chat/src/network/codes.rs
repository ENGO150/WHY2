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

//ENUMS
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub enum PacketCode //CONTROL CODES
{
    //CLIENT <> SERVER | TEXT MESSAGE
    Message
    {
        text: String,
        username: Option<String>,
        id: Option<usize>,
        colors: MessageColors,
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

    //SERVER -> CLIENT | START CHATTING
    Accept
    {
        id: usize,
        role: usize,
    },

    //SERVER -> CLIENT | CLIENT LEAVE MESSAGE
    Leave
    {
        username: String,
        id: usize,
    },

    //CLIENT <> SERVER | SEND MESSAGE ONLY TO ONE CLIENT
    PrivateMessage
    {
        text: String,
        username: Option<String>,
        id: usize,
    },

    //SERVER -> CLIENT | SEND MESSAGE BACK TO SENDER
    PrivateMessageBack
    {
        text: String,
        username: String,
        id: usize,
    },

    //SERVER -> CLIENT | CLIENT JOINED VOICE CHANNEL
    ChannelJoin
    {
        username: String,
        id: usize,
    },

    //SERVER -> CLIENT | CLIENT LEFT VOICE CHANNEL
    ChannelLeave
    {
        id: usize,
    },

    //CLIENT <> SERVER | REQUEST FILE UPLOAD (OR APPROVAL FROM SERVER)
    Upload
    {
        hash: [u8; 32],
        token: Option<[u8; 32]>,
        uid: Option<u64>,
    },

    //CLIENT <> SERVER | DOWNLOAD FILE FROM SERVER
    Download
    {
        id: Option<usize>,
        file_id: Option<usize>,
        token: Option<[u8; 32]>,
    },

    //SERVER -> CLIENT | ANNOUNCE NEW UPLOADED FILE
    Uploaded
    {
        filename: String,
        username: String,
    },

    //CLIENT <> SERVER | ATTACH CLIENT SCREENSHARE
    Attach
    {
        id: Option<usize>,
        username: Option<String>,
        token: Option<[u8; 32]>,
    },

    //CLIENT <> SERVER | READ AND WRITE server.toml
    ServerSettings
    {
        //REQUEST: None | SERVER ANSWER: THE WHOLE CONFIG | SAVE: THE ROWS THAT CHANGED
        settings: Option<Vec<ServerSetting>>,

        //FALSE = READ, TRUE = WRITE - AND THE SERVER ACKNOWLEDGES A WRITE WITH THE STORED CONFIG BACK
        save: bool,
    },

    Version { version: Option<String> },            //SERVER <> CLIENT | ASK CLIENT FOR THEIR PKG VERSION
    Username { username: Option<String> },          //SERVER <> CLIENT | PICK USERNAME
    PasswordL { password: Option<String> },         //SERVER -> CLIENT | LOGIN
    PasswordR { password: Option<String> },         //SERVER -> CLIENT | REGISTER
    Channel { channel: Option<String> },            //SERVER <> CLIENT | CHANNEL CHANGE
    ChannelCreated { name: String },                //SERVER -> CLIENT | CHANNEL CREATED
    ChannelDestroyed { name: String },              //SERVER -> CLIENT | CHANNEL ABANDONED
    VoiceClients { clients: Vec<(usize, String)> }, //SERVER -> CLIENT | TELL CLIENT ALL CONNECTED VOICE CLIENTS
    Files { users: Option<Vec<UserFile>> },         //CLIENT <> SERVER | LIST UPLOADED FILES
    Screens { users: Option<Vec<UserScreen>> },     //CLIENT <> SERVER | LIST SCREENSHARES
    Deattach { username: Option<String> },          //CLIENT <> SERVER | DEATTACH CLIENT SCREENSHARE
    Screen { token: Option<[u8; 32]> },             //CLIENT <> SERVER | TOGGLE SCREENSHARE
    Voice { token: Option<[u8; 32]> },              //CLIENT <> SERVER | ESTABLISH VOICE CONNECTION
    KeyExchange { ecc: String, pq: String },        //SERVER <> CLIENT | KEY EXCHANGE
    Join { username: String },                      //SERVER -> CLIENT | CLIENT JOIN MESSAGE
    List { users: Option<Vec<OnlineUser>> },        //CLIENT <> SERVER | PRINT CONNECTED USERS
    ServerKick { id: usize },                       //CLIENT -> SERVER | KICK USER
    ServerMute { id: usize },                       //CLIENT -> SERVER | MUTE USER
    ServerBan { id: usize },                        //CLIENT -> SERVER | BAN USER

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

//STRUCTS
//ONE server.toml KEY AS THE CLIENT EDITS IT. THE SERVER IS THE ONLY PLACE THAT KNOWS WHICH KEYS EXIST,
//SO IT SENDS THE HEADING AND THE TRAILING COMMENT ALONG - THE CLIENT RENDERS WHATEVER IT IS GIVEN
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct ServerSetting
{
    pub key: String,
    pub value: SettingValue,
    pub section: String,     //THE '# Network' HEADING THE KEY SITS UNDER
    pub description: String, //THE TRAILING COMMENT ON THE KEY'S OWN LINE
    pub restart: bool,       //THE SERVER READS THIS ONE ONLY WHILE STARTING UP (consts::SERVER_RESTART_SETTINGS)
}

//THE THREE DATATYPES config_read UNDERSTANDS - A VALUE THAT COMES BACK AS A DIFFERENT ONE IS REFUSED
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub enum SettingValue
{
    Toggle(bool),
    Number(i64),
    Text(String),
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
pub struct OnlineUser //USER CONNECTED TO THE SERVER
{
    pub username: String,
    pub id: usize,
    pub channel: Option<String>,
}
