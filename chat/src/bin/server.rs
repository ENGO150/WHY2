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
    process,
    sync::Arc,
    time::Duration,
};

use tokio::
{
    io::Result,
    time,
    signal,
    sync::Mutex,
    io::AsyncReadExt,
    net::{ TcpListener, UdpSocket },
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
        file::server as file,
        screen::server as screen,
        voice::server as voice_server,
        server::{ self, ConnectionType },
    },
};

//CONSTS
const BIND_ATTEMPTS: usize = 15;   //~3 SECONDS OF THEM
const BIND_RETRY_DELAY: u64 = 200; //MS BETWEEN THEM

//FUNCTIONS
//THE PORT IS WAITED FOR RATHER THAN GIVEN UP ON AT THE FIRST TRY: A RESTART ON A PLATFORM WITHOUT exec
//STARTS THE REPLACEMENT BESIDE THE OLD PROCESS, WHICH MAY STILL BE HOLDING IT FOR A MOMENT
async fn bind<T>(what: &str, address: &str, bind: impl AsyncFn() -> Result<T>) -> T
{
    let mut last = None;

    for attempt in 0..BIND_ATTEMPTS
    {
        match bind().await
        {
            Ok(bound) => return bound,

            Err(error) =>
            {
                last = Some(error);

                if attempt + 1 < BIND_ATTEMPTS { time::sleep(Duration::from_millis(BIND_RETRY_DELAY)).await; }
            }
        }
    }

    log::error!("Binding {what} on {address} failed: {}", last.unwrap());
    process::exit(1);
}

async fn quit() //DISCONNECT ALL USERS
{
    log::info!("Exiting...");
    server::disconnect_all().await; //DISCONNECT ALL USERS
}

#[tokio::main]
async fn main()
{
    //INIT LOGGER
    SimpleLogger::new()
        .with_level(LevelFilter::Info)
        .with_module_level("ureq", LevelFilter::Warn) //DISABLE UREQ INFO LOGS
        .with_module_level("rustls", LevelFilter::Warn) //DISABLE RUSTLS INFO LOGS
        .init()
        .unwrap();

    //CONFIGURATION
    misc::check_version().await; //CHECK WHY2 VERSION
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
    let listener = bind("TCP", &address, async || TcpListener::bind(&address).await).await;

    //BIND UDP (VOICE)
    let udp_socket = match options::voice_chat_enabled()
    {
        true => Some(bind("UDP", &address, async || UdpSocket::bind(&address).await).await), //VOICE ENABLED
        false => None,                                                                       //VOICE DISABLED
    };

    log::info!("Listening on {address}"); //PRINT INFO

    //CREATE KEEPALIVE & INACTIVITY WATCHDOG TASK
    tokio::spawn(async move
    {
        let mut n = 0;

        loop
        {
            time::sleep(Duration::from_secs(5)).await;

            //DISCONNECT INACTIVE CLIENTS
            server::disconnect_inactive().await;

            //REMOVE OLD PENDING TOKENS
            server::PENDING_TOKENS.retain(|_, (_, _, created)| created.elapsed().as_secs() < 5);

            //SEND KEEPALIVE PACKET TO ALL CLIENTS
            n = (n + 1) % 6;
            if n == 0
            {
                server::send_keepalive().await;
            }
        }
    });

    //SET Ctrl+C HANDLER
    tokio::spawn(async
    {
        signal::ctrl_c().await.expect("Setting Ctrl+C handler failed");

        //DISCONNECT ALL USERS AND EXIT
        quit().await;
        process::exit(0);
    });

    //CREATE TASK FOR VOICE
    if options::voice_chat_enabled()
    {
        tokio::spawn(voice_server::listen_client_voice(udp_socket.unwrap()));
    }

    //ACCEPT CLIENTS
    loop
    {
        match listener.accept().await
        {
            Ok((mut stream, peer_addr)) =>
            {
                //TAKE A HANDSHAKE SLOT - AN UNIDENTIFIED SOCKET COUNTS AGAINST NO OTHER LIMIT
                let slot = match server::HandshakeSlot::reserve(peer_addr.ip())
                {
                    Some(s) => s,
                    None =>
                    {
                        log::error!("Connection rejected (handshake limit): {peer_addr}");
                        continue;
                    }
                };

                //READ THE HEADER IN THE CONNECTION'S OWN TASK, NEVER HERE: A PEER THAT CONNECTS AND SAYS
                //NOTHING WOULD OTHERWISE HOLD THE ACCEPT LOOP FOR THE WHOLE TIMEOUT, ONE SOCKET AT A TIME
                tokio::spawn(async move
                {
                    let _slot = slot; //RELEASED HOWEVER THE HANDSHAKE ENDS

                    //READ TOKEN (WITH TIMEOUT FOR ZOMBIE CONNECTIONS)
                    let mut token = [0u8; 32];
                    if let Ok(Ok(_)) = time::timeout(Duration::from_millis(2000), stream.read_exact(&mut token)).await
                    {
                        //SET TCP_NODELAY
                        match stream.set_nodelay(true)
                        {
                            Ok(_) => {},
                            Err(_) => return
                        }

                        if let Some((_, (id, conn_type, _))) = server::PENDING_TOKENS.remove(&token)
                        {
                            match conn_type
                            {
                                ConnectionType::FileUpload { uid } =>
                                {
                                    server::spawn_with_abort(move |task| async move
                                    {
                                        let (mut read_stream, write_stream) = stream.into_split();
                                        file::download(token, id, &mut (&mut read_stream, Arc::new(Mutex::new(write_stream))), uid, task).await;
                                    });
                                    return;
                                },

                                ConnectionType::FileDownload { uid, file: file_data } =>
                                {
                                    server::spawn_with_abort(move |task| async move
                                    {
                                        let (_read_stream, write_stream) = stream.into_split();
                                        file::upload(token, id, write_stream, file_data, uid, task).await;
                                    });
                                    return;
                                },

                                ConnectionType::Screen =>
                                {
                                    server::spawn_with_abort(move |task| async move
                                    {
                                        let (mut read_stream, write_stream) = stream.into_split();
                                        screen::screen(token, id, &mut (&mut read_stream, Arc::new(Mutex::new(write_stream))), task).await;
                                    });
                                    return;
                                },

                                ConnectionType::Attach { id: sharer_id } =>
                                {
                                    //ONLY THE WRITE HALF IS EVER USED FOR AN ATTACHED VIEWER
                                    let (_read_stream, write_stream) = stream.into_split();

                                    if let Some(mut conn) = server::CONNECTIONS.iter_mut().find(|c| c.id() == Some(&id))
                                    {
                                        conn.attach_screen(sharer_id, Arc::new(Mutex::new(write_stream)), token);
                                    }

                                    return;
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
                                return;
                            }

                            server::spawn_with_abort(move |task| async move
                            {
                                let (mut read_stream, write_stream) = stream.into_split();
                                server::listen_client(&mut (&mut read_stream, Arc::new(Mutex::new(write_stream))), peer_addr, token, task).await;
                            });
                            return;
                        }
                    }

                    log::error!("Connection rejected (header): {peer_addr}");
                });
            },

            Err(e) =>
            {
                log::error!("Connection failed: {}", e);
            }
        }
    }
}
