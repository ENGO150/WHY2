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
    backend::FromCrossterm,
    style::{ Color, Style },
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
    config,
    options,
};

#[cfg(feature = "client_voice")]
use crate::network::voice::client::options as voice_options;

use super::
{
    theme,
    state::{ self, App },
    palette::
    {
        self,
        Entry,
        Values,
        PaletteMode,
    },
    tofu::
    {
        self,
        Prompt,
        Stage,
    },
    settings::
    {
        self,
        Row,
        Value,
        Settings,
        DeviceEntry,
    },
    login::
    {
        Login,
        Stage as LoginStage,
    },
};

//CONSTS
const SIDEBAR_WIDTH: u16          = 24;
const SIDEBAR_MIN_TERM_WIDTH: u16 = 70; //BELOW THIS THE SIDEBAR IS DROPPED AND MESSAGES GO FULL-WIDTH
const INPUT_MIN_HEIGHT: u16       = 3;
const INPUT_MAX_HEIGHT: u16       = 8;
const CHANNELS_MIN_HEIGHT: u16    = 12; //SIDEBAR ROWS NEEDED BEFORE THE CHANNEL LIST IS WORTH SHOWING
const SETTINGS_WIDTH: u16         = 62; //SETTINGS OVERLAY, CAPPED TO THE TERMINAL
const TOFU_WIDTH: u16             = 64; //SERVER IDENTITY OVERLAY, CAPPED TO THE TERMINAL
const LOGIN_WIDTH: u16            = 52; //CONNECT PROMPT, CAPPED TO THE TERMINAL
const FIELD_ROW: u16              = 1;  //THE ADDRESS FIELD SITS ONE ROW UNDER ITS OWN LABEL
const SETTINGS_VALUE_WIDTH: u16   = 20; //NARROWEST THE VALUE COLUMN MAY GET (BAR + PERCENTAGE)

//THE PROJECT LOGO, PAINTED IN THE MIDDLE OF THE MESSAGE PANE AS A WATERMARK
const LOGO: &str = include_str!("./assets/rexlogo");

#[cfg(feature = "client_voice")]
const SLIDER_WIDTH: usize         = 14; //CELLS OF VOLUME BAR

//ROWS KEPT BETWEEN THE SELECTION AND EITHER EDGE OF A SCROLLING LIST, SO THERE IS ALWAYS VISIBLE PROOF THAT
//THE LIST GOES ON BEFORE IT STARTS MOVING
const SCROLL_GAP: usize           = 4;

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

    //THE CONNECT BOX ASKS FOR EVERYTHING UNTIL WE ARE IN - THE INPUT BAR HAS NOTHING TO SAY YET, THE SAME
    //WAY THE SIDEBAR HAS NOBODY TO LIST
    let connecting = app.login.is_some();

    //PAINT THE BASE FOREGROUND FIRST
    frame.buffer_mut().set_style(area, theme::TEXT);

    //MEASURE THE INPUT FIRST - THE MAIN AREA GETS WHATEVER IS LEFT
    let input_width = area.width.saturating_sub(4).max(1); //BORDERS + "> "
    let (input_lines, cursor) = app.input.render(input_width, false);
    let input_height = if connecting { 0 }
        else { (input_lines.len() as u16 + 2).clamp(INPUT_MIN_HEIGHT, INPUT_MAX_HEIGHT) };

    let [main_area, input_area] = Layout::vertical
    ([
        Constraint::Min(INPUT_MIN_HEIGHT),
        Constraint::Length(input_height),
    ]).areas(area);

    //MESSAGES + SIDEBAR (THE SIDEBAR IS EMPTY UNTIL WE ARE AUTHENTICATED - DO NOT SPEND THE WIDTH ON IT)
    let (messages_area, sidebar_area) = if area.width >= SIDEBAR_MIN_TERM_WIDTH && options::get_sending_messages()
    {
        let [m, s] = Layout::horizontal([Constraint::Min(0), Constraint::Length(SIDEBAR_WIDTH)]).areas(main_area);
        (m, Some(s))
    } else
    {
        (main_area, None)
    };

    draw_messages(frame, app, messages_area);

    if let Some(sidebar_area) = sidebar_area { draw_sidebar(frame, app, sidebar_area); }

    if !connecting { draw_input(frame, app, input_area, input_lines, cursor); }

    //THE LOGO GOES BEHIND ALL OF IT, IN THE MIDDLE OF THE SCREEN - THE OVERLAYS BELOW Clear THEIR OWN RECT, SO IT
    //NEVER REACHES THEM
    if !app.theme.disable_logo { draw_logo(frame, area); }

    //THE PALETTE FLOATS OVER THE BOTTOM OF THE MESSAGE PANE
    if app.palette.is_visible() { draw_palette(frame, app, messages_area); }

    //THE SETTINGS OVERLAY COVERS EVERYTHING ELSE (THE INPUT CURSOR IS SUPPRESSED IN draw_input)
    if app.settings.open { draw_settings(frame, &mut app.settings, area); }

    //THE CONNECT BOX IS THE FIRST THING THE CLIENT EVER DRAWS, AND IT KEEPS ASKING UNTIL WE ARE LOGGED IN
    if let Some(login) = &app.login { draw_login(frame, login, area); }

    //...AND THE SERVER-KEY PROMPT COVERS EVEN THAT, BECAUSE IT IS THE ONLY THING THE USER MAY ANSWER
    if let Some(prompt) = &app.tofu { draw_tofu(frame, prompt, area); }
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

    //THE BACKLOG IS THE ONE THING THE MESSAGE PANE CANNOT SHOW BY ITSELF
    draw_scrollbar(frame, area, total as usize, viewport as usize, offset as usize);
}

