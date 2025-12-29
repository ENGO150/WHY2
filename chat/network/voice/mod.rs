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
#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "server")]
pub mod server;

use std::
{
    io::Result,
    net::UdpSocket,
};

pub fn send(socket: &UdpSocket, data: &[u8]) -> Result<usize> //SEND DATA TO UDP
{
    socket.send(data)
}

pub fn receive(socket: &UdpSocket) -> Vec<u8> //RECEIVE UDP PACKET & DECODE
{
    let mut buffer = [0u8; 2048];
    loop
    {
        let len = match socket.recv_from(&mut buffer)
        {
            Ok(result) => result.0,
            Err(_) => continue
        };

        return buffer[..len].to_vec();
    }
}
