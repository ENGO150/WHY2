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

use std::net::TcpStream;

use serde::{ Serialize, Deserialize };

use crate::chat::crypto;

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
fn key_exchange_client(stream: TcpStream)
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
pub fn listen_client(stream: TcpStream) //CLIENT -> SERVER COMMUNICATION
{
}

pub fn listen_server(stream: TcpStream) //SERVER -> CLIENT COMMUNICATION
{
    key_exchange_client(stream);
}

pub fn send(stream: TcpStream, packet: MessagePacket, key: Option<String>) //SEND packet TO stream
{
    //ENCODE THE PACKET STRUCT TO Vec<u8>
    let encoded_packet = bincode::serde::encode_to_vec(packet, bincode::config::standard()).expect("Encoding packet failed");

    println!("{:?}", encoded_packet);
}