//A SCROLLBAR DOWN THE RIGHT-HAND BORDER OF A BOX WHOSE LIST DOES NOT FIT. IT REPLACES BORDER CELLS RATHER THAN
//CLAIMING A COLUMN OF ITS OWN, SO NOTHING GETS NARROWER FOR HAVING ONE, AND IT IS DRAWN ONLY WHILE THERE IS
//SOMETHING OFF-SCREEN - A VISIBLE BAR ALWAYS MEANS "THERE IS MORE", NEVER JUST "THIS IS A LIST".
//THE THUMB IS SIZED AND PLACED HERE INSTEAD OF BY ratatui's Scrollbar BECAUSE THAT ONE NEVER QUITE LANDS ON EITHER
//END OF THE TRACK, AND THE ONE THING THE BAR HAS TO SAY UNAMBIGUOUSLY IS WHEN THE USER IS AT THE BOTTOM
//WHERE A SCROLLING LIST STARTS DRAWING. THE OFFSET IS KEPT BY THE LIST BETWEEN FRAMES AND ONLY MOVES WHEN THE
//SELECTION RUNS INTO THE GAP AT EITHER EDGE - DERIVING IT FROM THE SELECTION ALONE PINS THE SELECTION TO AN EDGE,
//WHICH BOTH HIDES THAT THERE IS MORE BELOW AND SCROLLS ON EVERY SINGLE KEY ON THE WAY BACK UP
fn window(offset: usize, selected: usize, total: usize, visible: usize) -> usize
{
    let max = total.saturating_sub(visible);

    //A LIST TOO SHORT TO HOLD TWO GAPS AND A SELECTION KEEPS WHATEVER IT CAN
    let gap = SCROLL_GAP.min(visible.saturating_sub(1) / 2);

    let mut first = offset.min(max);

    if selected < first + gap { first = selected.saturating_sub(gap); }

    if selected + gap >= first + visible { first = (selected + gap + 1).saturating_sub(visible); }

    first.min(max)
}

fn draw_scrollbar(frame: &mut Frame, area: Rect, total: usize, visible: usize, first: usize)
{
    if total <= visible || visible == 0 || area.width == 0 || area.height < 3 { return; }

    let track = area.height as usize - 2; //THE CORNERS STAY CORNERS

    if track == 0 { return; }

    let max_first = total - visible;

    //ROUNDED, SO A LONG LIST STILL GETS A THUMB AND A SHORT SCROLL STILL MOVES IT
    let thumb = ((visible * track + total / 2) / total).clamp(1, track);
    let room = track - thumb;
    let start = if max_first == 0 { 0 } else { (first.min(max_first) * room + max_first / 2) / max_first };

    let x = area.x + area.width - 1;
    let buffer = frame.buffer_mut();

    for row in 0..track
    {
        let Some(cell) = buffer.cell_mut((x, area.y + 1 + row as u16)) else { continue; };

        if row >= start && row < start + thumb
        {
            cell.set_symbol("\u{2588}");
            cell.set_style(theme::ACCENT);
        } else
        {
            cell.set_symbol("\u{2502}");
            cell.set_style(theme::BORDER);
        }
    }
}

