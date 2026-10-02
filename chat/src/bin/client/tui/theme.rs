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

use std::sync::atomic::
{
    AtomicUsize,
    Ordering,
};

use chrono::
{
    DateTime,
    Local,
};

use ratatui::
{
    text::{ Line, Span },
    backend::FromCrossterm,
    style::
    {
        Color,
        Modifier,
        Style,
    },
};

use crate::{ colors, config };

use super::
{
    consts,
    markup,
    state::{ self, Entry, Picture, Transfer },
};

//STRUCTS
pub struct Theme //CACHED CONFIG-DRIVEN STYLING
{
    pub disable_colors: bool,
    pub disable_logo: bool,
    pub show_id: bool,
    pub show_message_ids: bool,
    pub show_timestamps: bool,
    pub message_stripes: bool,
    pub render_math: bool,
}

pub struct Palette //ONE COLOUR THEME
{
    pub id: &'static str, //AS STORED IN client.toml
    pub name: &'static str,
    pub background: Option<(u8, u8, u8)>, //None = THE TERMINAL'S
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub highlight: Color,
    pub error: Color,
    pub heart: Color,
    pub ok: Color,
    pub logo: Color,
    pub code_bg: Color,
    pub selected: Color,
    pub mention: Color,
    pub selection: Color,
    pub arg_required: Color,
    pub arg_optional: Color,
    pub arg_active: Color,
    pub ansi: Option<[Color; 16]>, //None = THE TERMINAL'S
}

//IMPLEMENTATIONS
impl Theme
{
    pub fn load() -> Self
    {
        ACTIVE.store(palette_index(&config::read_config::<String>("theme")), Ordering::Relaxed);

        Self
        {
            disable_colors: config::read_config::<bool>("disable_colors"),
            disable_logo: config::read_config::<bool>("disable_logo"),
            show_id: config::read_config::<bool>("show_id"),
            show_message_ids: config::read_config::<bool>("show_message_ids"),
            show_timestamps: config::read_config::<bool>("show_timestamps"),
            message_stripes: config::read_config::<bool>("message_stripes"),
            render_math: config::read_config::<bool>("render_math"),
        }
    }

    pub fn reload(&mut self) //RE-READ AFTER A config::client_write
    {
        *self = Self::load();
    }

    //STYLE AND WRAP ONE HISTORY ENTRY
    pub fn render(&self, entry: &Entry, width: u16, target: Option<&Entry>, me: &str) -> Vec<Line<'static>>
    {
        let mut lines: Vec<Line<'static>> = entry.reply().map(|reply| self.reply_row(reply, target, width)).into_iter().collect();

        let rows = self.render_entry(entry, width);

        lines.extend(match entry.message_id()
        {
            Some(message_id) => self.trailer(rows, message_id, entry.hearts(), entry.edited(), me, width),
            None => rows,
        });

        lines
    }

