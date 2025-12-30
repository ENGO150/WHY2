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
    sync::LazyLock,
    net::{ UdpSocket, SocketAddr },
};

use dashmap::DashMap;

use crate::chat::network::voice;

//LISTS
pub static CONNECTIONS: LazyLock<DashMap<usize, Option<SocketAddr>>> = LazyLock::new(|| DashMap::new()); //LIST FOR EACH CLIENT CONNECTION

pub fn listen_client_voice(socket: UdpSocket)
{
    //LOOP RECEIVING
    loop
    {
        let (received, addr) = voice::receive(&socket);

        //GET ID
        let id = usize::from_be_bytes(received[..8].try_into().unwrap());

        //CHECK IF ID IS IN CONNECTIONS
        if let Some(mut conn) = CONNECTIONS.get_mut(&id)
        {
            if conn.value().is_none()
            {
                *conn = Some(addr);
            }
        }

        //REMOVE ID FROM PACKET
        let received = &received[8..];

        //SEND TO ALL
        for connection in CONNECTIONS.iter()
        {
            if let Some(addr) = connection.value()
            {
                socket.send_to(received, addr).unwrap();
            }
        }
    }
}
