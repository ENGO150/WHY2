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

use ratatui::text::{ Line, Span };

use crate::
{
    options,
    network::client::ClientEvent,
};

use super::
{
    theme,
    state::App,
    tofu::Prompt,
    login::Stage,
};

//IMPLEMENTATIONS
impl App
{
    //TRANSLATES ONE SERVER/CLIENT EVENT INTO STATE. NOTHING HERE PRINTS OR TOUCHES THE TERMINAL.
    pub fn apply(&mut self, event: ClientEvent)
    {
        match event
        {
            //THE PROMPTS THEMSELVES LIVE IN THE CONNECT BOX - NOTHING GOES INTO THE HISTORY
            //NOTHING IS PUSHED HERE, SO THE REDRAW HAS TO BE ASKED FOR - THE TICK ONLY DRAWS WHEN DIRTY
            ClientEvent::Register =>
            {
                if let Some(login) = self.login.as_mut() { login.ask(Stage::Password { register: true }, None); }

                self.dirty = true;
            },

            ClientEvent::Login =>
            {
                if let Some(login) = self.login.as_mut() { login.ask(Stage::Password { register: false }, None); }

                self.dirty = true;
            },

            ClientEvent::FirstUser =>
            {
                self.push_styled("You are the first user to register, owner role has been granted to you.", theme::NOTICE);
            },

            ClientEvent::Authenticated(role) =>
            {
                self.login = None; //THE BOX HAS ASKED FOR EVERYTHING IT WAS GOING TO ASK FOR
                self.role = role;
                self.push_styled("Login successful. Press Ctrl+H for help.", theme::OK);
                self.refresh_online = true;
            },

            ClientEvent::Connected(server_name) =>
            {
                self.push_styled(format!("Successfully connected to {server_name}."), theme::OK);
                self.server_name = server_name;
            },

            //STORED UNRENDERED - App::theme TURNS IT INTO A LINE, AGAIN AFTER EVERY THEME CHANGE
            ClientEvent::Message(message, username, id, colors) => self.push_message(username, id, message, colors),

            ClientEvent::PrivateMessageSent(to, id, msg) =>
            {
                self.push(Line::from(vec!
                [
                    Span::styled("[PM TO] ", theme::ACCENT),
                    Span::raw(format!("{to} ({id}): {msg}")),
                ]));
            },

            ClientEvent::PrivateMessageRecv(from, id, msg) =>
            {
                self.push(Line::from(vec!
                [
                    Span::styled("[PM FROM] ", theme::ACCENT),
                    Span::raw(format!("{from} ({id}): {msg}")),
                ]));
            },

            ClientEvent::TofuPrompt(request) =>
            {
                self.tofu = Some(Prompt::new(request));
                self.dirty = true;
            },

            //REFUSING (OR FAILING) THE CHECK JUST ENDS THE SESSION - THE PROMPT ALREADY SAID WHY
            ClientEvent::TofuError => self.quit(1, None),

            //THE SERVER WENT AWAY BETWEEN THE TWO CONNECTIONS - BACK TO THE ADDRESS, THE KEY IS PINNED NOW
            ClientEvent::ReconnectFailed => self.disconnected("Reconnecting to the server failed."),

            ClientEvent::TofuSkip(hash) =>
            {
                self.push_styled("SECURITY WARNING: UNKNOWN SERVER IDENTITY", theme::ERROR);
                self.push_styled("The server's identity key cannot be verified due to disabled ToFU \
                    verification. If you don't recognize the identity key below, disconnect immediately!", theme::NOTICE);
                self.push_styled(hash, theme::NOTICE);
            },

            ClientEvent::VoiceActivity(users) =>
            {
                self.voice = users;
                self.dirty = true;
            },

            ClientEvent::Join(uname) =>
            {
                self.push(Line::from(vec!
                [
                    Span::styled(format!("[{}] ", options::get_server_username()), theme::DIM),
                    Span::styled(format!("{uname} connected."), theme::OK),
                ]));

                self.refresh_online = true;
            },

            ClientEvent::Leave(uname) =>
            {
                self.push(Line::from(vec!
                [
                    Span::styled(format!("[{}] ", options::get_server_username()), theme::DIM),
                    Span::styled(format!("{uname} disconnected."), theme::DIM),
                ]));

                self.refresh_online = true;
            },

            ClientEvent::InvalidUsage =>
            {
                self.push_styled("Invalid usage! Press Ctrl+H for help.", theme::ERROR);
            },

            ClientEvent::UnsafeVersion(newer_versions, current_version, newest_version) =>
            {
                self.push_styled(format!("This release could be unsafe! You are {newer_versions} \
                    versions behind! ({current_version}/{newest_version})"), theme::NOTICE);
            },

            ClientEvent::Username(disabled_registration, min_uname, max_uname) =>
            {
                let hint = if disabled_registration
                {
                    String::from("Registration is disabled.")
                } else
                {
                    format!("a-Z, 0-9; {min_uname}-{max_uname} characters")
                };

                if let Some(login) = self.login.as_mut() { login.ask(Stage::Username, Some(hint)); }

                self.dirty = true;
            },

            ClientEvent::VoiceEnabled =>
            {
                self.voice_enabled = true;
                self.push_styled("Voice enabled.", theme::OK);
            },

            ClientEvent::VoiceDeviceFailed =>
            {
                self.push_styled("Switching the audio device failed - the previous one is still in use.", theme::ERROR);

                //THE VOICE CLIENT POINTED THE CONFIG BACK AT THE DEVICE THAT IS ACTUALLY PLAYING
                #[cfg(feature = "client_voice")]
                self.settings.refresh_devices();
            },

            ClientEvent::VoiceHandshakeFailed =>
            {
                self.push_styled("The server never answered the voice handshake - is UDP getting through?", theme::ERROR);
            },

            ClientEvent::VoiceDisabled =>
            {
                self.voice_enabled = false;
                self.voice.clear();
                self.push_styled("Voice disabled.", theme::DIM);
            },

            //server.toml CAME BACK - EITHER THE COPY WE ASKED FOR, OR THE ONE THE SERVER JUST STORED
            ClientEvent::ServerSettings(settings, saved) =>
            {
                match saved
                {
                    //THE ANSWER TO A SAVE IS THE CONFIG AS IT ACTUALLY STANDS, SO A REFUSED ROW SNAPS BACK
                    true =>
                    {
                        if self.settings.open && self.settings.server { self.settings.stored(settings); }

                        self.push_styled("Server settings saved.", theme::OK);
                    },

                    false => self.settings.open_server(settings),
                }

                self.dirty = true;
            },

            ClientEvent::List(users) =>
            {
                //ALWAYS REFRESH THE SIDEBAR; ONLY ECHO A BLOCK WHEN THE USER ASKED FOR ONE
                self.online = users;

                //AUTHORITATIVE: A CHANNEL EXISTS EXACTLY AS LONG AS SOMEBODY IS IN IT
                self.channels = self.online.iter().filter_map(|user| user.channel.clone()).collect();

                if self.list_requested
                {
                    self.list_requested = false;

                    let here = options::get_channel();
                    let width = id_width(self.online.iter().map(|user| user.id));
                    let last = self.online.len().saturating_sub(1);

                    self.push_styled(format!("Online clients ({}):", self.online.len()), theme::TITLE);

                    let rows = self.online.iter().enumerate().map(|(index, user)|
                    {
                        let mut spans = vec![Span::styled(super::branch(index == last), theme::BORDER)];

                        spans.extend(id_column(user.id, width));
                        spans.push(Span::raw(user.username.clone()));

                        //OUR OWN CHANNEL IS ACCENTED SO THE ROSTER SPLITS AT A GLANCE
                        if let Some(channel) = user.channel.clone()
                        {
                            let style = if channel == here { theme::ACCENT } else { theme::DIM };
                            spans.push(Span::styled(format!("  #{channel}"), style));
                        }

                        Line::from(spans)
                    }).collect::<Vec<Line<'static>>>();

                    for row in rows { self.push(row); }
                }

                self.dirty = true;
            },

            ClientEvent::Upload(filename) =>
            {
                self.push_text(format!("Uploading file \"{filename}\"..."));
            },

            ClientEvent::Uploaded(username, filename) =>
            {
                self.push(Line::from(vec!
                [
                    Span::styled(format!("[{}] ", options::get_server_username()), theme::DIM),
                    Span::raw(format!("{username} uploaded file \"{filename}\".")),
                ]));
            },

            ClientEvent::Download(filename) =>
            {
                self.push_text(format!("Downloading file \"{filename}\"..."));
            },

            ClientEvent::Downloaded(filename) =>
            {
                self.push_styled(format!("File \"{filename}\" downloaded."), theme::OK);
            },

            ClientEvent::DownloadFailed(filename) =>
            {
                self.push_styled(format!("Downloading \"{filename}\" failed."), theme::ERROR);
            },

            ClientEvent::Files(users) =>
            {
                if users.is_empty()
                {
                    self.push_styled("No available files.", theme::DIM);
                } else
                {
                    self.push_styled(format!("Available files ({}):", users.len()), theme::TITLE);

                    //THE OWNER IS THE BRANCH, THEIR FILES HANG OFF IT - THE TWO IDS SIDE BY SIDE
                    //ARE THE TWO ARGUMENTS TO /download
                    let width = id_width(users.iter().map(|user| user.id));
                    let last = users.len() - 1;

                    for (index, user) in users.into_iter().enumerate()
                    {
                        let mut spans = vec![Span::styled(super::branch(index == last), theme::BORDER)];

                        spans.extend(id_column(user.id, width));
                        spans.push(Span::raw(user.username.clone()));

                        self.push(Line::from(spans));

                        //THE TRUNK KEEPS RUNNING PAST THE FILES UNLESS THIS IS THE LAST OWNER
                        let trunk = format!("{}  ", if index == last { " " } else { "│" });
                        let file_width = id_width(user.upload.iter().map(|(_, id)| *id));
                        let last_file = user.upload.len().saturating_sub(1);

                        for (file, (filename, file_id)) in user.upload.into_iter().enumerate()
                        {
                            let mut spans = vec![Span::styled(format!("{trunk}{}", super::branch(file == last_file)), theme::BORDER)];

                            spans.extend(id_column(file_id, file_width));
                            spans.push(Span::raw(filename));

                            self.push(Line::from(spans));
                        }
                    }
                }
            },

            ClientEvent::Screens(users) =>
            {
                #[cfg(feature = "client_screen")]
                { self.screens_requested = false; }

                if users.is_empty()
                {
                    self.push_styled("No available screenshares.", theme::DIM);
                } else
                {
                    self.push_styled(format!("Screensharing clients ({}):", users.len()), theme::TITLE);

                    let width = id_width(users.iter().map(|user| user.id));
                    let last = users.len() - 1;

                    for (index, user) in users.into_iter().enumerate()
                    {
                        let mut spans = vec![Span::styled(super::branch(index == last), theme::BORDER)];

                        spans.extend(id_column(user.id, width));
                        spans.push(Span::raw(user.username));

                        self.push(Line::from(spans));
                    }
                }
            },

            ClientEvent::UploadLimit =>
            {
                self.push_styled("Maximum concurrent uploads reached!", theme::ERROR);
            },

            ClientEvent::Screen(enabled) =>
            {
                self.push_styled(format!("{} screen sharing.", if enabled { "Started" } else { "Stopped" }), theme::OK);
            },

            ClientEvent::ScreenFailed(reason) =>
            {
                self.push_styled(format!("Screen sharing failed: {reason}."), theme::ERROR);
            },

            ClientEvent::Attach(username) =>
            {
                self.push_text(format!("Attached {username}'s screen sharing."));
            },

            ClientEvent::Deattach(username) =>
            {
                self.push_text(format!("Deattached {username}'s screen sharing."));
            },

            ClientEvent::IncompatibleVersion(version, server_version) =>
            {
                self.push_styled(format!("Incompatible version! ({version}/{server_version})"), theme::ERROR);
            },

            ClientEvent::VersionMismatch(client_version, server_version) =>
            {
                self.push_styled(format!("Version mismatch - some features may not work \
                    ({client_version}/{server_version})"), theme::NOTICE);
            },

            //A REJECTION IS ALWAYS FOLLOWED BY THE RE-PROMPT, AND Login::ask KEEPS THE ERROR ON SCREEN
            ClientEvent::UsernameRejected =>
            {
                match self.login.as_mut()
                {
                    Some(login) => login.error = Some(String::from("Username rejected!")),
                    None => self.push_styled("Username rejected!", theme::ERROR),
                }

                self.dirty = true;
            },

            ClientEvent::PasswordRejected(min_pass) =>
            {
                let message = format!("Password rejected! Enter at least {min_pass} characters.");

                match self.login.as_mut()
                {
                    Some(login) => login.error = Some(message),
                    None => self.push_styled(message, theme::ERROR),
                }

                self.dirty = true;
            },

            ClientEvent::SpamWarning =>
            {
                self.push_styled("Slow down! You're sending messages too quickly.", theme::NOTICE);
            },

            ClientEvent::Socks5Voice =>
            {
                self.push_styled("Voice chat cannot be enabled while using SOCKS5.", theme::ERROR);
            },

            ClientEvent::DisabledFeature =>
            {
                self.push_styled("Server has disabled the feature you requested.", theme::ERROR);
            },

            ClientEvent::VersionFailed =>
            {
                self.push_styled("Fetching versions failed, this release could be unsafe!", theme::NOTICE);
            },

            //THE SOCKET IS GONE, BUT THE CLIENT IS NOT: THE CONNECT BOX COMES BACK SO ANOTHER SERVER (OR THE
            //SAME ONE AGAIN) IS ONE ENTER AWAY. ONLY A DISCONNECT THE USER ASKED FOR ENDS THE PROCESS.
            ClientEvent::Quit =>
            {
                if self.leaving
                {
                    self.quit(0, Some(String::from("Disconnected from the server.")));
                } else if self.logging_out
                {
                    self.disconnected("Logged out.");
                } else { self.disconnected("Server quit communication."); }
            },

            //SIDEBAR-ONLY - THE CHANNEL LIST TRACKS THESE, THE HISTORY DOES NOT.
            //NONE OF THEM ASKS THE SERVER FOR ANYTHING: A PacketCode::List HERE WOULD FOLLOW THE /channel
            //THAT CAUSED IT INSIDE min_message_delay AND EARN A SPAM WARNING. THE SERVER BROADCASTS
            //ChannelCreated/ChannelDestroyed TO EVERYONE, WHICH IS ALREADY THE WHOLE TRUTH ABOUT WHICH
            //CHANNELS EXIST - A CHANNEL LIVES EXACTLY AS LONG AS SOMEBODY SITS IN IT.
            ClientEvent::ChannelChanged(channel) =>
            {
                self.clear_messages();

                if let Some(name) = channel.clone() { self.channels.insert(name); }

                //KEEP OUR OWN ROW HONEST UNTIL THE NEXT LIST REFRESHES EVERYBODY ELSE'S
                let me = self.username.clone();

                if let Some(user) = self.online.iter_mut().find(|user| user.username == me)
                {
                    user.channel = channel;
                }

                self.dirty = true;
            },

            ClientEvent::ChannelCreated(name) =>
            {
                self.channels.insert(name);
                self.dirty = true;
            },

            ClientEvent::ChannelDestroyed(name) =>
            {
                self.channels.remove(&name);
                self.dirty = true;
            },
        }
    }

    pub fn quit(&mut self, code: i32, message: Option<String>)
    {
        self.should_quit = true;
        self.exit_code = code;
        self.quit_message = message;
        self.dirty = true;
    }
}

//PRIVATE
//EVERY LIST BLOCK IS A TREE: ONE BRANCH PER ROW, THEN A RIGHT-ALIGNED ID COLUMN, THEN THE NAME
fn id_width(ids: impl Iterator<Item = usize>) -> usize
{
    ids.map(|id| id.to_string().len()).max().unwrap_or(1)
}

fn id_column(id: usize, width: usize) -> Vec<Span<'static>>
{
    vec![Span::styled(format!("{id:>width$}  "), theme::DIM)]
}
