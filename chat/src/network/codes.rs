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

//! # Packet Codes
//!
//! [`PacketCode`] is every message of the main-connection protocol, plus the plain data types
//! the messages carry.
//!
//! # Conventions
//! - Each variant's documentation starts with its direction: *client → server*,
//!   *server → client*, or *both ways*.
//! - A request from the client is usually named `...Request`, and the server's answer by the
//!   bare name (`ListRequest` → `List`).
//! - Anything that names the sender — its id, its username, its colors — is filled in by the
//!   server, never trusted from the client.
//! - A save acknowledgement carries the **whole** stored state again (`save: true`) rather than
//!   an "ok", so whatever the server refused snaps back on the client.
//!
//! # Compatibility
//! Packets are encoded with `wincode`, which identifies variants by position: new variants go
//! at the end, and existing variants and fields must not be reordered.

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
/// Every message of the main-connection protocol.
#[derive(SchemaWrite, SchemaRead, Clone)]
pub enum PacketCode //CONTROL CODES
{
    //CLIENT -> SERVER | TEXT MESSAGE REQUEST
    /// *Client → server.* Sends a chat message to the current channel.
    MessageRequest
    {
        /// The message text.
        text: String,
        /// The id of the stored message this replies to.
        reply: Option<u64>,
    },

    //SERVER -> CLIENT | TEXT MESSAGE
    /// *Server → client.* A chat message.
    Message
    {
        /// The message text.
        text: String,
        /// The sender's username.
        username: String,
        /// The sender's session id.
        id: usize,
        /// Server-assigned message id.
        message_id: u64,
        /// The sender's current colors, looked up by the server.
        colors: MessageColors,
        /// Where it was said: `Some(None)` is the lobby, `Some(Some(name))` a channel, and `None`
        /// every pane.
        channel: Option<Option<String>>,
        /// When it was sent, in Unix seconds, if timestamps are on.
        timestamp: Option<u64>, //UNIX SECONDS
        /// The id of the message this replies to.
        reply: Option<u64>,     //ID OF THE MESSAGE REPLIED TO
    },

    //SERVER -> CLIENT | KEY EXCHANGE OFFER
    /// *Server → client.* The server's signed ephemeral keys; the first step of a key exchange.
    KeyExchangeOffer
    {
        #[wincode(with = "BoxedOffer")]
        offer: Box<Offer>,
    },

    //CLIENT -> SERVER | KEY EXCHANGE REPLY
    /// *Client → server.* The client's ephemeral key and ML-KEM ciphertext; completes a key
    /// exchange.
    KeyExchangeReply
    {
        #[wincode(with = "BoxedReply")]
        reply: Box<Reply>,
    },

    //SERVER -> CLIENT | INFORMATIONS
    /// *Server → client.* The server's name and login rules, sent once keys are established.
    Welcome
    {
        /// Minimum password length.
        min_pass: u64,
        /// Maximum username length.
        max_uname: u64,
        /// Minimum username length.
        min_uname: u64,
        /// The server's display name.
        server_name: String,
        /// The name the server signs its own messages with.
        server_uname: String,
        /// Git commit the server was built from.
        git_hash: String,
    },

    //CLIENT -> SERVER | PICK USERNAME
    /// *Client → server.* The username the client logs in or registers as.
    Username
    {
        username: String,
        /// The kind of client, shown to other users.
        device: Option<Device>,
    },

    //SERVER -> CLIENT | START CHATTING
    /// *Server → client.* Login succeeded; the client may start chatting.
    Accept
    {
        /// The client's session id.
        id: usize,
        /// The account's role.
        role: Role,
        /// The account's colors.
        colors: MessageColors,
    },

    //SERVER -> CLIENT | A PAGE OF THE LOBBY'S STORED MESSAGES
    /// *Server → client.* A page of the lobby's stored messages, oldest first.
    History
    {
        /// The messages.
        messages: Vec<StoredMessage>,
        /// Id of the first message; the cursor for the next older page.
        start: u64,  //ABSOLUTE INDEX OF THE FIRST ONE
        /// Whether older messages remain.
        more: bool,  //OLDER ONES LEFT
        /// Total number of messages kept.
        kept: u64,   //MESSAGES KEPT
        /// Whether this answers a [`HistoryRequest`](PacketCode::HistoryRequest) rather than being the
        /// login page.
        older: bool, //AN ANSWER TO HistoryRequest
    },

