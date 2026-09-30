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

//IMPLEMENTATIONS
impl Theme
{
    pub fn load() -> Self
    {
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
            Some(message_id) => self.trailer(rows, message_id, entry.hearts(), me, width),
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
                    Span::styled(id, DIM),
                    Span::styled(": ", DIM),
                ];

                markup::render(prefix, text, self.style(colors.message_color), width, self.render_math)
            },

            //THE SAME LINE WITHOUT THE ID COLUMN
            Entry::History { username, timestamp, text, colors, .. } => markup::render(vec!
            [
                self.timestamp(*timestamp),
                self.name(username.clone(), colors.username_color),
                Span::styled(": ", DIM),
            ], text, self.style(colors.message_color), width, self.render_math),

            Entry::Private { sent, username, id, text, colors } =>
            {
                let prefix = vec!
                [
                    Span::styled(if *sent { "PM → " } else { "PM ← " }, ACCENT),
                    self.name(username.clone(), colors.username_color),
                    Span::styled(format!(" ({id}): "), DIM),
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
                        Some(color) => Span::styled(username.clone(), Style::new().fg(Color::from_crossterm(color)).add_modifier(Modifier::BOLD)),
                        None => Span::styled(username.clone(), ACCENT.add_modifier(Modifier::BOLD)),
                    },
                    Span::styled(format!(" sent an image ({filename})"), DIM),
                ];

                match picture
                {
                    Picture::Absent => spans.push(Span::styled(" [ show ]", ACCENT)),
                    Picture::Waiting | Picture::Deferred => spans.push(Span::styled(" [ loading... ]", DIM)),
                    Picture::Gone => spans.push(Span::styled(" [ unavailable ]", ERROR)),
                    Picture::Ready(..) => {},
                }

                state::wrap_line(&Line::from(spans), width)
            },
        }
    }

    //ONE ROW NAMING THE MESSAGE A REPLY ANSWERS
    fn reply_row(&self, reply: u64, target: Option<&Entry>, width: u16) -> Line<'static>
    {
        let mut spans = vec![Span::styled(consts::REPLY, BORDER)];

        match target
        {
            Some(Entry::Message { username, text, colors, .. } | Entry::History { username, text, colors, .. }) =>
            {
                spans.push(self.colorize(username.clone(), colors.username_color));
                spans.push(Span::styled(format!(": {}", text.lines().next().unwrap_or_default()), DIM));
            },

            Some(Entry::Image { username, filename, username_color, .. }) =>
            {
                spans.push(self.colorize(username.clone(), *username_color));
                spans.push(Span::styled(format!(" sent an image ({filename})"), DIM));
            },

            //NOT IN THE PANE
            _ => spans.push(Span::styled(format!("#{reply}"), DIM)),
        }

        //ALWAYS ONE ROW
        let mut rows = state::wrap_line(&Line::from(spans), width.saturating_sub(1));
        let cut = rows.len() > 1;
        let mut row = rows.swap_remove(0);

        if cut
        {
            if row.spans.last().is_some_and(|span| span.content.trim().is_empty()) { row.spans.pop(); }
            row.spans.push(Span::styled("…", DIM));
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

        Span::styled(time.format(format).to_string(), DIM)
    }

    //HEARTS AND MESSAGE ID, RIGHT-ALIGNED ON THE LAST ROW
    fn trailer(&self, mut lines: Vec<Line<'static>>, message_id: u64, hearts: &[String], me: &str, width: u16) -> Vec<Line<'static>>
    {
        let mut tag: Vec<Span<'static>> = Vec::new();

        if !hearts.is_empty()
        {
            let style = if hearts.iter().any(|name| name == me) { HEART } else { DIM };
            tag.push(Span::styled(format!("{} {}", consts::HEART, hearts.len()), style));
        }

        if self.show_message_ids
        {
            if !tag.is_empty() { tag.push(Span::raw(" ")); }
            tag.push(Span::styled(format!("#{message_id}"), DIM));
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
            Some(c) if !self.disable_colors => Style::new().fg(Color::from_crossterm(c)),
            _ => Style::new(),
        }
    }
}

//FUNCTIONS
//THE TERMINAL'S BACKGROUND, A LITTLE LIGHTER
pub fn stripe(background: Option<(u8, u8, u8)>) -> Color
{
    let Some((r, g, b)) = background else { return STRIPE_FALLBACK };

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
        (None, true) => (format!("Uploading {kind} \"{filename}\""), DIM),
        (None, false) => (format!("Downloading {kind} \"{filename}\""), DIM),
        (Some(true), true) => (format!("Uploaded {kind} \"{filename}\""), OK),
        (Some(true), false) => (format!("Downloaded {kind} \"{filename}\""), OK),
        (Some(false), _) => (format!("Transferring {kind} \"{filename}\" failed"), ERROR),
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
            Span::styled("▕", BORDER),
            Span::styled("█".repeat(filled), if outcome.is_some() { style } else { ACCENT }),
            Span::styled("░".repeat(cells - filled), BORDER),
            Span::styled("▏", BORDER),
        ]);
    }

    spans.push(Span::styled(format!(" {percent:>3}%  {}/{}", size(*done), size(*total)), DIM));

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

