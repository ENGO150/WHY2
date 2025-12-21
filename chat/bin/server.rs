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
    misc,
    crypto,
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
    //CONFIGURATION
    misc::check_version(); //CHECK WHY2 VERSION
    config::init_server_config(); //CREATE server.toml CONFIGURATION
    crypto::generate_server_keys(); //GENERATE STATIC ECC KEYPAIR

    let address = format!("{}:{}", config::server_config::<String>("server_ip"), config::server_config::<u16>("server_port")); //GET ADDRESS
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
                    //CHECK FOR MAXIMAL CONNECTIONS
                    if server::CONNECTIONS.len() >= config::server_config::<usize>("max_clients")
                    {
                        eprintln!
                        (
                            "Connection rejected (Server full): {}",
                            stream.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "unknown".to_string())
                        );

                        continue;
                    }

                    //SET TCP_NODELAY
                    match stream.set_nodelay(true)
                    {
                        Ok(_) => {},
                        Err(_) => continue
                    }

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