    //SERVER -> CLIENT | CLIENT JOIN MESSAGE
    /// *Server → client.* A user connected.
    Join
    {
        username: String,
        username_color: Option<u8>,
        id: usize,
        device: Option<Device>,
        role: Role,
    },

    //SERVER -> CLIENT | CLIENT LEAVE MESSAGE
    /// *Server → client.* A user disconnected.
    Leave
    {
        username: String,
        id: usize,
        /// `false` if the account was deleted with the session.
        registered: bool, //FALSE = THE ACCOUNT WENT WITH THEM
    },

    //CLIENT -> SERVER | SEND MESSAGE ONLY TO ONE CLIENT
    /// *Client → server.* Sends a private message to one session.
    PrivateMessageRequest
    {
        text: String,
        /// The recipient's session id.
        id: usize,
    },

    //SERVER -> CLIENT | SEND MESSAGE ONLY TO ONE CLIENT
    /// *Server → client.* A private message to us.
    PrivateMessage
    {
        text: String,
        username: String,
        id: usize,
        colors: MessageColors,
    },

    //SERVER -> CLIENT | SEND MESSAGE BACK TO SENDER
    /// *Server → client.* Echo of a private message we sent, naming the recipient.
    ///
    /// `colors` carries the recipient's username color and the sender's message color, so the line
    /// is painted the way it is drawn.
    PrivateMessageBack
    {
        text: String,
        username: String,
        id: usize,
        colors: MessageColors,
    },

    //CLIENT -> SERVER | REWORD A STORED MESSAGE
    /// *Client → server.* Rewords one of our stored messages.
    EditRequest
    {
        message_id: u64,
        text: String,
    },

    //SERVER -> CLIENT | A MESSAGE WAS REWORDED
    /// *Server → client.* A stored message was reworded.
    Edited
    {
        message_id: u64,
        text: String,
    },

    //SERVER -> CLIENT | A MESSAGE WAS PINNED OR UNPINNED
    /// *Server → client.* A stored message was pinned or unpinned.
    Pinned
    {
        message_id: u64,
        pinned: bool,
    },

    //SERVER -> CLIENT | EVERY PINNED MESSAGE
    /// *Server → client.* Every pinned message.
    Pins
    {
        messages: Vec<StoredMessage>,
    },

    //SERVER -> CLIENT | HEART REACTION
    /// *Server → client.* The new set of usernames that hearted a message.
    Heart
    {
        message_id: u64,
        hearts: Vec<String>,
    },

    //SERVER -> CLIENT | CLIENT JOINED VOICE
    /// *Server → client.* A user joined voice in `channel` (`None` is the lobby).
    VoiceJoin
    {
        username: String,
        id: usize,
        channel: Option<String>,
    },

    //SERVER -> CLIENT | FILE UPLOAD APPROVAL
    /// *Server → client.* A file upload was approved; the upload goes over a new socket
    /// authenticated by `token`.
    Upload
    {
        hash: [u8; 32],
        token: [u8; 32],
        uid: u64,
    },

    //CLIENT -> SERVER | DOWNLOAD FILE FROM SERVER
    /// *Client → server.* Downloads file `file_id` shared by session `id`.
    DownloadRequest
    {
        id: usize,
        file_id: usize,
    },

    //CLIENT -> SERVER | REQUEST IMAGE UPLOAD
    /// *Client → server.* Asks to post a picture.
    ImageRequest
    {
        hash: [u8; 32],
        filename: String,
    },

    //SERVER -> CLIENT | IMAGE UPLOAD APPROVAL
    /// *Server → client.* A picture upload was approved; the upload goes over a new socket
    /// authenticated by `token`.
    Image
    {
        hash: [u8; 32],
        token: [u8; 32],
        uid: u64,
    },

    //SERVER -> CLIENT | A STORED IMAGE, AS IT WAS UPLOADED
    /// *Server → client.* A picture posted to the chat.
    ///
    /// `data` carries the picture the first time the server broadcasts it; a repost of a picture
    /// the server already holds goes out as `None`, and a client without it in its cache fetches it
    /// with [`ImageDataRequest`](PacketCode::ImageDataRequest).
    ImageDisplay
    {
        /// The poster's username.
        username: String,
        /// The uploaded file's name.
        filename: String,
        /// Server-assigned message id.
        message_id: u64,
        /// SHA-256 of the picture.
        hash: [u8; 32],
        /// The picture bytes, if sent.
        data: Option<Vec<u8>>,
        /// The poster's username color.
        username_color: Option<u8>,
        /// Where it was posted; as in [`Message`](PacketCode::Message).
        channel: Option<Option<String>>,
        /// When it was posted, in Unix seconds, if timestamps are on.
        timestamp: Option<u64>, //UNIX SECONDS
    },

