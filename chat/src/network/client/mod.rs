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

//! # Client Connection
//!
//! The client's side of the main connection: [`listen_server`] runs the handshake and then
//! reads packets for the rest of the session, translating each into a [`ClientEvent`] for the
//! UI.
//!
//! # Events, Not Output
//! Nothing here prints or draws. Every packet that matters to the user becomes a
//! [`ClientEvent`] on an `mpsc` channel, which is the whole interface between the protocol and
//! a front end — every front end consumes the same events. A front end
//! never needs to see a [`PacketCode`].
//!
//! # Pictures
//! An [`AttachmentDisplay`](PacketCode::AttachmentDisplay) is resolved here as far as the protocol can
//! take it: bytes that arrived are hashed and cached, then decoded off the async runtime when
//! `auto_show_images` is on, or offered to be requested on demand when it is off; a picture without bytes is
//! looked up in the cache before the UI is asked to fetch it. A picture for another channel is
//! cached but never decoded.
//!
//! # Submodules
//! - [`handshake`] — dialing and the key exchange.
//! - [`image`] — bounded picture decoding and avatar cutting.

//MODULES
pub mod image;

#[cfg(feature = "client_voice")]
pub mod media;
pub mod handshake;

use std::
{
    sync::{ Arc, Mutex },
    path::PathBuf,
    collections::BTreeMap,
};

use tokio::
{
    task,
    io::AsyncWriteExt,
    sync::
    {
        oneshot,
        mpsc::Sender,
    },
};

use rand::
{
    TryRng,
    rngs::SysRng,
};

use zeroize::Zeroizing;

use semver::Version;

use image::Animation;

#[cfg(feature = "client_voice")]
use image::ImageFrame;

use handshake::Handshake;

use crate::
{
    misc,
    cache,
    role::Role,
    crypto,
    options::{ self, LoginState },
    consts::Streams,
    network::
    {
        self,
        file::{ client as file, UploadKind },
        codes::
        {
            PacketCode,
            MessageColors,
            StoredMessage,
            VoiceNote,
            UserFile,
            OnlineUser,
            OfflineUser,
            UserScreen,
            Device,
            ServerSetting,
            UserProfile,
            BanEntry,
        },
    },
};

#[cfg(feature = "client_voice")]
use crate::network::voice::client::
{
    self as voice_client,
    options as voice_options,
    message as voice_message,
};

#[cfg(feature = "client_screen")]
use crate::network::screen::client::
{
    self as screen,
    options as screen_options,
};

//STRUCTS
/// A trust-on-first-use decision put to the user.
pub struct TofuRequest
{
    /// The host the key is pinned against.
    pub host: String,           //WHAT THE KEY IS PINNED AGAINST
    /// Fingerprint of the server's identity key.
    pub hash: String,           //SHA256 OF THE SERVER'S PUBLIC KEYS
    /// Whether a different key is pinned for this host.
    pub mismatch: bool,         //A KEY IS PINNED FOR host AND DIFFERS
    /// The fingerprint on record, if any.
    pub pinned: Option<String>, //THE FINGERPRINT ON RECORD
    /// Where the user's answer goes: `true` to trust and pin the key. Dropping it refuses.
    pub reply: oneshot::Sender<bool>,
}

/// One row of the voice panel.
pub struct VoiceUser
{
    /// The user's session id.
    pub id: usize,             //ID OF USER
    /// The username to show.
    pub username: String,      //USERNAME TO DISPLAY
    /// Whether they are speaking right now.
    pub is_speaking: bool,     //TAKE A WILD GUESS
    /// Their round-trip time in milliseconds, if we are receiving them.
    pub latency: Option<u128>, //USER'S PING
    /// Whether this row is us.
    pub is_local: bool,        //AM I THE USER?
}

