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
    thread,
    net::TcpStream,
    io::{ Error, Write },
    collections::HashMap,
    path::PathBuf,
    sync::
    {
        Arc,
        Mutex,
        LazyLock,
        mpsc::Sender,
    },
};

use rand::
{
    TryRng,
    rngs::SysRng,
};

use socks::Socks5Stream;

use zeroize::Zeroizing;

use serde_json::Value;

use semver::Version;

use crate::
{
    options,
    misc,
    crypto::kex,
    consts::{ self, Streams },
    config::{ self, TofuCode },
    network::
    {
        self,
        MessageCode,
        MessagePacket,
        file::client as file,
        screen::client as screen,
        voice::
        {
            client as voice_client,
            options as voice_options,
        }
    },
};

//STRUCTS
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
    Register,                                  //REGISTER PROMPT
    Login,                                     //LOGIN PROMPT
    Authenticated,                             //LOGIN SUCCESSFUL
    Connected(String),                         //SUCCESSFUL CONNECTION MESSAGE
    Message(MessagePacket),                    //RECEIVED MESSAGE
    Prompt,                                    //">>>" PROMPT
    PrivateMessageSent(String, usize, String), //SENT PM
    PrivateMessageRecv(String, usize, String), //RECEIVED PM
    TofuError(TofuCode),                       //TOFU VERIFICATION FAILED
    VoiceActivity(Vec<VoiceUser>),             //VOICE OVERLAY
    Join(String),                              //CLIENT CONNECTED
    Leave(String),                             //CLIENT DISCONNECTED
    Clear(usize),                              //CLEAR n LINES
    InvalidUsage,                              //INVALID COMMAND USAGE
    VersionFailed,                             //FETCHING VERSIONS FAILED
    VersionMismatch(String, String),           //MISMATCH GIT HASH
    UnsafeVersion(usize, Version, String),     //OLD VERSION
    Username(bool, u64, u64),                  //USERNAME PROMPT
    VoiceEnabled,                              //VOICE CHAT ENABLED
    VoiceDisabled,                             //VOICE CHAT DISABLED
    List(Value),                               //LIST OF USERS
    Upload(String),                            //UPLOADING FILE
    Uploaded(String, String),                  //USER UPLOADED FILE
    Download(String),                          //DOWNLOADING FILE
    Downloaded(String),                        //DOWNLOADED FILE
    DownloadFailed(String),                    //DOWNLOADING FAILED
    Files(Vec<Value>),                         //FILE LIST
    UploadLimit,                               //MAX CONCURRENT UPLOADS REACHED
    ScreenUpload(bool),                        //TOGGLED SCREENSHARE
    ExtraSpace,                                //JUST RANDOM NEWLINE
    IncompatibleVersion(String, String),       //INCOMPATIBLE SERVER VERSION
    UsernameRejected,                          //USERNAME REJECTED BY SERVER
    PasswordRejected(u64),                     //PASSWORD REJECTED BY SERVER
    SpamWarning,                               //SPAM WARNING
    Socks5Voice,                               //DISABLED VOICE ON SOCKS5
    DisabledFeature,                           //DISABLED FEATURE
    Quit,                                      //SERVER QUIT COMMUNICATION
}

//LISTS
pub static ACTIVE_UPLOADS: LazyLock<Arc<Mutex<HashMap<[u8; 32], PathBuf>>>> = LazyLock::new(|| //ACTIVE UPLOADS
{
    Arc::new(Mutex::new(HashMap::new()))
});