    //SERVER -> CLIENT | A VOICE MESSAGE, AS IT WAS RECORDED
    /// *Server → client.* A voice message posted to the chat; `data` follows the same rule as in
    /// [`ImageDisplay`](PacketCode::ImageDisplay).
    VoiceMessage
    {
        username: String,
        message_id: u64,
        voice: VoiceNote,
        data: Option<Vec<u8>>,
        username_color: Option<u8>,
        channel: Option<Option<String>>,
        timestamp: Option<u64>, //UNIX SECONDS
    },

    //SERVER -> CLIENT | VOICE MESSAGE UPLOAD APPROVAL
    /// *Server → client.* A voice message upload was approved.
    VoiceMessageUpload
    {
        hash: [u8; 32],
        token: [u8; 32],
        uid: u64,
    },

    //SERVER -> CLIENT | ASK FOR A STORED PICTURE
    /// *Server → client.* A stored picture or clip, answering
    /// [`ImageDataRequest`](PacketCode::ImageDataRequest).
    ///
    /// Empty `data` means the server holds nothing under that hash.
    ImageData
    {
        hash: [u8; 32],
        data: Vec<u8>,
    },

    //SERVER -> CLIENT | ANNOUNCE NEW UPLOADED FILE
    /// *Server → client.* A user shared a file.
    Uploaded
    {
        filename: String,
        username: String,
    },

    //SERVER -> CLIENT | SCREENSHARE ATTACH APPROVAL
    /// *Server → client.* Attaching to a screen share was approved; the viewer connects with
    /// `token`.
    Attach
    {
        username: String,
        token: [u8; 32],
    },

    //SERVER -> CLIENT | PRINT CONNECTED USERS
    /// *Server → client.* The connected users, and the registered ones offline.
    List
    {
        online: Vec<OnlineUser>,
        /// `None` when the server does not disclose offline users (`show_offline_users`).
        offline: Option<Vec<OfflineUser>>,
    },

    //SERVER -> CLIENT | THE WHOLE BAN LIST
    /// *Server → client.* The whole ban list.
    ServerBans
    {
        users: Vec<BanEntry>,
        ips: Vec<BanEntry>,
    },

    //CLIENT -> SERVER | SET A USER'S ROLE
    /// *Client → server.* Sets a user's role.
    ServerRoleRequest
    {
        /// A username, or a live session id.
        target: String,
        role: Role,
    },

    //SERVER -> CLIENT | A ROLE WAS SET
    /// *Server → client.* A role was set.
    ServerRole
    {
        username: String,
        role: Role,
    },

    //SERVER -> CLIENT | THE WHOLE server.toml
    /// *Server → client.* Every key of `server.toml`.
    ServerSettings
    {
        settings: Vec<ServerSetting>,
        /// `false` for an answer to a read, `true` for the acknowledgement of a save.
        save: bool, //FALSE = READ ANSWER, TRUE = SAVE ACK
    },

    //CLIENT -> SERVER | PASSWORD CHANGE REQUEST
    /// *Client → server.* Changes our password.
    AccountPasswdRequest
    {
        old_password: String,
        new_password: String,
    },

    //CLIENT -> SERVER | ACCOUNT DELETION REQUEST
    /// *Client → server.* Deletes our account.
    AccountDeleteRequest { password: String },

    //CLIENT -> SERVER | SET ONE CHAT COLOR
    /// *Client → server.* Sets one of our colors.
    ColorRequest
    {
        /// `true` for the username color, `false` for the message color.
        username: bool, //TRUE = THE USERNAME'S COLOR, FALSE = THE MESSAGE'S
        /// The wire color code.
        color: u8,
    },

    //SERVER -> CLIENT | THE SERVER'S PICTURE
    /// *Server → client.* The server icon.
    ServerIcon
    {
        hash: Option<[u8; 32]>,
        /// `true` only for the acknowledgement of our own change.
        save: bool, //FALSE = READ ANSWER OR SOMEBODY ELSE'S CHANGE, TRUE = SAVE ACK
    },