//CONSTS
pub const TEXT: Style = Style::new().fg(Color::Rgb(0xEE, 0xD1, 0xD6));          //WARM OFF-WHITE - THE BASE FOREGROUND
pub const BORDER: Style = Style::new().fg(Color::Rgb(0xCA, 0xB4, 0xB7));        //MUTED ROSE GREY
pub const BORDER_ACTIVE: Style = Style::new().fg(Color::Rgb(0x9D, 0xCE, 0xFF)); //SKY BLUE
pub const TITLE: Style = Style::new().fg(Color::Rgb(0x9D, 0xCE, 0xFF)).add_modifier(Modifier::BOLD);
pub const DIM: Style = Style::new().fg(Color::Rgb(0xCA, 0xB4, 0xB7));
pub const ACCENT: Style = Style::new().fg(Color::Rgb(0x9D, 0xCE, 0xFF));
pub const NOTICE: Style = Style::new().fg(Color::Rgb(0xFF, 0xDD, 0xE2));        //PALE PINK
pub const ERROR: Style = Style::new().fg(Color::Rgb(0xF6, 0x46, 0xC6));         //HOT MAGENTA
pub const HEART: Style = Style::new().fg(Color::Rgb(0xFF, 0x6B, 0x8B));         //ROSE RED - A HEART WE GAVE
pub const OK: Style = Style::new().fg(Color::Rgb(0xFF, 0xBB, 0xBA));            //SALMON
pub const SPEAKING: Style = Style::new().fg(Color::Rgb(0xFF, 0xBB, 0xBA)).add_modifier(Modifier::BOLD);

pub const LOGO_COLOR: Color = Color::Rgb(0x5C, 0x46, 0x4B);                     //DEEP ROSE - THE WATERMARK BEHIND EVERYTHING
pub const LOGO: Style = Style::new().fg(LOGO_COLOR);                            //ON A FREE CELL THE GLYPH ITSELF IS DRAWN...
pub const LOGO_UNDER: Style = Style::new().bg(LOGO_COLOR);                      //...UNDER TEXT ONLY THE BACKGROUND IS

//CODE, AS A BOX PADDED TO THE PANE
pub const CODE_BG: Color = Color::Rgb(0x2E, 0x24, 0x28);                        //DEEP ROSE-BROWN
pub const CODE: Style = Style::new().fg(Color::Rgb(0xFF, 0xBB, 0xBA)).bg(CODE_BG);        //INLINE `code`
pub const CODE_BLOCK: Style = Style::new().fg(Color::Rgb(0xEE, 0xD1, 0xD6)).bg(CODE_BG);
pub const CODE_BAR: Style = Style::new().fg(Color::Rgb(0x9D, 0xCE, 0xFF)).bg(CODE_BG);    //THE BLOCK'S LEFT EDGE
pub const CODE_LANG: Style = Style::new().fg(Color::Rgb(0xCA, 0xB4, 0xB7)).bg(CODE_BG)
    .add_modifier(Modifier::ITALIC);

//MARKDOWN
pub const HEADING: Style = Style::new().fg(Color::Rgb(0xFF, 0xDD, 0xE2));      //A HEADING THE MESSAGE GAVE NO COLOUR
pub const QUOTE: Style = Style::new().fg(Color::Rgb(0x9D, 0xCE, 0xFF));        //A BLOCKQUOTE'S EDGE
pub const BULLET: Style = Style::new().fg(Color::Rgb(0x9D, 0xCE, 0xFF));       //AND A LIST MARKER
pub const RULE: Style = Style::new().fg(Color::Rgb(0xCA, 0xB4, 0xB7));
pub const LINK: Style = Style::new().fg(Color::Rgb(0x9D, 0xCE, 0xFF)).add_modifier(Modifier::UNDERLINED);

pub const MATH: Style = Style::new().fg(Color::Rgb(0xFF, 0xDD, 0xE2));         //MATH THE MESSAGE GAVE NO COLOUR

pub const SELECTED: Style = Style::new().bg(Color::Rgb(0x00, 0x5F, 0x5F));

//EVERY OTHER MESSAGE, A BACKGROUND ONLY
pub const STRIPE_FALLBACK: Color = Color::Rgb(0x1B, 0x1F, 0x24);               //FAINT SLATE, WHEN THE TERMINAL WILL NOT SAY
const STRIPE_LIFT: f32 = 0.07;                                                  //HOW FAR OFF THE TERMINAL'S BACKGROUND

//A MESSAGE THAT MENTIONS US, A BACKGROUND ONLY
pub const MENTION: Style = Style::new().bg(Color::Rgb(0x4B, 0x3A, 0x1F));

//THE DRAG SELECTION, A BACKGROUND ONLY
pub const SELECTION: Style = Style::new().bg(Color::Rgb(0x30, 0x45, 0x63));

pub const ARG_REQUIRED: Style = Style::new().fg(Color::Rgb(0xD7, 0xAF, 0x87));  //SOFT SAND
pub const ARG_OPTIONAL: Style = Style::new().fg(Color::Rgb(0xFF, 0xB4, 0xAB));  //FADED CORAL
pub const ARG_ACTIVE: Style = Style::new().fg(Color::Rgb(0xFF, 0xAF, 0x5F))     //WARM AMBER - THE PARAMETER BEING TYPED
    .add_modifier(Modifier::BOLD)
    .add_modifier(Modifier::UNDERLINED);
