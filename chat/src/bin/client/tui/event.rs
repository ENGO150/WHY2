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
    //TRANSLATE ONE EVENT INTO STATE, NEVER DRAWING
    pub fn apply(&mut self, event: ClientEvent)
    {
        match event
        {
            //THE PROMPTS LIVE IN THE CONNECT BOX
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

            //STORED UNRENDERED - App::theme MAKES THE LINE
            ClientEvent::Message(message, username, id, colors) => self.push_message(username, id, message, colors),

            //A PICTURE IS AN ENTRY OF ITS OWN
            ClientEvent::ImageDisplay(username, filename, image, color) =>
                self.push_image(username, filename, image, color),

            //A PICTURE WE DO NOT HOLD: CAPTION IT NOW
            ClientEvent::ImagePending(username, filename, hash, color) =>
            {
                self.push_caption(username, filename, hash, true, color);
                self.image_requests.push(hash);
            },

            //THE SAME LINE WITH THE BUTTON ON IT
            ClientEvent::ImageOffer(username, filename, hash, color) =>
                self.push_caption(username, filename, hash, false, color),

            //A CLICK THE CACHE COULD NOT ANSWER
            ClientEvent::ImageRequest(hash) => self.image_requests.push(hash),

            ClientEvent::ImageFailed(username, filename, _) => self.push_styled(
                format!("{username} sent an image that could not be displayed ({filename})."), theme::ERROR),

            ClientEvent::PrivateMessageSent(to, id, msg) =>
            {
                self.push_prefixed(vec!
                [
                    Span::styled("[PM TO] ", theme::ACCENT),
                    Span::raw(format!("{to} ({id}): ")),
                ], msg);
            },

            ClientEvent::PrivateMessageRecv(from, id, msg) =>
            {
                self.push_prefixed(vec!
                [
                    Span::styled("[PM FROM] ", theme::ACCENT),
                    Span::raw(format!("{from} ({id}): ")),
                ], msg);
            },

            ClientEvent::TofuPrompt(request) =>
            {
                self.tofu = Some(Prompt::new(request));
                self.dirty = true;
            },

            //A REFUSED CHECK ENDS THE SESSION
            ClientEvent::TofuError => self.quit(1, None),

            //BACK TO THE ADDRESS, THE KEY IS PINNED NOW
            ClientEvent::ReconnectFailed => self.disconnected("Reconnecting to the server failed."),

            //THERE WAS NO PROMPT, SO THE REASON GOES BACK
            ClientEvent::HandshakeFailed(reason) => self.disconnected(reason),

            ClientEvent::TofuSkip(hash) =>
            {
                self.push_styled("SECURITY WARNING: UNKNOWN SERVER IDENTITY", theme::ERROR);
                self.push_styled("The server's identity key cannot be verified due to disabled ToFU \
                    verification. If you don't recognize the identity key below, disconnect immediately!", theme::NOTICE);
                self.push_styled(hash, theme::NOTICE);
            },

            ClientEvent::VoiceActivity(users) =>
            {
                self.voice_activity = users;
                self.rebuild_voice();
            },

            //THE WHOLE ROSTER - IT REPLACES WHAT WE HELD
            ClientEvent::VoiceRoster(clients) =>
            {
                self.voice_roster = clients.into_iter().collect();
                self.rebuild_voice();
            },

            ClientEvent::VoiceJoin(id, username) =>
            {
                self.voice_roster.insert(id, username);
                self.rebuild_voice();
            },

            ClientEvent::VoiceLeave(id) =>
            {
                self.voice_roster.remove(&id);
                self.rebuild_voice();
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

            ClientEvent::Leave(uname, id) =>
            {
                self.push(Line::from(vec!
                [
                    Span::styled(format!("[{}] ", options::get_server_username()), theme::DIM),
                    Span::styled(format!("{uname} disconnected."), theme::DIM),
                ]));

                //Leave NAMES THE USER, SO DROP THEM HERE
                self.online.retain(|user| user.id != id);

                //A DISCONNECT CARRIES NO VoiceLeave
                if self.voice_roster.remove(&id).is_some() { self.rebuild_voice(); }

                //A CHANNEL EXISTS WHILE SOMEBODY IS IN IT
                self.channels = self.online.iter().filter_map(|user| user.channel.clone()).collect();
                self.prune_panes();
            },

            ClientEvent::Muted =>
            {
                self.push_styled("You have been muted by moderator.", theme::NOTICE);
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
                self.rebuild_voice();
                self.push_styled("Voice enabled.", theme::OK);
            },

            ClientEvent::VoiceDeviceFailed =>
            {
                self.push_styled("Switching the audio device failed - the previous one is still in use.", theme::ERROR);

                //THE CONFIG POINTS AT THE DEVICE THAT PLAYS
                #[cfg(feature = "client_voice")]
                self.settings.refresh_devices();
            },

            ClientEvent::VoiceHandshakeFailed =>
            {
                self.push_styled("The server never answered the voice handshake - is UDP getting through?", theme::ERROR);
            },

            ClientEvent::VoiceDisabled =>
            {
                //ONLY OUR OWN HALF OF THE PANEL GOES
                self.voice_enabled = false;
                self.voice_activity.clear();
                self.rebuild_voice();
                self.push_styled("Voice disabled.", theme::DIM);
            },

            //SERVER MESSAGE
            ClientEvent::ServerSay(message) =>
            {
                self.push(Line::from(vec!
                [
                    Span::styled(format!("[{}] ", options::get_server_username()), theme::DIM),
                    Span::styled(message, theme::NOTICE),
                ]));
            },

            //A ROLE WAS SET; AN UNNAMED ONE IS OURS
            ClientEvent::Role(role, username) =>
            {
                match username
                {
                    Some(username) => self.push_styled(format!("{username} is now {role}."), theme::NOTICE),

                    None =>
                    {
                        self.role = role;
                        self.push_styled(format!("You are now {role}."), theme::NOTICE);
                    },
                }
            },

            //THE LOBBY'S STORED MESSAGES
            ClientEvent::History(messages, cached) =>
            {
                self.push_styled(format!("Message history ({}):", messages.len()), theme::TITLE);

                for message in messages
                {
                    match message.image
                    {
                        //A PICTURE WE HOLD IS ALREADY ON ITS WAY
                        Some(hash) => self.push_caption(message.username, message.text, hash,
                            cached.contains(&hash), message.colors.username_color),
                        None => self.push_history(message.username, message.text, message.colors),
                    }
                }
            },

            //THE ANSWER TO A CLICKED CAPTION
            ClientEvent::ImageData(hash, image) => self.deliver_image(hash, image),

            //server.toml CAME BACK
            ClientEvent::ServerSettings(settings, saved) =>
            {
                match saved
                {
                    //A REFUSED ROW SNAPS BACK
                    true =>
                    {
                        if self.settings.open && self.settings.server { self.settings.stored(settings); }

                        self.push_styled("Server settings saved.", theme::OK);
                    },

                    false => self.settings.open_server(settings),
                }

                self.dirty = true;
            },

            //THE ANSWER TO A /color
            ClientEvent::Colors => self.push_styled("Color set successfully.", theme::OK),

            //THE BAN LIST
            ClientEvent::ServerBans(users, ips) =>
            {
                if users.is_empty() && ips.is_empty()
                {
                    self.push_styled("No bans.", theme::DIM);
                } else
                {
                    self.push_styled(format!("Bans ({}):", users.len() + ips.len()), theme::TITLE);

                    //TWO SECTIONS, EACH NUMBERED FROM ZERO
                    let sections = [("users", users), ("addresses", ips)];
                    let last_section = sections.iter().filter(|(_, bans)| !bans.is_empty()).count().saturating_sub(1);

                    let mut section_index = 0;
                    for (name, bans) in sections
                    {
                        if bans.is_empty() { continue; }

                        let last = section_index == last_section;
                        section_index += 1;

                        self.push(Line::from(vec!
                        [
                            Span::styled(super::branch(last), theme::BORDER),
                            Span::raw(name),
                        ]));

                        //THE TRUNK RUNS PAST A NON-LAST SECTION
                        let trunk = format!("{}  ", if last { " " } else { "│" });
                        let width = id_width(bans.iter().map(|ban| ban.id));
                        let last_ban = bans.len() - 1;

                        for (index, ban) in bans.into_iter().enumerate()
                        {
                            let mut spans = vec![Span::styled(format!("{trunk}{}", super::branch(index == last_ban)), theme::BORDER)];

                            spans.extend(id_column(ban.id, width));
                            spans.push(Span::raw(ban.subject));

                            self.push(Line::from(spans));
                        }
                    }
                }
            },

            ClientEvent::List(users) =>
            {
                //ALWAYS REFRESH THE SIDEBAR; ECHO ONLY IF ASKED
                self.online = users;

                //A CHANNEL EXISTS WHILE SOMEBODY IS IN IT
                self.channels = self.online.iter().filter_map(|user| user.channel.clone()).collect();
                self.prune_panes();

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

                        //ACCENT OUR OWN CHANNEL
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

            ClientEvent::Image(filename) =>
            {
                self.push_text(format!("Uploading image \"{filename}\"..."));
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

                    //THE OWNER IS THE BRANCH, THEIR FILES HANG OFF IT
                    let width = id_width(users.iter().map(|user| user.id));
                    let last = users.len() - 1;

                    for (index, user) in users.into_iter().enumerate()
                    {
                        let mut spans = vec![Span::styled(super::branch(index == last), theme::BORDER)];

                        spans.extend(id_column(user.id, width));
                        spans.push(Span::raw(user.username.clone()));

                        self.push(Line::from(spans));

                        //THE TRUNK RUNS PAST A NON-LAST OWNER
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

            //BROADCAST TO EVERYBODY, US INCLUDED
            ClientEvent::Screenshare(username) if username != self.username =>
            {
                self.push(Line::from(vec!
                [
                    Span::styled(format!("[{}] ", options::get_server_username()), theme::DIM),
                    Span::styled(format!("{username} started screen sharing."), theme::NOTICE),
                ]));
            },

            ClientEvent::ScreenshareEnd(username) if username != self.username =>
            {
                self.push(Line::from(vec!
                [
                    Span::styled(format!("[{}] ", options::get_server_username()), theme::DIM),
                    Span::styled(format!("{username} stopped screen sharing."), theme::DIM),
                ]));
            },

            ClientEvent::Screenshare(_) | ClientEvent::ScreenshareEnd(_) => {},

            ClientEvent::Attached(username) =>
            {
                self.push_text(format!("{username} attached your screen sharing."));
            },

            ClientEvent::Deattached(username) =>
            {
                self.push_text(format!("{username} deattached your screen sharing."));
            },

            ClientEvent::IncompatibleVersion(version, server_version) =>
            {
                //THE BOX IS STILL UP, SO THE HISTORY IS NOT WHERE THIS IS READ
                self.disconnect_reason = Some(format!("Incompatible version! ({version}/{server_version})"));
            },

            ClientEvent::VersionMismatch(client_version, server_version) =>
            {
                self.push_styled(format!("Version mismatch - some features may not work \
                    ({client_version}/{server_version})"), theme::NOTICE);
            },

            //Login::ask KEEPS THE ERROR ON SCREEN
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

            //BACK TO THE CONNECT BOX UNLESS WE ASKED TO LEAVE
            ClientEvent::Quit =>
            {
                if self.leaving
                {
                    self.quit(0, Some(String::from("Disconnected from the server.")));
                } else if self.logging_out
                {
                    self.disconnected("Logged out.");
                } else
                {
                    let reason = self.disconnect_reason.take();
                    self.disconnected(reason.unwrap_or_else(|| String::from("Server quit communication.")));
                }
            },

            //SIDEBAR-ONLY, NOTHING IS ASKED OF THE SERVER
            ClientEvent::ChannelChanged(channel) =>
            {
                self.switch_channel(channel.clone().unwrap_or_default());

                if let Some(name) = channel.clone() { self.channels.insert(name); }

                //KEEP OUR OWN ROW HONEST UNTIL THE NEXT LIST
                let me = self.username.clone();

                if let Some(user) = self.online.iter_mut().find(|user| user.username == me)
                {
                    user.channel = channel;
                }

                //THE NEW CHANNEL'S ROSTER ARRIVES BEHIND THIS
                self.voice_roster.clear();
                self.voice_activity.clear();
                self.rebuild_voice();
            },

            ClientEvent::ChannelCreated(name) =>
            {
                self.channels.insert(name);
                self.dirty = true;
            },

            ClientEvent::ChannelDestroyed(name) =>
            {
                self.channels.remove(&name);
                self.panes.remove(&name);
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
//EVERY LIST BLOCK IS A TREE: BRANCH, ID, NAME
fn id_width(ids: impl Iterator<Item = usize>) -> usize
{
    ids.map(|id| id.to_string().len()).max().unwrap_or(1)
}

fn id_column(id: usize, width: usize) -> Vec<Span<'static>>
{
    vec![Span::styled(format!("{id:>width$}  "), theme::DIM)]
}
