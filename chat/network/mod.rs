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

//MODULES
#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "client")]
pub mod client;

pub mod voice;

use std::
{
    str::FromStr,
    net::TcpStream,
    mem::MaybeUninit,
    io::{ Read, Write },
    fmt::
    {
        self,
        Display,
        Formatter,
    },
};

use wincode::
{
    SchemaWrite,
    SchemaRead,
    TypeMeta,
    WriteResult,
    ReadResult,
    error::ReadError,
    io::
    {
        Writer,
        Reader,
    },
};

use colored::Color;

use crate::chat::
{
    crypto,
    options as chat_options,
};

#[cfg(feature = "server")]
use std::time::{ Instant, Duration };

#[cfg(feature = "server")]
use crate::chat::config;

//STRUCTS
#[derive(SchemaWrite, SchemaRead, PartialEq, Clone)]
pub enum MessageCode //CONTROL CODES
{
    KeyExchange,        //SERVER <> CLIENT | KEY EXCHANGE
    Rekey,              //SERVER -> CLIENT | TRIGGER KEY EXCHANGE (USED FOR RE-KEYING)
    Welcome,            //SERVER -> CLIENT | INFORMATIONS
    Disconnect,         //SERVER <> CLIENT | QUIT COMMUNICATION
    Username,           //SERVER -> CLIENT | PICK USERNAME
    PasswordL,          //SERVER -> CLIENT | LOGIN
    PasswordR,          //SERVER -> CLIENT | REGISTER
    Accept,             //SERVER -> CLIENT | START CHATTING
    Join,               //SERVER -> CLIENT | CLIENT JOIN MESSAGE
    Leave,              //SERVER -> CLIENT | CLIENT LEAVE MESSAGE
    List,               //CLIENT <> SERVER | PRINT CONNECTED USERS
    PrivateMessage,     //CLIENT <> SERVER | SEND MESSAGE ONLY TO ONE CLIENT
    PrivateMessageBack, //SERVER -> CLIENT | SEND MESSAGE BACK TO SENDER
    SpamWarning,        //SERVER -> CLIENT | TELL CLIENT TO CALM TF DOWN
    RegisterDisabled,   //SERVER -> CLIENT | REGISTRATION IS DISABLED
    Version,            //SERVER <> CLIENT | ASK CLIENT FOR THEIR PKG VERSION
    Channel,            //SERVER <> CLIENT | CHANNEL CHANGE
    Voice,              //CLIENT <> SERVER | ESTABLISH VOICE CHAT
    InvalidUsage,       //SERVER -> CLIENT | INVALID PARAMETERS TO A COMMAND
    InvalidFeature,     //SERVER -> CLIENT | CLIENT REQUESTED DISABLED FEATURE
}

#[derive(Clone)]
pub struct SerColor(pub Color); //SERIALIZABLE Color

#[derive(SchemaWrite, SchemaRead, Clone)]
pub struct MessageColors //COLORS OF MESSAGE (ALL OF THE STRING VALUES WILL GET COVERTED TO colored::Color)
{
    pub username_color: Option<SerColor>, //COLOR OF SENDER
    pub message_color: Option<SerColor>,  //COLOR OF MESSAGE
}

#[derive(SchemaWrite, SchemaRead, Clone)]
pub struct MessagePacket //MESSAGE PACKET (WHAT IS BEING SENT)
{
    pub text: Option<String>,      //MESSAGE
    pub username: Option<String>,  //USERNAME (SENT ONLY BY SERVER, AS SERVER DOESN'T ACCEPT USERNAMES FROM CLIENT)
    pub id: Option<usize>,         //ID OF USER
    pub code: Option<MessageCode>, //CONTROL CODE
    pub colors: MessageColors,     //MESSAGE COLORS
    pub seq: usize,                //SEQUENCE NUMBER
}

//IMPLEMENTATIONS
impl Default for MessagePacket //DEFAULT
{
    fn default() -> Self
    {
        Self
        {
            text: None,
            username: None,
            id: None,
            code: None,
            colors: MessageColors
            {
                username_color: None,
                message_color: None,
            },
            seq: 0,
        }
    }
}

impl Display for SerColor //PARSE SerColor TO STRING
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result
    {
        let color_str = match self.0
        {
            Color::Black => "black",
            Color::Red => "red",
            Color::Green => "green",
            Color::Yellow => "yellow",
            Color::Blue => "blue",
            Color::Magenta => "magenta",
            Color::Cyan => "cyan",
            Color::BrightBlack => "bright black",
            Color::BrightRed => "bright red",
            Color::BrightGreen => "bright green",
            Color::BrightYellow => "bright yellow",
            Color::BrightBlue => "bright blue",
            Color::BrightMagenta => "bright magenta",
            Color::BrightCyan => "bright cyan",
            Color::BrightWhite => "bright white",

            _ => "white",
        };

        write!(f, "{color_str}")
    }
}

