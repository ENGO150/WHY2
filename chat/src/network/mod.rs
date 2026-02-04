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
    config::Config,
    io::
    {
        Writer,
        Reader,
    },
};

use colored::Color;

use why2::consts;

use crate::{ crypto, options };

#[cfg(feature = "server")]
use std::time::{ Instant, Duration };

#[cfg(feature = "server")]
use crate::config;

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
    Voice,              //CLIENT <> SERVER | ESTABLISH VOICE CONNECTION
    ChannelJoin,        //SERVER -> CLIENT | CLIENT JOINED VOICE CHANNEL
    ChannelLeave,       //SERVER -> CLIENT | CLIENT LEFT VOICE CHANNEL
    VoiceClients,       //SERVER -> CLIENT | TELL CLIENT ALL CONNECTED VOICE CLIENTS
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
unsafe impl<C: Config> SchemaWrite<C> for SerColor
{
    type Src = Self;
    const TYPE_META: TypeMeta = TypeMeta::Dynamic;

    fn size_of(src: &Self::Src) -> WriteResult<usize>
    {
        <String as SchemaWrite<C>>::size_of(&src.to_string())
    }

    fn write(writer: &mut impl Writer, src: &Self::Src) -> WriteResult<()>
    {
        <String as SchemaWrite<C>>::write(writer, &src.to_string())
    }
}

//DESERIALIZE SerColor
unsafe impl<'de, C: Config> SchemaRead<'de, C> for SerColor
{
    type Dst = Self;
    const TYPE_META: TypeMeta = TypeMeta::Dynamic;

    fn read(reader: &mut impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()>
    {
        dst.write(<String as SchemaRead<C>>::get(reader)?.parse::<SerColor>()?);
        Ok(())
    }
}

//FUNCTIONS
//PRIVATE
fn obfuscate_data(data: &[u8]) -> Vec<u8> //XOR BYTES (USED FOR OBFUSCATION)
{
    let mut obfuscated = data.to_vec();
    for (i, byte) in obfuscated.iter_mut().enumerate()
    {
        //XOR EACH BYTE WITH OBFUSCATION KEY
        *byte ^= options::OBFUSCATION_KEY[i % options::OBFUSCATION_KEY.len()];
    }

    obfuscated
}

//PUBLIC
pub fn send(stream: &mut TcpStream, mut packet: MessagePacket, keys: Option<&options::SharedKeys>) //SEND packet TO stream
{
    //ADD SEQUENCE NUMBER TO packet (FROM CLIENT)
    #[cfg(feature = "client")]
    {
        packet.seq = options::get_seq() + 1;
        options::set_seq(packet.seq);
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

    let mut final_bytes = if let Some(keys) = keys
    {
        crypto::encrypt_packet::<{ consts::DEFAULT_GRID_WIDTH }, { consts::DEFAULT_GRID_HEIGHT }>(packet_bytes, keys)
    } else
    {
        obfuscate_data(&packet_bytes) //NO ENCRYPTION, OBFUSCATE
    };

    //CONVERT ENCRYPTED OUTPUT TO BYTES ([LENGTH][DATA])
    let packet_len = final_bytes.len();
    let mut transmission_packet = Vec::with_capacity(4 + packet_len);
    transmission_packet.extend_from_slice(&(packet_len as u32).to_be_bytes());
    transmission_packet.append(&mut final_bytes);

    //SEND
    let _ = stream.write_all(&transmission_packet);
    stream.flush().expect("Flushing stream failed");
}

pub fn receive(stream: &mut TcpStream, keys: Option<&options::SharedKeys>) -> Option<MessagePacket>
{
    //SERVER SIDE PACKET SIZE LIMIT
    #[cfg(feature = "server")]
    let max_packet_size: usize;

    //SERVER SIDE SPAM PROTECTION
    #[cfg(feature = "server")]
    let spam_protection = config::read_config::<bool>("spam_protection");

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
            config::read_config("max_packet_size")
        };
    }

    //READ MESSAGE LENGTH
    let mut len_buf = [0u8; 4];
    if stream.read_exact(&mut len_buf).is_err() { return None; } //READ LENGTH
    let len = u32::from_be_bytes(len_buf) as usize;

    //CHECK PACKET SIZE
    #[cfg(feature = "server")]
    if len > max_packet_size
    {
         server::remove_connection(&peer_addr, true);
         return None;
    }

    //READ REST OF PACKET
    let mut decoded_packet = vec![0u8; len];
    if stream.read_exact(&mut decoded_packet).is_err() { return None; } //READ

    //DECRYPT
    if let Some(keys) = keys
    {
        decoded_packet = match crypto::decrypt_packet::<{ consts::DEFAULT_GRID_WIDTH }, { consts::DEFAULT_GRID_HEIGHT }>(decoded_packet, keys)
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
    } else
    {
        decoded_packet = obfuscate_data(&decoded_packet); //NO ENCRYPTION, REMOVE OBFUSCATION
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
            if config::read_config("spam_protection") && conn.is_authenticated() &&
                Instant::now().duration_since(*conn.last_activity()) <
                    Duration::from_millis(config::read_config::<u64>("min_message_delay"))
            {
                //INCREMENT SPAM VIOLATIONS
                *conn.spam_violations_mut().unwrap() += 1;

                //WARN
                spam_warning = true;
                shared_key = conn.keys().cloned();

                //CHECK FOR TOO MANY VIOLATIONS
                disconnect = *conn.spam_violations().unwrap() > config::read_config::<usize>("max_message_delay_violations");
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

    //DESERIALIZE AND RETURN
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
                if packet.seq > options::get_server_seq() || options::get_server_seq() == 0 || packet.code == Some(MessageCode::Disconnect) //VALID
                {
                    //SET SEQ
                    options::set_server_seq(packet.seq);
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