    //SERVER -> CLIENT | ONE USER'S PROFILE
    /// *Server → client.* One user's profile.
    Profile
    {
        username: String,
        profile: UserProfile,
        /// Whether it is ours, and so editable.
        own: bool,  //OURS, SO EDITABLE
        /// `false` for an answer to a read, `true` for the acknowledgement of a save.
        save: bool, //FALSE = READ ANSWER, TRUE = SAVE ACK
    },

    /// *Client → server.* Reads a profile; `None` for our own.
    ProfileRequest { target: Option<String> },         //CLIENT -> SERVER | READ A PROFILE | None = OUR OWN
    /// *Client → server.* Stores our profile's typed fields.
    ProfileSave { profile: UserProfile },              //CLIENT -> SERVER | WRITE OUR OWN PROFILE
    /// *Client → server.* Sets our avatar to a picture; `None` drops it.
    AvatarRequest { hash: Option<[u8; 32]> },          //CLIENT -> SERVER | SET OUR PICTURE | None = DROP IT
    /// *Server → client.* Somebody in the channel is typing.
    Typing { username: String },                       //SERVER -> CLIENT | SOMEBODY IN THE CHANNEL IS WRITING
    /// *Both ways.* The sender's package version.
    Version { version: String },                       //SERVER <> CLIENT | THE SENDER'S PKG VERSION
    /// *Server → client.* Asks for a username.
    UsernameRequest,                                   //SERVER -> CLIENT | PICK USERNAME
    /// *Server → client.* Asks for the password of an existing account.
    LoginRequest,                                      //SERVER -> CLIENT | LOGIN
    /// *Server → client.* Asks for a password to register a new account.
    RegisterRequest,                                   //SERVER -> CLIENT | REGISTER
    /// *Client → server.* The password of an existing account.
    Login { password: String },                        //CLIENT -> SERVER | LOGIN
    /// *Client → server.* The password for a new account.
    Register { password: String },                     //CLIENT -> SERVER | REGISTER
    /// *Client → server.* Asks for the history page before message `before`.
    HistoryRequest { before: u64 },                    //CLIENT -> SERVER | THE PAGE BEFORE THIS MESSAGE ID
    /// *Client → server.* Deletes a stored message.
    DeleteRequest { message_id: u64 },                 //CLIENT -> SERVER | DELETE A STORED MESSAGE
    /// *Server → client.* A stored message was deleted.
    Deleted { message_id: u64 },                       //SERVER -> CLIENT | A MESSAGE WAS DELETED
    /// *Client → server.* Answers the last private message.
    Re { message: String },                            //CLIENT -> SERVER | REPLY TO LAST PM
    /// *Client → server.* Toggles our heart on a message.
    HeartRequest { message_id: u64 },                  //CLIENT -> SERVER | REQUEST HEART REACTION TO MESSAGE
    /// *Client → server.* Toggles a message's pin.
    PinRequest { message_id: u64 },                    //CLIENT -> SERVER | TOGGLE A MESSAGE'S PIN
    /// *Both ways.* Switches channel; `None` is the lobby.
    Channel { channel: Option<String> },               //SERVER <> CLIENT | CHANNEL CHANGE
    /// *Server → client.* A channel was created.
    ChannelCreated { name: String },                   //SERVER -> CLIENT | CHANNEL CREATED
    /// *Server → client.* A channel was abandoned.
    ChannelDestroyed { name: String },                 //SERVER -> CLIENT | CHANNEL ABANDONED
    /// *Server → client.* The voice roster of our channel, as `(id, username)`, ourselves excluded.
    VoiceClients { clients: Vec<(usize, String)> },    //SERVER -> CLIENT | THE CHANNEL'S VOICE ROSTER
    /// *Server → client.* A user left voice in `channel`.
    VoiceLeave { id: usize, channel: Option<String> }, //SERVER -> CLIENT | CLIENT LEFT VOICE
    /// *Client → server.* Asks to share a file.
    UploadRequest { hash: [u8; 32] },                  //CLIENT -> SERVER | REQUEST FILE UPLOAD
    /// *Server → client.* A download was approved; the client connects with `token`.
    Download { token: [u8; 32] },                      //SERVER -> CLIENT | DOWNLOAD FILE FROM SERVER
    /// *Server → client.* The server already holds this picture, so nothing needs uploading.
    ImageDuplicate { hash: [u8; 32] },                 //SERVER -> CLIENT | IMAGE ALREADY UPLOADED
    /// *Client → server.* Fetches a stored picture or clip.
    ImageDataRequest { hash: [u8; 32] },               //CLIENT -> SERVER | ASK FOR A STORED PICTURE
    /// *Client → server.* Asks to post a voice message.
    VoiceMessageRequest { hash: [u8; 32] },            //CLIENT -> SERVER | REQUEST VOICE MESSAGE UPLOAD
    /// *Client → server.* Asks for the shared files.
    FilesRequest,                                      //CLIENT -> SERVER | REQUEST FILE LIST
    /// *Client → server.* Asks for the user list.
    ListRequest,                                       //CLIENT -> SERVER | REQUEST CONNECTED USERS
    /// *Client → server.* Asks for the pinned messages.
    PinsRequest,                                       //CLIENT -> SERVER | REQUEST PINNED MESSAGES
    /// *Client → server.* We are typing.
    TypingRequest,                                     //CLIENT -> SERVER | WE ARE WRITING A MESSAGE
    /// *Client → server.* Asks for the running screen shares.
    ScreensRequest,                                    //CLIENT -> SERVER | REQUEST SCREENSHARE LIST
    /// *Client → server.* Stops watching a screen share.
    DeattachRequest,                                   //CLIENT -> SERVER | DEATTACH CLIENT SCREENSHARE
    /// *Client → server.* Toggles the sound of the watched screen share.
    MuteScreenRequest,                                 //CLIENT -> SERVER | MUTE SCREEN SHARE
    /// *Server → client.* The watched screen share's new mute state.
    MuteScreen { muted: bool },                        //SERVER -> CLIENT | MUTE SCREEN SHARE ACK
    /// *Client → server.* Starts or stops our screen share.
    ScreenRequest,                                     //CLIENT -> SERVER | TOGGLE SCREENSHARE
    /// *Client → server.* Watches session `id`'s screen share.
    AttachRequest { id: usize },                       //CLIENT -> SERVER | ATTACH CLIENT SCREENSHARE
    /// *Client → server.* Joins or leaves voice.
    VoiceRequest,                                      //CLIENT -> SERVER | ESTABLISH VOICE CONNECTION
    /// *Client → server.* Reads the ban list.
    ServerBansRequest,                                 //CLIENT -> SERVER | READ server_bans.toml
    /// *Client → server.* Reads `server.toml`.
    ServerSettingsRequest,                             //CLIENT -> SERVER | READ server.toml
    /// *Client → server.* Reads the server icon.
    ServerIconRequest,                                 //CLIENT -> SERVER | READ THE SERVER'S PICTURE
    /// *Server → client.* The shared files.
    Files { users: Vec<UserFile> },                    //SERVER -> CLIENT | LIST UPLOADED FILES
    /// *Server → client.* The running screen shares.
    Screens { users: Vec<UserScreen> },                //SERVER -> CLIENT | LIST SCREENSHARES
    /// *Server → client.* We stopped watching a share.
    Deattach { username: String },                     //SERVER -> CLIENT | DEATTACH CLIENT SCREENSHARE
    /// *Server → client.* Somebody started watching our share.
    Attached { username: String },                     //SERVER -> CLIENT | CLIENT ATTACHED LOCAL CLIENT SHARE
    /// *Server → client.* Somebody stopped watching our share.
    Deattached { username: String },                   //SERVER -> CLIENT | CLIENT DEATTACHED LOCAL CLIENT SHARE
    /// *Server → client.* Our screen share was approved with `token`; `None` means it stopped.
    Screen { token: Option<[u8; 32]> },                //SERVER -> CLIENT | SCREENSHARE APPROVAL | None = SHARE STOPPED
    /// *Server → client.* A user started sharing their screen.
    Screenshare { username: String },                  //SERVER -> CLIENT | CLIENT STARTED SCREENSHARING
    /// *Server → client.* A user stopped sharing their screen.
    ScreenshareEnd { username: String },               //SERVER -> CLIENT | CLIENT STOPPED SCREENSHARING
    /// *Server → client.* Voice was approved with `token`; `None` means we left.
    Voice { token: Option<[u8; 32]> },                 //SERVER -> CLIENT | VOICE APPROVAL | None = VOICE LEFT
    /// *Server → client.* A color was set.
    Color,                                             //SERVER -> CLIENT | COLOR SET