//ENUMS
/// Everything the protocol has to tell a front end.
pub enum ClientEvent
{
    /// The server asks for a password to register.
    Register,                                                    //REGISTER PROMPT
    /// The server asks for the password of an existing account.
    Login,                                                       //LOGIN PROMPT
    /// We registered first and became the owner.
    FirstUser,                                                   //FIRST USER
    /// Login succeeded, with our role.
    Authenticated
    {
        /// The role the server gave us.
        role: Role,
    },                                                           //LOGIN SUCCESSFUL, ROLE
    /// The connection is up: the server's display name.
    Connected
    {
        /// The name the server goes by.
        server_name: String,
    },                                                           //SUCCESSFUL CONNECTION MESSAGE
    /// A chat message.
    Message
    {
        /// The message text.
        message: String,
        /// The sender's username.
        username: String,
        /// The sender's session id.
        id: usize,
        /// Server-assigned message id.
        message_id: u64,
        /// The sender's current colors.
        colors: MessageColors,
        /// Where it was said, as in [`PacketCode::Message`]; `None` is every pane.
        channel: Option<Option<String>>,
        /// When it was sent, in Unix seconds.
        timestamp: Option<u64>,
        /// The id of the message this replies to.
        reply: Option<u64>,
    },                                                           //RECEIVED MESSAGE, ITS CHANNEL, WHEN, WHAT IT REPLIES TO
    /// A private message we sent.
    PrivateMessageSent
    {
        /// The message text.
        message: String,
        /// The recipient's username.
        username: String,
        /// The recipient's session id.
        id: usize,
        /// The sender's current colors.
        colors: MessageColors,
    },                                                           //SENT PM
    /// A private message we received.
    PrivateMessageRecv
    {
        /// The message text.
        message: String,
        /// The sender's username.
        username: String,
        /// The sender's session id.
        id: usize,
        /// The sender's current colors.
        colors: MessageColors,
    },                                                           //RECEIVED PM
    /// The user refused the server's identity.
    TofuError,                                                   //TOFU VERIFICATION REJECTED BY THE USER
    /// The server's identity needs a decision from the user.
    TofuPrompt
    {
        /// The decision to put to the user.
        request: TofuRequest,
    },                                                           //TOFU DECISION ASKED OF THE USER
    /// Identity verification is disabled in this build: the server's fingerprint.
    TofuSkip
    {
        /// The server's fingerprint.
        hash: String,
    },                                                           //TOFU VERIFICATION SKIPPED
    /// Dialing again after pinning a key failed.
    ReconnectFailed,                                             //RECONNECTING AFTER PINNING THE KEY FAILED
    /// The key exchange failed, with the reason.
    HandshakeFailed
    {
        /// Why the exchange failed.
        reason: String,
    },                                                           //THE KEY EXCHANGE DID NOT ADD UP
    /// The local voice session's activity: who is speaking, and their ping.
    VoiceActivity
    {
        /// One row per user in the voice session.
        users: Vec<VoiceUser>,
    },                                                           //VOICE OVERLAY
    /// Our channel's whole voice roster, as `(id, username)`, ourselves excluded.
    VoiceRoster
    {
        /// The roster, as `(id, username)`.
        clients: Vec<(usize, String)>,
    },                                                           //THE CHANNEL'S WHOLE VOICE ROSTER, SELF EXCLUDED
    /// Somebody joined voice in our channel.
    VoiceJoin
    {
        /// The joiner's session id.
        id: usize,
        /// The joiner's username.
        username: String,
    },                                                           //SOMEBODY JOINED VOICE IN OUR CHANNEL
    /// Somebody left voice in our channel.
    VoiceLeave
    {
        /// The leaver's session id.
        id: usize,
    },                                                           //SOMEBODY LEFT VOICE IN OUR CHANNEL
    /// A user connected.
    Join
    {
        /// The user's username.
        username: String,
        /// The user's username color.
        username_color: Option<u8>,
        /// The user's session id.
        id: usize,
        /// The device the user is on.
        device: Option<Device>,
        /// The user's role.
        role: Role,
    },                                                           //CLIENT CONNECTED
    /// A user disconnected.
    Leave
    {
        /// The user's username.
        username: String,
        /// The user's session id.
        id: usize,
        /// Whether their account still exists.
        registered: bool,
    },                                                           //CLIENT DISCONNECTED (ACCOUNT STILL EXISTS)
    /// A message said as the server.
    ServerSay
    {
        /// The message text.
        message: String,
    },                                                           //SERVER MESSAGE
    /// A role was set.
    Role
    {
        /// The new role.
        role: Role,
        /// The username it was set on.
        username: String,
    },                                                           //A ROLE WAS SET (THE ROLE, AND WHO ON)
    /// A page of lobby history.
    History
    {
        /// The stored messages.
        messages: Vec<StoredMessage>,
        /// The id the page starts at.
        start: u64,
        /// Whether more pages exist.
        more: bool,
        /// How many messages were kept.
        kept: u64,
        /// Whether this answers a request for an older page.
        older: bool,
    },                                                           //A PAGE OF STORED MESSAGES (START, MORE, KEPT, OLDER)
    /// We switched channel; `None` is the lobby.
    ChannelChanged
    {
        /// The channel, or `None` for the lobby.
        channel: Option<String>,
    },                                                           //WE SWITCHED CHANNEL
    /// A channel was created.
    ChannelCreated
    {
        /// The channel's name.
        channel: String,
    },                                                           //CHANNEL CREATED
    /// A channel was abandoned.
    ChannelDestroyed
    {
        /// The channel's name.
        channel: String,
    },                                                           //CHANNEL ABANDONED
    /// Somebody in our channel is typing.
    Typing
    {
        /// Who is typing.
        username: String,
    },                                                           //SOMEBODY IN OUR CHANNEL IS WRITING
    /// We are muted.
    Muted,                                                       //HAHA
    /// A command had invalid parameters or was not permitted.
    InvalidUsage,                                                //INVALID COMMAND USAGE
    /// A message has no readable attachment on the server.
    AttachmentUnavailable { message_id: u64 },
    /// A message was deleted.
    Deleted
    {
        /// The deleted message's id.
        message_id: u64,
    },                                                           //A MESSAGE WAS DELETED (MESSAGE ID)
    /// A message's hearts changed.
    Hearts
    {
        /// The message's id.
        message_id: u64,
        /// Who hearted it.
        hearts: Vec<String>,
    },                                                           //A MESSAGE'S HEARTS CHANGED (MESSAGE ID, WHO)
    /// A message was reworded.
    Edited
    {
        /// The message's id.
        message_id: u64,
        /// The new text.
        text: String,
    },                                                           //A MESSAGE WAS REWORDED (MESSAGE ID, TEXT)
    /// A message was pinned or unpinned.
    Pinned
    {
        /// The message's id.
        message_id: u64,
        /// Whether it is pinned.
        pinned: bool,
    },                                                           //A MESSAGE WAS PINNED OR UNPINNED (MESSAGE ID, PINNED)
    /// Every pinned message.
    Pins
    {
        /// The pinned messages.
        messages: Vec<StoredMessage>,
    },                                                           //EVERY PINNED MESSAGE
    /// The version check could not reach crates.io.
    VersionFailed,                                               //FETCHING VERSIONS FAILED
    /// The server was built from a different commit.
    VersionMismatch
    {
        /// Our commit hash.
        client_version: String,
        /// The server's commit hash.
        server_version: String,
    },                                                           //MISMATCH GIT HASH
    /// This build is outdated.
    UnsafeVersion
    {
        /// How many versions behind this build is.
        newer_versions: usize,
        /// Our version.
        current_version: Version,
        /// The newest version.
        newest_version: String,
    },                                                           //OLD VERSION
    /// The server asks for a username.
    Username
    {
        /// Whether registration is disabled.
        disabled_registration: bool,
        /// The minimum username length.
        min_uname: u64,
        /// The maximum username length.
        max_uname: u64,
    },                                                           //USERNAME PROMPT
    /// We joined voice.
    VoiceEnabled,                                                //VOICE CHAT ENABLED
    /// Switching audio devices failed; the previous ones are back.
    VoiceDeviceFailed,                                           //REBUILDING THE AUDIO STREAMS FAILED
    /// The server never answered the voice handshake.
    VoiceHandshakeFailed,                                        //THE SERVER NEVER ANSWERED THE UDP HANDSHAKE
    /// We left voice.
    VoiceDisabled,                                               //VOICE CHAT DISABLED
    /// The user list.
    List
    {
        /// Connected users.
        users: Vec<OnlineUser>,
        /// Offline users, if the server discloses them.
        offline: Option<Vec<OfflineUser>>,
    },                                                           //LIST OF USERS, CONNECTED AND NOT
    /// Every key of `server.toml`, and whether this acknowledges a save.
    ServerSettings
    {
        /// The settings as the server holds them.
        settings: Vec<ServerSetting>,
        /// Whether this acknowledges a save.
        saved: bool,
    },                                                           //server.toml AS THE SERVER HOLDS IT
    /// A password change was answered; `false` means the old password was wrong.
    Passwd
    {
        /// Whether the password was changed.
        changed: bool,
    },                                                           //A PASSWORD CHANGE ANSWERED
    /// An account deletion was answered; `false` means the password was wrong.
    AccountDeleted
    {
        /// Whether the account was deleted.
        deleted: bool,
    },                                                           //AN ACCOUNT DELETION ANSWERED
    /// A profile.
    Profile
    {
        /// Whose profile it is.
        username: String,
        /// The profile itself.
        profile: UserProfile,
        /// Whether it is ours.
        own: bool,
        /// Whether this acknowledges a save.
        saved: bool,
    },                                                           //A PROFILE (WHOSE, IT, OURS, A SAVE ACK)
    /// A color change landed on the server.
    Colors
    {
        /// The new colors, or `None` for a `/color` acknowledgement.
        colors: Option<MessageColors>,
    },                                                           //A /color LANDED ON THE SERVER OR INIT COLORS
    /// The server icon, and whether this acknowledges our own change.
    ServerIcon
    {
        /// The icon's hash, if there is one.
        hash: Option<[u8; 32]>,
        /// Whether this acknowledges a save.
        save: bool,
    },                                                           //THE SERVER'S PICTURE (HASH, A SAVE ACK)
    /// The ban list.
    ServerBans
    {
        /// Banned usernames.
        usernames: Vec<BanEntry>,
        /// Banned addresses.
        addresses: Vec<BanEntry>,
    },                                                           //server_bans.toml (USERNAMES, ADDRESSES)
    /// Our file upload started.
    Upload
    {
        /// The upload id.
        uid: u64,
        /// The file name.
        filename: String,
        /// The file size.
        size: u64,
    },                                                           //UPLOADING FILE (UID, NAME, SIZE)
    /// Our persistent attachment upload started.
    Attachment
    {
        /// The upload id.
        uid: u64,
        /// The file name.
        filename: String,
        /// The file size.
        size: u64,
    },                                                           //UPLOADING ATTACHMENT (UID, NAME, SIZE)
    /// A picture, decoded and ready to draw.
    ImageDisplay
    {
        /// The original attachment content hash.
        hash: [u8; 32],
        /// The sender's username.
        username: String,
        /// The picture's file name.
        filename: String,
        /// The message's id.
        message_id: u64,
        /// When it was sent, in Unix seconds.
        timestamp: Option<u64>,
        /// The decoded frames.
        image: Animation,
        /// The sender's username color.
        username_color: Option<u8>,
    },                                                           //SOMEBODY'S IMAGE, DECODED AND READY TO DRAW
    /// A picture that was asked for, by hash; `None` means it is not coming.
    ImageData
    {
        /// The picture's SHA-256.
        hash: [u8; 32],
        /// The decoded frames, if it is coming.
        image: Option<Animation>,
    },                                                           //A HISTORY IMAGE THAT WAS ASKED FOR (None = NOT COMING)
    /// An attachment on its way from the server.
    AttachmentPending
    {
        /// The sender's username.
        username: String,
        /// The attachment's file name.
        filename: String,
        /// The message's id.
        message_id: u64,
        /// When it was sent, in Unix seconds.
        timestamp: Option<u64>,
        /// The attachment's SHA-256.
        hash: [u8; 32],
        /// The sender's username color.
        username_color: Option<u8>,
    },                                                           //SOMEBODY'S ATTACHMENT, ASKED FOR AND ON ITS WAY
    /// An attachment waiting to be fetched (`auto_show_images` off).
    AttachmentOffer
    {
        /// The sender's username.
        username: String,
        /// The attachment's file name.
        filename: String,
        /// The message's id.
        message_id: u64,
        /// When it was sent, in Unix seconds.
        timestamp: Option<u64>,
        /// The attachment's SHA-256.
        hash: [u8; 32],
        /// The sender's username color.
        username_color: Option<u8>,
    },                                                           //SOMEBODY'S ATTACHMENT, WAITING TO BE ASKED FOR
    /// An attachment posted in another channel.
    AttachmentParked
    {
        /// The channel it was posted in.
        channel: String,
        /// The sender's username.
        username: String,
        /// The attachment's file name.
        filename: String,
        /// The message's id.
        message_id: u64,
        /// When it was sent, in Unix seconds.
        timestamp: Option<u64>,
        /// The attachment's SHA-256.
        hash: [u8; 32],
        /// The sender's username color.
        username_color: Option<u8>,
    },                                                           //SOMEBODY'S ATTACHMENT IN ANOTHER CHANNEL (CHANNEL FIRST)
    /// An attachment the cache could not answer; the front end should request it from the server.
    AttachmentRequest
    {
        /// The attachment's SHA-256.
        hash: [u8; 32],
    },                                                           //A CLICKED CAPTION THE CACHE COULD NOT ANSWER
    /// Cutting our avatar or the server icon failed, with the reason.
    AvatarFailed
    {
        /// Why it failed.
        error: String,
    },                                                           //CUTTING OUR AVATAR OR SERVER ICON FAILED
    /// An attachment without a decodable image preview.
    AttachmentPreviewUnavailable
    {
        /// The sender's username.
        username: String,
        /// The attachment's file name.
        filename: String,
        /// The message's id.
        message_id: u64,
        /// The attachment's content hash.
        hash: [u8; 32],
        /// The message timestamp.
        timestamp: Option<u64>,
        /// The sender's username color.
        username_color: Option<u8>,
    },                                                           //AN ATTACHMENT WITHOUT AN IMAGE PREVIEW
    /// A bounded video frame ready for the terminal renderer.
    #[cfg(feature = "client_voice")]
    MediaFrame
    {
        message_id: u64,
        generation: usize,
        frame: ImageFrame,
    },
    /// A requested media attachment arrived.
    #[cfg(feature = "client_voice")]
    MediaFetched { hash: [u8; 32] },
    /// Embedded media decoding or output failed.
    #[cfg(feature = "client_voice")]
    MediaFailed { reason: String },
    /// A voice message.
    VoiceMessage
    {
        /// The sender's username.
        username: String,
        /// The message's id.
        message_id: u64,
        /// When it was sent, in Unix seconds.
        timestamp: Option<u64>,
        /// The clip.
        voice: VoiceNote,
        /// The sender's username color.
        username_color: Option<u8>,
        /// The channel it was posted in.
        channel: Option<Option<String>>,
    },                                                           //SOMEBODY'S VOICE MESSAGE AND ITS CHANNEL
    /// A voice clip that was asked for arrived, by hash; `false` means it is not coming.
    VoiceData
    {
        /// The clip's SHA-256.
        hash: [u8; 32],
        /// Whether it is coming.
        valid: bool,
    },                                                           //A VOICE MESSAGE THAT WAS ASKED FOR (FALSE = NOT COMING)
    /// Recording or playing a voice message failed, with the reason.
    VoiceMessageFailed
    {
        /// Why it failed.
        reason: String,
    },                                                           //RECORDING OR PLAYING A VOICE MESSAGE FAILED
    /// A user shared a file.
    Uploaded
    {
        /// The sender's username.
        username: String,
        /// The file name.
        filename: String,
    },                                                           //USER UPLOADED FILE
    /// Our upload is complete.
    UploadDone
    {
        /// The upload id.
        uid: u64,
        /// The file name.
        filename: String,
    },                                                           //OUR OWN UPLOAD IS ON THE WIRE
    /// A download started.
    Download
    {
        /// The transfer id.
        uid: u64,
        /// The file name.
        filename: String,
        /// The file size.
        size: u64,
    },                                                           //DOWNLOADING FILE (UID, NAME, SIZE)
    /// A download completed and verified.
    Downloaded
    {
        /// The transfer id.
        uid: u64,
        /// The file name.
        filename: String,
    },                                                           //DOWNLOADED FILE
    /// A download failed verification.
    DownloadFailed
    {
        /// The transfer id.
        uid: u64,
        /// The file name.
        filename: String,
    },                                                           //DOWNLOADING FAILED
    /// How far a transfer has got.
    TransferProgress
    {
        /// The transfer id.
        uid: u64,
        /// Bytes transferred so far.
        done: u64,
    },                                                           //HOW FAR A TRANSFER HAS GOT (UID, BYTES)
    /// User got banned.
    Banned
    {
        /// The banned username.
        username: String,
    },                                                           //BANNED
    /// The shared files.
    Files
    {
        /// One entry per user with files.
        users: Vec<UserFile>,
    },                                                           //FILE LIST
    /// The running screen shares.
    Screens
    {
        /// One entry per user sharing a screen.
        users: Vec<UserScreen>,
    },                                                           //SCREENSHARE LIST
    /// We have too many uploads in flight.
    UploadLimit,                                                 //MAX CONCURRENT UPLOADS REACHED
    /// Our screen share started (`true`) or stopped.
    Screen
    {
        /// Whether it started.
        enabled: bool,
    },                                                           //TOGGLED SCREENSHARE
    /// Screen capture failed, with the reason.
    ScreenFailed
    {
        /// Why it failed.
        reason: String,
    },                                                           //SCREEN CAPTURE FAILED
    /// We started watching a share.
    Attach
    {
        /// The sharer's username.
        username: String,
    },                                                           //ATTACHED SCREENSHARE
    /// We stopped watching a share.
    Deattach
    {
        /// The sharer's username.
        username: String,
    },                                                           //DEATTACHED SCREENSHARE
    /// Somebody started watching our share.
    Attached
    {
        /// The watcher's username.
        username: String,
    },                                                           //SOMEBODY ATTACHED OUR SCREENSHARE
    /// Somebody stopped watching our share.
    Deattached
    {
        /// The watcher's username.
        username: String,
    },                                                           //SOMEBODY DEATTACHED OUR SCREENSHARE
    /// The watched share's sound was muted (`true`) or unmuted.
    MuteScreen
    {
        /// Whether it is muted.
        muted: bool,
    },                                                           //TOGGLED ATTACHED SCREENSHARE MUTE
    /// Somebody started sharing their screen.
    Screenshare
    {
        /// The sharer's username.
        username: String,
    },                                                           //SOMEBODY STARTED SCREENSHARING
    /// Somebody stopped sharing their screen.
    ScreenshareEnd
    {
        /// The sharer's username.
        username: String,
    },                                                           //SOMEBODY STOPPED SCREENSHARING
    /// The server runs an incompatible version.
    IncompatibleVersion
    {
        /// Our version.
        client_version: String,
        /// The server's version.
        server_version: String,
    },                                                           //INCOMPATIBLE SERVER VERSION
    /// The server rejected our username.
    UsernameRejected,                                            //USERNAME REJECTED BY SERVER
    /// The server rejected our password; the minimum length.
    PasswordRejected
    {
        /// The minimum password length.
        min_pass: u64,
    },                                                           //PASSWORD REJECTED BY SERVER
    /// We are sending messages too fast.
    SpamWarning,                                                 //SPAM WARNING
    /// Voice is unavailable over SOCKS5.
    Socks5Voice,                                                 //DISABLED VOICE ON SOCKS5
    /// The requested feature is disabled on this server.
    DisabledFeature,                                             //DISABLED FEATURE
    /// The session ended; `true` if the server ended it gracefully.
    Quit
    {
        /// Whether the server ended it gracefully.
        graceful: bool,
    },                                                           //SERVER QUIT COMMUNICATION
}

