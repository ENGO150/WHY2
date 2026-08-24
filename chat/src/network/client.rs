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
    io::Error,
    sync::Mutex,
    path::PathBuf,
    collections::BTreeMap,
};

use tokio::
{
    io::AsyncWriteExt,
    sync::
    {
        oneshot,
        mpsc::Sender,
    },
    net::
    {
        TcpStream,
        tcp::{ OwnedReadHalf, OwnedWriteHalf },
    },
};

use rand::
{
    TryRng,
    rngs::SysRng,
};

use tokio_socks::tcp::Socks5Stream;

use zeroize::Zeroizing;

use semver::Version;

use crate::
{
    misc,
    crypto::kex,
    config::{ self, TofuCode },
    options::{ self, LoginState },
    consts::
    {
        Streams,
        SharedKeys,
    },
    network::
    {
        self,
        file::client as file,
        codes::
        {
            PacketCode,
            MessageColors,
            UserFile,
            OnlineUser,
            UserScreen,
        },
    },
};

#[cfg(feature = "client_voice")]
use crate::network::voice::client::
{
    self as voice_client,
    options as voice_options,
};

#[cfg(feature = "client_screen")]
use crate::network::screen::client::
{
    self as screen,
    options as screen_options,
};

//STRUCTS
pub struct TofuRequest
{
    pub host: String,           //WHAT THE KEY IS PINNED AGAINST
    pub hash: String,           //SHA256 OF THE SERVER'S PUBLIC KEYS
    pub mismatch: bool,         //A KEY IS ALREADY PINNED FOR host AND DIFFERS (NOT A FIRST CONTACT)
    pub pinned: Option<String>, //THE FINGERPRINT ON RECORD, SHOWN BESIDE THE NEW ONE ON A MISMATCH
    pub reply: oneshot::Sender<bool>,
}

pub struct VoiceUser
{
    pub id: usize,          //ID OF USER
    pub username: String,   //USERNAME TO DISPLAY
    pub is_speaking: bool,  //TAKE A WILD GUESS
    pub latency: u128,      //USER'S PING
    pub is_local: bool,     //AM I THE USER?
}

//ENUMS
pub enum ClientEvent
{
    Register,                                      //REGISTER PROMPT
    Login,                                         //LOGIN PROMPT
    Authenticated,                                 //LOGIN SUCCESSFUL
    Connected(String),                             //SUCCESSFUL CONNECTION MESSAGE
    Message(String, String, usize, MessageColors), //RECEIVED MESSAGE
    PrivateMessageSent(String, usize, String),     //SENT PM
    PrivateMessageRecv(String, usize, String),     //RECEIVED PM
    TofuError,                                     //TOFU VERIFICATION REJECTED BY THE USER
    TofuPrompt(TofuRequest),                       //TOFU DECISION ASKED OF THE USER
    TofuSkip(String),                              //TOFU VERIFICATION SKIPPED
    VoiceActivity(Vec<VoiceUser>),                 //VOICE OVERLAY
    Join(String),                                  //CLIENT CONNECTED
    Leave(String),                                 //CLIENT DISCONNECTED
    ChannelChanged(Option<String>),                //WE SWITCHED CHANNEL
    ChannelCreated(String),                        //CHANNEL CREATED
    ChannelDestroyed(String),                      //CHANNEL ABANDONED
    InvalidUsage,                                  //INVALID COMMAND USAGE
    VersionFailed,                                 //FETCHING VERSIONS FAILED
    VersionMismatch(String, String),               //MISMATCH GIT HASH
    UnsafeVersion(usize, Version, String),         //OLD VERSION
    Username(bool, u64, u64),                      //USERNAME PROMPT
    VoiceEnabled,                                  //VOICE CHAT ENABLED
    VoiceDeviceFailed,                             //REBUILDING THE AUDIO STREAMS FAILED
    VoiceDisabled,                                 //VOICE CHAT DISABLED
    List(Vec<OnlineUser>),                         //LIST OF USERS
    Upload(String),                                //UPLOADING FILE
    Uploaded(String, String),                      //USER UPLOADED FILE
    Download(String),                              //DOWNLOADING FILE
    Downloaded(String),                            //DOWNLOADED FILE
    DownloadFailed(String),                        //DOWNLOADING FAILED
    Files(Vec<UserFile>),                          //FILE LIST
    Screens(Vec<UserScreen>),                      //SCREENSHARE LIST
    UploadLimit,                                   //MAX CONCURRENT UPLOADS REACHED
    Screen(bool),                                  //TOGGLED SCREENSHARE
    ScreenFailed(String),                          //SCREEN CAPTURE FAILED
    Attach(String),                                //ATTACHED SCREENSHARE
    Deattach(String),                              //DEATTACHED SCREENSHARE
    IncompatibleVersion(String, String),           //INCOMPATIBLE SERVER VERSION
    UsernameRejected,                              //USERNAME REJECTED BY SERVER
    PasswordRejected(u64),                         //PASSWORD REJECTED BY SERVER
    SpamWarning,                                   //SPAM WARNING
    Socks5Voice,                                   //DISABLED VOICE ON SOCKS5
    DisabledFeature,                               //DISABLED FEATURE
    Quit,                                          //SERVER QUIT COMMUNICATION
}

