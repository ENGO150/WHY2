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
use crossterm::
{
    terminal,
    QueueableCommand,
    style::
    {
        Print,
        SetForegroundColor,
        Color,
        ResetColor,
        SetAttribute,
        Attribute
    },
    cursor::
    {
        MoveTo,
        SavePosition,
        RestorePosition,
    },
};

use crate::
{
    colors,
    options,
    config::{ self, TofuCode },
    network::client::ClientEvent,
};

//PRIVATE
fn colorize(text: String, color: Option<u8>) -> String //COLORIZE text IF PASSED COLOR
{
    match color.and_then(|c| colors::u8_to_color(c))
    {
        Some(c) if !config::read_config::<bool>("disable_colors") => text.color(c).to_string(),
        _ => text
    }
}

//PUBLIC
pub fn draw_event(event: ClientEvent)
{
    match event
    {
        ClientEvent::Register =>
        {
            println!("\n\rEnter password: (REGISTER)");
        },

        ClientEvent::Login =>
        {
            clear_lines(3);
            println!("\nEnter password: (LOGIN)");
        },

        ClientEvent::Authenticated =>
        {
            clear_lines(3);
            println!("Login successful. Press Ctrl+H for help.\n");
        },

        ClientEvent::Connected(server_name) =>
        {
            println!("Successfully connected to {server_name}.\n");
        }

        ClientEvent::Message(message) => //MESSAGE RECEIVED
        {
            clear_lines(2);

            println!
            (
                "{}{}: {}\n",

                colorize(message.username.unwrap(), message.colors.username_color),                                 //USERNAME
                if config::read_config("show_id") { format!(" ({})", message.id.unwrap()) } else { String::new() }, //ID
                colorize(message.text.unwrap(), message.colors.message_color)                                       //MESSAGE
            );
        },

        ClientEvent::Prompt => //SHOW PROMPT BAR
        {
            let channel = match options::get_channel()
            {
                c if c.is_empty() => String::new(),
                c => format!("#{c} | "),
            };

            print!("\r{channel}>>> {}", options::INPUT_READ.lock().unwrap().iter().collect::<String>());
        },

        ClientEvent::PrivateMessageSent(to, id, msg) =>
        {
            clear_lines(2);
            println!("[PM TO] {to} ({id}): {msg}\n");
        },

        ClientEvent::PrivateMessageRecv(from, id, msg) =>
        {
            clear_lines(2);
            println!("[PM FROM] {from} ({id}): {msg}\n");
        },

        ClientEvent::TofuError(status) =>
        {
            //DISABLE RAW MODE
            terminal::disable_raw_mode().unwrap();

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
        },

        ClientEvent::VoiceActivity(users) =>
        {
            //PREPARE TERMINAL
            let mut stdout = io::stdout();
            let (cols, rows) = terminal::size().unwrap_or((80, 24));

            stdout.queue(SavePosition).unwrap();

            let overlay_width = 25;
            let bottom_row = rows.saturating_sub(2);
            let available_height = rows.saturating_sub(4) as usize;
            let limit = available_height.min(15);

            let header_text = "VOICE CHANNEL:"; //HEADER
            let mut max_content_width = header_text.len();

            //FIND WIDEST LINE
            for user in users.iter().take(limit)
            {
                let mut width = user.username.chars().count() + 3;

                //ALSO ADD LATENCY TO WIDTH (IF LATENCY SHOWN)
                if !user.is_local
                {
                    width += user.latency.to_string().len() + 4;
                }

                if width > max_content_width
                {
                    max_content_width = width;
                }
            }

            let clear_width = overlay_width.max(max_content_width);
            let align_x = cols.saturating_sub(max_content_width as u16).saturating_sub(1);

            //CLEAR WINDOW
            for i in 0..=limit
            {
                let y = bottom_row.saturating_sub(i as u16);
                let x = cols.saturating_sub(clear_width as u16);
                stdout.queue(MoveTo(x, y)).unwrap();
                stdout.queue(Print(" ".repeat(clear_width as usize))).unwrap();
            }

            //PRINT
            for (i, user) in users.iter().take(limit).rev().enumerate()
            {
                let y = bottom_row.saturating_sub(i as u16);
                let text = if user.is_local
                {
                    format!("- {} ", user.username)
                } else
                {
                    format!("- {} [{}ms]", user.username, user.latency)
                };

                stdout.queue(MoveTo(align_x, y)).unwrap();

                if user.is_speaking
                {
                    //ACTIVE
                    stdout.queue(SetForegroundColor(Color::Green)).unwrap();
                    stdout.queue(SetAttribute(Attribute::Bold)).unwrap();
                    stdout.queue(Print(text)).unwrap();
                    stdout.queue(SetAttribute(Attribute::Reset)).unwrap();
                    stdout.queue(ResetColor).unwrap();
                } else
                {
                    //INACTIVE
                    stdout.queue(SetForegroundColor(Color::DarkGrey)).unwrap();
                    stdout.queue(Print(text)).unwrap();
                    stdout.queue(ResetColor).unwrap();
                }
            }

            //HEADER PRINT
            if !users.is_empty()
            {
                let count = users.len().min(limit);
                let y = bottom_row.saturating_sub(count as u16);

                stdout.queue(MoveTo(align_x, y)).unwrap();
                stdout.queue(SetAttribute(Attribute::Underlined)).unwrap();
                stdout.queue(Print(header_text)).unwrap();
                stdout.queue(SetAttribute(Attribute::Reset)).unwrap();
            }

            stdout.queue(RestorePosition).unwrap();
        },

        ClientEvent::Join(uname) =>
        {
            println!("[{}]: {uname} connected.\n", options::get_server_username());
        },

        ClientEvent::Leave(uname) =>
        {
            clear_lines(2);
            println!("[{}]: {uname} disconnected.\n", options::get_server_username());
        },

        ClientEvent::InvalidUsage =>
        {
            clear_lines(2);
            println!("Invalid usage! Press Ctrl+H for help.\n");
        },

        ClientEvent::UnsafeVersion(newer_versions, current_version, newest_version) =>
        {
            println!("This release could be unsafe! You are {newer_versions} versions behind! ({current_version}/{newest_version})");
        },

        ClientEvent::Username(disabled_registration, min_uname, max_uname) =>
        {
            println!
            (
                "\n\rEnter username ({}):",

                if disabled_registration
                {
                    String::from("Registration disabled!")
                } else
                {
                    format!("a-Z, 0-9; {min_uname}-{max_uname} characters")
                }
            );
        },

        ClientEvent::VoiceEnabled =>
        {
            clear_lines(2);
            println!("Voice enabled.\n");
        },

        ClientEvent::VoiceDisabled =>
        {
            clear_lines(2);
            println!("Voice disabled.\n");
        },

        ClientEvent::List(users_json) =>
        {
            clear_lines(2);

            println!("Online clients:");

            //PRINT USERS
            for user in users_json.as_array().unwrap()
            {
                //GET CHANNEL
                let c = if let Some(c) = user["channel"].as_str().map(String::from)
                {
                    format!(" | #{c}")
                } else
                {
                    String::new()
                };

                println!("\r{} ({}){}", user["username"].as_str().unwrap(), user["id"], c);
            }

            println!();
        },

        ClientEvent::Upload(filename) =>
        {
            clear_lines(1);
            println!("Uploading file \"{filename}\"...\n");
        },

        ClientEvent::Uploaded(username, filename) =>
        {
            clear_lines(2);
            println!("[{}]: {username} uploaded file \"{filename}\".\n", options::get_server_username());
        },

        ClientEvent::Download(filename) =>
        {
            clear_lines(2);
            println!("Downloading file \"{filename}\"...\n");
        },

        ClientEvent::Downloaded(filename) =>
        {
            clear_lines(2);
            println!("File \"{filename}\" downloaded.\n");
        },

        ClientEvent::DownloadFailed(filename) =>
        {
            clear_lines(2);
            println!("Downloading \"{filename}\" failed.\n");
        },

        ClientEvent::Files(uploads_json) =>
        {
            clear_lines(2);
            if uploads_json.is_empty()
            {
                println!("No available files.\n");
            } else
            {
                if !options::get_extra_space() { println!(); }
                println!("Available files:");

                for user_obj in uploads_json
                {
                    let username = user_obj["username"].as_str().unwrap();
                    let id = user_obj["id"].as_u64().unwrap();

                    println!("\r{} ({}):", username, id);

                    //GET FILENAMES
                    for file in user_obj["uploads"].as_array().unwrap()
                    {
                        println!("\r - {} ({})", file[0], file[1]);
                    }
                }

                println!();
            }
        },

        ClientEvent::Screens(screens_json) =>
        {
            clear_lines(2);
            if screens_json.is_empty()
            {
                println!("No available screenshares.\n");
            } else
            {
                if !options::get_extra_space() { println!(); }
                println!("Screensharing clients:");

                for user_obj in screens_json
                {
                    let username = user_obj["username"].as_str().unwrap();
                    let id = user_obj["id"].as_u64().unwrap();

                    println!("\r - {} ({})", username, id);
                }

                println!();
            }
        },

        ClientEvent::UploadLimit =>
        {
            clear_lines(1);
            println!("Maximum concurrent uploads reached!\n");
        },

        ClientEvent::Screen(enabled) =>
        {
            clear_lines(2);
            println!("{} screen sharing.\n", if enabled { "Started" } else { "Stopped" });
        },

        ClientEvent::IncompatibleVersion(version, server_version) =>
        {
            clear_lines(1);
            println!("Incompatible version! ({version}/{server_version})");
        },

        ClientEvent::VersionMismatch(client_version, server_version) =>
        {
            clear_lines(1);
            println!("Version mismatch - some features may not work ({client_version}/{server_version})\r");
        },

        ClientEvent::UsernameRejected =>
        {
            clear_lines(2);
            print!("Username rejected!");
        },

        ClientEvent::PasswordRejected(min_pass) =>
        {
            clear_lines(1);
            print!("Password rejected! Enter at least {min_pass} characters.");
        },

        ClientEvent::SpamWarning =>
        {
            clear_lines(2);
            println!("Slow down! You're sending messages too quickly.\n");
        },

        ClientEvent::Socks5Voice =>
        {
            clear_lines(2);
            println!("Voice chat cannot be enabled while using SOCKS5.\n");
        },

        ClientEvent::DisabledFeature =>
        {
            clear_lines(2);
            println!("Server has disabled the feature you requested.\n");
        },

        ClientEvent::VersionFailed => println!("Fetching versions failed, this release could be unsafe!"),
        ClientEvent::Clear(n) => clear_lines(n),
        ClientEvent::ExtraSpace => println!(),
        ClientEvent::Quit =>
        {
            terminal::disable_raw_mode().unwrap();
            println!("\nServer quit communication.");
            process::exit(0);
        },
    }

    //FLUSH STDOUT
    io::stdout().flush().unwrap();
}

pub fn clear_lines(n: usize) //CLEARS n LINES (ALSO MOVES THE CURSOR n LINES UP)
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