//THE LOGO GOES IN LAST AND CLAIMS NO CELL THAT IS ALREADY SPOKEN FOR: ON A FREE CELL IT DRAWS ITS OWN GLYPH, AND
//UNDER A CHARACTER SOMEBODY ELSE PUT THERE IT ONLY TAKES THE BACKGROUND - SO MESSAGES READ OVER THE LOGO INSTEAD
//OF PUNCHING HOLES IN IT, AND THE SHAPE STAYS WHOLE EITHER WAY
fn draw_logo(frame: &mut Frame, area: Rect)
{
    let rows = LOGO.lines().collect::<Vec<&str>>();
    let height = rows.len() as u16;
    let width = rows.iter().map(|row| row.chars().count()).max().unwrap_or(0) as u16;

    if width == 0 || area.width < width || area.height < height { return; } //TOO CRAMPED TO READ - LEAVE IT OUT

    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height - height) / 2;
    let buffer = frame.buffer_mut();

    for (row_index, row) in rows.iter().enumerate()
    {
        for (column, symbol) in row.chars().enumerate()
        {
            if symbol == ' ' { continue; }

            let Some(cell) = buffer.cell_mut((x + column as u16, y + row_index as u16)) else { continue; };

            if cell.symbol().trim().is_empty() //FREE CELL - THE LOGO OWNS IT OUTRIGHT
            {
                cell.set_char(symbol);
                cell.set_style(theme::LOGO);
            } else if cell.bg == Color::Reset //TAKEN, BUT NOTHING IS PAINTED BEHIND IT YET
            {
                cell.set_style(theme::LOGO_UNDER);
            }
        }
    }
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
        //A MUTE ONLY MEANS ANYTHING WHILE WE ARE THE ONE LISTENING
        #[cfg(feature = "client_voice")]
        let muted = app.voice_enabled && options::is_muted(if user.is_local { None } else { Some(user.id) });

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

        //NO PING FOR SOMEBODY WE ARE NOT RECEIVING - THE ROSTER SAYS THEY ARE IN VOICE, NOTHING MORE
        let latency = match user.latency
        {
            Some(latency) => format!(" {latency}ms"),
            None => String::new(),
        };

        Line::from(vec!
        [
            Span::styled(format!("{marker} {}", user.username), style),
            Span::styled(latency, theme::DIM),
        ])
    }).collect::<Vec<Line>>();

    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_input(frame: &mut Frame, app: &App, area: Rect, lines: Vec<Line<'static>>, cursor: (u16, u16))
{
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

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::BORDER_ACTIVE)
        .title_bottom(Line::from(Span::styled(left, theme::DIM)))
        .title_bottom(Line::from(Span::styled(right_status(app), theme::DIM)).right_aligned());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 { return; }

    //"> " GUTTER
    let [gutter, text_area] = Layout::horizontal([Constraint::Length(2), Constraint::Min(0)]).areas(inner);
    frame.render_widget(Paragraph::new(Span::styled("> ", theme::ACCENT)), gutter);

    //SCROLL THE INPUT SO THE CURSOR STAYS VISIBLE IN A CAPPED-HEIGHT BOX
    let offset = cursor.1.saturating_sub(text_area.height.saturating_sub(1));
    frame.render_widget(Paragraph::new(lines).scroll((offset, 0)), text_area);

    //NO CARET WHILE AN OVERLAY OWNS THE KEYBOARD (THE CONNECT PROMPT DRAWS ITS OWN)
    if app.settings.open || app.tofu.is_some() || app.login.is_some() { return; }

    frame.set_cursor_position(Position::new
    (
        text_area.x + cursor.0.min(text_area.width.saturating_sub(1)),
        text_area.y + cursor.1.saturating_sub(offset),
    ));
}

fn draw_palette(frame: &mut Frame, app: &mut App, area: Rect)
{
    //HOW MANY ROWS THERE ARE, HOW MANY FIT AND WHAT THEY ARE CALLED - THE BOX IS MEASURED FROM THAT, AND ONLY THEN
    //IS THERE A WIDTH TO FILL
    let (total, selected, title) = match &app.palette.mode
    {
        PaletteMode::Hidden => return,

        PaletteMode::Menu(matches, selected) => (matches.len(), *selected, String::from(" Commands ")),

        //THE PARAMETER NAMES ITSELF - THE LIST IS ITS VOCABULARY, NOT THE COMMAND'S
        PaletteMode::Values(values) =>
            (values.matches.len(), values.selected, format!(" {} ", capitalize(values.arg.name))),

        PaletteMode::Signature(..) => (1, 0, String::from(" Parameters ")),
    };

    let rows = total.min(palette::MAX_ROWS);

    //KEEP THE SELECTION IN VIEW, A GAP SHORT OF EITHER EDGE - ONE PLACE, SO THE ROWS AND THE SCROLLBAR CANNOT
    //DISAGREE ABOUT WHICH ONES THEY ARE
    let first = window(app.palette.offset, selected, total, rows);

    app.palette.offset = first;

    let height = rows as u16 + 2;

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

    let lines = match &app.palette.mode
    {
        PaletteMode::Values(values) => value_lines(values, rows, first),
        _ => entry_lines(app, rows, first, inner.width as usize),
    };

    frame.render_widget(Paragraph::new(lines), inner);

    draw_scrollbar(frame, popup, total, rows, first);
}

//ONE ROW PER ANSWER THE PARAMETER ACCEPTS, EACH SHOWING ITS OWN COLOR - A NAME ALONE WOULD STILL BE A GUESS
fn value_lines(values: &Values, rows: usize, first: usize) -> Vec<Line<'static>>
{
    values.matches.iter().skip(first).take(rows).enumerate().map(|(row, value)|
    {
        let selected = first + row == values.selected;

        let mut spans = vec![Span::styled(if selected { "▌" } else { " " }, theme::ACCENT)];

        //THE SWATCH IS PAINTED AS A BACKGROUND, SO EVEN black AND dark_grey ARE SOMETHING TO LOOK AT
        if let Some(color) = values.swatch(value)
        {
            spans.push(Span::styled("    ", Style::new().bg(Color::from_crossterm(color))));
            spans.push(Span::raw(" "));
        }

        spans.push(Span::raw(value.clone()));

        let line = Line::from(spans);

        if selected { line.style(theme::SELECTED) } else { line }
    }).collect()
}

