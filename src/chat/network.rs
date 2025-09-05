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

use std::
{
    net::TcpStream,
    io::
    {
        Write,
        BufReader,
        BufRead,
    },
};

use serde::{ Serialize, Deserialize };
use serde_json::{ json, Value };

use crate::
{
    chat::
    {
        config,
        crypto,
        options as chat_options,
    },
    core::
    {
        encrypter,
        decrypter,
        options,
    },
};

//STRUCTS
#[derive(Serialize, Deserialize, PartialEq)]
pub enum MessageCode //CONTROL CODES
{
    ClientServerKE, //CLIENT -> SERVER KEY EXCHANGE
    ServerClientKE, //SERVER -> CLIENT KEY EXCHANGE
    Welcome,        //SERVER -> CLIENT INFORMATIONS
}

#[derive(Serialize, Deserialize)]
pub struct MessagePacket
{
    pub text: Option<String>, //MESSAGE
    pub username: Option<String>, //USERNAME (SENT ONLY BY SERVER, AS SERVER DOESN'T ACCEPT USERNAMES FROM CLIENT)
    pub code: Option<MessageCode>, //CONTROL CODE
}

//PRIVATE
fn key_exchange_client(stream: &mut TcpStream) -> (String, String) //(SharedKey, ServerUsername)
{
    //SEND ECC PUBKEY TO SERVER
    send(stream, MessagePacket
    {
        text: Some(crypto::get_public_key()),
        username: Some(config::server_config("server_username")),
        code: Some(MessageCode::ClientServerKE),
    }, None);

    //WAIT FOR ServerClientKE
    let message = loop
    {
        match receive(stream, None)
        {
            Some(msg) =>
            {
                //MATCH, EXIT LOOP
                if msg.code == Some(MessageCode::ServerClientKE)
                {
                    break msg;
                }
            },

            None => continue
        }
    };

    //CALCULATE SHARED SECRET
    (crypto::get_shared_key(message.text.unwrap()), message.username.unwrap())
}

fn key_exchange_server(stream: &mut TcpStream) -> String
{
    //WAIT FOR ClientServerKE
    let message = loop
    {
        match receive(stream, None)
        {
            Some(msg) =>
            {
                //MATCH, EXIT LOOP
                if msg.code == Some(MessageCode::ClientServerKE) && msg.text != None
                {
                    break msg;
                }
            },

            None => continue
        }
    };

    //SEND ECC PUBKEY TO CLIENT
    send(stream, MessagePacket
    {
        text: Some(crypto::get_public_key()),
        username: Some(config::server_config("server_username")),
        code: Some(MessageCode::ServerClientKE),
    }, None);

    //CALCULATE SHARED SECRET
    crypto::get_shared_key(message.text.unwrap())
}

fn send_welcome_packet(stream: &mut TcpStream, key: &str)
{
    let welcome_json = json!(
    {
        "max_uname": config::server_config("max_username_length"),
        "min_uname": config::server_config("min_username_length"),
        "max_tries": config::server_config("max_username_tries"),
        "server_name": config::server_config("server_name"),
    }).to_string();

    send(stream, MessagePacket
    {
        text: Some(welcome_json),
        username: Some(config::server_config("server_username")),
        code: Some(MessageCode::Welcome),
    }, Some(key));
}

//PUBLIC
pub fn listen_client(stream: &mut TcpStream) //CLIENT -> SERVER COMMUNICATION
{
    //GET SHARED KEY
    let shared_key = key_exchange_server(stream);
    
    //SEND PACKET WITH REQUIRED SERVER INFO
    send_welcome_packet(stream, &shared_key);
}