//FUNCTIONS
//PRIVATE
fn key_exchange
(
    streams: &mut Streams,
    keys: &mut consts::SharedKeys,
    tx: &Sender<ClientEvent>,
    exchange_keys: Option<&consts::SharedKeys>,
) -> bool //KEY EXCHANGE FOR CLIENT-SIDE
{
    //WAIT FOR KeyExchange
    let message = loop
    {
        //READ MESSAGE
        let received = network::receive(streams, exchange_keys, None).unwrap();

        if received.code == Some(MessageCode::KeyExchange) { break received; }
    };

    let message_text = message.text.as_ref().unwrap();

    //PARSE SERVER KEYS JSON
    let server_keys: Value = serde_json::from_str(message_text).expect("Failed to parse server keys JSON");
    let server_ecc_pk = server_keys["ecc"].as_str().expect("Parsing server ECC key failed");
    let server_pq_pk = server_keys["pq"].as_str().expect("Parsing server PQ key failed");

    //VERIFY PUBKEY VALIDITY (TOFU)
    match config::server_keys_check(&streams.0.peer_addr().unwrap().ip().to_string(), message_text)
    {
        TofuCode::Valid => {},

        status @ (TofuCode::Mismatch | TofuCode::Unknown(_, _)) =>
        {
            //GRACEFULLY DISCONNECT FROM SERVER
            network::send(&mut streams.1.lock().unwrap(), MessagePacket
            {
                code: Some(MessageCode::Disconnect),
                ..Default::default()
            }, None);

            //PRINT SECURITY MESSAGE
            tx.send(ClientEvent::TofuError(status)).unwrap();

            //EXIT
            return false;
        },
    }

    //GENERATE EPHEMERAL ECC KEYS
    let (sk, pk) = kex::generate_ephemeral_keys();

    //ENCAPSULATE PQ
    let (pq_ciphertext, pq_secret) = kex::encapsulate_pq(server_pq_pk);

    //PREPARE RESPONSE JSON
    let response_text = serde_json::json!
    ({
        "ecc": pk,
        "pq": pq_ciphertext,
    }).to_string();

    //SEND ECC PUBKEY TO SERVER
    network::send(&mut streams.1.lock().unwrap(), MessagePacket
    {
        text: Some(response_text),
        code: Some(MessageCode::KeyExchange),
        ..Default::default()
    }, exchange_keys);

    //CALCULATE SHARED SECRET (HYBRID)
    *keys = kex::derive_shared_secret(sk, server_ecc_pk.to_string(), pq_secret).expect("Shared secret derivation failed");

    //SET GLOBAL KEYS VARIABLE
    options::set_keys(keys.clone());

    true
}

//PUBLIC
pub fn connect(connecting_addr: String) -> Result<TcpStream, Error> //CONNECT TO SERVER
{
    if !options::socks5_enabled() //NO SOCKS5
    {
        TcpStream::connect(connecting_addr)
    } else //USE PROXY
    {
        Socks5Stream::connect(config::read_config::<String>("socks5_addr"), connecting_addr.as_str())
            .map(|s| s.into_inner())
    }.and_then(|s|
    {
        //SET TCP_NODELAY
        s.set_nodelay(true)?;
        Ok(s)
    })
}