    /// *Client → server.* Kicks a session.
    ServerKick { id: usize },                            //CLIENT -> SERVER | KICK USER
    /// *Client → server.* Toggles a session's server-side mute.
    ServerMute { id: usize },                            //CLIENT -> SERVER | MUTE USER
    /// *Client → server.* Bans an account, by username or live session id.
    ServerBan { target: String },                        //CLIENT -> SERVER | BAN USER
    /// *Client → server.* Bans a session's address.
    ServerBanIp { id: usize },                           //CLIENT -> SERVER | BAN USER'S IP
    /// *Client → server.* Lifts the username ban numbered `id`.
    ServerPardon { id: usize },                          //CLIENT -> SERVER | LIFT A USERNAME BAN
    /// *Client → server.* Lifts the address ban numbered `id`.
    ServerPardonIp { id: usize },                        //CLIENT -> SERVER | LIFT AN IP BAN
    /// *Both ways.* A message said as the server.
    ServerSay { message: String },                       //CLIENT <> SERVER | SAY AS SERVER
    /// *Client → server.* Writes changed keys of `server.toml`.
    ServerSettingsSave { settings: Vec<ServerSetting> }, //CLIENT -> SERVER | WRITE server.toml
    /// *Client → server.* Sets the server icon; `None` drops it.
    ServerIconSave { hash: Option<[u8; 32]> },           //CLIENT -> SERVER | SET THE SERVER'S PICTURE | None = DROP IT

