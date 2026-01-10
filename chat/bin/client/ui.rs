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

use std::
{
    env,
    process,
    io::{ self, Write },
};

use colored::Colorize;

use why2::chat::
{
    config::{ self, TofuCode },
    network::
    {
        SerColor,
        client::ClientEvent,
    },
};

//PRIVATE
fn clear_lines(n: usize) //CLEARS n LINES (ALSO MOVES THE CURSOR n LINES UP)
{
    for i in 0..n
    {
        //CLEAR CURRENT LINE
        print!("\x1B[2K\r");

        //MOVE UP
        if i < n - 1
        {
            print!("\x1B[1A");
        }
    }
}

fn colorize(text: String, color: Option<SerColor>) -> String //COLORIZE text IF PASSED COLOR
{
    match color
    {
        Some(c) if !config::client_config::<bool>("disable_colors") => text.color(c.0).to_string(),
        _ => text
    }
}

//PUBLIC
pub fn draw_event(event: ClientEvent)
{
    match event
    {
        ClientEvent::Message(message) => //MESSAGE RECEIVED
        {
            clear_lines(2);

            println!
            (
                "{}{}: {}\n",

                colorize(message.username.unwrap(), message.colors.username_color),                                   //USERNAME
                if config::client_config("show_id") { format!(" ({})", message.id.unwrap()) } else { String::new() }, //ID
                colorize(message.text.unwrap(), message.colors.message_color)                                         //MESSAGE
            );
        },

        ClientEvent::Prompt(channel, message) => //SHOW PROMPT BAR
        {
            print!("\r{}>>> {}", channel, message);
            io::stdout().flush().unwrap();
        },

        ClientEvent::TofuError(status) =>
        {
            match status
            {
                TofuCode::Mismatch => //SOMETHING FUNNY HAPPENING
                {
                    println!
                    (
                        "\n\rSECURITY WARNING: SERVER IDENTITY MISMATCH
                        \n\rThe server's identity key is different from the
                        \rkey stored in local configuration. This could
                        \rmean that someone is intercepting your connection
                        \r(Man-in-the-Middle attack) or that the server
                        \rkey has been changed.
                        \n\rConnection aborted to protect your privacy."
                    );
                },

                TofuCode::Unknown(hash, ip) => //NEW ONE
                {
                    println!
                    (
                        "\n\rSECURITY WARNING: UNKNOWN SERVER IDENTITY
                        \n\rThe server's identity key is not stored in local
                        \rconfiguration. If you are sure that the key below
                        \ris valid, enter following command and connect again.
                        \n\r{} --verify {ip} {hash}",

                        env::args().nth(0).unwrap()
                    );
                },

                _ => panic!("what") //what
            }

            process::exit(1);
        }
    }
}