    fn render_entry(&self, entry: &Entry, width: u16) -> Vec<Line<'static>>
    {
        match entry
        {
            Entry::Line(line) => state::wrap_line(line, width),

            Entry::Message { username, id, timestamp, text, colors, .. } =>
            {
                let id = if self.show_id { format!(" ({id})") } else { String::new() };

                let prefix = vec!
                [
                    self.timestamp(*timestamp),
                    self.name(username.clone(), colors.username_color),
                    Span::styled(id, dim()),
                    Span::styled(": ", dim()),
                ];

                markup::render(prefix, text, self.style(colors.message_color), width, self.render_math)
            },

            //THE SAME LINE WITHOUT THE ID COLUMN
            Entry::History { username, timestamp, text, colors, .. } => markup::render(vec!
            [
                self.timestamp(*timestamp),
                self.name(username.clone(), colors.username_color),
                Span::styled(": ", dim()),
            ], text, self.style(colors.message_color), width, self.render_math),

            Entry::Private { sent, username, id, text, colors } =>
            {
                let prefix = vec!
                [
                    Span::styled(if *sent { "PM → " } else { "PM ← " }, accent()),
                    self.name(username.clone(), colors.username_color),
                    Span::styled(format!(" ({id}): "), dim()),
                ];

                markup::render(prefix, text, self.style(colors.message_color), width, self.render_math)
            },

            Entry::Transfer(transfer) => state::wrap_line(&Line::from(progress(transfer, width)), width),

            //ONLY THE CAPTION; THE PICTURE IS DRAWN UNDER IT
            Entry::Image { username, filename, timestamp, username_color, picture, .. } =>
            {
                let mut spans = vec!
                [
                    self.timestamp(*timestamp),
                    //THE SENDER'S COLOR, ELSE THE CHROME'S ACCENT
                    match username_color.filter(|_| !self.disable_colors).and_then(colors::u8_to_color)
                    {
                        Some(color) => Span::styled(username.clone(), Style::new().fg(ansi(Color::from_crossterm(color))).add_modifier(Modifier::BOLD)),
                        None => Span::styled(username.clone(), accent().add_modifier(Modifier::BOLD)),
                    },
                    Span::styled(format!(" sent an image ({filename})"), dim()),
                ];

                match picture
                {
                    Picture::Absent => spans.push(Span::styled(" [ show ]", accent())),
                    Picture::Waiting | Picture::Deferred => spans.push(Span::styled(" [ loading... ]", dim())),
                    Picture::Gone => spans.push(Span::styled(" [ unavailable ]", error())),
                    Picture::Ready(..) => {},
                }

                state::wrap_line(&Line::from(spans), width)
            },
        }
    }

    //ONE ROW NAMING THE MESSAGE A REPLY ANSWERS
    fn reply_row(&self, reply: u64, target: Option<&Entry>, width: u16) -> Line<'static>
    {
        let mut spans = vec![Span::styled(consts::REPLY, border())];

        match target
        {
            Some(Entry::Message { username, text, colors, .. } | Entry::History { username, text, colors, .. }) =>
            {
                spans.push(self.colorize(username.clone(), colors.username_color));
                spans.push(Span::styled(format!(": {}", text.lines().next().unwrap_or_default()), dim()));
            },

            Some(Entry::Image { username, filename, username_color, .. }) =>
            {
                spans.push(self.colorize(username.clone(), *username_color));
                spans.push(Span::styled(format!(" sent an image ({filename})"), dim()));
            },

            //NOT IN THE PANE
            _ => spans.push(Span::styled(format!("#{reply}"), dim())),
        }

        //ALWAYS ONE ROW
        let mut rows = state::wrap_line(&Line::from(spans), width.saturating_sub(1));
        let cut = rows.len() > 1;
        let mut row = rows.swap_remove(0);

        if cut
        {
            if row.spans.last().is_some_and(|span| span.content.trim().is_empty()) { row.spans.pop(); }
            row.spans.push(Span::styled("…", dim()));
        }

        row
    }

    fn timestamp(&self, timestamp: Option<u64>) -> Span<'static> //SEND TIME PREFIX, LOCAL
    {
        let Some(time) = timestamp.filter(|_| self.show_timestamps)
            .and_then(|timestamp| DateTime::from_timestamp(timestamp as i64, 0))
            .map(|time| time.with_timezone(&Local))
        else { return Span::raw("") };

        //OLDER THAN TODAY GETS THE DATE
        let format = match time.date_naive() == Local::now().date_naive()
        {
            true => "%H:%M ",
            false => "%Y-%m-%d %H:%M ",
        };

        Span::styled(time.format(format).to_string(), dim())
    }