    /// *Server → client.* Password change result; `false` means the old password was wrong.
    AccountPasswd { valid: bool }, //SERVER -> CLIENT | PASSWORD CHANGE ACK (FALSE = INVALID PASSWORD)
    /// *Server → client.* Account deletion result; `false` means the password was wrong.
    AccountDelete { valid: bool }, //SERVER -> CLIENT | ACCOUNT DELETION ACK (FALSE = INVALID PASSWORD)

    /// *Server → client.* We are the first to register and became the owner.
    FirstUser,        //SERVER -> CLIENT | FIRST ONE TO REGISTER, OWNER ROLE ADDED
    /// *Server → client.* Starts a key exchange to rotate the session keys.
    Rekey,            //SERVER -> CLIENT | TRIGGER KEY EXCHANGE (USED FOR RE-KEYING)
    /// *Both ways.* Ends the session.
    Disconnect,       //SERVER <> CLIENT | QUIT COMMUNICATION
    /// *Server → client.* We are sending messages too fast.
    SpamWarning,      //SERVER -> CLIENT | TELL CLIENT TO CALM TF DOWN
    /// *Server → client.* Registration is disabled.
    RegisterDisabled, //SERVER -> CLIENT | REGISTRATION IS DISABLED
    /// *Server → client.* We have too many uploads in flight.
    UploadLimit,      //SERVER -> CLIENT | MAX CONCURRENT UPLOADS REACHED
    /// *Server → client.* We are muted.
    Muted,            //SERVER -> CLIENT | TELL CLIENT TO STFU
    /// *Server → client.* A command had invalid parameters or was not permitted.
    InvalidUsage,     //SERVER -> CLIENT | INVALID PARAMETERS TO A COMMAND
    /// *Server → client.* The requested feature is disabled on this server.
    InvalidFeature,   //SERVER -> CLIENT | CLIENT REQUESTED DISABLED FEATURE
    /// *Both ways.* Keeps an idle connection alive.
    KeepAlive,        //SERVER <> CLIENT | A BIT LESS STUPID KEEP-ALIVE
    /// *Client → server.* Restarts the server process.
    ServerRestart,    //CLIENT -> SERVER | RESTART THE SERVER PROCESS
}

