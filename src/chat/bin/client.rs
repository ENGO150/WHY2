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
    process,
    io::{ self, Write },
    net::TcpStream,
};

use why2::
{
    core::misc,
    chat::
    {
        config,
        crypto,
        options,

        network::
        {
            self,
            MessagePacket,
            clear_lines,
        },
    },
};

fn main()
{
    misc::check_version(); //CHECK FOR UPDATES
    config::init_client_config(); //CREATE client.toml CONFIGURATION
    crypto::init_keys(); //GENERATE ECC KEYS
    options::set_core_options(); //SET ENCRYPTION OPTIONS

    println!("Welcome.\n");

    //GET CONNECTING IP
    let mut connecting_ip = if config::client_config("auto_connect") == "true" //USER ENABLED AUTOMATIC CONNECTION
    {
        let ip = config::client_config("auto_connect_ip"); //USE CONFIG IP

        //PRINT OUT IP
        println!(">>> {ip}");
        io::stdout().flush().unwrap();

        ip
    } else //NO AUTO CONNECT
    {
        print!("Enter IP Address:\n>>> ");
        io::stdout().flush().unwrap();

        //GET IP FROM USER INPUT
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        input.trim().to_owned()
    };

    //SPACER ("=") TO BE ADDED
    let mut spacer_add_spaces = 4;

    //ADD PORT TO IP IF MISSING
    if !connecting_ip.contains(':')
    {
        //APPEND DEFAULT PORT TO connecting_ip
        connecting_ip.push_str(&format!(":{}", config::client_config("default_port")));
    } else
    {
        spacer_add_spaces += connecting_ip.len() - connecting_ip.find(":").unwrap();
    }

    //PRINT SPACER
    println!("{}", "=".repeat(connecting_ip.find(":").unwrap() + spacer_add_spaces));

    //CONNECT TO SERVER
    let mut stream = TcpStream::connect(connecting_ip).unwrap_or_else(|_|
    {
        eprintln!("\nConnecting failed.");
        process::exit(1);
    });

    //CLONE SOCKET FOR CLIENT INPUT
    let mut client_stream = stream.try_clone().expect("Failed cloning stream");

    //LISTEN TO SERVER
    thread::spawn(move || network::listen_server(&mut stream));

    //LOOP FOR CLIENT-SIDE USER INPUT
    loop
    {
        //READ INPUT
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        input = input.trim().to_owned(); //TRIM

        //USER ENTERED USERNAME - STORE
        if options::get_asking_username()
        {
            options::set_username(input.clone()); //STORE USERNAME
            options::set_asking_username(false); //DISABLE ASKING_USERNAME
        }

        //USER ENTERED PASSWORD - HASH
        if options::get_asking_password()
        {
            //CHECK LENGTH
            if input.len() <= options::MIN_PASSWORD_LEN
            {
                print!("\x1B[2FPassword too short! Enter at least {} characters.\x1B[3E", options::MIN_PASSWORD_LEN);
                clear_lines(1);
                print!(">>> ");

                io::stdout().flush().unwrap();
                continue;
            }

            //HASH
            input = crypto::sha256(&input);
            options::set_asking_password(false); //ENABLE ECHO
        }

        //SEND input TO SERVER
        network::send(&mut client_stream, MessagePacket
        {
            text: Some(input),
            username: None,
            code: None,
        }, options::get_shared_key().as_deref());
    }
}