    //EDITED, HEARTS AND MESSAGE ID, RIGHT-ALIGNED ON THE LAST ROW
    fn trailer(&self, mut lines: Vec<Line<'static>>, message_id: u64, hearts: &[String], edited: bool, me: &str, width: u16) -> Vec<Line<'static>>
    {
        let mut tag: Vec<Span<'static>> = Vec::new();

        if edited { tag.push(Span::styled(consts::EDITED, dim())); }

        if !hearts.is_empty()
        {
            if !tag.is_empty() { tag.push(Span::raw(" ")); }

            let style = if hearts.iter().any(|name| name == me) { heart() } else { dim() };
            tag.push(Span::styled(format!("{} {}", consts::HEART, hearts.len()), style));
        }

        if self.show_message_ids
        {
            if !tag.is_empty() { tag.push(Span::raw(" ")); }
            tag.push(Span::styled(format!("#{message_id}"), dim()));
        }

        if tag.is_empty() { return lines; }

        let tag_width: usize = tag.iter().map(Span::width).sum();
        let width = width as usize;

        match lines.last_mut()
        {
            Some(last) if last.width() + 1 + tag_width <= width =>
            {
                last.spans.push(Span::raw(" ".repeat(width - last.width() - tag_width)));
                last.spans.extend(tag);
            },

            _ =>
            {
                let mut row = vec![Span::raw(" ".repeat(width.saturating_sub(tag_width)))];
                row.extend(tag);
                lines.push(Line::from(row));
            },
        }

        lines
    }

    fn name(&self, username: String, color: Option<u8>) -> Span<'static> //A SENDER'S NAME, BOLD
    {
        Span::styled(username, self.style(color).add_modifier(Modifier::BOLD))
    }

    pub fn colorize(&self, text: String, color: Option<u8>) -> Span<'static> //COLORIZE text IF PASSED COLOR
    {
        Span::styled(text, self.style(color))
    }

    pub fn style(&self, color: Option<u8>) -> Style //THE USER'S OWN COLOUR, WHERE THEY HAVE ONE
    {
        match color.and_then(colors::u8_to_color)
        {
            Some(c) if !self.disable_colors => Style::new().fg(ansi(Color::from_crossterm(c))),
            _ => Style::new(),
        }
    }
}

//FUNCTIONS
//THE BACKGROUND, A LITTLE LIGHTER
pub fn stripe(terminal: Option<(u8, u8, u8)>) -> Color
{
    let Some((r, g, b)) = palette().background.or(terminal) else { return STRIPE_FALLBACK };

    //A LIGHT BACKGROUND GOES DARKER INSTEAD
    let light = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000 > 128;
    let target = if light { 0.0 } else { 255.0 };
    let nudge = |c: u8| (c as f32 + (target - c as f32) * STRIPE_LIFT).round() as u8;

    Color::Rgb(nudge(r), nudge(g), nudge(b))
}

//A TRANSFER'S ROW - WHAT IT IS, ITS BAR, AND WHAT IT HAS MOVED
fn progress(transfer: &Transfer, width: u16) -> Vec<Span<'static>>
{
    let Transfer { upload, image, filename, done, total, outcome, .. } = transfer;

    let kind = if *image { "image" } else { "file" };

    let (label, style) = match (outcome, upload)
    {
        (None, true) => (format!("Uploading {kind} \"{filename}\""), dim()),
        (None, false) => (format!("Downloading {kind} \"{filename}\""), dim()),
        (Some(true), true) => (format!("Uploaded {kind} \"{filename}\""), ok()),
        (Some(true), false) => (format!("Downloaded {kind} \"{filename}\""), ok()),
        (Some(false), _) => (format!("Transferring {kind} \"{filename}\" failed"), error()),
    };

    let percent = state::percent(*done, *total);

    //THE BAR NEVER WIDER THAN HALF THE PANE
    let cells = consts::PROGRESS_CELLS.min(width as usize / 2);
    let filled = (percent as usize * cells) / 100;

    let mut spans = vec![Span::styled(label, style)];

    //A PANE TOO NARROW FOR A BAR STILL GETS THE NUMBERS
    if cells >= consts::MIN_PROGRESS_CELLS
    {
        spans.extend(
        [
            Span::raw(" "),
            Span::styled("▕", border()),
            Span::styled("█".repeat(filled), if outcome.is_some() { style } else { accent() }),
            Span::styled("░".repeat(cells - filled), border()),
            Span::styled("▏", border()),
        ]);
    }

    spans.push(Span::styled(format!(" {percent:>3}%  {}/{}", size(*done), size(*total)), dim()));

    spans
}

//BYTES AS SOMETHING READABLE
fn size(bytes: u64) -> String
{
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];

    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1000.0 && unit < UNITS.len() - 1
    {
        value /= 1000.0;
        unit += 1;
    }