//IMPLEMENTATIONS
impl PacketCode
{
    //THE VARIANT'S NAME, FOR THE SERVER LOG
    /// Returns the variant's name, for logging.
    ///
    /// Only the name: a packet's fields are the users' data and must never reach a log, which is
    /// why [`PacketCode`] does not derive `Debug`.
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
            Self::EditRequest { .. }           => "EditRequest",
            Self::Edited { .. }                => "Edited",
            Self::HeartRequest { .. }          => "HeartRequest",
            Self::Heart { .. }                 => "Heart",
            Self::PinRequest { .. }            => "PinRequest",
            Self::Pinned { .. }                => "Pinned",
            Self::PinsRequest { .. }           => "PinsRequest",
            Self::Pins { .. }                  => "Pins",
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
            Self::VoiceMessageRequest { .. }   => "VoiceMessageRequest",
            Self::VoiceMessageUpload { .. }    => "VoiceMessageUpload",
            Self::VoiceMessage { .. }          => "VoiceMessage",
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
            Self::ServerIconRequest { .. }     => "ServerIconRequest",
            Self::ServerIconSave { .. }        => "ServerIconSave",
            Self::ServerIcon { .. }            => "ServerIcon",
            Self::AccountPasswdRequest { .. }  => "AccountPasswdRequest",
            Self::AccountPasswd { .. }         => "AccountPasswd",
            Self::AccountDeleteRequest { .. }  => "AccountDeleteRequest",
            Self::AccountDelete { .. }         => "AccountDelete",
            Self::ColorRequest { .. }          => "ColorRequest",
            Self::Color { .. }                 => "Color",
            Self::ProfileRequest { .. }        => "ProfileRequest",
            Self::ProfileSave { .. }           => "ProfileSave",
            Self::AvatarRequest { .. }         => "AvatarRequest",
            Self::Profile { .. }               => "Profile",
            Self::TypingRequest { .. }         => "TypingRequest",
            Self::Typing { .. }                => "Typing",
            Self::Version { .. }               => "Version",
            Self::UsernameRequest { .. }       => "UsernameRequest",
            Self::Username { .. }              => "Username",
            Self::LoginRequest { .. }          => "LoginRequest",
            Self::Login { .. }                 => "Login",
            Self::RegisterRequest { .. }       => "RegisterRequest",
            Self::Register { .. }              => "Register",
            Self::History { .. }               => "History",
            Self::HistoryRequest { .. }        => "HistoryRequest",
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
            Self::MuteScreenRequest { .. }     => "MuteScreenRequest",
            Self::MuteScreen { .. }            => "MuteScreen",
            Self::VoiceRequest { .. }          => "VoiceRequest",
            Self::Voice { .. }                 => "Voice",
            Self::Join { .. }                  => "Join",
            Self::ListRequest { .. }           => "ListRequest",
            Self::List { .. }                  => "List",
            Self::DeleteRequest { .. }         => "DeleteRequest",
            Self::Deleted { .. }               => "Deleted",
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

impl UserProfile
{
    //THE KEYS server_users.toml STORES THE FIELDS UNDER, IN THEIR OWN ORDER
    /// The keys the typed profile fields are stored under, in order.
    ///
    /// This is the single list of profile fields: the storage, the wire and the client's profile
    /// box all walk it.
    pub const KEYS: [&'static str; 4] = [ "bio", "pronouns", "website", "status" ];

    //THE FIELDS IN THE ORDER server_users.toml SPELLS THEM
    /// Returns the typed profile fields, in [`KEYS`](Self::KEYS) order.
    pub fn fields(&self) -> [&str; 4]
    {
        [ &self.bio, &self.pronouns, &self.website, &self.status ]
    }

    //ONE FIELD BY THE KEY IT IS STORED UNDER
    /// Returns a mutable reference to a typed field by its key.
    pub fn field_mut(&mut self, key: &str) -> Option<&mut String>
    {
        match key
        {
            "bio" => Some(&mut self.bio),
            "pronouns" => Some(&mut self.pronouns),
            "website" => Some(&mut self.website),
            "status" => Some(&mut self.status),

            _ => None,
        }
    }
}

//STRUCTS
//ONE server.toml KEY AS THE CLIENT EDITS IT
/// One key of `server.toml`, as the client edits it.
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct ServerSetting
{
    /// The key.
    pub key: String,
    /// Its value.
    pub value: SettingValue,
    /// The `#` heading the key sits under.
    pub section: String,     //THE '# Network' HEADING THE KEY SITS UNDER
    /// The trailing comment on the key's line.
    pub description: String, //THE TRAILING COMMENT ON THE KEY'S OWN LINE
    /// Whether the key is only read at startup.
    pub restart: bool,       //READ ONLY WHILE THE SERVER IS STARTING UP
}

//THE THREE DATATYPES config_read UNDERSTANDS
/// The value of a server setting; the datatypes the configuration reader understands.
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub enum SettingValue
{
    /// A boolean.
    Toggle(bool),
    /// An integer.
    Number(i64),
    /// A string.
    Text(String),
}

//ONE USER'S PROFILE AS server_users.toml KEEPS IT
/// One user's profile.
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq, Default)]
pub struct UserProfile
{
    /// What the user says about themselves.
    pub bio: String,              //WHAT THE USER SAYS ABOUT THEMSELVES
    /// Their pronouns.
    pub pronouns: String,         //HOW THEY ARE REFERRED TO
    /// A link of theirs; `http`/`https` only.
    pub website: String,          //A LINK OF THEIRS, http/https ONLY
    /// What they are up to, until they change it.
    pub status: String,           //WHAT THEY ARE UP TO, UNTIL THEY CHANGE IT
    /// Hash of their avatar; set by upload, never by a profile save.
    pub avatar: Option<[u8; 32]>, //PROFILE PICTURE
    /// Their rank; filled in by the server.
    pub role: Role,               //THEIR RANK
}