//ONE ROW PER COMMAND (OR THE SINGLE PARAMETER HINT), IN ALIGNED COLUMNS
fn entry_lines(app: &App, rows: usize, first: usize, width: usize) -> Vec<Line<'static>>
{
    //(COMMAND, PARAMETER TO HIGHLIGHT) PER ROW, PLUS WHICH ROW IS SELECTED
    let (entries, selected) = match &app.palette.mode
    {
        PaletteMode::Menu(matches, selected) =>
        {
            let entries = matches.iter().copied()
                .skip(first)
                .take(rows)
                .map(|entry| (entry, None))
                .collect::<Vec<(Entry, Option<usize>)>>();

            (entries, Some(selected - first))
        },

        PaletteMode::Signature(entry, active) => (vec![(*entry, *active)], None),

        _ => return Vec::new(),
    };

    //COLUMNS ARE MEASURED ACROSS THE VISIBLE ROWS SO DESCRIPTIONS AND SHORTCUTS LINE UP
    let signature_width = entries.iter().map(|(entry, _)| entry.width()).max().unwrap_or(0);
    let shortcut_width = entries.iter().map(|(entry, _)| entry.shortcut().width()).max().unwrap_or(0);

    entries.iter().enumerate().map(|(row, (entry, active))|
    {
        let mut spans = vec![Span::styled(if Some(row) == selected { "▌" } else { " " }, theme::ACCENT)];

        //THE ACTIVE PARAMETER'S OWN DESCRIPTION TAKES OVER THE COLUMN WHILE IT'S BEING TYPED
        let description = active.and_then(|i| entry.args().get(i)).map_or(entry.description(), |arg| arg.description);

        spans.extend(entry.spans(*active));
        spans.push(Span::raw(" ".repeat(signature_width - entry.width() + 2)));
        spans.push(Span::styled(description.to_string(), theme::DIM));

        //SHORTCUTS HUG THE RIGHT EDGE, IN THEIR OWN COLUMN
        if shortcut_width > 0
        {
            let used = 1 + signature_width + 2 + description.width();
            let shortcut = entry.shortcut();

            spans.push(Span::raw(" ".repeat(width.saturating_sub(used + shortcut_width + 1))));
            spans.push(Span::styled(format!("{shortcut:>shortcut_width$} "), theme::ACCENT));
        }

        let line = Line::from(spans);

        if Some(row) == selected { line.style(theme::SELECTED) } else { line }
    }).collect()
}

//"COLOR" -> "Color" - PARAMETER NAMES ARE STORED SHOUTED, TITLES ARE NOT
fn capitalize(name: &str) -> String
{
    let mut chars = name.chars();

    match chars.next()
    {
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
        None => String::new(),
    }
}