    match unit
    {
        0 => format!("{bytes}B"),
        _ => format!("{value:.1}{}", UNITS[unit]),
    }
}

//ACCESSORS FOR THE ACTIVE PALETTE
pub fn text() -> Style { Style::new().fg(palette().text) }                     //THE BASE FOREGROUND
pub fn base() -> Style { background().map_or(text(), |bg| text().bg(bg)) }     //THE BASE, WITH THE PALETTE'S BACKGROUND
pub fn border() -> Style { Style::new().fg(palette().muted) }
pub fn border_active() -> Style { Style::new().fg(palette().accent) }
pub fn title() -> Style { Style::new().fg(palette().accent).add_modifier(Modifier::BOLD) }
pub fn dim() -> Style { Style::new().fg(palette().muted) }
pub fn accent() -> Style { Style::new().fg(palette().accent) }
pub fn notice() -> Style { Style::new().fg(palette().highlight) }
pub fn error() -> Style { Style::new().fg(palette().error) }
pub fn heart() -> Style { Style::new().fg(palette().heart) }                   //A HEART WE GAVE
pub fn ok() -> Style { Style::new().fg(palette().ok) }
pub fn speaking() -> Style { ok().add_modifier(Modifier::BOLD) }

pub fn logo() -> Style { Style::new().fg(palette().logo) }                     //ON A FREE CELL THE GLYPH ITSELF IS DRAWN...
pub fn logo_under() -> Style { Style::new().bg(palette().logo) }               //...UNDER TEXT ONLY THE BACKGROUND IS

//CODE, AS A BOX PADDED TO THE PANE
pub fn code() -> Style { Style::new().fg(palette().ok).bg(palette().code_bg) } //INLINE `code`
pub fn code_block() -> Style { Style::new().fg(palette().text).bg(palette().code_bg) }
pub fn code_bar() -> Style { Style::new().fg(palette().accent).bg(palette().code_bg) } //THE BLOCK'S LEFT EDGE
pub fn code_lang() -> Style { Style::new().fg(palette().muted).bg(palette().code_bg).add_modifier(Modifier::ITALIC) }

//MARKDOWN
pub fn heading() -> Style { Style::new().fg(palette().highlight) }             //A HEADING THE MESSAGE GAVE NO COLOUR
pub fn quote() -> Style { Style::new().fg(palette().accent) }                  //A BLOCKQUOTE'S EDGE
pub fn bullet() -> Style { Style::new().fg(palette().accent) }                 //AND A LIST MARKER
pub fn rule() -> Style { Style::new().fg(palette().muted) }
pub fn link() -> Style { Style::new().fg(palette().accent).add_modifier(Modifier::UNDERLINED) }

pub fn math() -> Style { Style::new().fg(palette().highlight) }                //MATH THE MESSAGE GAVE NO COLOUR

pub fn selected() -> Style { Style::new().bg(palette().selected) }
pub fn mention() -> Style { Style::new().bg(palette().mention) }               //A MESSAGE THAT MENTIONS US, A BACKGROUND ONLY
pub fn selection() -> Style { Style::new().bg(palette().selection) }           //THE DRAG SELECTION, A BACKGROUND ONLY

pub fn arg_required() -> Style { Style::new().fg(palette().arg_required) }
pub fn arg_optional() -> Style { Style::new().fg(palette().arg_optional) }
pub fn arg_active() -> Style                                                   //THE PARAMETER BEING TYPED
{
    Style::new().fg(palette().arg_active).add_modifier(Modifier::BOLD).add_modifier(Modifier::UNDERLINED)
}