//LISTS
pub static ACTIVE_UPLOADS: Mutex<BTreeMap<[u8; 32], PathBuf>> = Mutex::new(BTreeMap::new()); //ACTIVE UPLOADS

//FUNCTIONS
//PRIVATE
async fn key_exchange
(
    streams: &mut Streams<'_>,
    keys: &mut SharedKeys,
    tx: &Sender<ClientEvent>,
    exchange_keys: Option<&SharedKeys>,
) -> bool //KEY EXCHANGE FOR CLIENT-SIDE
{
    //WAIT FOR KeyExchange
    let (ecc, pq) = loop
    {
        //READ MESSAGE
        let received = network::receive(streams, exchange_keys, None).await.unwrap();

        if let PacketCode::KeyExchange { ecc, pq } = received { break (ecc, pq); }
    };

    //CONCAT PUBLIC KEYS
    let pks = ecc.clone() + &pq;

    //VERIFY PUBKEY VALIDITY (TOFU)
    let host = streams.0.peer_addr().unwrap().ip().to_string();
    let verdict = if env!("WHY2_SKIP_TOFU") == "false"
    {
        Some(config::server_keys_check(&host, &pks))
    } else { None };

    //GENERATE EPHEMERAL ECC KEYS
    let (sk, pk) = kex::generate_ephemeral_keys();

    //ENCAPSULATE PQ
    let (pq_ciphertext, pq_secret) = kex::encapsulate_pq(&pq);

    //SEND PUBKEYS TO SERVER
    network::send(&mut *streams.1.lock().await, PacketCode::KeyExchange
    {
        ecc: pk,
        pq: pq_ciphertext,
    }, exchange_keys).await;

    //CALCULATE SHARED SECRET (HYBRID)
    *keys = kex::derive_shared_secret(sk, ecc.to_string(), pq_secret).expect("Shared secret derivation failed");

    //SET GLOBAL VARIABLES
    options::set_keys(keys.clone());

    //ACT ON THE TOFU VERDICT NOW THAT THE SERVER HAS ITS ANSWER
    let hash = config::server_keys_hash(&pks);

    match verdict
    {
        //VERIFICATION DISABLED AT BUILD TIME
        None => tx.send(ClientEvent::TofuSkip(hash)).await.unwrap(),

        Some(TofuCode::Valid) => {},

        Some(status) =>
        {
            //ASK THE USER IN THE TUI INSTEAD OF DROPPING THEM BACK TO THE SHELL
            let (reply, answer) = oneshot::channel();

            tx.send(ClientEvent::TofuPrompt(TofuRequest
            {
                host: host.clone(),
                hash: hash.clone(),
                mismatch: matches!(status, TofuCode::Mismatch),
                pinned: config::server_keys_pinned(&host),
                reply,
            })).await.unwrap();

            //A DROPPED SENDER (THE CLIENT IS QUITTING) COUNTS AS A REFUSAL
            if !answer.await.unwrap_or(false)
            {
                //GRACEFULLY DISCONNECT FROM SERVER
                network::send(&mut *streams.1.lock().await, PacketCode::Disconnect, Some(keys)).await;

                //END THE SESSION
                tx.send(ClientEvent::TofuError).await.unwrap();

                //EXIT
                return false;
            }

            //PIN THE KEY - THE NEXT CONNECTION TO host VERIFIES AGAINST IT
            config::server_keys_save(&host, &hash);
        },
    }

    true
}

