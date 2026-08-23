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

use ratatui::
{
    Frame,
    style::Style,
    text::{ Line, Span },
    widgets::
    {
        Block,
        BorderType,
        Clear,
        Paragraph,
    },
    layout::
    {
        Constraint,
        Layout,
        Position,
        Rect,
    },
};

use unicode_width::UnicodeWidthStr;

use crate::
{
    command,
    options::{ self, LoginState },
};

use super::
{
    palette,
    state::App,
    theme,
};

//CONSTS
const SIDEBAR_WIDTH: u16          = 24;
const SIDEBAR_MIN_TERM_WIDTH: u16 = 70; //BELOW THIS THE SIDEBAR IS DROPPED AND MESSAGES GO FULL-WIDTH
const INPUT_MIN_HEIGHT: u16       = 3;
const INPUT_MAX_HEIGHT: u16       = 8;
const CHANNELS_MIN_HEIGHT: u16    = 12; //SIDEBAR ROWS NEEDED BEFORE THE CHANNEL LIST IS WORTH SHOWING

//ENUMS
enum Panel //SIDEBAR SECTIONS, IN THE ORDER THEY ARE STACKED
{
    Online,
    Channels,
    Voice,
}

//PUBLIC
pub fn draw(frame: &mut Frame, app: &mut App)
{
    let area = frame.area();
    let masked = options::get_asking_password();

    //PAINT THE BASE FOREGROUND FIRST
    frame.buffer_mut().set_style(area, theme::TEXT);

    //MEASURE THE INPUT FIRST - THE MAIN AREA GETS WHATEVER IS LEFT
    let input_width = area.width.saturating_sub(4).max(1); //BORDERS + "> "
    let (input_lines, cursor) = app.input.render(input_width, masked);
    let input_height = (input_lines.len() as u16 + 2).clamp(INPUT_MIN_HEIGHT, INPUT_MAX_HEIGHT);

    let [main_area, input_area] = Layout::vertical
    ([
        Constraint::Min(INPUT_MIN_HEIGHT),
        Constraint::Length(input_height),
    ]).areas(area);

    //MESSAGES + SIDEBAR
    let (messages_area, sidebar_area) = if area.width >= SIDEBAR_MIN_TERM_WIDTH
    {
        let [m, s] = Layout::horizontal([Constraint::Min(0), Constraint::Length(SIDEBAR_WIDTH)]).areas(main_area);
        (m, Some(s))
    } else
    {
        (main_area, None)
    };

    draw_messages(frame, app, messages_area);

    if let Some(sidebar_area) = sidebar_area { draw_sidebar(frame, app, sidebar_area); }

    draw_input(frame, app, input_area, input_lines, cursor, masked);

    //THE PALETTE FLOATS OVER THE BOTTOM OF THE MESSAGE PANE
    if app.palette.is_visible() { draw_palette(frame, app, messages_area); }
}

//PRIVATE
fn draw_messages(frame: &mut Frame, app: &mut App, area: Rect)
{
    //WHY2 ── <SERVER NAME> ── <ADDRESS AS TYPED> ── SOCKS5
    let mut parts = vec![String::from("WHY2")];

    if !app.server_name.is_empty() { parts.push(app.server_name.clone()); }
    if !app.address.is_empty() { parts.push(app.address.clone()); }
    if options::socks5_enabled() { parts.push(String::from("SOCKS5")); }

    let title = format!(" {} ", parts.join(" ── "));

    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::BORDER)
        .title(Span::styled(title, theme::TITLE));

    //SCROLLED AWAY - ADVERTISE THE BACKLOG
    if app.scroll.is_some() && app.unread > 0
    {
        block = block.title_bottom(Line::from(Span::styled(format!(" ↓ {} new ", app.unread), theme::NOTICE)).right_aligned());
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 { return; }

    //WRAP OURSELVES SO THE SCROLL OFFSET IS EXACT
    let viewport = inner.height;
    let total = app.wrapped_lines(inner.width).len() as u16;
    let max_offset = total.saturating_sub(viewport);
    let offset = app.scroll.map(|o| o.min(max_offset)).unwrap_or(max_offset);

    let visible = app.wrapped_lines(inner.width)
        .iter()
        .skip(offset as usize)
        .take(viewport as usize)
        .cloned()
        .collect::<Vec<Line<'static>>>();

    frame.render_widget(Paragraph::new(visible), inner);
}

fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect)
{
    //THE ONLINE LIST TAKES WHATEVER THE OTHER PANELS LEAVE BEHIND
    let mut constraints = vec![Constraint::Min(3)];
    let mut panels = vec![Panel::Online];

    let limit = area.height.saturating_sub(3).max(3);

    if area.height >= CHANNELS_MIN_HEIGHT && !app.channels.is_empty()
    {
        constraints.push(Constraint::Length((app.channels.len() as u16 + 2).clamp(3, limit)));
        panels.push(Panel::Channels);
    }

    if voice_visible(app)
    {
        constraints.push(Constraint::Length((app.voice.len() as u16 + 2).clamp(3, limit)));
        panels.push(Panel::Voice);
    }

    let areas = Layout::vertical(constraints).split(area);

    for (area, panel) in areas.iter().zip(panels)
    {
        match panel
        {
            Panel::Online => draw_online(frame, app, *area),
            Panel::Channels => draw_channels(frame, app, *area),
            Panel::Voice => draw_voice(frame, app, *area),
        }
    }
}

fn draw_online(frame: &mut Frame, app: &App, area: Rect)
{
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::BORDER)
        .title(Span::styled(format!(" Online ({}) ", app.online.len()), theme::TITLE));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    //ID FIRST, RIGHT-ALIGNED, SO THE USERNAMES LINE UP IN ONE COLUMN
    let width = app.online.iter().map(|user| user.id.to_string().len()).max().unwrap_or(1);

    let me = app.username.clone();
    let lines = app.online.iter().map(|user|
    {
        let style = if user.username == me { theme::ACCENT } else { Style::default() };

        Line::from(vec!
        [
            Span::styled(format!("{id:>width$}  ", id = user.id), theme::DIM),
            Span::styled(user.username.clone(), style),
        ])
    }).collect::<Vec<Line>>();

    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_channels(frame: &mut Frame, app: &App, area: Rect)
{
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::BORDER)
        .title(Span::styled(format!(" Channels ({}) ", app.channels.len()), theme::TITLE));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let current = options::get_channel();

    let lines = app.channels.iter().map(|name|
    {
        let here = current == *name;

        Line::from(vec!
        [
            Span::styled(if here { "▸ " } else { "  " }, theme::ACCENT),
            Span::styled("#", theme::DIM),
            Span::styled(name.clone(), if here { theme::ACCENT } else { Style::default() }),
        ])
    }).collect::<Vec<Line>>();

    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_voice(frame: &mut Frame, app: &App, area: Rect)
{
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::BORDER)
        .title(Span::styled(" Voice ", theme::TITLE));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = app.voice.iter().map(|user|
    {
        #[cfg(feature = "client_voice")]
        let muted = options::is_muted(if user.is_local { None } else { Some(user.id) });

        #[cfg(not(feature = "client_voice"))]
        let muted = false;

        let marker = if muted { "✕" } else if user.is_speaking { "●" } else { "○" };
        let style = if muted
        {
            theme::ERROR
        } else if user.is_speaking
        {
            theme::SPEAKING
        } else
        {
            theme::DIM
        };

        let latency = if user.is_local { String::new() } else { format!(" {}ms", user.latency) };

        Line::from(vec!
        [
            Span::styled(format!("{marker} {}", user.username), style),
            Span::styled(latency, theme::DIM),
        ])
    }).collect::<Vec<Line>>();

    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_input(frame: &mut Frame, app: &App, area: Rect, lines: Vec<Line<'static>>, cursor: (u16, u16), masked: bool)
{
    //THE LOGIN PROMPT LIVES HERE, NOT IN THE HISTORY - IT DISAPPEARS THE MOMENT IT IS ANSWERED
    let prompt = match options::get_login_state()
    {
        LoginState::Username => "Username",
        LoginState::PasswordLogin => "Password (login)",
        LoginState::PasswordRegister => "Password (register)",
        LoginState::None => "",
    };

    let title = match (prompt, app.login_hint.as_deref())
    {
        ("", _) => String::new(),
        (prompt, Some(hint)) => format!(" {prompt} ── {hint} "),
        (prompt, None) => format!(" {prompt} "),
    };

    //STATUS LINE - THE INPUT BLOCK'S BOTTOM BORDER
    let channel = match options::get_channel()
    {
        c if c.is_empty() => String::new(),
        c => format!(" #{c} "),
    };

    let left = match (channel.trim(), app.username.as_str())
    {
        ("", "") => String::new(),
        (c, "") => format!(" {c} "),
        ("", u) => format!(" {u} "),
        (c, u) => format!(" {c} │ {u} "),
    };

    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(if masked { theme::NOTICE } else { theme::BORDER_ACTIVE })
        .title_bottom(Line::from(Span::styled(left, theme::DIM)))
        .title_bottom(Line::from(Span::styled(right_status(app), theme::DIM)).right_aligned());

    if !title.is_empty() { block = block.title(Span::styled(title, theme::TITLE)); }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 { return; }

    //"> " GUTTER
    let [gutter, text_area] = Layout::horizontal([Constraint::Length(2), Constraint::Min(0)]).areas(inner);
    frame.render_widget(Paragraph::new(Span::styled("> ", theme::ACCENT)), gutter);

    //SCROLL THE INPUT SO THE CURSOR STAYS VISIBLE IN A CAPPED-HEIGHT BOX
    let offset = cursor.1.saturating_sub(text_area.height.saturating_sub(1));
    frame.render_widget(Paragraph::new(lines).scroll((offset, 0)), text_area);

    frame.set_cursor_position(Position::new
    (
        text_area.x + cursor.0.min(text_area.width.saturating_sub(1)),
        text_area.y + cursor.1.saturating_sub(offset),
    ));
}

fn draw_palette(frame: &mut Frame, app: &App, area: Rect)
{
    //(COMMAND, PARAMETER TO HIGHLIGHT) PER ROW, PLUS WHICH ROW IS SELECTED
    let (entries, selected, title) = match &app.palette.mode
    {
        palette::PaletteMode::Hidden => return,

        palette::PaletteMode::Menu(matches, selected) =>
        {
            //KEEP THE SELECTION IN VIEW
            let first = selected.saturating_sub(palette::MAX_ROWS.saturating_sub(1));

            let entries = matches.iter().copied()
                .skip(first)
                .take(palette::MAX_ROWS)
                .map(|info| (info, None))
                .collect::<Vec<(&'static command::CommandInfo, Option<usize>)>>();

            (entries, Some(selected - first), " Commands ")
        },

        palette::PaletteMode::Signature(info, active) => (vec![(*info, *active)], None, " Parameters "),
    };

    let height = entries.len() as u16 + 2;

    if area.height < height || area.width < 10 { return; }

    //THE POPUP SITS ON THE BOTTOM EDGE OF THE MESSAGE PANE, DIRECTLY ABOVE THE INPUT
    let popup = Rect
    {
        x: area.x,
        y: area.y + area.height - height,
        width: area.width,
        height,
    };

    frame.render_widget(Clear, popup); //Clear RESETS THE CELLS, SO THE BASE FOREGROUND GOES BACK ON

    frame.buffer_mut().set_style(popup, theme::TEXT);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::BORDER_ACTIVE)
        .title(Span::styled(title, theme::TITLE));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    //COLUMNS ARE MEASURED ACROSS THE VISIBLE ROWS SO DESCRIPTIONS AND SHORTCUTS LINE UP
    let signature_width = entries.iter().map(|(info, _)| palette::signature_width(info)).max().unwrap_or(0);
    let shortcut_width = entries.iter().map(|(info, _)| palette::format_shortcut(info).width()).max().unwrap_or(0);

    let lines = entries.iter().enumerate().map(|(row, (info, active))|
    {
        let mut spans = vec![Span::styled(if Some(row) == selected { "▌" } else { " " }, theme::ACCENT)];

        spans.extend(palette::signature_spans(info, *active));
        spans.push(Span::raw(" ".repeat(signature_width - palette::signature_width(info) + 2)));
        spans.push(Span::styled(info.description.to_string(), theme::DIM));

        //SHORTCUTS HUG THE RIGHT EDGE, IN THEIR OWN COLUMN
        if shortcut_width > 0
        {
            let used = 1 + signature_width + 2 + info.description.width();
            let shortcut = palette::format_shortcut(info);

            spans.push(Span::raw(" ".repeat((inner.width as usize).saturating_sub(used + shortcut_width + 1))));
            spans.push(Span::styled(format!("{shortcut:>shortcut_width$} "), theme::ACCENT));
        }

        let line = Line::from(spans);

        if Some(row) == selected { line.style(theme::SELECTED) } else { line }
    }).collect::<Vec<Line>>();

    frame.render_widget(Paragraph::new(lines), inner);
}

fn right_status(_app: &App) -> String
{
    let mut parts: Vec<String> = Vec::new();

    #[cfg(feature = "client_voice")]
    if _app.voice_enabled
    {
        parts.push(String::from(if options::is_muted(None) { "mic off" } else { "mic on" }));
    }

    parts.push(String::from("Ctrl+H help"));

    format!(" {} ", parts.join(" │ "))
}

fn voice_visible(app: &App) -> bool
{
    app.voice_enabled && !app.voice.is_empty()
}
