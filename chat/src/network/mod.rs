/*
This is part of WHY2
Copyright (C) 2022-2025 Václav Šmejkal

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

//MODULES
#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "client")]
pub mod client;

use std::
{
    str::FromStr,
    net::TcpStream,
    io::{ Read, Write },
};

use serde::{ Deserialize, Deserializer, Serialize, Serializer };

use colored::Color;

use hmac::{ Hmac, Mac };
use sha2::Sha256;

use why2_core::rex::
{
    encrypter,
    decrypter,
    options,
    Grid,
};

use crate::chat::options as rex_options;

#[cfg(feature = "server")]
use std::time::{ Instant, Duration };

#[cfg(feature = "server")]
use crate::chat::config;

//STRUCTS
#[derive(Serialize, Deserialize, PartialEq, Clone)]
pub enum MessageCode //CONTROL CODES
{
    KeyExchange,        //SERVER <> CLIENT | KEY EXCHANGE
    Welcome,            //SERVER -> CLIENT | INFORMATIONS
    Disconnect,         //SERVER <> CLIENT | QUIT COMMUNICATION
    Username,           //SERVER -> CLIENT | PICK USERNAME
    PasswordL,          //SERVER -> CLIENT | LOGIN
    PasswordR,          //SERVER -> CLIENT | REGISTER
    Accept,             //SERVER -> CLIENT | START CHATTING
    Join,               //SERVER -> CLIENT | CLIENT JOIN MESSAGE
    Leave,              //SERVER -> CLIENT | CLIENT LEAVE MESSAGE
    List,               //CLIENT <> SERVER | PRINT CONNECTED USERS
    PrivateMessage,     //CLIENT <> SERVER | SEND MESSAGE ONLY TO ONE CLIENT
    PrivateMessageBack, //SERVER -> CLIENT | SEND MESSAGE BACK TO SENDER
    SpamWarning,        //SERVER -> CLIENT | TELL CLIENT TO CALM TF DOWN
    RegisterDisabled,   //SERVER -> CLIENT | REGISTRATION IS DISABLED
    Version,            //SERVER <> CLIENT | ASK CLIENT FOR THEIR PKG VERSION
    Channel,            //SERVER <> CLIENT | CHANNEL CHANGE
    InvalidUsage,       //SERVER -> CLIENT | INVALID PARAMETERS TO A COMMAND
}

#[derive(Clone)]
pub struct SerColor(pub Color); //SERIALIZABLE Color

#[derive(Clone, Serialize, Deserialize)]
pub struct MessageColors //COLORS OF MESSAGE (ALL OF THE STRING VALUES WILL GET COVERTED TO colored::Color)
{
    pub username_color: Option<SerColor>, //COLOR OF SENDER
    pub message_color: Option<SerColor>,  //COLOR OF MESSAGE
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MessagePacket //MESSAGE PACKET (WHAT IS BEING SENT)
{
    pub text: Option<String>,      //MESSAGE
    pub username: Option<String>,  //USERNAME (SENT ONLY BY SERVER, AS SERVER DOESN'T ACCEPT USERNAMES FROM CLIENT)
    pub id: Option<usize>,         //ID OF USER
    pub code: Option<MessageCode>, //CONTROL CODE
    pub colors: MessageColors,     //MESSAGE COLORS
    pub seq: usize,                //SEQUENCE NUMBER
}

//CONSTS
const GRID_W: usize = rex_options::GRID_DIMENSIONS.0;
const GRID_H: usize = rex_options::GRID_DIMENSIONS.1;

//IMPLEMENTATIONS
impl Default for MessagePacket //DEFAULT
{
    fn default() -> Self
    {
        Self
        {
            text: None,
            username: None,
            id: None,
            code: None,
            colors: MessageColors
            {
                username_color: None,
                message_color: None,
            },
            seq: 0,
        }
    }
}

//SERIALIZE
impl Serialize for SerColor
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = match self.0
        {
            Color::Black => "black",
            Color::Red => "red",
            Color::Green => "green",
            Color::Yellow => "yellow",
            Color::Blue => "blue",
            Color::Magenta => "magenta",
            Color::Cyan => "cyan",
            Color::BrightBlack => "bright black",
            Color::BrightRed => "bright red",
            Color::BrightGreen => "bright green",
            Color::BrightYellow => "bright yellow",
            Color::BrightBlue => "bright blue",
            Color::BrightMagenta => "bright magenta",
            Color::BrightCyan => "bright cyan",
            Color::BrightWhite => "bright white",

            _ => "white",
        };

        serializer.serialize_str(s)
    }
}

//DESERIALIZE
impl<'de> Deserialize<'de> for SerColor
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Color::from_str(&s)
            .map(SerColor)
            .map_err(|_| serde::de::Error::custom(format!("Invalid color string: {}", s)))
    }
}

//FUNCTIONS
pub fn send(stream: &mut TcpStream, packet: MessagePacket, keys: Option<&(Vec<i64>, Vec<u8>)>) //SEND packet TO stream
{
    //COPY PACKET
    let mut packet = packet;

    //ADD SEQUENCE NUMBER TO packet (FROM CLIENT)
    #[cfg(feature = "client")]
    {
        packet.seq = rex_options::get_seq() + 1;
        rex_options::set_seq(packet.seq);
    }

    //ADD SEQUENCE NUMBER TO packet (FROM SERVER)
    #[cfg(feature = "server")]
    {
        if let Some(mut conn) = server::CONNECTIONS.get_mut(&stream.peer_addr().unwrap())
        {
            if conn.is_authenticated()
            {
                packet.seq = conn.server_seq().unwrap() + 1;
                *conn.server_seq_mut().unwrap() = packet.seq;
            }
        }
    }

    //ENCODE THE PACKET STRUCT TO Vec<u8>
    let packet_bytes = bincode::serde::encode_to_vec(packet, bincode::config::standard()).expect("Encoding packet failed");

    let final_bytes = if let Some(keys) = keys
    {
        //CONVERT packet_bytes to BINARY
        let mut input_i64 = Vec::with_capacity((packet_bytes.len() + 7) / 8);
        for chunk in packet_bytes.chunks(8)
        {
            let mut buf = [0u8; 8];
            buf[..chunk.len()].copy_from_slice(chunk);
            input_i64.push(i64::from_be_bytes(buf));
        }

        //ENCRYPT
        let encrypted_data = encrypter::encrypt::<GRID_W, GRID_H>(input_i64, Some(keys.0.clone())).expect("Encrypting packet failed");

        //SERIALIZE ENCRYPTED PACKET
        let mut grids = encrypted_data.output;
        grids.insert(0, encrypted_data.iv); //INITIALIZATION VECTOR

        //CONVERT ENCRYPTED PACKET (FROM Vec<Grid>) TO Vec<u8>
        let encrypted_bytes: Vec<u8> = grids.iter()
            .flat_map(|grid| grid.iter()
                .flat_map(|row| row.iter()
                    .flat_map(|&val| val.to_be_bytes()))).collect();

        //COMPUTE HMAC OVER CIPHERTEXT
        let mut mac = Hmac::<Sha256>::new_from_slice(&keys.1).expect("HMAC initialization failed");
        mac.update(&encrypted_bytes);

        let mac_tag = mac.finalize().into_bytes();

        //PREPEND MAC TO CIPHERTEXT ([32-byte HMAC][CIPHERTEXT])
        let mut transmission_bytes = Vec::with_capacity(32 + encrypted_bytes.len());
        transmission_bytes.extend_from_slice(&mac_tag);
        transmission_bytes.extend_from_slice(&encrypted_bytes);
        transmission_bytes
    } else
    {
        packet_bytes //NO ENCRYPTION
    };

    //ENCODE ENCRYPTED OUTPUT TO BASE91
    let encoded_string = String::from_utf8(base91::slice_encode(&final_bytes)).expect("Encoding packet failed");

    //SEND
    stream.write_all((encoded_string + "\n").as_bytes()).expect("Sending packet failed");
    stream.flush().expect("Flushing stream failed");
}

pub fn receive(stream: &mut TcpStream, buffer: &mut Vec<u8>, keys: Option<&(Vec<i64>, Vec<u8>)>) -> Option<MessagePacket>
{
    //SERVER SIDE PACKET SIZE LIMIT
    #[cfg(feature = "server")]
    let max_packet_size: usize;

    //SERVER SIDE SPAM PROTECTION
    #[cfg(feature = "server")]
    let spam_protection = config::server_config::<bool>("spam_protection");

    //SETUP LIMITS
    #[cfg(feature = "server")]
    {
        //CHECK IF CLIENT IS AUTHENTICATED
        let authenticated = stream.peer_addr().ok()
            .and_then(|addr| server::CONNECTIONS.get(&addr))
            .map(|conn| conn.is_authenticated())
            .unwrap_or(false);

        //ALLOW BIG MESSAGES WHEN SPAM PROTECTION IS OFF AND CLIENT IS AUTHENTICATED
        max_packet_size = if !spam_protection && authenticated
        {
            usize::MAX
        } else //SET MAX PACKET SIZE IF SPAM PROTECTION IS ENABLED
        {
            config::server_config("max_packet_size")
        };
    }

    //LOOP READING UNTIL MESSAGE ARRIVES
    loop
    {
        //CHECK IF THERE IS A NEWLINE IN BUFFER
        if let Some(pos) = buffer.iter().position(|&b| b == b'\n')
        {
            //EXTRACT LINE (INCLUDING \n)
            let mut line: Vec<u8> = buffer.drain(..=pos).collect();

            //REMOVE \n
            if let Some(&b'\n') = line.last() { line.pop(); }

            //DECODE PACKET (BASE91)
            let mut decoded_packet = base91::slice_decode(&line);

            //DECRYPT
            if let Some(keys) = keys
            {
                //HMAC VERIFICATION CLOSURE
                let verify_mac = |packet: Vec<u8>| -> Option<Vec<u8>>
                {
                    //VERIFY MAC LENGTH
                    if packet.len() < 32
                    {
                        return None;
                    }

                    //SEPARATE MAC FROM CIPHERTEXT
                    let received_mac: [u8; 32] = packet[..32].try_into().unwrap();
                    let ciphertext = &packet[32..];

                    //COMPUTE EXPECTED HMAC
                    let mut mac = Hmac::<Sha256>::new_from_slice(&keys.1).expect("HMAC initialization failed");
                    mac.update(ciphertext);

                    //VERIFY MAC
                    if mac.verify_slice(&received_mac).is_ok()
                    {
                        Some(ciphertext.to_vec())
                    } else
                    {
                        None
                    }
                };

                //VERIFY HMAC
                if let Some(ciphertext) = verify_mac(decoded_packet)
                {
                    decoded_packet = ciphertext;
                } else
                {
                    //LOG IF ON SERVER
                    #[cfg(feature = "server")]
                    println!("HMAC verification failed: {}", stream.peer_addr().unwrap());

                    return None;
                }

                //DESERIALIZE ENCRYPTED PACKET
                let mut grids = Grid::<GRID_W, GRID_H>::from_bytes(decoded_packet).ok()?;

                //EXTRACT INITIALIZATION VECTOR
                let iv = grids.remove(0);

                //DECRYPT
                let decrypted_packet = decrypter::decrypt(options::EncryptedData
                {
                    output: grids,
                    key: Grid::from_key(keys.0.clone()).unwrap(),
                    iv: iv,
                }).ok()?;

                //OVERWRITE decoded_packet
                decoded_packet = Vec::with_capacity(decrypted_packet.output.len() * 8);
                for val in decrypted_packet.output
                {
                    decoded_packet.extend_from_slice(&val.to_be_bytes());
                }
            }

            //ACTIVITY TIMER ON SERVER
            #[cfg(feature = "server")]
            {
                let peer_addr = stream.peer_addr().ok()?; //GET CURRENT PEER ADDRESS
                let mut spam_warning = false;
                let mut shared_key = None;
                let mut disconnect = false;

                //FIND CONNECTION AND SET last_activity
                if let Some(mut conn) = server::CONNECTIONS.get_mut(&peer_addr)
                {
                    //SPAM
                    if config::server_config("spam_protection") && conn.is_authenticated() &&
                        Instant::now().duration_since(*conn.last_activity()) <
                            Duration::from_millis(config::server_config::<u64>("min_message_delay"))
                    {
                        //INCREMENT SPAM VIOLATIONS
                        *conn.spam_violations_mut().unwrap() += 1;

                        //WARN
                        spam_warning = true;
                        shared_key = conn.keys().cloned();

                        //CHECK FOR TOO MANY VIOLATIONS
                        disconnect = *conn.spam_violations().unwrap() > config::server_config::<usize>("max_message_delay_violations");
                    }

                    *conn.last_activity_mut() = Instant::now(); //RESET last_activity
                }

                //SEND WARNING CODE
                if spam_warning
                {
                    server::send_code(stream, None, MessageCode::SpamWarning, shared_key.as_ref());
                }

                //TOO MANY VIOLATIONS, BYE
                if disconnect
                {
                    server::remove_connection(stream, true);
                    return None;
                }
            }

            //DECODE AND RETURN
            match bincode::serde::decode_from_slice::<MessagePacket, _>(&decoded_packet, bincode::config::standard())
            {
                Ok((packet, _)) =>
                {
                    //VERIFY SEQUENCE NUMBER
                    #[cfg(feature = "server")] //ON SERVER
                    {
                        if let Some(mut conn) = server::CONNECTIONS.get_mut(&stream.peer_addr().ok()?)
                        {
                            if packet.seq > *conn.seq() //VALID SEQ
                            {
                                //SET SEQ TO CURRENT
                                *conn.seq_mut() = packet.seq;
                            } else
                            {
                                //INVALID SEQ
                                drop(conn); //PREVENT DEADLOCK
                                println!("SEQ verification failed: {}", stream.peer_addr().ok()?);
                                server::remove_connection(stream, false);
                            }
                        }
                    }

                    //VERIFY SEQUENCE NUMBER
                    #[cfg(feature = "client")] //ON CLIENT
                    {
                        if packet.seq > rex_options::get_server_seq() || rex_options::get_server_seq() == 0 || packet.code == Some(MessageCode::Disconnect) //VALID
                        {
                            //SET SEQ
                            rex_options::set_server_seq(packet.seq);
                        } else //INVALID, DISCONNECT
                        {
                            return None;
                        }
                    }

                    return Some(packet);
                },

                Err(_) =>
                {
                    //FORCEFULLY DISCONNECT CLIENT ON INVALID PACKET
                    #[cfg(feature = "server")]
                    server::remove_connection(stream, false);

                    return None;
                }
            }
        }

        //CHECK IF CLIENT IS STILL IN ACTIVE CONNECTION LIST
        #[cfg(feature = "server")]
        {
            if !server::CONNECTIONS.contains_key(&stream.peer_addr().ok()?)
            {
                return None;
            }
        }

        //READ FROM STREAM
        let mut chunk = [0u8; 1024];
        match stream.read(&mut chunk)
        {
            Ok(0) => //CLIENT DISCONNECTED
            {
                #[cfg(feature = "server")]
                {
                    server::remove_connection(stream, false);
                }

                return None;
            },

            Ok(n) => //VALID READ
            {
                #[cfg(feature = "server")]
                {
                    //CHECK MAX PACKET SIZE
                    if buffer.len() + n > max_packet_size
                    {
                         server::remove_connection(stream, true);
                         return None;
                    }
                }

                buffer.extend_from_slice(&chunk[..n]);
            },

            Err(_) => return None //ERROR OR TIMEOUT
        }
    }
}
