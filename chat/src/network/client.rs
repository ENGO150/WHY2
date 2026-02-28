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
    fs::File,
    io::Read,
    thread,
    path::PathBuf,
    net::TcpStream,
    collections::HashMap,
    sync::
    {
        Arc,
        Mutex,
        LazyLock,
        mpsc::Sender,
    },
};

use zeroize::Zeroizing;

use serde_json::Value;

use crate::
{
    options,
    misc,
    crypto::kex,
    consts,
    config::{ self, TofuCode },
    network::
    {
        self,
        MessageCode,
        MessagePacket,
        FilePayload,
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
    Connected(String),             //SUCCESSFUL CONNECTION MESSAGE
    Message(MessagePacket),        //RECEIVED MESSAGE
    Info(String, bool, usize),     //INFO/STATUS LOG, WITH NEWLINE BOOLEAN AND LINES TO CLEAR
    Warn(String, bool, usize),     //WARN LOG/POPUP, WITH NEWLINE BOOLEAN AND LINES TO CLEAR
    Prompt(String, String),        //">>>" PROMPT, WITH CHANNEL AND WRITTEN MESSAGE
    TofuError(TofuCode),           //TOFU VERIFICATION FAILED
    VoiceActivity(Vec<VoiceUser>), //VOICE OVERLAY
    Clear(usize),                  //CLEAR n LINES
    ExtraSpace,                    //JUST RANDOM NEWLINE
    Quit,                          //SERVER QUIT COMMUNICATION
}

//LISTS
pub static ACTIVE_UPLOADS: LazyLock<Arc<Mutex<HashMap<[u8; 32], PathBuf>>>> = LazyLock::new(|| //ACTIVE UPLOADS
{
    Arc::new(Mutex::new(HashMap::new()))
});

//FUNCTIONS
//PRIVATE
fn key_exchange(stream: &mut TcpStream, keys: &mut consts::SharedKeys, tx: &Sender<ClientEvent>) -> bool //KEY EXCHANGE FOR CLIENT-SIDE
{
    //WAIT FOR KeyExchange
    let message = loop
    {
        //READ MESSAGE
        let received = network::receive(stream, None).unwrap();

        if received.code == Some(MessageCode::KeyExchange) { break received; }
    };

    let message_text = message.text.as_ref().unwrap();

    //PARSE SERVER KEYS JSON
    let server_keys: Value = serde_json::from_str(message_text).expect("Failed to parse server keys JSON");
    let server_ecc_pk = server_keys["ecc"].as_str().expect("Parsing server ECC key failed");
    let server_pq_pk = server_keys["pq"].as_str().expect("Parsing server PQ key failed");

    //VERIFY PUBKEY VALIDITY (TOFU)
    match config::server_keys_check(&stream.peer_addr().unwrap().ip().to_string(), message_text)
    {
        TofuCode::Valid => {},

        status @ (TofuCode::Mismatch | TofuCode::Unknown(_, _)) =>
        {
            //GRACEFULLY DISCONNECT FROM SERVER
            network::send(stream, MessagePacket
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
    network::send(stream, MessagePacket
    {
        text: Some(response_text),
        code: Some(MessageCode::KeyExchange),
        ..Default::default()
    }, None);

    //CALCULATE SHARED SECRET (HYBRID)
    *keys = kex::derive_shared_secret(sk, server_ecc_pk.to_string(), pq_secret).expect("Shared secret derivation failed");

    //SET GLOBAL KEYS VARIABLE
    options::set_keys(keys.clone());

    true
}

//PUBLIC
pub fn listen_server(stream: &mut TcpStream, tx: Sender<ClientEvent>) //SERVER -> CLIENT COMMUNICATION
{
    //SET GLOBAL CLIENT ENCRYPTION & MAC KEY
    let mut keys = (Zeroizing::new(vec![]), Zeroizing::new(vec![]));
    if !key_exchange(stream, &mut keys, &tx) { return; }

    //SERVER INFO VARIABLES
    let mut min_pass: Option<u64> = None;
    let mut max_uname: Option<u64> = None;
    let mut min_uname: Option<u64> = None;
    let mut server_uname = String::new();

    let mut invalid_username = false; //PRINT "Invalid Username!"
    let mut invalid_password = false;

    let mut disabled_registration = false; //PRINT "Registration disabled!"

    //FORMATTING SHIT
    let mut first_message = true;
    let mut extra_space: bool;

    let mut channel = String::new();

    //CONNECTION PROPERTIES
    let mut id = 0usize; //ID SET BY SERVER
    let mut username: Option<String> = None;

    //LOOP READING
    loop
    {
        let read = match network::receive(stream, Some(&keys))
        {
            Some(packet) => packet,
            None => continue
        };

        //CHECK FOR MUTED CLIENT
        if options::is_muted(read.id) { continue; }

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
                        tx.send(ClientEvent::Warn(String::from("Incompatible version! ({version}/{server_version})"), true, 1)).unwrap();
                    }

                    //RESPOND
                    network::send(stream, MessagePacket
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
                    server_uname = welcome_json["server_uname"].as_str().expect("Invalid welcome json").to_string();

                    tx.send(ClientEvent::Connected(welcome_json["server_name"].as_str().expect("Invalid welcome json").to_string())).unwrap();
                },

                //REKEY - CHANGE KEYS
                MessageCode::Rekey =>
                {
                    //WAIT FOR SERVER TO INIT KEY EXCHANGE
                    key_exchange(stream, &mut keys, &tx);
                }

                //PICK_USERNAME CODE - guess what
                MessageCode::Username =>
                {
                    tx.send(ClientEvent::Clear(2)).unwrap();

                    //INVALID UNAME
                    if invalid_username
                    {
                        tx.send(ClientEvent::Clear(2)).unwrap();
                        tx.send(ClientEvent::Warn(String::from("Username rejected!"), false, 0)).unwrap();
                    } else //VALID
                    {
                        //SET INVALID USERNAME FOR POSSIBLE NEXT CODE
                        invalid_username = true;
                    }

                    tx.send(ClientEvent::Info(format!
                    (
                        "\n\rEnter username ({}):",

                        if disabled_registration
                        {
                            String::from("Registration disabled!")
                        } else
                        {
                            format!("a-Z, 0-9; {}-{} characters", min_uname.unwrap(), max_uname.unwrap())
                        }
                    ), true, 0)).unwrap();
                },

                //REGISTER
                MessageCode::PasswordR =>
                {
                    tx.send(ClientEvent::Clear(3)).unwrap();
                    options::set_asking_password(true);

                    //INVALID PASS
                    if invalid_password
                    {
                        tx.send(ClientEvent::Warn(format!("Password rejected! Enter at least {} characters.", min_pass.unwrap()), false, 3)).unwrap();
                    } else
                    {
                        invalid_password = true;
                    }

                    tx.send(ClientEvent::Info(String::from("\n\rEnter password: (REGISTER)"), true, 0)).unwrap();
                },

                //LOGIN
                MessageCode::PasswordL =>
                {
                    options::set_asking_password(true);
                    tx.send(ClientEvent::Info(String::from("\nEnter password: (LOGIN)"), true, 3)).unwrap();
                },

                //START CHATTING
                MessageCode::Accept =>
                {
                    tx.send(ClientEvent::Info(String::from("Login successful. Press Ctrl+H for help.\n"), true, 3)).unwrap();

                    //SET SERVER-SIDE ID
                    id = read.text.unwrap_or("0".to_string()).parse().unwrap();

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

                    tx.send(ClientEvent::Info(format!("[{}]: {} connected.\n", read.username.unwrap(), user), true, 0)).unwrap();
                }

                //LEAVE MESSAGE (CLIENT DISCONNECTED)
                MessageCode::Leave =>
                {
                    tx.send(ClientEvent::Info(format!("[{}]: {} disconnected.\n", read.username.unwrap(), read.text.unwrap()), true, 2)).unwrap();
                    voice_client::remove_consumer(&read.id.unwrap());
                },

                //CHANNEL CHANGE
                MessageCode::Channel =>
                {
                    //REMOVE ALL STORED VOICE CLIENTS
                    voice_client::remove_all_consumers();

                    channel = if let Some(c) = read.text
                    {
                        format!("#{c} | ")
                    } else
                    {
                        String::new()
                    };

                    tx.send(ClientEvent::Clear(1)).unwrap();
                },

                //SERVER ALLOWED VOICE
                MessageCode::Voice =>
                {
                    if options::socks5_enabled()
                    {
                        tx.send(ClientEvent::Warn(String::from("Voice chat cannot be enabled while using SOCKS5.\n"), true, 2)).unwrap();
                        continue;
                    }

                    //TOGGLE VOICE
                    let status = if voice_options::swap_use_voice()
                    {
                        let username = username.clone();
                        let voice_tx = tx.clone();
                        let mut stream = stream.try_clone().unwrap();
                        thread::spawn(move || voice_client::listen_server_voice(id, username.unwrap(), voice_tx, &mut stream));
                        "en"
                    } else
                    {
                        "dis"
                    };

                    //PRINT STATUS
                    tx.send(ClientEvent::Info(format!("Voice {}abled.\n", status), true, 2)).unwrap();
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
                    tx.send(ClientEvent::Info(String::from("Online clients:"), true, 2)).unwrap();

                    //PARSE JSON
                    let users_json: Value = serde_json::from_str(&read.text.unwrap()).unwrap();

                    //PRINT USERS
                    for user in users_json.as_array().unwrap()
                    {
                        //GET CHANNEL
                        let c = if let Some(c) = user["channel"].as_str().map(String::from)
                        {
                            format!(" | #{c}")
                        } else
                        {
                            String::new()
                        };

                        tx.send(ClientEvent::Info(format!("\r{} ({}){}", user["username"].as_str().unwrap(), user["id"], c), true, 0)).unwrap();
                    }

                    tx.send(ClientEvent::ExtraSpace).unwrap();

                    extra_space = true;
                    options::set_extra_space(true);
                },

                //UPLOAD APPROVAL
                MessageCode::Upload =>
                {
                    //UPLOAD CONSTANTS
                    let payload = read.file.unwrap();
                    let file_hash = payload.hash.unwrap();

                    //GET FILE PATH
                    let path = ACTIVE_UPLOADS.lock().unwrap().remove(&file_hash).unwrap(); //(CRASHES IF SERVER REQUESTS FILE THAT ISN'T FOR UPLOAD)
                    let filename = path.clone().file_name().and_then(|n| n.to_str()
                        .map(|s| s.to_string())).unwrap_or(String::from("Unknown")); //GET FILENAME FOR CONSOLE LOG

                    //CLONE SERVER STREAM FOR UPLOAD THREAD
                    let mut upload_stream = stream.try_clone().unwrap();

                    //SPAWN UPLOAD THREAD
                    thread::spawn(move ||
                    {
                        let mut file = File::open(path).expect("Cannot open file for upload");
                        let mut buffer = vec![0; consts::UPLOAD_CHUNK_SIZE];

                        //LOOP READING
                        loop
                        {
                            match file.read(&mut buffer)
                            {
                                Ok(0) => break, //EOF
                                Ok(bytes) =>
                                {
                                    //SEND FILE CHUNK
                                    network::send(&mut upload_stream, MessagePacket
                                    {
                                        code: Some(MessageCode::Upload),
                                        file: Some(FilePayload
                                        {
                                            uid: payload.uid,
                                            data: Some(buffer[..bytes].to_vec()),
                                            ..Default::default()
                                        }),
                                        ..Default::default()
                                    }, options::get_keys().as_ref());
                                },
                                Err(_) => {}, //TODO: Implement
                            }
                        }
                    });

                    tx.send(ClientEvent::Info(format!("Uploading file \"{}\"...\n", filename), true, 1)).unwrap();
                }

                //PRIVATE MESSAGE INCOMING
                MessageCode::PrivateMessage =>
                {
                    tx.send(ClientEvent::Info(format!("[PM FROM] {} ({}): {}\n", read.username.unwrap(), read.id.unwrap(), read.text.unwrap()), true, 2)).unwrap();
                },

                //PRIVATE MESSAGE INCOMING
                MessageCode::PrivateMessageBack =>
                {
                    tx.send(ClientEvent::Info(format!("[PM TO] {} ({}): {}\n", read.username.unwrap(), read.id.unwrap(), read.text.unwrap()), true, 2)).unwrap();
                },

                //SPAM WARNING
                MessageCode::SpamWarning =>
                {
                    tx.send(ClientEvent::Warn(String::from("Slow down! You're sending messages too quickly.\n"), true, 2)).unwrap();
                },

                //REGISTRATION DISABLED
                MessageCode::RegisterDisabled =>
                {
                    disabled_registration = true;
                },

                //CLIENT MESSED SOME COMMAND UP
                MessageCode::InvalidUsage =>
                {
                    tx.send(ClientEvent::Info(String::from("Invalid usage! Press Ctrl+H for help.\n"), true, 2)).unwrap();
                },

                //CLIENTED REQUESTED DISABLED FEATURE
                MessageCode::InvalidFeature =>
                {
                    tx.send(ClientEvent::Warn(String::from("Server has disabled the feature you requested.\n"), true, 2)).unwrap();
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
        tx.send(ClientEvent::Prompt(channel.clone(), options::INPUT_READ.lock().unwrap().iter().collect::<String>())).unwrap();
        if !extra_space { options::set_extra_space(false); } //DISABLE EXTRA SPACE
    }
}