//ONE MESSAGE AS server_messages.bin KEEPS IT
/// One stored lobby message, as sent in a history page.
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct StoredMessage
{
    /// Server-assigned message id.
    pub message_id: u64,
    /// The author's username.
    pub username: String,
    /// The message, or the filename when this is a picture.
    pub text: String,             //THE MESSAGE - OR THE FILENAME, WHEN THIS LINE IS AN IMAGE
    /// The author's current colors.
    pub colors: MessageColors,
    /// SHA-256 of the picture, if this is one.
    pub image: Option<[u8; 32]>,  //CONTENT HASH OF THE PICTURE
    /// When it was sent, in Unix seconds.
    pub timestamp: Option<u64>,   //UNIX SECONDS
    /// The id of the message this replies to.
    pub reply: Option<u64>,       //ID OF THE MESSAGE REPLIED TO
    /// Usernames that hearted it.
    pub hearts: Vec<String>,      //USERNAMES THAT HEARTED IT
    /// Whether it was edited since it was sent.
    pub edited: bool,             //CHANGED SINCE SENT
    /// The clip, if this is a voice message.
    pub voice: Option<VoiceNote>, //THE CLIP, WHEN THIS LINE IS A VOICE MESSAGE
    /// Whether a moderator pinned it.
    pub pinned: bool,             //PINNED BY A MODERATOR
}

//ONE STORED VOICE MESSAGE
/// One stored voice message.
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct VoiceNote
{
    /// SHA-256 of the clip.
    pub hash: [u8; 32], //CONTENT HASH OF THE CLIP
    /// Length in milliseconds, measured by the server.
    pub duration: u32,  //LENGTH IN MS
}

/// The colors of a chat line, as wire color codes.
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct MessageColors //COLORS OF MESSAGE
{
    /// Color of the username.
    pub username_color: Option<u8>, //COLOR OF USERNAME
    /// Color of the message text.
    pub message_color: Option<u8>,  //COLOR OF MESSAGE
}

/// One user's shared files.
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct UserFile //USER FILE LIST ITEM
{
    /// The owner's username.
    pub username: String,
    /// The owner's session id.
    pub id: usize,
    /// Each file's name and id.
    pub upload: Vec<(String, usize)>,
}

/// One running screen share.
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct UserScreen //USER SCREEN SHARE LIST ITEM
{
    /// The sharer's username.
    pub username: String,
    /// The sharer's session id.
    pub id: usize,
}

/// One ban list entry.
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct BanEntry //ONE BANNED SUBJECT
{
    /// The entry's position, which a pardon names.
    pub id: usize,
    /// The banned username or address.
    pub subject: String,
}

/// A user connected to the server.
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct OnlineUser //USER CONNECTED TO THE SERVER
{
    pub username: String,
    pub username_color: Option<u8>,
    pub id: usize,
    pub channel: Option<String>,
    pub device: Option<Device>,
    pub role: Role,
}

/// A registered user who is not connected.
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub struct OfflineUser //OFFLINE USER REGISTERED ON THE SERVER
{
    pub username: String,
    pub username_color: Option<u8>,
    pub id: usize,
    pub role: Role,
}

//ENUMS
/// The kind of client a user is on.
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq)]
pub enum Device
{
    /// The terminal client.
    TUI,
    /// WHY2-Desktop.
    Desktop,
    /// A phone client.
    Phone,
}
