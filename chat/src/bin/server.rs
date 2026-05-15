/*
This is part of WHY2
Copyright (C) 2022-2026 Václav Šmejkal

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
    thread,
    process,
    io::Read,
    time::Duration,
    sync::{ Arc, Mutex },
    net::
    {
        TcpListener,
        UdpSocket,
        Shutdown,
    },
};

use log::LevelFilter;
use simple_logger::SimpleLogger;

use why2_chat::
{
    misc,
    config,
    options,
    crypto::kex,
    network::
    {
        voice::server as voice_server,
        server::
        {
            self,
            file,
            ConnectionType,
        },
    },
};

fn quit() //DISCONNECT ALL USERS
{
    log::info!("Exiting...");
    server::disconnect_all(); //DISCONNECT ALL USERS
}

fn main()
{
    //INIT LOGGER
    SimpleLogger::new()
        .with_level(LevelFilter::Info)
        .with_module_level("ureq", LevelFilter::Warn) //DISABLE UREQ INFO LOGS
        .with_module_level("rustls", LevelFilter::Warn) //DISABLE RUSTLS INFO LOGS
        .init()
        .unwrap();

    //CONFIGURATION
    misc::check_version(); //CHECK WHY2 VERSION
    config::init_config(); //CREATE server.toml CONFIGURATION
    kex::generate_server_keys(); //GENERATE STATIC ECC KEYPAIR

    //CHECK IF VOICE IS ENABLED
    if config::read_config("enable_voice_chat")
    {
        options::enable_voice_chat();
    }

    //SET SERVER USERNAME
    options::set_server_username(&config::read_config::<String>("server_username"));

    //SERIALIZE ADDRESS
    let address = format!("{}:{}", config::read_config::<String>("server_ip"), config::read_config::<u16>("server_port")); //GET ADDRESS

    //BIND TCP (TEXT)
    let listener = match TcpListener::bind(&address)
    {
        Ok(l) => l,
        Err(_) =>
        {
            log::error!("Binding on {address} failed!");
            process::exit(1);
        }
    };

    //BIND UDP (VOICE)
    let udp_socket = if options::voice_chat_enabled() //VOICE ENABLED
    {
        Some(UdpSocket::bind(&address).expect("Binding UDP failed"))
    } else { None }; //VOICE DISABLED

    log::info!("Listening on {address}"); //PRINT INFO

    //CREATE KEEPALIVE & INACTIVITY WATCHDOG THREAD
    thread::spawn(move ||
    {
        let mut n = 0;

        loop
        {
            //DISCONNECT INACTIVE CLIENTS
            thread::sleep(Duration::from_secs(5));
            server::disconnect_inactive();

            //SEND KEEPALIVE PACKET TO ALL CLIENTS
            n = (n + 1) % 6;
            if n == 0
            {
                server::send_keepalive();
            }
        }
    });

    //SET Ctrl+C HANDLER
    ctrlc::set_handler(move ||
    {
        //DISCONNECT ALL USERS AND EXIT
        quit();
        process::exit(0);
    }).expect("Setting Ctrl+C handler failed");

    //CREATE THREAD FOR VOICE
    if options::voice_chat_enabled()
    {
        thread::spawn(move || voice_server::listen_client_voice(udp_socket.unwrap()));
    }

    //CREATE THREAD FOR ACCEPTING CLIENTS
    for stream in listener.incoming()
    {
        match stream
        {
            Ok(mut stream) =>
            {
                //GET PEER ADDR
                let peer_addr = match stream.peer_addr()
                {
                    Ok(p) => p,
                    Err(_) => continue
                };

                //SET TIMEOUT
                stream.set_read_timeout(Some(Duration::from_millis(2000))).expect("Failed to set read timeout"); //SET TIMEOUT

                //READ TOKEN
                let mut token = [0u8; 32];
                if let Ok(_) = stream.read_exact(&mut token)
                {
                    //REMOVE TIMEOUT
                    stream.set_read_timeout(None).expect("Failed to set read timeout");

                    //SET TCP_NODELAY
                    match stream.set_nodelay(true)
                    {
                        Ok(_) => {},
                        Err(_) => continue
                    }

                    if let Some((_, (id, conn_type))) = server::PENDING_TOKENS.remove(&token)
                    {
                        match conn_type
                        {
                            ConnectionType::FileUpload { uid } =>
                            {
                                let write_stream = Arc::new(Mutex::new(stream.try_clone().expect("Failed cloning stream")));
                                thread::spawn(move || file::download(id, &mut (&mut stream, write_stream), uid));
                                continue;
                            },

                            ConnectionType::FileDownload { uid, path } =>
                            {
                                thread::spawn(move || file::upload(id, stream, path, uid));
                                continue;
                            },
                        }
                    } else
                    {
                        //COUNT SLOTS
                        let auth_clients = server::CONNECTIONS.iter().filter(|c| c.is_authenticated()).count();
                        let unauth_clients = server::CONNECTIONS.len() - auth_clients;

                        //COUNT CONNECTIONS FROM SAME IP
                        let ip_clients = server::CONNECTIONS.iter().filter(|c| c.peer_addr().ip() == peer_addr.ip()).count();

                        //CHECK FOR MAXIMAL CONNECTIONS
                        if auth_clients >= config::read_config::<usize>("max_clients") ||
                            unauth_clients >= config::read_config::<usize>("max_unauth_clients") ||
                            ip_clients >= config::read_config::<usize>("max_ip_clients")
                        {
                            log::error!("Connection rejected (limit): {peer_addr}");
                            stream.shutdown(Shutdown::Both).ok();
                            continue;
                        }

                        let write_stream = Arc::new(Mutex::new(stream.try_clone().expect("Failed cloning stream")));
                        thread::spawn(move || server::listen_client(&mut (&mut stream, write_stream), peer_addr, token));
                        continue;
                    }
                }

                log::error!("Connection rejected (header): {peer_addr}");
                stream.shutdown(Shutdown::Both).ok();
            },

            Err(e) =>
            {
                log::error!("Connection failed: {}", e);
            }
        }
    }
}