//THE /settings OVERLAY - ONE CENTERED BOX, EITHER THE SETTING ROWS OR THE DEVICE PICKER ON TOP OF THEM
fn draw_settings(frame: &mut Frame, state: &mut Settings, area: Rect)
{
    let width = SETTINGS_WIDTH.min(area.width.saturating_sub(2)).max(1);
    let inner_width = width.saturating_sub(2) as usize;

    if area.height < 5 || inner_width < 12 { return; }

    //THE PICKER BORROWS THE SAME BOX, SO BOTH MODES ARE MEASURED THE SAME WAY
    let (title, total, selected) = match &state.picker
    {
        Some(picker) => (picker.title.to_string(), picker.entries.len(), picker.selected),
        None => (state.title(), state.rows.len(), state.selected),
    };

    //THE SERVER'S COMMENT ON A KEY IS A WHOLE SENTENCE AND HAS NO BUSINESS IN A TITLE BAR, WHERE IT SHARES
    //THE WIDTH WITH THE TITLE AND IS CUT OFF. IT IS WRAPPED INTO THE FOOT OF THE BOX INSTEAD, ACROSS THE
    //FULL WIDTH AND OVER AS MANY LINES AS IT NEEDS
    let hint_lines = match state.picker.is_none().then(|| state.rows.get(state.selected)).flatten()
    {
        Some(row) => description_lines(state, row, inner_width as u16),
        None => Vec::new(),
    };

    //THE FOOT IS SIZED FOR THE LONGEST COMMENT IN THE BOX, NOT FOR THE ONE UNDER THE CURSOR - OTHERWISE THE
    //WHOLE BOX GROWS AND SHRINKS AS THE SELECTION MOVES, WHICH IS UNREADABLE TO SCROLL THROUGH
    let hint_height = match state.picker.is_some()
    {
        true => 0,
        false => state.rows.iter()
            .map(|row| description_lines(state, row, inner_width as u16).len())
            .max().unwrap_or(0),
    };

    let room = area.height.saturating_sub(4) as usize; //BORDERS PLUS A LINE OF AIR TOP AND BOTTOM

    //THE RULE ABOVE IT COUNTS TOO - AND ON A TERMINAL WITH NO ROOM FOR BOTH, THE ROWS WIN
    let footer = match hint_height { 0 => 0, height => height + 1 };
    let footer = if room > footer { footer } else { 0 };

    let rows_room = room - footer;

    let visible = match &state.picker
    {
        Some(_) => total.min(settings::MAX_PICKER_ROWS).min(rows_room),
        None => total.min(rows_room),
    }.max(1);

    //KEEP THE SELECTION IN VIEW, A GAP SHORT OF EITHER EDGE - AND HAND THE LIST BACK BOTH WHERE THE VIEW ENDED
    //UP AND HOW MANY ROWS FIT, WHICH IS WHAT PageUp/PageDown MOVE BY
    let offset = match &state.picker
    {
        Some(picker) => picker.offset,
        None => state.offset,
    };

    let first = window(offset, selected, total, visible);

    state.page = visible;

    match state.picker.as_mut()
    {
        Some(picker) => picker.offset = first,
        None => state.offset = first,
    }

    //THE VALUE COLUMN STARTS RIGHT BEHIND THE LONGEST LABEL, NOT AT SOME GUESSED OFFSET
    let label_width = state.rows.iter().filter_map(|row| match row
    {
        Row::Item(item) => Some(item.label.width()),
        Row::Header(_) | Row::Action(_) => None,
    }).max().unwrap_or(0).min(inner_width.saturating_sub(SETTINGS_VALUE_WIDTH as usize + 3));

    //ON A NARROW TERMINAL THE LABELS GIVE WAY FIRST - THE VALUES ARE WHAT THE USER IS HERE FOR

    let mut lines = match &state.picker
    {
        Some(picker) => picker.entries.iter().enumerate()
            .skip(first)
            .take(visible)
            .map(|(index, entry)| picker_line(entry, index == picker.selected, inner_width))
            .collect::<Vec<Line>>(),

        None => state.rows.iter().enumerate()
            .skip(first)
            .take(visible)
            .map(|(index, row)| settings_line(state, row, index == state.selected, label_width, inner_width))
            .collect::<Vec<Line>>(),
    };

    let rows_height = lines.len() as u16 + 2; //WHAT THE SCROLLBAR IS ALLOWED TO RUN DOWN

    //THE DESCRIPTION SITS UNDER A RULE, SO IT READS AS AN EXPLANATION OF THE SELECTED ROW AND NOT AS A ROW
    if footer > 0
    {
        lines.push(Line::from(Span::styled("\u{2500}".repeat(inner_width), theme::BORDER)));

        let blanks = hint_height - hint_lines.len(); //A SHORT COMMENT LEAVES THE REST OF THE FOOT EMPTY

        lines.extend(hint_lines);
        lines.extend(std::iter::repeat_n(Line::default(), blanks));
    }

    let height = lines.len() as u16 + 2;

    let popup = Rect
    {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, popup); //Clear RESETS THE CELLS, SO THE BASE FOREGROUND GOES BACK ON

    frame.buffer_mut().set_style(popup, theme::TEXT);

    let hint = match (&state.picker, state.edit.is_some(), state.server)
    {
        (Some(_), ..) => " ↑↓ select │ ⏎ apply │ Esc back ",
        (None, true, _) => " type a value │ ⏎ keep │ Esc cancel ",
        (None, false, true) => " ↑↓ move │ ←→ change │ ⏎ edit │ ^S save │ Esc close ",
        (None, false, false) => " ↑↓ move │ ←→ change │ ⏎ select │ Esc close ",
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::BORDER_ACTIVE)
        .title(Span::styled(title, theme::TITLE))
        .title_bottom(Line::from(Span::styled(hint, theme::DIM)).centered());

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    frame.render_widget(Paragraph::new(lines), inner);

    //BOTH MODES SCROLL - THE DEVICE PICKER ALWAYS, THE SETTING ROWS ON A SHORT TERMINAL. THE TRACK IS THE
    //ROWS' OWN HEIGHT, NOT THE BOX'S: THE DESCRIPTION UNDER THEM IS NOT SOMETHING THE BAR IS MEASURING
    draw_scrollbar(frame, Rect { height: rows_height, ..popup }, total, visible, first);
}