//LISTS
/// Files waiting for an upload approval, by SHA-256.
pub static ACTIVE_UPLOADS: Mutex<BTreeMap<[u8; 32], PathBuf>> = Mutex::new(BTreeMap::new()); //ACTIVE UPLOADS
//FUNCTIONS
//PUBLIC
/// Runs the client side of a session: the handshake, then every packet until the connection
/// ends.
///
/// The handshake is repeated on a fresh connection when the user has just pinned the server's
/// key. Afterwards, each packet is either answered directly (keepalives, version checks,
/// rekeys) or turned into [`ClientEvent`]s. A lost connection ends with
/// [`ClientEvent::Quit`].
///
/// # Parameters
/// - `streams`: The main connection.
/// - `tx`: The UI event channel.
pub async fn listen_server(streams: &mut Streams<'_>, tx: Sender<ClientEvent>) //SERVER -> CLIENT COMMUNICATION
{
    //SET GLOBAL CLIENT ENCRYPTION & MAC KEY
    let mut keys = (Zeroizing::new(vec![]), Zeroizing::new(vec![]));

    //PIN THE KEY AND RUN THE HANDSHAKE AGAIN
    loop
    {
        //SEND HEADER
        let mut header = [0u8; 32];
        SysRng.try_fill_bytes(&mut header).unwrap(); //GENERATE RANDOM HEADER
        options::set_obfuscation_key(&header);
        streams.1.lock().await.write_all(&header).await.unwrap();

        match handshake::key_exchange(streams, &mut keys, &tx, None).await
        {
            Handshake::Ready => break,
            Handshake::Failed => return,

            //THE SERVER WENT AWAY BETWEEN CONNECTIONS
            Handshake::Reconnect => if !handshake::reconnect(streams).await
            {
                tx.send(ClientEvent::ReconnectFailed).await.unwrap();
                return;
            },
        }
    }

    //SERVER INFO VARIABLES
    let mut min_pass: Option<u64> = None;
    let mut max_uname: Option<u64> = None;
    let mut min_uname: Option<u64> = None;

    let mut invalid_username = false; //PRINT "Invalid Username!"
    let mut invalid_password = false;

    let mut disabled_registration = false; //PRINT "Registration disabled!"

    //CONNECTION PROPERTIES
    let mut id = 0usize; //ID SET BY SERVER
    #[cfg(feature = "client_voice")]
    let mut username: Option<String> = None; //OUR OWN NAME, FOR THE VOICE PANEL

    //LOOP READING
    loop
    {
        //SERVER DIED CHECK
        let mut dropped = false;

        let read = match network::receive(streams, Some(&keys), None).await
        {
            Some(code) => code,
            None =>
            {
                dropped = true; //YUP, SERVER IS 6 FEET UNDER
                PacketCode::Disconnect
            },
        };

        //CHECK FOR MUTED CLIENT
        #[cfg(feature = "client_voice")]
        if let PacketCode::Message { id, .. } = read
        {
            if options::is_muted(Some(id)) { continue; }
        }

        //KEEPALIVE
        if matches!(read, PacketCode::KeepAlive)
        {
            //ECHO
            network::send(&mut *streams.1.lock().await, PacketCode::KeepAlive, options::get_keys().as_ref()).await;
            continue;
        }

        //CODES
        match read
        {
            //REGULAR MESSAGE
            PacketCode::Message { text, username, id, message_id, colors, channel, timestamp, reply } =>
            {
                tx.send(ClientEvent::Message
                {
                    message: text,
                    username,
                    id,
                    message_id,
                    colors,
                    channel,
                    timestamp,
                    reply,
                }).await.unwrap();
            }

            //THE LOBBY'S STORED MESSAGES
            PacketCode::History { messages, start, more, kept, older } =>
            {
                tx.send(ClientEvent::History { messages, start, more, kept, older }).await.unwrap();

                continue;
            }

            //VERSION CHECK
            PacketCode::Version { version } =>
            {
                let local_version = misc::get_version().to_string();

                //NON MATCHING VERSION (WILL GET DISCONNECTED)
                if version != local_version
                {
                    tx.send(ClientEvent::IncompatibleVersion { client_version: local_version.clone(), server_version: version }).await.unwrap();
                }

                //RESPOND
                network::send(&mut *streams.1.lock().await,
                    PacketCode::Version { version: local_version }, Some(&keys)).await;

                continue;
            }

            //WELCOME CODE - SERVER INFORMATIONS
            PacketCode::Welcome { min_pass: smin_pass, max_uname: smax_uname,
                min_uname: smin_uname, server_name, server_uname, git_hash } =>
            {
                options::set_server_username(&server_uname);

                min_pass = Some(smin_pass);
                max_uname = Some(smax_uname);
                min_uname = Some(smin_uname);

                //COMPARSE HASHES
                let client_hash = env!("WHY2_GIT_HASH");
                if !client_hash.is_empty() && !git_hash.is_empty() && !client_hash.starts_with(&git_hash) && !git_hash.starts_with(client_hash)
                {
                    //DISPLAY VERSION MISMATCH
                    tx.send(ClientEvent::VersionMismatch { client_version: client_hash.to_string(), server_version: git_hash.to_string() }).await.unwrap();
                }

                tx.send(ClientEvent::Connected { server_name }).await.unwrap();
            },

            //REKEY - CHANGE KEYS
            PacketCode::Rekey =>
            {
                //WAIT FOR SERVER TO INIT KEY EXCHANGE
                let current_keys = keys.clone();

                //A REKEY THAT DOES NOT VERIFY ENDS THE SESSION
                if handshake::key_exchange(streams, &mut keys, &tx, Some(&current_keys)).await != Handshake::Ready { return; }
            }

            //PICK USERNAME
            PacketCode::UsernameRequest =>
            {
                //INVALID UNAME
                if invalid_username
                {
                    tx.send(ClientEvent::UsernameRejected).await.unwrap();
                } else //VALID
                {
                    //SET INVALID USERNAME FOR POSSIBLE NEXT CODE
                    invalid_username = true;
                }

                options::set_login_state(LoginState::Username);
                tx.send(ClientEvent::Username { disabled_registration, min_uname: min_uname.unwrap(), max_uname: max_uname.unwrap() }).await.unwrap();
            },

            //REGISTER
            PacketCode::RegisterRequest =>
            {
                options::set_asking_password(true);

                //INVALID PASS
                if invalid_password
                {
                    tx.send(ClientEvent::PasswordRejected { min_pass: min_pass.unwrap() }).await.unwrap();
                } else
                {
                    invalid_password = true;
                }

                options::set_login_state(LoginState::Register);
                tx.send(ClientEvent::Register).await.unwrap();
            },

            //LOGIN
            PacketCode::LoginRequest =>
            {
                options::set_asking_password(true);
                options::set_login_state(LoginState::Login);
                tx.send(ClientEvent::Login).await.unwrap();
            },

            //FIRST USER
            PacketCode::FirstUser =>
            {
                tx.send(ClientEvent::FirstUser).await.unwrap();
            },

            //START CHATTING
            PacketCode::Accept { id: sid, username: uname, role, colors } =>
            {
                tx.send(ClientEvent::Authenticated { role }).await.unwrap();

                //SET SERVER-SIDE ID
                id = sid;

                //OUR OWN NAME IS ONLY FOR THE VOICE PANEL
                #[cfg(feature = "client_voice")]
                {
                    username = Some(uname);
                }

                //SET COLORS
                tx.send(ClientEvent::Colors { colors: Some(colors) }).await.unwrap();

                //ALLOW MESSAGE HISTORY & COMMANDS
                options::set_sending_messages(true);
                options::set_login_state(LoginState::None);
            },

            //JOIN MESSAGE (CLIENT CONNECTED)
            PacketCode::Join { username: user, username_color, id, device, role } =>
            {
                tx.send(ClientEvent::Join { username: user, username_color, id, device, role }).await.unwrap();
            }

            //LEAVE MESSAGE (CLIENT DISCONNECTED)
            PacketCode::Leave { username, id, registered } =>
            {
                tx.send(ClientEvent::Leave { username, id, registered }).await.unwrap();

                #[cfg(feature = "client_voice")]
                voice_client::remove_consumer(&id);
            },

            //CHANNEL CHANGE
            PacketCode::Channel { channel } =>
            {
                //REMOVE ALL STORED VOICE CLIENTS
                #[cfg(feature = "client_voice")]
                voice_client::remove_all_consumers();

                options::set_channel(channel.clone().unwrap_or_default());

                tx.send(ClientEvent::ChannelChanged { channel }).await.unwrap();
            },

            //CHANNEL CREATED
            PacketCode::ChannelCreated { name } =>
            {
                tx.send(ClientEvent::ChannelCreated { channel: name }).await.unwrap();
            },

            //CHANNEL ABANDONED
            PacketCode::ChannelDestroyed { name } =>
            {
                tx.send(ClientEvent::ChannelDestroyed { channel: name }).await.unwrap();
            },

            //SOMEBODY IS WRITING
            PacketCode::Typing { username } =>
            {
                tx.send(ClientEvent::Typing { username }).await.unwrap();
            },

            //SERVER ALLOWED VOICE
            #[cfg(feature = "client_voice")]
            PacketCode::Voice { token } =>
            {
                if options::socks5_enabled()
                {
                    tx.send(ClientEvent::Socks5Voice).await.unwrap();
                    continue;
                }

                //FOLLOW THE SERVER INSTEAD OF TOGGLING
                voice_options::set_use_voice(token.is_some());

                //TOGGLE VOICE (& PRINT STATUS)
                tx.send(match token
                {
                    Some(token) =>
                    {
                        let username = username.clone();
                        let voice_tx = tx.clone();
                        let stream = streams.1.clone();
                        tokio::spawn(voice_client::listen_server_voice(id, username.unwrap(), voice_tx, stream, token));
                        ClientEvent::VoiceEnabled
                    },

                    None => ClientEvent::VoiceDisabled,
                }).await.unwrap();
            },

            //THE WHOLE VOICE ROSTER
            PacketCode::VoiceClients { clients } =>
            {
                //ADD CLIENTS WE CAN ACTUALLY HEAR
                #[cfg(feature = "client_voice")]
                if voice_options::get_use_voice()
                {
                    for (id, username) in clients.iter()
                    {
                        voice_client::add_consumer(*id, username.clone());
                    }
                }

                tx.send(ClientEvent::VoiceRoster { clients }).await.unwrap();
            }

            //CLIENT JOINED VOICE
            PacketCode::VoiceJoin { username, id: sid, channel } =>
            {
                //OUR OWN JOIN COMES BACK TO US TOO
                if id == sid || channel.unwrap_or_default() != options::get_channel() { continue; }

                #[cfg(feature = "client_voice")]
                if voice_options::get_use_voice()
                {
                    voice_client::add_consumer(sid, username.clone());
                }

                tx.send(ClientEvent::VoiceJoin { id: sid, username }).await.unwrap();
            },

            //CLIENT LEFT VOICE
            PacketCode::VoiceLeave { id, channel } =>
            {
                //ANOTHER CHANNEL'S VOICE
                if channel.unwrap_or_default() != options::get_channel() { continue; }

                #[cfg(feature = "client_voice")]
                voice_client::remove_consumer(&id);

                tx.send(ClientEvent::VoiceLeave { id }).await.unwrap();
            },

            //SERVER MESSAGE
            PacketCode::ServerSay { message } =>
            {
                tx.send(ClientEvent::ServerSay { message }).await.unwrap();
            }

            //A ROLE WAS SET, ON US OR ON SOMEBODY ELSE
            PacketCode::ServerRole { username, role } =>
            {
                tx.send(ClientEvent::Role { role, username }).await.unwrap();
            },

            //server.toml, ASKED FOR OR JUST STORED
            PacketCode::ServerSettings { settings, save } =>
            {
                tx.send(ClientEvent::ServerSettings { settings, saved: save }).await.unwrap();
            },

            //A PASSWORD CHANGE ANSWERED
            PacketCode::AccountPasswd { valid } =>
            {
                tx.send(ClientEvent::Passwd { changed: valid }).await.unwrap();
            },

            //AN ACCOUNT DELETION ANSWERED
            PacketCode::AccountDelete { valid } =>
            {
                tx.send(ClientEvent::AccountDeleted { deleted: valid }).await.unwrap();
            },

            //A PROFILE, ASKED FOR OR JUST STORED
            PacketCode::Profile { username, profile, own, save } =>
            {
                tx.send(ClientEvent::Profile { username, profile, own, saved: save }).await.unwrap();
            },

            //THE SERVER'S PICTURE, ASKED FOR OR CHANGED
            PacketCode::ServerIcon { hash, save } =>
            {
                tx.send(ClientEvent::ServerIcon { hash, save }).await.unwrap();
            },

            //THE BAN LIST, ASKED FOR OR JUST LIFTED
            PacketCode::ServerBans { users, ips } =>
            {
                tx.send(ClientEvent::ServerBans { usernames: users, addresses: ips }).await.unwrap();
            },

            //THE SERVER STORED A COLOR
            PacketCode::Color =>
            {
                tx.send(ClientEvent::Colors { colors: None }).await.unwrap();
            },

            //LIST OF USERS
            PacketCode::List { online, offline } =>
            {
                tx.send(ClientEvent::List { users: online, offline }).await.unwrap();
            },

            //UPLOAD APPROVAL
            PacketCode::Upload { hash, token, uid } | PacketCode::Attachment { hash, token, uid, .. }
                | PacketCode::VoiceMessageUpload { hash, token, uid } =>
            {
                let kind = match read
                {
                    PacketCode::Attachment { .. } => UploadKind::Attachment,
                    PacketCode::VoiceMessageUpload { .. } => UploadKind::Voice,
                    _ => UploadKind::File,
                };

                //SPAWN UPLOAD TASK
                let file_tx = tx.clone(); //CLONE TX
                tokio::spawn(file::upload(token, uid, hash, file_tx, kind));
                continue;
            },

            //DUPLICATE ATTACHMENT
            PacketCode::AttachmentDuplicate { hash } =>
            {
                if let Some(path) = ACTIVE_UPLOADS.lock().unwrap().remove(&hash) { misc::drop_upload_temp(&path); }
                continue;
            },

            //DOWNLOAD
            PacketCode::Download { token } =>
            {
                //SPAWN DOWNLOAD TASK
                let file_tx = tx.clone(); //CLONE TX
                tokio::spawn(file::download(token, file_tx));
                continue;
            },

            //USER GOT BANNED
            PacketCode::Banned { username } =>
            {
                tx.send(ClientEvent::Banned { username }).await.unwrap();
            },

            //SOMEBODY'S ATTACHMENT
            //ONE OF THOSE, DECODED OFF THE LOOP
            PacketCode::AttachmentData { hash, data } =>
            {
                let attachment_tx = tx.clone();

                //MEDIA ATTACHMENTS USE THE SAME HASH FETCH AS VOICE MESSAGES
                #[cfg(feature = "client_voice")]
                if let Some((message_id, generation)) = media::awaited(&hash)
                {
                    tokio::spawn(async move
                    {
                        if !data.is_empty() && crypto::sha256(&data) == hash { cache::store(&hash, &data).await; }
                        media::play_data(hash, message_id, data, generation, attachment_tx).await;
                    });

                    continue;
                }

                //A CLIP ASKED FOR TO PLAY
                #[cfg(feature = "client_voice")]
                if let Some(generation) = voice_message::awaited(&hash)
                {
                    tokio::spawn(async move
                    {
                        let data = Arc::new(data);
                        let digest = task::spawn_blocking
                        ({
                            let data = data.clone();
                            move || crypto::sha256(&data)
                        }).await.expect("Hashing voice message panicked");

                        let valid = digest == hash && !data.is_empty();
                        attachment_tx.send(ClientEvent::VoiceData { hash, valid }).await.unwrap();

                        if !valid { return; }

                        cache::store(&hash, &data).await;
                        voice_message::play_data(hash, Arc::unwrap_or_clone(data), generation, attachment_tx).await;
                    });

                    continue;
                }

                tokio::spawn(async move
                {
                    let data = Arc::new(data);
                    let (digest, image) = image::digest_and_decode(data.clone()).await;
                    if digest == hash { cache::store(&hash, &data).await; }

                    attachment_tx.send(ClientEvent::ImageData { hash, image }).await.unwrap();
                });

                continue;
            },

            //EITHER THE PICTURE OR THE OFFER OF IT
            PacketCode::AttachmentDisplay { username, filename, message_id, hash, data, username_color, channel, timestamp } =>
            {
                let attachment_tx = tx.clone();

                //ANOTHER CHANNEL'S PICTURE: CACHE IT, DECODE NOTHING
                let elsewhere = channel.map(Option::unwrap_or_default).filter(|channel| *channel != options::get_channel());

                if elsewhere.is_some() || !image::auto_show_images()
                {
                    tokio::spawn(async move
                    {
                        if let Some(data) = data
                        {
                            let data = Arc::new(data);
                            let digest = task::spawn_blocking
                            ({
                                let data = data.clone();
                                move || crypto::sha256(&data)
                            }).await.expect("Hashing image panicked");

                            cache::store(&digest, &data).await;
                        }

                        attachment_tx.send(match elsewhere
                        {
                            Some(channel) => ClientEvent::AttachmentParked
                            {
                                channel,
                                username,
                                filename,
                                message_id,
                                timestamp,
                                hash,
                                username_color,
                            },
                            None => ClientEvent::AttachmentOffer
                            {
                                username,
                                filename,
                                message_id,
                                timestamp,
                                hash,
                                username_color,
                            },
                        }).await.unwrap();
                    });

                    continue;
                }

                tokio::spawn(async move
                {
                    //ANSWER AN OFFER OUT OF THE CACHE FIRST
                    let (data, fresh) = match data
                    {
                        Some(data) => (Some(Arc::new(data)), true),
                        None => (cache::load(&hash).await.map(Arc::new), false),
                    };

                    let Some(data) = data else
                    {
                        //ASK FROM THE EVENT LOOP, WHICH OWNS THE WRITE HALF
                        attachment_tx.send(ClientEvent::AttachmentPending
                        {
                            username,
                            filename,
                            message_id,
                            timestamp,
                            hash,
                            username_color,
                        }).await.unwrap();

                        return;
                    };

                    let image = match fresh
                    {
                        //OFF THE WIRE: FILE IT UNDER WHAT IT HASHES TO
                        true =>
                        {
                            let (digest, image) = image::digest_and_decode(data.clone()).await;

                            cache::store(&digest, &data).await;

                            image
                        },

                        //OUT OF THE CACHE, ALREADY FILED UNDER THIS HASH
                        false => task::spawn_blocking(move || image::decode_attachment(&data))
                            .await.expect("Decoding image panicked"),
                    };

                    attachment_tx.send(match image
                    {
                        Some(image) => ClientEvent::ImageDisplay
                        {
                            hash,
                            username,
                            filename,
                            message_id,
                            timestamp,
                            image,
                            username_color,
                        },
                        None => ClientEvent::AttachmentPreviewUnavailable { username, filename, message_id, hash, timestamp, username_color },
                    }).await.unwrap();
                });

                continue;
            },

            //SOMEBODY'S VOICE MESSAGE, CACHED IF IT CAME WHOLE
            PacketCode::VoiceMessage { username, message_id, voice, data, username_color, channel, timestamp } =>
            {
                let voice_tx = tx.clone();

                tokio::spawn(async move
                {
                    if let Some(data) = data
                    {
                        let data = Arc::new(data);
                        let digest = task::spawn_blocking
                        ({
                            let data = data.clone();
                            move || crypto::sha256(&data)
                        }).await.expect("Hashing voice message panicked");

                        if digest == voice.hash { cache::store(&digest, &data).await; }
                    }

                    voice_tx.send(ClientEvent::VoiceMessage
                    {
                        username,
                        message_id,
                        timestamp,
                        voice,
                        username_color,
                        channel,
                    }).await.unwrap();
                });

                continue;
            },

            //UPLOADED ANNOUNCEMENT
            PacketCode::Uploaded { filename, username } =>
            {
                tx.send(ClientEvent::Uploaded { username, filename }).await.unwrap();
            },

            //FILE LIST
            PacketCode::Files { users } =>
            {
                tx.send(ClientEvent::Files { users }).await.unwrap();
            },

            //SCREENSHARE LIST
            PacketCode::Screens { users } =>
            {
                tx.send(ClientEvent::Screens { users }).await.unwrap();
            },

            //MAX PARALLEL UPLOADS
            PacketCode::UploadLimit =>
            {
                tx.send(ClientEvent::UploadLimit).await.unwrap();
            },

            //SCREEN UPLOAD APPROVAL, OR AN UNSOLICITED STOP
            #[cfg(feature = "client_screen")]
            PacketCode::Screen { token } =>
            {
                //FOLLOW THE SERVER INSTEAD OF TOGGLING
                screen_options::set_use_screen(token.is_some());

                //THE MONITOR WAS THIS SHARE'S, NOT THIS SESSION'S
                if token.is_none() { screen_options::set_monitor(None); }

                tx.send(match token
                {
                    Some(token) =>
                    {
                        //SPAWN UPLOAD TASK
                        tokio::spawn(screen::screen(token, tx.clone()));
                        ClientEvent::Screen { enabled: true }
                    },

                    None => ClientEvent::Screen { enabled: false },
                }).await.unwrap();
            },

            //SCREENSHARE ATTACH
            #[cfg(feature = "client_screen")]
            PacketCode::Attach { username, token } =>
            {
                //ENABLE ATTACH
                screen_options::set_attach_screen(true);

                //SPAWN DOWNLOAD TASK
                let main_stream = streams.1.clone();
                tokio::spawn(screen::attach(token, main_stream));
                tx.send(ClientEvent::Attach { username }).await.unwrap();
            },

            //SCREENSHARE DEATTACH
            #[cfg(feature = "client_screen")]
            PacketCode::Deattach { username } =>
            {
                //DISABLE ATTACH
                screen_options::set_attach_screen(false);

                tx.send(ClientEvent::Deattach { username }).await.unwrap();
            },

            //ATTACHED SCREENSHARE MUTE ACK
            #[cfg(feature = "client_screen")]
            PacketCode::MuteScreen { muted } =>
            {
                tx.send(ClientEvent::MuteScreen { muted }).await.unwrap();
            },

            //SOMEBODY STARTED SCREENSHARING
            #[cfg(feature = "client_screen")]
            PacketCode::Screenshare { username } =>
            {
                tx.send(ClientEvent::Screenshare { username }).await.unwrap();
            },

            //SOMEBODY STOPPED SCREENSHARING
            #[cfg(feature = "client_screen")]
            PacketCode::ScreenshareEnd { username } =>
            {
                tx.send(ClientEvent::ScreenshareEnd { username }).await.unwrap();
            },

            //SOMEBODY ATTACHED OUR SCREENSHARE
            #[cfg(feature = "client_screen")]
            PacketCode::Attached { username } =>
            {
                tx.send(ClientEvent::Attached { username }).await.unwrap();
            },

            //SOMEBODY DEATTACHED OUR SCREENSHARE
            #[cfg(feature = "client_screen")]
            PacketCode::Deattached { username } =>
            {
                tx.send(ClientEvent::Deattached { username }).await.unwrap();
            },

            //PRIVATE MESSAGE INCOMING
            PacketCode::PrivateMessage { text, username, id, colors } =>
            {
                tx.send(ClientEvent::PrivateMessageRecv { message: text, username, id, colors }).await.unwrap();
            },

            //PRIVATE MESSAGE INCOMING
            PacketCode::PrivateMessageBack { text, username, id, colors } =>
            {
                tx.send(ClientEvent::PrivateMessageSent { message: text, username, id, colors }).await.unwrap();
            },

            //SPAM WARNING
            PacketCode::SpamWarning =>
            {
                tx.send(ClientEvent::SpamWarning).await.unwrap();
            },

            //REGISTRATION DISABLED
            PacketCode::RegisterDisabled =>
            {
                disabled_registration = true;
            },

            //HAHA, YOU'RE MUTED
            PacketCode::Muted =>
            {
                tx.send(ClientEvent::Muted).await.unwrap();
            }

            //A MESSAGE WAS DELETED
            PacketCode::Deleted { message_id } =>
            {
                tx.send(ClientEvent::Deleted { message_id }).await.unwrap();
            },

            //A MESSAGE WAS REWORDED
            PacketCode::Edited { message_id, text } =>
            {
                tx.send(ClientEvent::Edited { message_id, text }).await.unwrap();
            },

            //A MESSAGE'S HEARTS CHANGED
            PacketCode::Heart { message_id, hearts } =>
            {
                tx.send(ClientEvent::Hearts { message_id, hearts }).await.unwrap();
            },

            //A MESSAGE WAS PINNED OR UNPINNED
            PacketCode::Pinned { message_id, pinned } =>
            {
                tx.send(ClientEvent::Pinned { message_id, pinned }).await.unwrap();
            },

            //THE PINNED MESSAGES
            PacketCode::Pins { messages } =>
            {
                tx.send(ClientEvent::Pins { messages }).await.unwrap();
            },

            //CLIENT MESSED SOME COMMAND UP
            PacketCode::AttachmentUnavailable { message_id } =>
            {
                tx.send(ClientEvent::AttachmentUnavailable { message_id }).await.unwrap();
            },

            PacketCode::InvalidUsage =>
            {
                tx.send(ClientEvent::InvalidUsage).await.unwrap();
            },

            //CLIENTED REQUESTED DISABLED FEATURE
            PacketCode::InvalidFeature =>
            {
                tx.send(ClientEvent::DisabledFeature).await.unwrap();
            },

            //SERVER DOESN'T LIKE YA ANYMORE - EXIT
            PacketCode::Disconnect =>
            {
                tx.send(ClientEvent::Quit { graceful: !dropped }).await.unwrap();
                return;
            },

            _ => continue //EITHER INVALID CODE OR A KEY EXCHANGE CODE
        }
    }
}