pub fn background() -> Option<Color> //THE PALETTE'S OWN BACKGROUND, IF IT PAINTS ONE
{
    palette().background.map(|(r, g, b)| Color::Rgb(r, g, b))
}

pub fn ansi(color: Color) -> Color //A NAMED COLOR, AS THE PALETTE FIXES IT
{
    let index = match color
    {
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
        Color::Indexed(index) if index < 16 => index as usize,
        _ => return color,
    };

    palette().ansi.map_or(color, |ansi| ansi[index])
}

pub fn palette() -> &'static Palette
{
    &PALETTES[ACTIVE.load(Ordering::Relaxed)]
}

pub fn palette_index(id: &str) -> usize //A STORED ID, UNKNOWN IS THE DEFAULT
{
    PALETTES.iter().position(|palette| palette.id.eq_ignore_ascii_case(id.trim())).unwrap_or(0)
}

//CONSTS
const fn rgb(hex: u32) -> Color
{
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn bg(hex: u32) -> Option<(u8, u8, u8)>
{
    Some(((hex >> 16) as u8, (hex >> 8) as u8, hex as u8))
}

static ACTIVE: AtomicUsize = AtomicUsize::new(0);

//EVERY OTHER MESSAGE, A BACKGROUND ONLY
const STRIPE_FALLBACK: Color = Color::Rgb(0x1B, 0x1F, 0x24);                   //FAINT SLATE, WHEN THE TERMINAL WILL NOT SAY
const STRIPE_LIFT: f32 = 0.07;                                                  //HOW FAR OFF THE BACKGROUND

//THE BUILT-IN PALETTES, THE FIRST IS THE DEFAULT
pub const PALETTES: &[Palette] = &
[
    Palette
    {
        id: "why2", name: "WHY2",
        background: bg(0x1B1A1C),
        text: rgb(0xEED1D6), muted: rgb(0xCAB4B7), accent: rgb(0x9DCEFF), highlight: rgb(0xFFDDE2),
        error: rgb(0xF646C6), heart: rgb(0xFF6B8B), ok: rgb(0xFFBBBA), logo: rgb(0x5C464B),
        code_bg: rgb(0x2E2428), selected: rgb(0x005F5F), mention: rgb(0x4B3A1F), selection: rgb(0x304563),
        arg_required: rgb(0xD7AF87), arg_optional: rgb(0xFFB4AB), arg_active: rgb(0xFFAF5F),
        ansi: Some(
        [
            rgb(0x1B1A1C), rgb(0xE94AE6), rgb(0xFFBAC2), rgb(0xFFDDE7), rgb(0x92ABD6), rgb(0xCD98DC), rgb(0x94D0FB), rgb(0xEBD1D9),
            rgb(0xC7B4BB), rgb(0xFFA0F2), rgb(0xFFFCFF), rgb(0xFFFFFF), rgb(0xCADEF6), rgb(0xFACAFF), rgb(0xF7FAFF), rgb(0xE8E0E9),
        ]),
    },
    Palette
    {
        id: "catppuccin-mocha", name: "Catppuccin Mocha",
        background: bg(0x1E1E2E),
        text: rgb(0xCDD6F4), muted: rgb(0xA6ADC8), accent: rgb(0x89B4FA), highlight: rgb(0xF5C2E7),
        error: rgb(0xF38BA8), heart: rgb(0xEBA0AC), ok: rgb(0xA6E3A1), logo: rgb(0x495A80),
        code_bg: rgb(0x181825), selected: rgb(0x45475A), mention: rgb(0x433D3A), selection: rgb(0x394361),
        arg_required: rgb(0xFAB387), arg_optional: rgb(0xF2CDCD), arg_active: rgb(0xF9E2AF),
        ansi: None,
    },
    Palette
    {
        id: "catppuccin-latte", name: "Catppuccin Latte",
        background: bg(0xEFF1F5),
        text: rgb(0x4C4F69), muted: rgb(0x6C6F85), accent: rgb(0x1E66F5), highlight: rgb(0x8839EF),
        error: rgb(0xD20F39), heart: rgb(0xE64553), ok: rgb(0x40A02B), logo: rgb(0xC1D2F5),
        code_bg: rgb(0xE6E9EF), selected: rgb(0xBCC0CC), mention: rgb(0xECDDCA), selection: rgb(0xC5D5F5),
        arg_required: rgb(0xFE640B), arg_optional: rgb(0xEA76CB), arg_active: rgb(0xDF8E1D),
        ansi: None,
    },
    Palette
    {
        id: "dracula", name: "Dracula",
        background: bg(0x282A36),
        text: rgb(0xF8F8F2), muted: rgb(0x8390C0), accent: rgb(0xBD93F9), highlight: rgb(0x8BE9FD),
        error: rgb(0xFF5555), heart: rgb(0xFF79C6), ok: rgb(0x50FA7B), logo: rgb(0x645484),
        code_bg: rgb(0x21222C), selected: rgb(0x44475A), mention: rgb(0x4B4C3F), selection: rgb(0x4D4467),
        arg_required: rgb(0xF1FA8C), arg_optional: rgb(0xFF79C6), arg_active: rgb(0xFFB86C),
        ansi: None,
    },
    Palette
    {
        id: "nord", name: "Nord",
        background: bg(0x2E3440),
        text: rgb(0xECEFF4), muted: rgb(0x9AA5BB), accent: rgb(0x88C0D0), highlight: rgb(0xEBCB8B),
        error: rgb(0xBF616A), heart: rgb(0xB48EAD), ok: rgb(0xA3BE8C), logo: rgb(0x526C7A),
        code_bg: rgb(0x272C36), selected: rgb(0x434C5E), mention: rgb(0x54524F), selection: rgb(0x445764),
        arg_required: rgb(0xD08770), arg_optional: rgb(0xB48EAD), arg_active: rgb(0xEBCB8B),
        ansi: None,
    },
    Palette
    {
        id: "gruvbox-dark", name: "Gruvbox Dark",
        background: bg(0x282828),
        text: rgb(0xEBDBB2), muted: rgb(0xA89984), accent: rgb(0x83A598), highlight: rgb(0xFABD2F),
        error: rgb(0xFB4934), heart: rgb(0xD3869B), ok: rgb(0xB8BB26), logo: rgb(0x4C5A55),
        code_bg: rgb(0x1D2021), selected: rgb(0x504945), mention: rgb(0x524629), selection: rgb(0x3F4744),
        arg_required: rgb(0xFE8019), arg_optional: rgb(0xD3869B), arg_active: rgb(0xFABD2F),
        ansi: None,
    },
    Palette
    {
        id: "tokyo-night", name: "Tokyo Night",
        background: bg(0x1A1B26),
        text: rgb(0xC0CAF5), muted: rgb(0x8189B3), accent: rgb(0x7AA2F7), highlight: rgb(0xBB9AF7),
        error: rgb(0xF7768E), heart: rgb(0xFF007C), ok: rgb(0x9ECE6A), logo: rgb(0x40517A),
        code_bg: rgb(0x16161E), selected: rgb(0x292E42), mention: rgb(0x423933), selection: rgb(0x283457),
        arg_required: rgb(0xE0AF68), arg_optional: rgb(0x7DCFFF), arg_active: rgb(0xFF9E64),
        ansi: None,
    },
    Palette
    {
        id: "one-dark", name: "One Dark",
        background: bg(0x282C34),
        text: rgb(0xABB2BF), muted: rgb(0x7F848E), accent: rgb(0x61AFEF), highlight: rgb(0xC678DD),
        error: rgb(0xE06C75), heart: rgb(0xBE5046), ok: rgb(0x98C379), logo: rgb(0x3F607F),
        code_bg: rgb(0x21252B), selected: rgb(0x3E4451), mention: rgb(0x4E4A42), selection: rgb(0x364D63),
        arg_required: rgb(0xE5C07B), arg_optional: rgb(0x56B6C2), arg_active: rgb(0xD19A66),
        ansi: None,
    },
    Palette
    {
        id: "solarized-dark", name: "Solarized Dark",
        background: bg(0x002B36),
        text: rgb(0x93A1A1), muted: rgb(0x839496), accent: rgb(0x268BD2), highlight: rgb(0xB58900),
        error: rgb(0xDC322F), heart: rgb(0xD33682), ok: rgb(0x859900), logo: rgb(0x0F5174),
        code_bg: rgb(0x00212B), selected: rgb(0x073642), mention: rgb(0x243E2B), selection: rgb(0x0A435D),
        arg_required: rgb(0xCB4B16), arg_optional: rgb(0x6C71C4), arg_active: rgb(0xB58900),
        ansi: None,
    },
    Palette
    {
        id: "rose-pine", name: "Rosé Pine",
        background: bg(0x191724),
        text: rgb(0xE0DEF4), muted: rgb(0x908CAA), accent: rgb(0xC4A7E7), highlight: rgb(0xF6C177),
        error: rgb(0xEB6F92), heart: rgb(0xEBBCBA), ok: rgb(0x9CCFD8), logo: rgb(0x5D5172),
        code_bg: rgb(0x1F1D2E), selected: rgb(0x403D52), mention: rgb(0x453935), selection: rgb(0x443B55),
        arg_required: rgb(0xF6C177), arg_optional: rgb(0xEBBCBA), arg_active: rgb(0xEA9A97),
        ansi: None,
    },
    Palette
    {
        id: "github-dark", name: "GitHub Dark",
        background: bg(0x0D1117),
        text: rgb(0xE6EDF3), muted: rgb(0x8B949E), accent: rgb(0x58A6FF), highlight: rgb(0xD2A8FF),
        error: rgb(0xF85149), heart: rgb(0xFF7B72), ok: rgb(0x3FB950), logo: rgb(0x2B4D74),
        code_bg: rgb(0x161B22), selected: rgb(0x30363D), mention: rgb(0x342C19), selection: rgb(0x203651),
        arg_required: rgb(0xFFA657), arg_optional: rgb(0x79C0FF), arg_active: rgb(0xE3B341),
        ansi: None,
    },
    Palette
    {
        id: "github-light", name: "GitHub Light",
        background: bg(0xFFFFFF),
        text: rgb(0x1F2328), muted: rgb(0x59636E), accent: rgb(0x0969DA), highlight: rgb(0x8250DF),
        error: rgb(0xCF222E), heart: rgb(0xBF3989), ok: rgb(0x1A7F37), logo: rgb(0xC9DEF7),
        code_bg: rgb(0xF6F8FA), selected: rgb(0xEAEEF2), mention: rgb(0xF6EDD5), selection: rgb(0xC2DAF6),
        arg_required: rgb(0xBC4C00), arg_optional: rgb(0x0550AE), arg_active: rgb(0x9A6700),
        ansi: None,
    },
    Palette
    {
        id: "horizon", name: "Horizon",
        background: bg(0x1C1E26),
        text: rgb(0xD5D8DA), muted: rgb(0x9DA0B8), accent: rgb(0x26BBD9), highlight: rgb(0xEE64AC),
        error: rgb(0xE95678), heart: rgb(0xF09483), ok: rgb(0x29D398), logo: rgb(0x205D6E),
        code_bg: rgb(0x16161C), selected: rgb(0x2E303E), mention: rgb(0x483F3D), selection: rgb(0x1E4553),
        arg_required: rgb(0xFAB795), arg_optional: rgb(0xB877DB), arg_active: rgb(0xFAC29A),
        ansi: None,
    },
];
