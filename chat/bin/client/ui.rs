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

        ClientEvent::Info(message, newline, lines) =>
        {
            clear_lines(lines);
            print!("{message}");

            if newline { println!() }
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
            stdout.flush().unwrap();
        }

        ClientEvent::Clear(n) => clear_lines(n),
        ClientEvent::ExtraSpace => println!(),
        ClientEvent::Quit =>
        {
            terminal::disable_raw_mode().unwrap();
            println!("\nServer quit communication.");
            process::exit(0);
        },
    }
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
