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

#![cfg(feature = "server")]

use std::
{
    process,
    thread,
    time::Duration,
    io,
    net::TcpListener,
};

use why2::chat::
{
    config,
    crypto,
    options,
    misc,
    network::server,
    command::{ self, Command },
};

fn quit() //DISCONNECT ALL USERS
{
    println!("Exiting...");
    server::disconnect_all(); //DISCONNECT ALL USERS
}

fn main()
{
    misc::check_version(); //CHECK WHY2 VERSION
    config::init_server_config(); //CREATE server.toml CONFIGURATION
    crypto::init_keys(); //GENERATE ECC KEYS

    let address = format!("{}:{}", config::server_config::<String>("server_ip"), options::SERVER_PORT); //GET ADDRESS
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
                    thread::spawn(move || server::listen_client(&mut stream));
                },

                Err(e) =>
                {
                    eprintln!("Connection failed: {}", e);
                }
            }
        }
    });

    //CREATE INACTIVITY WATCHDOG THREAD
    thread::spawn(move ||
    {
        loop
        {
            thread::sleep(Duration::from_secs(5));
            server::disconnect_inactive();
        }
    });

    //SET Ctrl+C HANDLER
    ctrlc::set_handler(move ||
    {
        //DISCONNECT ALL USERS AND EXIT
        quit();
        process::exit(0);
    }).expect("Setting Ctrl+C handler failed");

    //LOOP FOR SERVER-SIDE USER INPUT
    loop
    {
        //READ INPUT
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        input = input.trim().to_owned(); //TRIM

        //EXIT
        if let (Some(command), _) = command::get_command(&input.to_uppercase())
        {
            match command
            {
                Command::Exit =>
                {
                    quit(); //DISCONNECT ALL USERS
                    break;
                },

                _ => {}
            }
        }
    }
}