pub fn listen_server(stream: &mut TcpStream) //SERVER -> CLIENT COMMUNICATION
{
    //GET SHARED KEY
    let (shared_key, server_username) = key_exchange_client(stream);
    chat_options::set_shared_key(shared_key); //SET GLOBAL CLIENT SHARED KEY

    //SERVER INFO VARIABLES
    let mut max_uname: u8;
    let mut min_uname: u8;
    let mut server_name: &str;
    let mut max_tries: u8;

    loop
    {
        let read = match receive(stream, Some(&chat_options::get_shared_key()))
        {
            Some(msg) => msg,
            None => continue
        };

        //CODES
        if let Some(code) = read.code
        {
            match code
            {
                //WELCOME CODE - SERVER INFORMATIONS
                MessageCode::Welcome =>
                {
                    //TEXT SHOULD CONTAIN JSON DATA
                    let text = match read.text
                    {
                        Some(text) => text,
                        None => continue //NO JSON DATA, CONTINUE
                    };

                    //PARSE JSON
                    let welcome_json: Value = serde_json::from_str(&text).expect("Parsing welcome json failed"); //PARSE WELCOME JSON

                    //GET INFO FROM JSON
                    max_uname = welcome_json["max_uname"].as_str().expect("Invalid welcome json").parse().expect("Parsing info to int failed");
                    min_uname = welcome_json["min_uname"].as_str().expect("Invalid welcome json").parse().expect("Parsing info to int failed");
                    server_name = welcome_json["server_name"].as_str().expect("Invalid welcome json");
                    max_tries = welcome_json["max_tries"].as_str().expect("Invalid welcome json").parse().expect("Parsing info to int failed");

                    println!("\nSuccessfully connected to {server_name}.\n");
                },

                MessageCode::PickUsername =>
                {
                    println!("A");
                },

                _ => continue //EITHER INVALID CODE OR A KEY EXCHANGE CODE
            }
        }
    }
}

pub fn send(stream: &mut TcpStream, packet: MessagePacket, key: Option<&str>) //SEND packet TO stream
{
    //ENCODE THE PACKET STRUCT TO Vec<u8>
    let encoded_packet = bincode::serde::encode_to_vec(packet, bincode::config::standard()).expect("Encoding packet failed");
    let mut encoded_packet_string = hex::encode(encoded_packet); //ENCODE TO HEX STRING

    //ENCRYPT
    if let Some(key) = key
    {
        //ENCRYPT
        let encrypted_packet = encrypter::encrypt_text(&encoded_packet_string, Some(&key)).output.expect("Encrypting packet failed");

        //CONVERT ENCRYPTED PACKET (FROM Vec<i64>) TO Vec<u8>
        let mut encrypted_packet_flattened = Vec::with_capacity(encrypted_packet.len() * 8);
        for num in &encrypted_packet
        {
            encrypted_packet_flattened.extend_from_slice(&num.to_le_bytes()); //FLATTEN i64s to u8s
        }

        //OVERWRITE encoded_packet_string
        encoded_packet_string = hex::encode(encrypted_packet_flattened);
    }

    //SEND
    stream.write_all((encoded_packet_string + "\n").as_bytes()).expect("Sending packet failed");
    stream.flush().expect("Flushing stream failed");
}

pub fn receive(stream: &mut TcpStream, key: Option<&str>) -> Option<MessagePacket>
{
    //READ
    let mut reader = BufReader::new(stream);
    let mut packet = String::new();
    reader.read_line(&mut packet).expect("Reading packet failed"); //TODO: Make function blocking

    if packet.is_empty() { return None; } //INVALID READ

    //DECODE PACKET (HEX)
    let mut decoded_packet = hex::decode(packet.trim()).expect("Decoding packet failed");

    //DECRYPT
    if let Some(key) = key
    {
        //CONVERT decoded_packet FROM Vec<u8> TO Vec<i64>
        let recovered_encrypted_packet: Vec<i64> = decoded_packet.chunks_exact(8).map(|chunk|
        {
            //CONVERT chunk TO [u8]
            let mut array = [0u8; 8];
            array.copy_from_slice(chunk);

            //RETURN i64
            i64::from_le_bytes(array)
        }).collect();

        //DECRYPT
        let decrypted_packet = decrypter::decrypt_text(options::EncryptedData
        {
            output: Some(recovered_encrypted_packet),
            key: Some(key.to_owned()),
        }).output.expect("Decrypting packet failed");

        //OVERWRITE decoded_packet
        decoded_packet = hex::decode(decrypted_packet).expect("Decoding packet failed");
    }

    //DECODE AND RETURN
    Some(bincode::serde::decode_from_slice::<MessagePacket, _>(&decoded_packet, bincode::config::standard()).expect("Decoding packet failed").0)
}