pub fn listen_server(streams: &mut Streams, tx: Sender<ClientEvent>) //SERVER -> CLIENT COMMUNICATION
{
    //SEND HEADER
    let mut header = [0u8; 32];
    SysRng.try_fill_bytes(&mut header).unwrap(); //GENERATE RANDOM HEADER
    options::set_obfuscation_key(&header);
    streams.1.lock().unwrap().write_all(&header).unwrap();

    //SET GLOBAL CLIENT ENCRYPTION & MAC KEY
    let mut keys = (Zeroizing::new(vec![]), Zeroizing::new(vec![]));
    if !key_exchange(streams, &mut keys, &tx, None) { return; }

    //SERVER INFO VARIABLES
    let mut min_pass: Option<u64> = None;
    let mut max_uname: Option<u64> = None;
    let mut min_uname: Option<u64> = None;

    let mut invalid_username = false; //PRINT "Invalid Username!"
    let mut invalid_password = false;

    let mut disabled_registration = false; //PRINT "Registration disabled!"

    //FORMATTING SHIT
    let mut first_message = true;
    let mut extra_space: bool;

    //CONNECTION PROPERTIES
    let mut id = 0usize; //ID SET BY SERVER
    let mut username: Option<String> = None;

    //LOOP READING
    loop
    {
        let read = match network::receive(streams, Some(&keys), None)
        {
            Some(packet) => packet,
            None => MessagePacket
            {
                code: Some(MessageCode::Disconnect),
                ..Default::default()
            }
        };

        //CHECK FOR MUTED CLIENT
        if read.id.is_some() && options::is_muted(read.id) { continue; }

        //KEEPALIVE
        if read.code == Some(MessageCode::KeepAlive)
        {
            //ECHO
            network::send(&mut streams.1.lock().unwrap(), MessagePacket
            {
                code: Some(MessageCode::KeepAlive),
                ..Default::default()
            }, options::get_keys().as_ref());
            continue;
        }

        extra_space = false; //RESET EXTRA SPACE

        //EXTRA SPACE
        if options::get_extra_space() { tx.send(ClientEvent::ExtraSpace).unwrap(); }

        //CODES
        if let Some(code) = read.code
        {
            match code
            {
                //VERSION CHECK
                MessageCode::Version =>
                {
                    let version = misc::get_version().to_string();
                    let server_version = read.text.unwrap();

                    //NON MATCHING VERSION (WILL GET DISCONNECTED)
                    if server_version != version
                    {
                        tx.send(ClientEvent::IncompatibleVersion(version.clone(), server_version)).unwrap();
                    }

                    //RESPOND
                    network::send(&mut streams.1.lock().unwrap(), MessagePacket
                    {
                        text: Some(version),
                        code: Some(MessageCode::Version),
                        ..Default::default()
                    }, Some(&keys));

                    continue;
                }

                //WELCOME CODE - SERVER INFORMATIONS
                MessageCode::Welcome =>
                {
                    //PARSE JSON
                    let welcome_json: Value = serde_json::from_str(&read.text.unwrap()).expect("Parsing welcome json failed"); //PARSE WELCOME JSON

                    //GET INFO FROM JSON
                    min_pass = Some(welcome_json["min_pass"].as_u64().expect("Invalid welcome json"));
                    max_uname = Some(welcome_json["max_uname"].as_u64().expect("Invalid welcome json"));
                    min_uname = Some(welcome_json["min_uname"].as_u64().expect("Invalid welcome json"));
                    options::set_server_username(welcome_json["server_uname"].as_str().expect("Invalid welcome json"));

                    //COMPARSE HASHES
                    let client_hash = env!("WHY2_GIT_HASH");
                    let server_hash = welcome_json["git_hash"].as_str().expect("Invalid welcome json");
                    if !client_hash.is_empty() && !server_hash.is_empty() && client_hash != server_hash
                    {
                        //DISPLAY VERSION MISMATCH
                        tx.send(ClientEvent::VersionMismatch(client_hash.to_string(), server_hash.to_string())).unwrap();
                    }

                    tx.send(ClientEvent::Connected(welcome_json["server_name"].as_str().expect("Invalid welcome json").to_string())).unwrap();
                },

                //REKEY - CHANGE KEYS
                MessageCode::Rekey =>
                {
                    //WAIT FOR SERVER TO INIT KEY EXCHANGE
                    let current_keys = keys.clone();
                    key_exchange(streams, &mut keys, &tx, Some(&current_keys));
                }

                //PICK_USERNAME CODE - guess what
                MessageCode::Username =>
                {
                    tx.send(ClientEvent::Clear(2)).unwrap();

                    //INVALID UNAME
                    if invalid_username
                    {
                        tx.send(ClientEvent::UsernameRejected).unwrap();
                    } else //VALID
                    {
                        //SET INVALID USERNAME FOR POSSIBLE NEXT CODE
                        invalid_username = true;
                    }

                    tx.send(ClientEvent::Username(disabled_registration, min_uname.unwrap(), max_uname.unwrap())).unwrap();
                },

                //REGISTER
                MessageCode::PasswordR =>
                {
                    tx.send(ClientEvent::Clear(3)).unwrap();
                    options::set_asking_password(true);

                    //INVALID PASS
                    if invalid_password
                    {
                        tx.send(ClientEvent::PasswordRejected(min_pass.unwrap())).unwrap();
                    } else
                    {
                        invalid_password = true;
                    }

                    tx.send(ClientEvent::Register).unwrap();
                },

                //LOGIN
                MessageCode::PasswordL =>
                {
                    options::set_asking_password(true);
                    tx.send(ClientEvent::Login).unwrap();
                },

                //START CHATTING
                MessageCode::Accept =>
                {
                    tx.send(ClientEvent::Authenticated).unwrap();

                    //SET SERVER-SIDE ID
                    id = read.text.unwrap_or_else(|| "0".to_string()).parse().unwrap();

                    //ALLOW MESSAGE HISTORY & COMMANDS
                    options::set_sending_messages(true);
                },

                //JOIN MESSAGE (CLIENT CONNECTED)
                MessageCode::Join =>
                {
                    tx.send(ClientEvent::Clear(2)).unwrap();

                    let user = read.text.unwrap();

                    if first_message
                    {
                        tx.send(ClientEvent::ExtraSpace).unwrap();
                        username = Some(user.clone());
                        first_message = false;
                    }

                    tx.send(ClientEvent::Join(user)).unwrap();
                }

                //LEAVE MESSAGE (CLIENT DISCONNECTED)
                MessageCode::Leave =>
                {
                    tx.send(ClientEvent::Leave(read.text.unwrap())).unwrap();
                    voice_client::remove_consumer(&read.id.unwrap());
                },

                //CHANNEL CHANGE
                MessageCode::Channel =>
                {
                    //REMOVE ALL STORED VOICE CLIENTS
                    voice_client::remove_all_consumers();

                    options::set_channel(read.text.unwrap_or_else(|| String::new()));

                    tx.send(ClientEvent::Clear(1)).unwrap();
                },

                //SERVER ALLOWED VOICE
                MessageCode::Voice =>
                {
                    if options::socks5_enabled()
                    {
                        tx.send(ClientEvent::Socks5Voice).unwrap();
                        continue;
                    }

                    //TOGGLE VOICE (& PRINT STATUS)
                    tx.send(if voice_options::swap_use_voice()
                    {
                        let username = username.clone();
                        let voice_tx = tx.clone();
                        let stream = streams.1.clone();
                        thread::spawn(move || voice_client::listen_server_voice(id, username.unwrap(), voice_tx, stream));
                        ClientEvent::VoiceEnabled
                    } else
                    {
                        ClientEvent::VoiceDisabled
                    }).unwrap();
                },

                //VOICE CLIENTS
                MessageCode::VoiceClients =>
                {
                    //PARSE JSON
                    let clients: Vec<(usize, String)> = serde_json::from_str(&read.text.unwrap()).expect("Parsing welcome json failed");

                    //ADD CLIENTS
                    for (id, username) in clients
                    {
                        voice_client::add_consumer(id, username);
                    }
                }

                //CLIENT JOINED VOICE CHANNEL
                MessageCode::ChannelJoin =>
                {
                    let joined_id = read.id.unwrap();
                    if voice_options::get_use_voice() && id != joined_id
                    {
                        voice_client::add_consumer(read.id.unwrap(), read.username.unwrap());
                    }
                },

                //CLIENT LEFT VOICE CHANNEL
                MessageCode::ChannelLeave =>
                {
                    voice_client::remove_consumer(&read.id.unwrap());
                },

                //LIST OF ONLINE USERS
                MessageCode::List =>
                {
                    if !options::get_extra_space() { tx.send(ClientEvent::ExtraSpace).unwrap(); }
                    tx.send(ClientEvent::List(serde_json::from_str(&read.text.unwrap()).unwrap())).unwrap();
                    extra_space = true;
                    options::set_extra_space(true);
                },

                //UPLOAD APPROVAL
                MessageCode::Upload =>
                {
                    //SPAWN UPLOAD THREAD
                    let file_tx = tx.clone(); //CLONE TX
                    thread::spawn(move || file::upload(read.file.unwrap(), file_tx));
                    continue;
                },

                //DOWNLOAD
                MessageCode::Download =>
                {
                    //SPAWN DOWNLOAD THREAD
                    let file_tx = tx.clone(); //CLONE TX
                    thread::spawn(move || file::download(read.file.unwrap(), file_tx));
                    continue;
                },

                //UPLOADED ANNOUNCEMENT
                MessageCode::Uploaded =>
                {
                    tx.send(ClientEvent::Uploaded(read.username.unwrap(), read.text.unwrap())).unwrap();
                },

                //FILE LIST
                MessageCode::Files =>
                {
                    //PARSE JSON
                    let uploads_json: Vec<Value> = serde_json::from_str(&read.text.unwrap()).unwrap();

                    if !uploads_json.is_empty()
                    {
                        if !options::get_extra_space() { tx.send(ClientEvent::ExtraSpace).unwrap(); }
                        extra_space = true;
                        options::set_extra_space(true);
                    }

                    tx.send(ClientEvent::Files(uploads_json)).unwrap();
                },

                //MAX PARALLEL UPLOADS
                MessageCode::UploadLimit =>
                {
                    tx.send(ClientEvent::UploadLimit).unwrap();
                },

                //SCREEN UPLOAD APPROVAL
                MessageCode::ScreenUpload =>
                {
                    //SPAWN UPLOAD THREAD
                    let file_tx = tx.clone(); //CLONE TX
                    thread::spawn(move || screen::screen_upload(read.token.unwrap(), file_tx));
                    continue;
                },

                //PRIVATE MESSAGE INCOMING
                MessageCode::PrivateMessage =>
                {
                    tx.send(ClientEvent::PrivateMessageRecv(read.username.unwrap(), read.id.unwrap(), read.text.unwrap())).unwrap();
                },

                //PRIVATE MESSAGE INCOMING
                MessageCode::PrivateMessageBack =>
                {
                    tx.send(ClientEvent::PrivateMessageSent(read.username.unwrap(), read.id.unwrap(), read.text.unwrap())).unwrap();
                },

                //SPAM WARNING
                MessageCode::SpamWarning =>
                {
                    tx.send(ClientEvent::SpamWarning).unwrap();
                },

                //REGISTRATION DISABLED
                MessageCode::RegisterDisabled =>
                {
                    disabled_registration = true;
                },

                //CLIENT MESSED SOME COMMAND UP
                MessageCode::InvalidUsage =>
                {
                    tx.send(ClientEvent::InvalidUsage).unwrap();
                },

                //CLIENTED REQUESTED DISABLED FEATURE
                MessageCode::InvalidFeature =>
                {
                    tx.send(ClientEvent::DisabledFeature).unwrap();
                },

                //SERVER DOESN'T LIKE YA ANYMORE - EXIT
                MessageCode::Disconnect =>
                {
                    tx.send(ClientEvent::Quit).unwrap();
                    return;
                },

                _ => continue //EITHER INVALID CODE OR A KEY EXCHANGE CODE
            }
        } else //NO CODE, PRINT MESSAGE
        {
            tx.send(ClientEvent::Message(read)).unwrap();
        }

        //PRINT INPUT PROMPT
        tx.send(ClientEvent::Prompt).unwrap();
        if !extra_space { options::set_extra_space(false); } //DISABLE EXTRA SPACE
    }
}