//THE SERVER IDENTITY PROMPT. THE WHOLE POINT IS THAT THE FINGERPRINT IS READABLE AND THE SAFE ANSWER IS
//THE ONE ALREADY SELECTED - TRUSTING TAKES A DELIBERATE MOVE PLUS ⏎. A MISMATCH GOES ONE STEP FURTHER AND
//ASKS A SECOND TIME (tofu::Stage::Confirm), WHERE THE ANSWER HAS TO BE TYPED OUT.
fn draw_tofu(frame: &mut Frame, prompt: &Prompt, area: Rect)
{
    let width = TOFU_WIDTH.min(area.width.saturating_sub(2)).max(1);
    let inner_width = width.saturating_sub(4); //BORDERS PLUS A COLUMN OF AIR EACH SIDE

    if area.height < 9 || inner_width < 20 { return; }

    let confirming = prompt.stage == Stage::Confirm;

    let warning = match (confirming, prompt.mismatch)
    {
        (true, _) => "Replacing a pinned key throws away the only thing that would \
            catch an interception. Do it only after checking the fingerprint with \
            the operator over a channel this server cannot touch.",

        (false, true) => "The server is presenting a different identity key than the one \
            pinned for this address. Either the operator replaced the server's \
            keys, or somebody is sitting between you and it.",

        (false, false) => "This address has no pinned identity key yet. Accept it only if the \
            fingerprint below matches the one the server's operator published.",
    };

    //THE BODY IS WRAPPED WITH THE SAME WRAPPER THE MESSAGE PANE USES, SO A NARROW TERMINAL STAYS READABLE
    let mut lines = state::wrap_line(&Line::from(Span::styled(warning, theme::NOTICE)), inner_width);

    lines.push(Line::default());
    lines.push(Line::from(vec!
    [
        Span::styled("Server   ", theme::DIM),
        Span::raw(prompt.host.clone()),
    ]));

    //ON A MISMATCH BOTH FINGERPRINTS ARE ON SCREEN AT ONCE - THE DECISION IS A COMPARISON, NOT A GUESS
    for (index, row) in prompt.pinned_fingerprint().into_iter().enumerate()
    {
        lines.push(Line::from(vec!
        [
            Span::styled(if index == 0 { "Pinned   " } else { "         " }, theme::DIM),
            Span::styled(row, theme::DIM),
        ]));
    }

    let label = if prompt.mismatch { "New key  " } else { "Key      " };

    for (index, row) in prompt.fingerprint().into_iter().enumerate()
    {
        lines.push(Line::from(vec!
        [
            Span::styled(if index == 0 { label } else { "         " }, theme::DIM),
            Span::styled(row, theme::ACCENT),
        ]));
    }

    lines.push(Line::default());

    if confirming
    {
        let typed = prompt.typed.chars().count();

        lines.append(&mut state::wrap_line(&Line::from(Span::styled(format!
        (
            "Type '{}' to replace the pinned key with this one:",
            tofu::CHALLENGE,
        ), theme::TEXT)), inner_width));

        lines.push(Line::from(vec!
        [
            Span::styled(prompt.typed.clone(), theme::ACCENT),
            Span::styled("_".repeat(tofu::CHALLENGE.chars().count().saturating_sub(typed)), theme::DIM),
        ]).centered());

        if prompt.wrong
        {
            lines.push(Line::from(Span::styled(format!("Type '{}' to go through with it.", tofu::CHALLENGE),
                theme::ERROR)).centered());
        }
    } else
    {
        lines.push(Line::from(vec!
        [
            button(" Reject ", !prompt.accept, theme::ERROR),
            Span::raw("  "),
            button(if prompt.mismatch { " Replace pinned key " } else { " Trust & save " }, prompt.accept, theme::OK),
        ]).centered());
    }

    let height = (lines.len() as u16 + 2).min(area.height);

    let popup = Rect
    {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, popup); //Clear RESETS THE CELLS, SO THE BASE FOREGROUND GOES BACK ON

    frame.buffer_mut().set_style(popup, theme::TEXT);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::ERROR)
        .title(Span::styled(prompt.title(), theme::ERROR))
        .title_bottom(Line::from(Span::styled(if confirming
        {
            " type the word │ ⏎ confirm │ ← back │ Esc reject "
        } else { " ←→ choose │ ⏎ confirm │ Esc reject " }, theme::DIM)).centered());

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    //ONE COLUMN OF AIR EACH SIDE, MATCHING WHAT inner_width WAS MEASURED AGAINST
    let [_, text_area, _] = Layout::horizontal
    ([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ]).areas(inner);

    frame.render_widget(Paragraph::new(lines), text_area);
}

fn draw_login(frame: &mut Frame, login: &Login, area: Rect)
{
    let width = LOGIN_WIDTH.min(area.width.saturating_sub(2)).max(1);
    let inner_width = width.saturating_sub(4); //BORDERS PLUS A COLUMN OF AIR EACH SIDE
    let field_width = inner_width.saturating_sub(2); //"> " GUTTER

    if area.height < 8 || field_width < 8 { return; }

    let (field, cursor) = login.input.render(field_width, login.masked());

    let mut lines = vec![Line::from(Span::styled(login.label(), theme::DIM))];

    for (index, line) in field.into_iter().enumerate()
    {
        let mut spans = vec![Span::styled(if index == 0 { "> " } else { "  " }, theme::ACCENT)];
        spans.extend(line.spans);

        lines.push(Line::from(spans));
    }

    lines.push(Line::default());

    //THE STATUS ROW, ALWAYS IN THE SAME PLACE: WHAT IS HAPPENING, WHAT WENT WRONG, OR THE SERVER'S RULES.
    //IT IS WRAPPED, NOT TRUNCATED - AN OS ERROR IS AS LONG AS IT IS, AND THE BOX GROWS A ROW INSTEAD OF
    //SPILLING PAST ITS OWN BORDER.
    let status = match (login.busy, login.error.as_deref(), login.hint.as_deref())
    {
        (true, ..) => Line::from(Span::styled(login.waiting(), theme::ACCENT)),
        (false, Some(error), _) => Line::from(Span::styled(error.to_string(), theme::ERROR)),
        (false, None, Some(hint)) => Line::from(Span::styled(hint.to_string(), theme::DIM)),
        (false, None, None) => Line::default(),
    };

    lines.extend(state::wrap_line(&status, inner_width));

    //THE PROXY IS THE ADDRESS STEP'S BUSINESS - BY THE TIME WE ARE LOGGING IN IT HAS ALREADY DONE ITS JOB
    if login.stage == LoginStage::Address && options::socks5_enabled()
    {
        let proxy = Line::from(Span::styled(format!("Through SOCKS5 {}",
            config::read_config::<String>("socks5_addr")), theme::DIM));

        lines.extend(state::wrap_line(&proxy, inner_width));
    }

    let height = (lines.len() as u16 + 2).min(area.height);

    let popup = Rect
    {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, popup); //Clear RESETS THE CELLS, SO THE BASE FOREGROUND GOES BACK ON

    frame.buffer_mut().set_style(popup, theme::TEXT);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::BORDER_ACTIVE)
        .title(Span::styled(login.title(), theme::TITLE))
        .title_bottom(Line::from(Span::styled(match (login.stage, login.busy, login.cancellable())
        {
            (_, true, true) => " Esc cancel ",
            (_, true, false) => " Esc quit ",
            (LoginStage::Address, false, _) => " ⏎ connect │ Esc quit ",
            (_, false, _) => " ⏎ continue │ Esc quit ",
        }, theme::DIM)).centered());

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    //ONE COLUMN OF AIR EACH SIDE, MATCHING WHAT inner_width WAS MEASURED AGAINST
    let [_, text_area, _] = Layout::horizontal
    ([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ]).areas(inner);

    frame.render_widget(Paragraph::new(lines), text_area);

    //THIS BOX IS THE ONLY THING BEING TYPED INTO WHILE IT IS UP, SO IT KEEPS THE CARET
    if !login.busy
    {
        frame.set_cursor_position(Position::new
        (
            text_area.x + 2 + cursor.0.min(field_width.saturating_sub(1)),
            text_area.y + FIELD_ROW + cursor.1,
        ));
    }
}

