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
    io::Write,
    net::TcpStream,
};

use serde::{ Serialize, Deserialize };

use crate::
{
    core::encrypter,
    chat::crypto,
};

//STRUCTS
#[derive(Serialize, Deserialize)]
pub enum MessageCode //CONTROL CODES
{
    CLIENT_SERVER_KE, //CLIENT -> SERVER KEY EXCHANGE
}

#[derive(Serialize, Deserialize)]
pub struct MessagePacket
{
    pub text: Option<String>, //MESSAGE
    pub username: Option<String>, //USERNAME (SENT ONLY BY SERVER, AS SERVER DOESN'T ACCEPT USERNAMES FROM CLIENT)
    pub code: Option<MessageCode>, //CONTROL CODE
}

//PRIVATE
fn key_exchange_client(stream: &mut TcpStream)
{
    let client_pubkey = crypto::get_public_key();

    send(stream, MessagePacket
    {
        text: None,
        username: None,
        code: Some(MessageCode::CLIENT_SERVER_KE),
    }, None);
}

//PUBLIC
pub fn listen_client(stream: &mut TcpStream) //CLIENT -> SERVER COMMUNICATION
{
}

pub fn listen_server(stream: &mut TcpStream) //SERVER -> CLIENT COMMUNICATION
{
    key_exchange_client(stream);
}

pub fn send(stream: &mut TcpStream, packet: MessagePacket, key: Option<String>) //SEND packet TO stream
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
    stream.write_all(encoded_packet_string.as_bytes()).expect("Sending packet failed");
}
