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
    thread,
    net::{ TcpListener, TcpStream },
};

//PRIVATE
fn listen_client(stream: TcpStream)
{
    println!("Skibidi");
}

//PUBLIC
pub fn accept_connections(listener: TcpListener) //ACCEPT CONNECTIONS TO SERVER
{
    for stream in listener.incoming()
    {
        match stream
        {
            Ok(stream) =>
            {
                println!("New connection: {}", stream.peer_addr().unwrap());
                thread::spawn(move || listen_client(stream));
            }
            Err(e) =>
            {
                eprintln!("Connection failed: {}", e);
            }
        }
    }
}
