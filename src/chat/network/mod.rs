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
    net::TcpStream,
    io::
    {
        Read,
        Write,
        BufReader,
        BufRead,
    },
};

use serde::{ Serialize, Deserialize };

use crate::
{
    chat::
    {
        config,
        options as rex_options,
    },

    core::rex::
    {
        encrypter,
        decrypter,
        options,
        Grid,
    },
};

#[cfg(feature = "server")]
use std::time::{ Instant, Duration };

#[cfg(feature = "server")]
use crate::chat::network::server::DisconnectType;

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
}

#[derive(Serialize, Deserialize)]
pub struct MessagePacket //MESSAGE PACKET (WHAT IS BEING SENT)
{
    pub text: Option<String>,      //MESSAGE
    pub username: Option<String>,  //USERNAME (SENT ONLY BY SERVER, AS SERVER DOESN'T ACCEPT USERNAMES FROM CLIENT)
    pub id: Option<usize>,         //ID OF USER
    pub code: Option<MessageCode>, //CONTROL CODE
}

//CONSTS
const GRID_W: usize = rex_options::GRID_DIMENSIONS.0;
const GRID_H: usize = rex_options::GRID_DIMENSIONS.1;

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
    //GET MAX PACKET SIZE
    let max_packet_size: usize;

    #[cfg(feature = "server")]
    {
        max_packet_size = config::server_config("max_packet_size").parse::<usize>().unwrap();
    }

    #[cfg(not(feature = "server"))]
    {
        max_packet_size = config::client_config("max_packet_size").parse::<usize>().unwrap();
    }

    //READ
    let mut reader = BufReader::new(&mut *stream).take(max_packet_size as u64 + 16);
    let mut packet = String::new();

    //LOOP UNTIL MESSAGE ARRIVES
    while packet.is_empty()
    {
        match reader.read_line(&mut packet)
        {
            Ok(0) | Err(_) => //CLIENT DISCONNECTED
            {
                #[cfg(feature = "server")]
                {
                    server::remove_connection(stream, DisconnectType::Forcefully);
                }

                return None;
            },

            Ok(_i) => //VALID MESSAGE
            {
                #[cfg(feature = "server")]
                {
                    if _i >= max_packet_size //INPUT TOO LONG
                    {
                        server::remove_connection(stream, DisconnectType::Gracefully);
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
            output: Grid::<GRID_W, GRID_H>::from_bytes(decoded_packet)?, //CONVERT decoded_packet FROM Vec<u8> TO Vec<Grid>
            key: Grid::from_key(key.to_vec()),
        });

        //OVERWRITE decoded_packet
        decoded_packet = base91::slice_decode(decrypted_packet.as_bytes());
    }

    //ACTIVITY TIMER ON SERVER
    #[cfg(feature = "server")]
    {
        let mut connections = server::CONNECTIONS.write().unwrap(); //WRITE LOCK
        let peer_addr = stream.peer_addr().unwrap(); //GET CURRENT PEER ADDRESS
        let mut disconnect = false;

        //FIND CONNECTION AND SET last_activity
        for conn in connections.iter_mut()
        {
            if conn.stream().lock().unwrap().peer_addr().unwrap() == peer_addr //CONNECTION FOUND
            {
                //SPAM
                if conn.is_authenticated() && Instant::now().duration_since(*conn.last_activity()) < Duration::from_millis(config::server_config("min_message_delay").parse().unwrap())
                {
                    //INCREMENT SPAM VIOLATIONS
                    *conn.spam_violations_mut().unwrap() += 1;

                    //SEND WARNING CODE
                    server::send_code(stream, None, MessageCode::SpamWarning, conn.shared_key());

                    //CHECK FOR TOO MANY VIOLATIONS
                    disconnect = *conn.spam_violations().unwrap() > config::server_config("max_message_delay_violations").parse().unwrap();
                }

                *conn.last_activity_mut() = Instant::now(); //RESET last_activity

                break;
            }
        }

        //TOO MANY VIOLATIONS, BYE
        if disconnect
        {
            drop(connections); //DROP WRITE LOCK
            server::remove_connection(stream, DisconnectType::Gracefully);
            return None;
        }
    }

    //DECODE AND RETURN
    Some(bincode::serde::decode_from_slice::<MessagePacket, _>(&decoded_packet, bincode::config::standard()).expect("Decoding packet failed").0)
}