impl FromStr for SerColor //PARSE STRING TO SerColor
{
    type Err = ReadError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Color::from_str(s)
            .map(SerColor)
            .map_err(|_| ReadError::Custom("Invalid color string"))
    }
}

//SERIALIZE SerColor
impl SchemaWrite for SerColor
{
    type Src = Self;
    const TYPE_META: TypeMeta = TypeMeta::Dynamic;

    fn size_of(src: &Self::Src) -> WriteResult<usize>
    {
        <String as SchemaWrite>::size_of(&src.to_string())
    }

    fn write(writer: &mut impl Writer, src: &Self::Src) -> WriteResult<()>
    {
        <String as SchemaWrite>::write(writer, &src.to_string())
    }
}

//DESERIALIZE SerColor
impl<'de> SchemaRead<'de> for SerColor
{
    type Dst = Self;
    const TYPE_META: TypeMeta = TypeMeta::Dynamic;

    fn read(reader: &mut impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()>
    {
        dst.write(<String as SchemaRead>::get(reader)?.parse::<SerColor>()?);
        Ok(())
    }
}

//FUNCTIONS
pub fn send(stream: &mut TcpStream, packet: MessagePacket, keys: Option<&chat_options::SharedKeys>) //SEND packet TO stream
{
    //COPY PACKET
    let mut packet = packet;

    //ADD SEQUENCE NUMBER TO packet (FROM CLIENT)
    #[cfg(feature = "client")]
    {
        packet.seq = chat_options::get_seq() + 1;
        chat_options::set_seq(packet.seq);
    }

    //ADD SEQUENCE NUMBER TO packet (FROM SERVER)
    #[cfg(feature = "server")]
    {
        let peer_addr = stream.peer_addr().ok();
        if peer_addr.is_some() && let Some(mut conn) = server::CONNECTIONS.get_mut(&peer_addr.unwrap())
        {
            if conn.is_authenticated()
            {
                packet.seq = conn.server_seq().unwrap() + 1;
                *conn.server_seq_mut().unwrap() = packet.seq;
            }
        }
    }

    //ENCODE THE PACKET STRUCT TO Vec<u8>
    let packet_bytes = wincode::serialize(&packet).expect("Encoding packet failed");

    let final_bytes = if let Some(keys) = keys
    {
        crypto::encrypt_packet(packet_bytes, keys)
    } else
    {
        packet_bytes //NO ENCRYPTION
    };

    //ENCODE ENCRYPTED OUTPUT TO BASE91
    let encoded_string = String::from_utf8(base91::slice_encode(&final_bytes)).expect("Encoding packet failed");

    //SEND
    stream.write_all((encoded_string + "\n").as_bytes()).expect("Sending packet failed");
    stream.flush().expect("Flushing stream failed");
}

pub fn receive(stream: &mut TcpStream, buffer: &mut Vec<u8>, keys: Option<&chat_options::SharedKeys>) -> Option<MessagePacket>
{
    //SERVER SIDE PACKET SIZE LIMIT
    #[cfg(feature = "server")]
    let max_packet_size: usize;

    //SERVER SIDE SPAM PROTECTION
    #[cfg(feature = "server")]
    let spam_protection = config::server_config::<bool>("spam_protection");

    #[cfg(feature = "server")]
    let peer_addr = stream.peer_addr().ok()?; //GET CURRENT PEER ADDRESS

    //SETUP LIMITS
    #[cfg(feature = "server")]
    {
        //CHECK IF CLIENT IS AUTHENTICATED
        let authenticated = server::CONNECTIONS.get(&peer_addr)
            .map(|conn| conn.is_authenticated())
            .unwrap_or(false);

        //ALLOW BIG MESSAGES WHEN SPAM PROTECTION IS OFF AND CLIENT IS AUTHENTICATED
        max_packet_size = if !spam_protection && authenticated
        {
            usize::MAX
        } else //SET MAX PACKET SIZE IF SPAM PROTECTION IS ENABLED
        {
            config::server_config("max_packet_size")
        };
    }

    //LOOP READING UNTIL MESSAGE ARRIVES
    loop
    {
        //CHECK IF THERE IS A NEWLINE IN BUFFER
        if let Some(pos) = buffer.iter().position(|&b| b == b'\n')
        {
            //EXTRACT LINE (INCLUDING \n)
            let mut line: Vec<u8> = buffer.drain(..=pos).collect();

            //REMOVE \n
            if let Some(&b'\n') = line.last() { line.pop(); }

            //DECODE PACKET (BASE91)
            let mut decoded_packet = base91::slice_decode(&line);

            //DECRYPT
            if let Some(keys) = keys
            {
                decoded_packet = match crypto::decrypt_packet(decoded_packet, keys)
                {
                    Some(d) => d,
                    None => //INVALID MAC
                    {
                        //LOG IF ON SERVER
                        #[cfg(feature = "server")]
                        log::warn!("HMAC verification failed: {}", peer_addr);

                        return None;
                    }
                }
            }

            //ACTIVITY TIMER ON SERVER
            #[cfg(feature = "server")]
            {
                let mut spam_warning = false;
                let mut shared_key = None;
                let mut disconnect = false;

                //FIND CONNECTION AND SET last_activity
                if let Some(mut conn) = server::CONNECTIONS.get_mut(&peer_addr)
                {
                    //SPAM
                    if config::server_config("spam_protection") && conn.is_authenticated() &&
                        Instant::now().duration_since(*conn.last_activity()) <
                            Duration::from_millis(config::server_config::<u64>("min_message_delay"))
                    {
                        //INCREMENT SPAM VIOLATIONS
                        *conn.spam_violations_mut().unwrap() += 1;

                        //WARN
                        spam_warning = true;
                        shared_key = conn.keys().cloned();

                        //CHECK FOR TOO MANY VIOLATIONS
                        disconnect = *conn.spam_violations().unwrap() > config::server_config::<usize>("max_message_delay_violations");
                    }

                    *conn.last_activity_mut() = Instant::now(); //RESET last_activity
                }

                //SEND WARNING CODE
                if spam_warning
                {
                    server::send_code(stream, None, MessageCode::SpamWarning, shared_key.as_ref());
                }

                //TOO MANY VIOLATIONS, BYE
                if disconnect
                {
                    server::remove_connection(&peer_addr, true);
                    return None;
                }
            }

            //DECODE AND RETURN
            match wincode::deserialize::<MessagePacket>(&decoded_packet)
            {
                Ok(packet) =>
                {
                    //VERIFY SEQUENCE NUMBER
                    #[cfg(feature = "server")] //ON SERVER
                    {
                        if let Some(mut conn) = server::CONNECTIONS.get_mut(&peer_addr)
                        {
                            if packet.seq > *conn.seq() //VALID SEQ
                            {
                                //SET SEQ TO CURRENT
                                *conn.seq_mut() = packet.seq;
                            } else
                            {
                                //INVALID SEQ
                                drop(conn); //PREVENT DEADLOCK
                                log::warn!("SEQ verification failed: {}", &peer_addr);
                                server::remove_connection(&peer_addr, false);
                            }
                        }
                    }

                    //VERIFY SEQUENCE NUMBER
                    #[cfg(feature = "client")] //ON CLIENT
                    {
                        if packet.seq > chat_options::get_server_seq() || chat_options::get_server_seq() == 0 || packet.code == Some(MessageCode::Disconnect) //VALID
                        {
                            //SET SEQ
                            chat_options::set_server_seq(packet.seq);
                        } else //INVALID, DISCONNECT
                        {
                            return None;
                        }
                    }

                    return Some(packet);
                },

                Err(_) =>
                {
                    //FORCEFULLY DISCONNECT CLIENT ON INVALID PACKET
                    #[cfg(feature = "server")]
                    server::remove_connection(&peer_addr, false);

                    return None;
                }
            }
        }

        //CHECK IF CLIENT IS STILL IN ACTIVE CONNECTION LIST
        #[cfg(feature = "server")]
        {
            if !server::CONNECTIONS.contains_key(&peer_addr)
            {
                return None;
            }
        }

        //READ FROM STREAM
        let mut chunk = [0u8; 1024];
        match stream.read(&mut chunk)
        {
            Ok(0) => //CLIENT DISCONNECTED
            {
                #[cfg(feature = "server")]
                {
                    server::remove_connection(&peer_addr, false);
                }

                return None;
            },

            Ok(n) => //VALID READ
            {
                #[cfg(feature = "server")]
                {
                    //CHECK MAX PACKET SIZE
                    if buffer.len() + n > max_packet_size
                    {
                         server::remove_connection(&peer_addr, true);
                         return None;
                    }
                }

                buffer.extend_from_slice(&chunk[..n]);
            },

            Err(_) => return None //ERROR OR TIMEOUT
        }
    }
}