//PUBLIC
pub async fn connect(connecting_addr: String) -> Result<(OwnedReadHalf, OwnedWriteHalf), Error> //CONNECT TO SERVER
{
    if !options::socks5_enabled() //NO SOCKS5
    {
        TcpStream::connect(connecting_addr).await
    } else //USE PROXY
    {
        let proxy_addr = config::read_config::<String>("socks5_addr");

        Socks5Stream::connect(proxy_addr.as_str(), connecting_addr.as_str()).await
            .map(|s| s.into_inner())
            .map_err(Error::other)
    }.and_then(|s|
    {
        //SET TCP_NODELAY
        s.set_nodelay(true)?;
        Ok(s.into_split())
    })
}

pub async fn listen_server(streams: &mut Streams<'_>, tx: Sender<ClientEvent>) //SERVER -> CLIENT COMMUNICATION
{
    //SEND HEADER
    let mut header = [0u8; 32];
    SysRng.try_fill_bytes(&mut header).unwrap(); //GENERATE RANDOM HEADER
    options::set_obfuscation_key(&header);
    streams.1.lock().await.write_all(&header).await.unwrap();

    //SET GLOBAL CLIENT ENCRYPTION & MAC KEY
    let mut keys = (Zeroizing::new(vec![]), Zeroizing::new(vec![]));
    if !key_exchange(streams, &mut keys, &tx, None).await { return; }

    //SERVER INFO VARIABLES
    let mut min_pass: Option<u64> = None;
    let mut max_uname: Option<u64> = None;
    let mut min_uname: Option<u64> = None;

    let mut invalid_username = false; //PRINT "Invalid Username!"
    let mut invalid_password = false;

    let mut disabled_registration = false; //PRINT "Registration disabled!"

    //FIRST Join PACKET CARRIES OUR OWN USERNAME
    #[cfg(feature = "client_voice")]
    let mut first_message = true;

    //CONNECTION PROPERTIES
    #[cfg(feature = "client_voice")]
    let mut id = 0usize; //ID SET BY SERVER
    #[cfg(feature = "client_voice")]
    let mut username: Option<String> = None;

    //LOOP READING
    loop
    {
        let read = match network::receive(streams, Some(&keys), None).await
        {
            Some(code) => code,
            None => PacketCode::Disconnect,
        };

        //CHECK FOR MUTED CLIENT
        #[cfg(feature = "client_voice")]
        if let PacketCode::Message { id, .. } = read
        {
            if id.is_some() && options::is_muted(id) { continue; }
        }

        //KEEPALIVE
        if read == PacketCode::KeepAlive
        {
            //ECHO
            network::send(&mut *streams.1.lock().await, PacketCode::KeepAlive, options::get_keys().as_ref()).await;
            continue;
        }

        //CODES
        match read
        {
            //REGULAR MESSAGE
            PacketCode::Message { text, username, id, colors } =>
            {
                tx.send(ClientEvent::Message(text, username.unwrap(), id.unwrap(), colors)).await.unwrap();
            }

            //VERSION CHECK
            PacketCode::Version { version } =>
            {
                let local_version = misc::get_version().to_string();

                //NON MATCHING VERSION (WILL GET DISCONNECTED)
                if let Some(version) = version && version != local_version
                {
                    tx.send(ClientEvent::IncompatibleVersion(local_version.clone(), version)).await.unwrap();
                }

                //RESPOND
                network::send(&mut *streams.1.lock().await,
                    PacketCode::Version { version: Some(local_version) }, Some(&keys)).await;

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
                if !client_hash.is_empty() && !git_hash.is_empty() && client_hash != git_hash
                {
                    //DISPLAY VERSION MISMATCH
                    tx.send(ClientEvent::VersionMismatch(client_hash.to_string(), git_hash.to_string())).await.unwrap();
                }

                tx.send(ClientEvent::Connected(server_name)).await.unwrap();
            },

            //REKEY - CHANGE KEYS
            PacketCode::Rekey =>
            {
                //WAIT FOR SERVER TO INIT KEY EXCHANGE
                let current_keys = keys.clone();
                key_exchange(streams, &mut keys, &tx, Some(&current_keys)).await;
            }

            //PICK_USERNAME CODE - guess what
            PacketCode::Username { .. } =>
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
                tx.send(ClientEvent::Username(disabled_registration, min_uname.unwrap(), max_uname.unwrap())).await.unwrap();
            },

            //REGISTER
            PacketCode::PasswordR { .. } =>
            {
                options::set_asking_password(true);

                //INVALID PASS
                if invalid_password
                {
                    tx.send(ClientEvent::PasswordRejected(min_pass.unwrap())).await.unwrap();
                } else
                {
                    invalid_password = true;
                }

                options::set_login_state(LoginState::PasswordRegister);
                tx.send(ClientEvent::Register).await.unwrap();
            },

            //LOGIN
            PacketCode::PasswordL { .. } =>
            {
                options::set_asking_password(true);
                options::set_login_state(LoginState::PasswordLogin);
                tx.send(ClientEvent::Login).await.unwrap();
            },

            //START CHATTING
            PacketCode::Accept { id: sid } =>
            {
                tx.send(ClientEvent::Authenticated).await.unwrap();

                //SET SERVER-SIDE ID (ONLY VOICE CARES WHO WE ARE)
                #[cfg(feature = "client_voice")]
                {
                    id = sid;
                }

                #[cfg(not(feature = "client_voice"))]
                let _ = sid;

                //ALLOW MESSAGE HISTORY & COMMANDS
                options::set_sending_messages(true);
                options::set_login_state(LoginState::None);
            },

            //JOIN MESSAGE (CLIENT CONNECTED)
            PacketCode::Join { username: user } =>
            {
                #[cfg(feature = "client_voice")]
                if first_message
                {
                    first_message = false;
                    username = Some(user.clone());
                }

                tx.send(ClientEvent::Join(user)).await.unwrap();
            }

            //LEAVE MESSAGE (CLIENT DISCONNECTED)
            PacketCode::Leave { username, id } =>
            {
                tx.send(ClientEvent::Leave(username)).await.unwrap();

                #[cfg(feature = "client_voice")]
                voice_client::remove_consumer(&id);

                #[cfg(not(feature = "client_voice"))]
                let _ = id;
            },

            //CHANNEL CHANGE
            PacketCode::Channel { channel } =>
            {
                //REMOVE ALL STORED VOICE CLIENTS
                #[cfg(feature = "client_voice")]
                voice_client::remove_all_consumers();

                options::set_channel(channel.clone().unwrap_or_default());

                tx.send(ClientEvent::ChannelChanged(channel)).await.unwrap();
            },

            //CHANNEL CREATED
            PacketCode::ChannelCreated { name } =>
            {
                tx.send(ClientEvent::ChannelCreated(name)).await.unwrap();
            },

            //CHANNEL ABANDONED
            PacketCode::ChannelDestroyed { name } =>
            {
                tx.send(ClientEvent::ChannelDestroyed(name)).await.unwrap();
            },

            //SERVER ALLOWED VOICE
            #[cfg(feature = "client_voice")]
            PacketCode::Voice =>
            {
                if options::socks5_enabled()
                {
                    tx.send(ClientEvent::Socks5Voice).await.unwrap();
                    continue;
                }

                //TOGGLE VOICE (& PRINT STATUS)
                tx.send(if voice_options::swap_use_voice()
                {
                    let username = username.clone();
                    let voice_tx = tx.clone();
                    let stream = streams.1.clone();
                    tokio::spawn(voice_client::listen_server_voice(id, username.unwrap(), voice_tx, stream));
                    ClientEvent::VoiceEnabled
                } else
                {
                    ClientEvent::VoiceDisabled
                }).await.unwrap();
            },

            //VOICE CLIENTS
            #[cfg(feature = "client_voice")]
            PacketCode::VoiceClients { clients } =>
            {
                //ADD CLIENTS
                for (id, username) in clients
                {
                    voice_client::add_consumer(id, username);
                }
            }

            //CLIENT JOINED VOICE CHANNEL
            #[cfg(feature = "client_voice")]
            PacketCode::ChannelJoin { username, id: sid } =>
            {
                if voice_options::get_use_voice() && id != sid
                {
                    voice_client::add_consumer(sid, username);
                }
            },

            //CLIENT LEFT VOICE CHANNEL
            #[cfg(feature = "client_voice")]
            PacketCode::ChannelLeave { id } =>
            {
                voice_client::remove_consumer(&id);
            },

            //LIST OF ONLINE USERS
            PacketCode::List { users } =>
            {
                tx.send(ClientEvent::List(users.unwrap())).await.unwrap();
            },

            //UPLOAD APPROVAL
            PacketCode::Upload { hash, token, uid } =>
            {
                //SPAWN UPLOAD TASK
                let file_tx = tx.clone(); //CLONE TX
                tokio::spawn(file::upload(token.unwrap(), uid.unwrap(), hash, file_tx));
                continue;
            },

            //DOWNLOAD
            PacketCode::Download { token, .. } =>
            {
                //SPAWN DOWNLOAD TASK
                let file_tx = tx.clone(); //CLONE TX
                tokio::spawn(file::download(token.unwrap(), file_tx));
                continue;
            },

            //UPLOADED ANNOUNCEMENT
            PacketCode::Uploaded { filename, username } =>
            {
                tx.send(ClientEvent::Uploaded(username, filename)).await.unwrap();
            },

            //FILE LIST
            PacketCode::Files { users } =>
            {
                if let Some(users) = users
                {
                    tx.send(ClientEvent::Files(users)).await.unwrap();
                }
            },

            //SCREENSHARE LIST
            PacketCode::Screens { users } =>
            {
                if let Some(users) = users
                {
                    tx.send(ClientEvent::Screens(users)).await.unwrap();
                }
            },

            //MAX PARALLEL UPLOADS
            PacketCode::UploadLimit =>
            {
                tx.send(ClientEvent::UploadLimit).await.unwrap();
            },

            //SCREEN UPLOAD APPROVAL (OR AN UNSOLICITED STOP WHEN THE SHARE DIES SERVER-SIDE)
            #[cfg(feature = "client_screen")]
            PacketCode::Screen { token } =>
            {
                //FOLLOW THE SERVER INSTEAD OF TOGGLING, SO BOTH SIDES CANNOT DRIFT APART
                screen_options::set_use_screen(token.is_some());

                tx.send(match token
                {
                    Some(token) =>
                    {
                        //SPAWN UPLOAD TASK
                        tokio::spawn(screen::screen(token, tx.clone()));
                        ClientEvent::Screen(true)
                    },

                    None => ClientEvent::Screen(false),
                }).await.unwrap();
            },

            //SCREENSHARE ATTACH
            #[cfg(feature = "client_screen")]
            PacketCode::Attach { username, token, .. } =>
            {
                //ENABLE ATTACH
                screen_options::set_attach_screen(true);

                //SPAWN DOWNLOAD TASK
                let main_stream = streams.1.clone();
                tokio::spawn(screen::attach(token.unwrap(), main_stream));
                tx.send(ClientEvent::Attach(username.unwrap())).await.unwrap();
            },

            //SCREENSHARE DEATTACH
            #[cfg(feature = "client_screen")]
            PacketCode::Deattach { username } =>
            {
                //DISABLE ATTACH
                screen_options::set_attach_screen(false);

                tx.send(ClientEvent::Deattach(username.unwrap())).await.unwrap();
            },

            //PRIVATE MESSAGE INCOMING
            PacketCode::PrivateMessage { text, username, id } =>
            {
                tx.send(ClientEvent::PrivateMessageRecv(username.unwrap(), id, text)).await.unwrap();
            },

            //PRIVATE MESSAGE INCOMING
            PacketCode::PrivateMessageBack { text, username, id } =>
            {
                tx.send(ClientEvent::PrivateMessageSent(username, id, text)).await.unwrap();
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

            //CLIENT MESSED SOME COMMAND UP
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
                tx.send(ClientEvent::Quit).await.unwrap();
                return;
            },

            _ => continue //EITHER INVALID CODE OR A KEY EXCHANGE CODE
        }
    }
}
