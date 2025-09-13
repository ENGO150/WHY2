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
    io,
    net::TcpListener,
};

use why2::
{
    core::misc,
    chat::
    {
        config,
        crypto,
        network,
        options,
    },
};

fn main()
{
    misc::check_version(); //CHECK FOR UPDATES
    config::init_server_config(); //CREATE server.toml CONFIGURATION
    crypto::init_keys(); //GENERATE ECC KEYS
    options::set_core_options(); //SET ENCRYPTION OPTIONS

    let address = format!("{}:{}", config::server_config("server_ip"), options::SERVER_PORT); //GET ADDRESS
    let listener = TcpListener::bind(&address).expect("Binding failed"); //BIND ADDRESS
    println!("Server enabled.\nListening on {address}\n"); //INFO PRINT

    //CREATE THREAD FOR ACCEPTING CLIENTS
    thread::spawn(move ||
    {
        for stream in listener.incoming()
        {
            match stream
            {
                Ok(mut stream) =>
                {
                    println!("New connection: {}", stream.peer_addr().unwrap());
                    thread::spawn(move || network::listen_client(&mut stream));
                },

                Err(e) =>
                {
                    eprintln!("Connection failed: {}", e);
                }
            }
        }
    });

    //LOOP FOR SERVER-SIDE USER INPUT
    loop
    {
        //READ INPUT
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        input = input.trim().to_owned(); //TRIM

        //EXIT
        if input == "!exit"
        {
            println!("Exiting...");
            break;
        }
    }
}
