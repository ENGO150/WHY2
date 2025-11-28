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
    io::
    {
        Write,
        BufReader,
        BufRead,
    },
};

use serde::{ Deserialize, Deserializer, Serialize, Serializer };

use colored::Color;

use crate::
{
    chat::options as rex_options,
    core::rex::
    {
        encrypter,
        decrypter,
        options,
        Grid,
    },
};

#[cfg(feature = "server")]
use std::
{
    io::Read,
    time::{ Instant, Duration },
};

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
    InvalidUsage,       //SERVER -> CLIENT | INVALID PARAMETERS TO A COMMAND
    Channel,            //CLIENT -> SERVER | CLIENTS WANTS TO SWITCH CHANNEL
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
pub fn send(stream: &mut TcpStream, packet: MessagePacket, key: Option<&Vec<i64>>) //SEND packet TO stream
{
    //ENCODE THE PACKET STRUCT TO Vec<u8>
    let encoded_packet = bincode::serde::encode_to_vec(packet, bincode::config::standard()).expect("Encoding packet failed");
    let mut encoded_packet_string = String::from_utf8(base91::slice_encode(&encoded_packet)).expect("Encoding packet failed"); //ENCODE TO BASE91 STRING

    //ENCRYPT
    if let Some(key) = key
    {
        //ENCRYPT
        let encrypted_packet = encrypter::encrypt_string::<GRID_W, GRID_H>(&encoded_packet_string, Some(key.to_vec())).expect("Encrypting packet failed").output;

        //CONVERT ENCRYPTED PACKET (FROM Vec<Grid>) TO Vec<u8>
        let encrypted_packet_flattened: Vec<u8> = encrypted_packet.iter()
            .flat_map(|grid| grid.iter()
                .flat_map(|row| row.iter()
                    .flat_map(|&val| val.to_be_bytes()))).collect();

        //OVERWRITE encoded_packet_string
        encoded_packet_string = String::from_utf8(base91::slice_encode(&encrypted_packet_flattened)).expect("Encoding encrypted packet failed");
    }

    //SEND
    stream.write_all((encoded_packet_string + "\n").as_bytes()).expect("Sending packet failed");
    stream.flush().expect("Flushing stream failed");
}

pub fn receive(stream: &mut TcpStream, key: Option<&Vec<i64>>) -> Option<MessagePacket>
{
    //READ VARIABLES
    let mut reader: Box<dyn BufRead>;
    let mut packet = String::new(); //RECEIVED STRING

    //SERVER SIDE PACKET SIZE LIMIT
    #[cfg(feature = "server")]
    let max_packet_size: usize;

    //SERVER SIDE SPAM PROTECTION
    #[cfg(feature = "server")]
    let spam_protection = config::server_config::<bool>("spam_protection");

    //INIT READER
    #[cfg(feature = "server")]
    {
        //CHECK IF CLIENT IS AUTHENTICATED
        let client_addr = stream.peer_addr().ok();
        let authenticated = server::CONNECTIONS.read().unwrap().iter().any(|conn|
        {
            conn.stream().lock().unwrap().peer_addr().ok() == client_addr && conn.is_authenticated()
        });

        reader = Box::new(BufReader::new(stream.try_clone().expect("Cloning stream failed")));

        //ALLOW BIG MESSAGES WHEN SPAM PROTECTION IS OFF AND CLIENT IS AUTHENTICATED
        max_packet_size = if !spam_protection && authenticated
        {
            usize::MAX
        } else //SET MAX PACKET SIZE IF SPAM PROTECTION IS ENABLED
        {
            let max = config::server_config("max_packet_size");
            reader = Box::new(reader.take(max as u64 + 16));

            max
        };
    }
    #[cfg(not(feature = "server"))]
    {
        reader = Box::new(BufReader::new(&mut *stream));
    }

    //LOOP READING UNTIL MESSAGE ARRIVES
    loop
    {
        //CHECK IF CLIENT IS STILL IN ACTIVE CONNETION LIST
        #[cfg(feature = "server")]
        {
            let peer_addr = stream.peer_addr().ok();
            if !server::CONNECTIONS.read().unwrap().iter().any(|conn| conn.stream().lock().unwrap().peer_addr().ok() == peer_addr)
            {
                return None;
            }
        }

        //EXIT LOOP ON MESSAGE
        if !packet.is_empty() { break; }

        //READ
        match reader.read_line(&mut packet)
        {
            Ok(0) | Err(_) => //CLIENT DISCONNECTED
            {
                #[cfg(feature = "server")]
                {
                    server::remove_connection(stream, false);
                }

                return None;
            },

            Ok(_i) => //VALID MESSAGE
            {
                #[cfg(feature = "server")]
                {
                    if _i >= max_packet_size //INPUT TOO LONG
                    {
                        server::remove_connection(stream, true);
                        return None;
                    }
                }
            }
        }
    }

    //DECODE PACKET (BASE91)
    let mut decoded_packet = base91::slice_decode(packet.trim().as_bytes());

    //DECRYPT
    if let Some(key) = key
    {
        //DECRYPT
        let decrypted_packet = decrypter::decrypt_string(options::EncryptedData
        {
            output: Grid::<GRID_W, GRID_H>::from_bytes(decoded_packet).ok()?, //CONVERT decoded_packet FROM Vec<u8> TO Vec<Grid>
            key: Grid::from_key(key.to_vec()).unwrap(),
        }).unwrap();

        //OVERWRITE decoded_packet
        decoded_packet = base91::slice_decode(decrypted_packet.as_bytes());
    }

    //ACTIVITY TIMER ON SERVER
    #[cfg(feature = "server")]
    {
        let mut connections = server::CONNECTIONS.write().unwrap(); //WRITE LOCK
        let peer_addr = stream.peer_addr().ok(); //GET CURRENT PEER ADDRESS
        let mut disconnect = false;

        //FIND CONNECTION AND SET last_activity
        for conn in connections.iter_mut()
        {
            if conn.stream().lock().unwrap().peer_addr().ok() == peer_addr //CONNECTION FOUND
            {
                //SPAM
                if config::server_config("spam_protection") && conn.is_authenticated() &&
                    Instant::now().duration_since(*conn.last_activity()) <
                        Duration::from_millis(config::server_config::<u64>("min_message_delay"))
                {
                    //INCREMENT SPAM VIOLATIONS
                    *conn.spam_violations_mut().unwrap() += 1;

                    //SEND WARNING CODE
                    server::send_code(stream, None, MessageCode::SpamWarning, conn.shared_key());

                    //CHECK FOR TOO MANY VIOLATIONS
                    disconnect = *conn.spam_violations().unwrap() > config::server_config::<usize>("max_message_delay_violations");
                }

                *conn.last_activity_mut() = Instant::now(); //RESET last_activity

                break;
            }
        }

        //TOO MANY VIOLATIONS, BYE
        if disconnect
        {
            drop(connections); //DROP WRITE LOCK
            server::remove_connection(stream, true);
            return None;
        }
    }

    //DECODE AND RETURN
    match bincode::serde::decode_from_slice::<MessagePacket, _>(&decoded_packet, bincode::config::standard())
    {
        Ok((packet, _)) => Some(packet),
        Err(_) =>
        {
            //FORCEFULLY DISCONNECT CLIENT ON INVALID PACKET
            #[cfg(feature = "server")]
            server::remove_connection(stream, false);

            None
        }
    }
}