fn button(label: &'static str, selected: bool, style: Style) -> Span<'static>
{
    if selected { Span::styled(label, style.patch(theme::SELECTED)) } else { Span::styled(label, theme::DIM) }
}

//THE SELECTED ROW'S EXPLANATION, WRAPPED RATHER THAN CUT: server.toml's COMMENTS ARE FULL SENTENCES AND
//THE ONLY THING SAYING WHAT A KEY DOES, SO LOSING THE END OF ONE LOSES THE POINT OF IT
fn description_lines(state: &Settings, row: &Row, width: u16) -> Vec<Line<'static>>
{
    let mut spans = Vec::new();

    match row
    {
        Row::Header(_) => return Vec::new(),

        //A BUTTON SAYS WHAT PRESSING IT DOES - AND, WHEN IT WILL NOT DO IT YET, WHY NOT
        Row::Action(label) if **label == *settings::RESTART_LABEL =>
        {
            spans.push(Span::styled("Restart the server \u{2014} every client is disconnected and the whole config is read again.", theme::DIM));

            if state.unsaved() { spans.push(Span::styled(" \u{b7} save your changes first", theme::NOTICE)); }
            else if state.confirm { spans.push(Span::styled(" \u{b7} press again to confirm", theme::ERROR)); }
        },

        Row::Action(_) => spans.push(Span::styled("Send the edited rows to the server.", theme::DIM)),

        Row::Item(item) =>
        {
            if !item.hint.is_empty() { spans.push(Span::styled(item.hint.clone(), theme::DIM)); }

            //A KEY THE SERVER ONLY READS AT STARTUP IS SAID SO HERE - IT IS STORED EITHER WAY, JUST NOT USED YET
            if item.restart
            {
                let note = match spans.is_empty() { true => "restart required", false => " \u{b7} restart required" };

                spans.push(Span::styled(note, theme::NOTICE));
            }
        },
    }

    if spans.is_empty() { return Vec::new(); }

    state::wrap_line(&Line::from(spans), width)
}

fn settings_line(_state: &Settings, row: &Row, selected: bool, label_width: usize, width: usize) -> Line<'static>
{
    let item = match row
    {
        //A SECTION HEADING CARRIES A RULE OUT TO THE EDGE, WHICH IS WHAT SEPARATES THE GROUPS
        Row::Header(label) => return Line::from(vec!
        [
            Span::styled(format!(" {label} "), theme::TITLE),
            Span::styled("─".repeat(width.saturating_sub(label.width() + 2)), theme::BORDER),
        ]),

        //A BUTTON IS THE WHOLE ROW - IT HAS NO VALUE COLUMN TO LINE UP WITH
        Row::Action(label) =>
        {
            //A BUTTON IS LIVE WHEN IT HAS SOMETHING TO DO: Save WITH EDITED ROWS IN THE BOX, Restart WITH NONE
            let restart = **label == *settings::RESTART_LABEL;
            let live = if restart { !_state.unsaved() } else { _state.unsaved() };
            let armed = restart && _state.confirm;

            let style = match (armed, selected, live)
            {
                (true, _, _) => theme::ERROR,
                (_, true, _) => theme::ACCENT,
                (_, false, true) => theme::TEXT,
                (_, false, false) => theme::DIM,
            };

            let text = match armed
            {
                true => format!("[ {label} \u{b7} press again ]"),
                false => format!("[ {label} ]"),
            };
            let padding = width.saturating_sub(text.width() + 1) / 2;

            let line = Line::from(vec!
            [
                Span::styled(if selected { "▌" } else { " " }, theme::ACCENT),
                Span::raw(" ".repeat(padding)),
                Span::styled(text, style),
            ]);

            return if selected { line.style(theme::SELECTED) } else { line };
        },

        Row::Item(item) => item,
    };

    let mut spans = vec!
    [
        Span::styled(if selected { "▌" } else { " " }, theme::ACCENT),
        Span::styled
        (
            format!(" {:<label_width$}  ", truncate(&item.label, label_width)),
            if selected { theme::ACCENT } else { theme::TEXT },
        ),
    ];

    let value_width = width.saturating_sub(label_width + 3);

    //THE ROW BEING TYPED INTO SHOWS THE TEXT AS IT STANDS, CARET AND ALL - THE STORED VALUE IS BEHIND IT
    match _state.edit.as_ref().filter(|_| selected)
    {
        Some(edit) => spans.push(Span::styled(format!("{}▏", truncate(edit, value_width.saturating_sub(1))), theme::ACCENT)),
        None => spans.extend(value_spans(_state, &item.value, value_width)),
    }

    //AN EDITED ROW IS MARKED UNTIL THE SERVER HAS SAID WHAT IT STORED
    if item.changed { spans.push(Span::styled(" ●", theme::NOTICE)); }

    //AND ONE THE SERVER WILL NOT PICK UP UNTIL IT IS RESTARTED CARRIES THAT ON THE ROW, SAVED OR NOT
    if item.restart { spans.push(Span::styled(" ↻", theme::DIM)); }

    let line = Line::from(spans);

    if selected { line.style(theme::SELECTED) } else { line }
}

fn value_spans(_state: &Settings, value: &Value, _width: usize) -> Vec<Span<'static>>
{
    match value
    {
        Value::Toggle { on: true, .. } => vec![Span::styled("● on", theme::OK)],
        Value::Toggle { on: false, .. } => vec![Span::styled("○ off", theme::DIM)],

        Value::Number(number) => vec![Span::styled(number.to_string(), theme::TEXT)],

        Value::Text(text) if text.is_empty() => vec![Span::styled("(empty)", theme::DIM)],
        Value::Text(text) => vec![Span::styled(truncate(text, _width), theme::TEXT)],

        #[cfg(feature = "client_voice")]
        Value::Volume(percent) =>
        {
            //THE BAR IS THE WHOLE SUPPORTED RANGE, SO 100% SITS EXACTLY IN THE MIDDLE
            let filled = (*percent as usize * SLIDER_WIDTH).div_ceil(voice_options::VOLUME_MAX as usize);

            vec!
            [
                Span::styled("█".repeat(filled), theme::ACCENT),
                Span::styled("░".repeat(SLIDER_WIDTH.saturating_sub(filled)), theme::BORDER),
                Span::styled(format!(" {percent:>3}%"), if *percent == 0 { theme::DIM } else { theme::TEXT }),
            ]
        },

        #[cfg(feature = "client_voice")]
        Value::Device { id, input } =>
        {
            if id.is_empty()
            {
                vec![Span::styled(settings::DEFAULT_DEVICE, theme::DIM)]
            } else
            {
                vec![Span::styled(truncate(&_state.device_label(id, *input), _width), theme::ACCENT)]
            }
        },
    }
}

#[cfg(feature = "client_voice")]
fn picker_line(entry: &DeviceEntry, selected: bool, width: usize) -> Line<'static>
{
    //ENTRY 0 IS THE EMPTY CONFIG VALUE, WHICH MEANS "WHATEVER THE SYSTEM PICKS"
    let (text, style) = if entry.id.is_empty()
    {
        (String::from(settings::DEFAULT_DEVICE), theme::DIM)
    } else
    {
        (truncate(&entry.label, width.saturating_sub(3)), theme::TEXT)
    };

    let line = Line::from(vec!
    [
        Span::styled(if selected { "▌" } else { " " }, theme::ACCENT),
        Span::styled(format!(" {text}"), style),
    ]);

    if selected { line.style(theme::SELECTED) } else { line }
}

#[cfg(not(feature = "client_voice"))]
fn picker_line(_entry: &DeviceEntry, _selected: bool, _width: usize) -> Line<'static> { Line::default() }

fn truncate(text: &str, width: usize) -> String //FIT text INTO width CELLS, ELLIPSIS AND ALL
{
    if text.width() <= width { return text.to_string(); }

    let mut out = String::new();
    let mut used = 0;

    for c in text.chars()
    {
        let next = used + c.to_string().width();

        if next > width.saturating_sub(1) { break; }

        out.push(c);
        used = next;
    }

    out.push('…');
    out
}

fn right_status(_app: &App) -> String
{
    let mut parts: Vec<String> = Vec::new();

    #[cfg(feature = "client_voice")]
    if _app.voice_enabled
    {
        //THE CAPTURE CALLBACK TREATS 0% AS OFF, SO THE STATUS LINE HAD BETTER AGREE
        let off = options::is_muted(None) || voice_options::get_input_volume() == 0;

        parts.push(String::from(if off { "mic off" } else { "mic on" }));
    }

    parts.push(String::from("Ctrl+, settings"));

    format!(" {} ", parts.join(" │ "))
}

//THE PANEL IS THE CHANNEL'S VOICE ROSTER, NOT OUR OWN SESSION - IT IS SHOWN WHETHER OR NOT WE ARE IN IT
fn voice_visible(app: &App) -> bool
{
    !app.voice.is_empty()
}


