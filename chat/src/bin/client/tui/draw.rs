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
    buffer::{ Cell, CellDiffOption },
    style::{ Color, Modifier, Style },
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

use ratatui_image::{ CropOptions, FontSize, Resize, ResizeEncodeRender };

use crate::
{
    t,
    i18n,
    config,
    options,
    role::Role,
    consts as chat_consts,
};

#[cfg(feature = "client_voice")]
use crate::network::voice::
{
    consts as voice_consts,
    client::options as voice_options,
};

use super::
{
    consts,
    theme,
    state::{ self, App },
    palette::
    {
        Entry,
        Values,
        PaletteMode,
    },
    tofu::
    {
        Prompt,
        Stage,
    },
    settings::
    {
        Mode,
        Row,
        Value,
        Settings,
        DeviceEntry,
    },
    login::
    {
        Login,
        Reconnect,
        Stage as LoginStage,
    },
    account::{ Account, Kind },
    input::InputBuffer,
};

//PROJECT LOGO WATERMARK
const LOGO: &str = include_str!("./assets/rexlogo");

//ENUMS
enum Panel //SIDEBAR SECTIONS, IN THE ORDER THEY ARE STACKED
{
    Online,
    Offline,
    Channels,
    Voice,
}

//PUBLIC
pub fn draw(frame: &mut Frame, app: &mut App)
{
    let area = frame.area();

    //NO INPUT BAR OR SIDEBAR BEFORE LOGIN
    let connecting = app.login.is_some();

    //PAINT THE BASE COLOURS FIRST
    frame.buffer_mut().set_style(area, theme::base());

    //MEASURE THE INPUT FIRST
    let input_width = area.width.saturating_sub(4).max(1); //BORDERS + "> "
    let (input_lines, cursor) = app.input.render(input_width, false);
    let input_height = if connecting { 0 }
        else { (input_lines.len() as u16 + 2).clamp(consts::INPUT_MIN_HEIGHT, consts::INPUT_MAX_HEIGHT) };

    let [main_area, input_area] = Layout::vertical
    ([
        Constraint::Min(consts::INPUT_MIN_HEIGHT),
        Constraint::Length(input_height),
    ]).areas(area);

    //MESSAGES + SIDEBAR
    let (messages_area, sidebar_area) = if area.width >= consts::SIDEBAR_MIN_TERM_WIDTH && options::get_sending_messages()
    {
        let [m, s] = Layout::horizontal([Constraint::Min(0), Constraint::Length(consts::SIDEBAR_WIDTH)]).areas(main_area);
        (m, Some(s))
    } else
    {
        (main_area, None)
    };

    draw_messages(frame, app, messages_area);

    if let Some(sidebar_area) = sidebar_area { draw_sidebar(frame, app, sidebar_area); }

    if !connecting { draw_input(frame, app, input_area, input_lines, cursor); }

    //LOGO BEHIND EVERYTHING
    if !app.theme.disable_logo { draw_logo(frame, area, app.stripe_bg); }

    //EVERY BOX SAYS WHAT IT COVERED
    let mut overlays: Vec<Rect> = Vec::new();

    //PALETTE OVER THE MESSAGE PANE
    if app.palette.is_visible() { overlays.push(draw_palette(frame, app, messages_area)); }

    //SETTINGS OVERLAY
    if app.settings.open
    {
        let font = app.picker.font_size();

        overlays.push(draw_settings(frame, &mut app.settings, area, font));
    }

    //ACCOUNT FORM
    if let Some(form) = &app.account { overlays.push(draw_account(frame, form, area)); }

    //CONNECT BOX
    if let Some(login) = &app.login { overlays.push(draw_login(frame, login, &app.reconnect, area)); }

    //SERVER-KEY PROMPT ON TOP
    if let Some(prompt) = &app.tofu { overlays.push(draw_tofu(frame, prompt, area)); }

    //PICTURES LAST, WITH WHATEVER A BOX HAS ON THEM PUT BACK ON TOP
    let rewritten = draw_pictures(frame, app, &overlays);

    //AND A PROFILE PICTURE ON TOP OF THE BOX THAT RESERVED THE ROWS FOR IT
    draw_avatar(frame, app, &rewritten);
}

//THE PROFILE PICTURE, INSIDE THE SETTINGS BOX
fn draw_avatar(frame: &mut Frame, app: &mut App, rewritten: &[u16])
{
    let area = app.settings.picture_area;

    if area.width == 0 || area.height == 0 { return; }

    //A BOX ABOVE IT OWNS THOSE CELLS, AND NOTHING DIFFS A PICTURE AWAY
    if app.login.is_some() || app.tofu.is_some() { return; }

    app.load_avatar(area.width);

    if let Some(ready) = app.settings.picture.as_mut() && let Some(protocol) = ready.protocol.as_mut()
    {
        protocol.resize_encode_render(&Resize::Crop(None), area, frame.buffer_mut());

        //A PANE PICTURE REWRITTEN ACROSS OUR ROWS TAKES THEM, SO THEY GO AGAIN AFTER IT
        let marks = rewritten.iter().copied().filter(|y| (area.y..area.y + area.height).contains(y)).map(|y|
        {
            //NEVER THE SAME MARK TWICE IN A ROW
            let times = match app.avatar_marks.iter().find(|(row, _)| *row == y) { Some((_, 1)) => 2, _ => 1 };

            mark(frame, area.x, y, times);

            (y, times)
        }).collect();

        app.avatar_marks = marks;
    }
}

//PRIVATE
fn draw_messages(frame: &mut Frame, app: &mut App, area: Rect)
{
    //TITLE: WHY2 ── NAME ── ADDRESS ── SOCKS5
    let mut parts = vec![String::from("WHY2")];

    if !app.server_name.is_empty() { parts.push(app.server_name.clone()); }
    if !app.address.is_empty() { parts.push(app.address.clone()); }
    if options::socks5_enabled() { parts.push(String::from("SOCKS5")); }

    let title = format!(" {} ", parts.join(" ── "));

    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border())
        .title(Span::styled(title, theme::title()));

    //SCROLLED AWAY - ADVERTISE THE BACKLOG
    if app.scroll.is_some() && app.unread > 0
    {
        block = block.title_bottom(Line::from(Span::styled(format!(" ↓ {} ", t!("pane.unread", count = app.unread)), theme::notice())).right_aligned());
    }

    //TOAST ON THE SAME BORDER
    if let Some(notice) = app.notice()
    {
        block = block.title_bottom(Span::styled(format!(" {notice} "), theme::ok()));
    }

    //AND WHOEVER IS WRITING
    if let Some(typing) = app.typing_line()
    {
        block = block.title_bottom(Span::styled(format!(" {typing} "), theme::dim()));
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 { return; }

    //WRAP OURSELVES SO THE SCROLL OFFSET IS EXACT
    let viewport = inner.height;
    let total = app.wrapped_lines(inner.width).len() as u16;
    let max_offset = total.saturating_sub(viewport);
    let offset = app.scroll.map(|o| o.min(max_offset)).unwrap_or(max_offset);

    //REMEMBER THE PANE'S GEOMETRY FOR CLICKS
    app.pane = inner;
    app.pane_offset = offset;

    let visible = app.wrapped_lines(inner.width)
        .iter()
        .skip(offset as usize)
        .take(viewport as usize)
        .cloned()
        .collect::<Vec<Line<'static>>>();

    frame.render_widget(Paragraph::new(visible), inner);

    //TINT THE STRIPED AND MENTIONING ROWS
    for y in inner.y..inner.y + viewport
    {
        let Some(tint) = app.tint(offset + (y - inner.y)) else { continue };

        for x in inner.x..inner.x + inner.width
        {
            if let Some(cell) = frame.buffer_mut().cell_mut((x, y))
                && (cell.bg == Color::Reset || Some(cell.bg) == theme::background()) { cell.set_style(tint); }
        }
    }

    //PAINT THE DRAG SELECTION
    for y in inner.y..inner.y + viewport
    {
        let Some((first, last)) = app.selection_columns(offset + (y - inner.y)) else { continue };

        for x in first..=last.min(inner.width.saturating_sub(1))
        {
            if let Some(cell) = frame.buffer_mut().cell_mut((inner.x + x, y)) { cell.set_style(theme::selection()); }
        }
    }

    //ONLY THE PICTURES ON SCREEN ARE HELD
    app.load_visible(inner.width, offset, viewport);

    //SHOW THE BACKLOG
    draw_scrollbar(frame, area, total as usize, viewport as usize, offset as usize);
}

//PICTURES GO ON LAST, AND A BOX IS PUT BACK OVER WHATEVER ROW ONE CLAIMS
fn draw_pictures(frame: &mut Frame, app: &mut App, overlays: &[Rect]) -> Vec<u16>
{
    let inner = app.pane;

    if inner.width == 0 || inner.height == 0 { return app.picture_rows_drawn(Vec::new()); }

    let (offset, viewport) = (app.pane_offset, inner.height);

    //WHERE THE BOXES WERE ON THE LAST FRAME
    let previous = app.overlays_drawn(overlays);

    //A BOX THAT MOVED GETS ONE MORE FRAME
    if previous != overlays { app.dirty = true; }

    let mut rows = Vec::new();

    for placement in app.placements(inner.width)
    {
        let bottom = placement.row + placement.height;

        let first = placement.row.max(offset);
        let last = bottom.min(offset + viewport);

        if last <= first { continue; }

        //CROP WHICHEVER END IS FURTHER OFF SCREEN
        let clip_top = offset.saturating_sub(placement.row) > bottom.saturating_sub(offset + viewport);

        let area = Rect
        {
            x: inner.x,
            y: inner.y + (first - offset),
            width: inner.width,
            height: last - first,
        };

        let covered = overlays.iter().any(|overlay| overlay.intersects(area));

        //WHAT A BOX HAS ON THESE CELLS, BEFORE THE PICTURE CLAIMS THE WHOLE ROW
        let kept = if covered { overlay_cells(frame, area, overlays) } else { Vec::new() };

        let (mut drawn, mut encoded) = (false, false);

        if let Some(state::Entry::Image { picture: state::Picture::Ready(ready), .. }) =
            app.messages.get_mut(placement.entry) && let Some(protocol) = ready.protocol.as_mut()
        {
            let resize = Resize::Crop(Some(CropOptions { clip_top, clip_left: false }));

            protocol.resize_encode_render(&resize, area, frame.buffer_mut());

            drawn = true;
            encoded = protocol.last_encoding_result().is_some();
        }

        //A BOX THAT MOVED OR WENT LEAVES GLYPHS ONLY THE ROW'S OWN WRITE CAN RUB OUT
        if drawn && previous != overlays && previous.iter().any(|overlay| overlay.intersects(area))
        {
            replace_rows(frame, area, overlays);
        }

        //A RETRANSMITTED PICTURE SENDS EVERY ROW AGAIN
        if drawn && encoded { resend_rows(frame, app, area, overlays); }

        //WHAT EACH ROW'S FIRST CELL WILL SEND
        if drawn
        {
            for y in area.y..area.y + area.height
            {
                if overlays.iter().any(|overlay| overlay.contains((area.x, y).into())) { continue; }

                if let Some(cell) = frame.buffer_mut().cell((area.x, y)) { rows.push((y, cell.symbol().to_string())); }
            }
        }

        //THE BOX GOES BACK ON TOP OF THE ROW THE PICTURE JUST WROTE
        for (x, y, cell) in kept
        {
            if let Some(target) = frame.buffer_mut().cell_mut((x, y))
            {
                *target = cell;
                target.set_diff_option(CellDiffOption::AlwaysUpdate);
            }
        }
    }

    //THE ROWS THE TERMINAL IS SENT AGAIN
    app.picture_rows_drawn(rows)
}

//THE CELLS A BOX HAS INSIDE area, COPIED OUT
fn overlay_cells(frame: &mut Frame, area: Rect, overlays: &[Rect]) -> Vec<(u16, u16, Cell)>
{
    let mut cells = Vec::new();

    for y in area.y..area.y + area.height
    {
        for x in area.x..area.x + area.width
        {
            if !overlays.iter().any(|overlay| overlay.contains((x, y).into())) { continue; }

            if let Some(cell) = frame.buffer_mut().cell((x, y)) { cells.push((x, y, cell.clone())); }
        }
    }

    cells
}

//ONE CELL CARRIES A WHOLE ROW OF A PICTURE, AND THE DIFF WRITES IT AGAIN ONLY IF IT READS DIFFERENTLY
fn replace_rows(frame: &mut Frame, area: Rect, overlays: &[Rect])
{
    for y in area.y..area.y + area.height
    {
        //A ROW WHOSE FIRST CELL IS A BOX'S IS NOT THE PICTURE'S
        if overlays.iter().any(|overlay| overlay.contains((area.x, y).into())) { continue; }

        let Some(cell) = frame.buffer_mut().cell_mut((area.x, y)) else { continue };

        //SAVING THE CURSOR TWICE IS THE SAME AS SAVING IT ONCE - THE CELL'S WIDTH STAYS THE ONE IT IS FORCED TO
        let symbol = format!("\x1b[s{}", cell.symbol());

        cell.set_symbol(&symbol);
    }
}

//MAKE EACH ROW'S FIRST CELL DIFFER FROM THE LAST FRAME'S
fn resend_rows(frame: &mut Frame, app: &App, area: Rect, overlays: &[Rect])
{
    for y in area.y..area.y + area.height
    {
        if overlays.iter().any(|overlay| overlay.contains((area.x, y).into())) { continue; }

        let Some(cell) = frame.buffer_mut().cell_mut((area.x, y)) else { continue };

        while app.picture_row_sent(y, cell.symbol())
        {
            let symbol = format!("\x1b[s{}", cell.symbol());

            cell.set_symbol(&symbol);
        }
    }
}

//MAKE ONE PICTURE ROW'S FIRST CELL READ DIFFERENTLY
fn mark(frame: &mut Frame, x: u16, y: u16, times: usize)
{
    let Some(cell) = frame.buffer_mut().cell_mut((x, y)) else { return };

    let symbol = format!("{}{}", "\x1b[s".repeat(times), cell.symbol());

    cell.set_symbol(&symbol);
}

//FIRST VISIBLE ROW OF A SCROLLING LIST
fn window(offset: usize, selected: usize, total: usize, visible: usize) -> usize
{
    let max = total.saturating_sub(visible);

    //A SHORT LIST KEEPS WHAT IT CAN
    let gap = consts::SCROLL_GAP.min(visible.saturating_sub(1) / 2);

    let mut first = offset.min(max);

    if selected < first + gap { first = selected.saturating_sub(gap); }

    if selected + gap >= first + visible { first = (selected + gap + 1).saturating_sub(visible); }

    first.min(max)
}

//SCROLLBAR DOWN A BOX'S RIGHT BORDER
fn draw_scrollbar(frame: &mut Frame, area: Rect, total: usize, visible: usize, first: usize)
{
    if total <= visible || visible == 0 || area.width == 0 || area.height < 3 { return; }

    let track = area.height as usize - 2; //THE CORNERS STAY CORNERS

    if track == 0 { return; }

    let max_first = total - visible;

    //ROUND SO THE THUMB IS NEVER EMPTY
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
            cell.set_style(theme::accent());
        } else
        {
            cell.set_symbol("\u{2502}");
            cell.set_style(theme::border());
        }
    }
}

//DRAW THE LOGO ON FREE CELLS ONLY
fn draw_logo(frame: &mut Frame, area: Rect, stripe: Color)
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

            //A PAINTED BACKGROUND IS A CLAIMED CELL, A STRIPE, MENTION OR THEME ONE IS NOT
            let unpainted = cell.bg == Color::Reset || cell.bg == stripe || Some(cell.bg) == theme::mention().bg
                || Some(cell.bg) == theme::background();

            if cell.symbol().trim().is_empty() && unpainted //FREE CELL - THE LOGO OWNS IT OUTRIGHT
            {
                cell.set_char(symbol);
                cell.set_style(theme::logo());
            } else if unpainted //TAKEN, BUT NOTHING IS PAINTED BEHIND IT YET
            {
                cell.set_style(theme::logo_under());
            }
        }
    }
}

fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect)
{
    let limit = area.height.saturating_sub(3).max(3);

    //max_clients BOUNDS THE ONLINE LIST, SO THE OFFLINE ONE TAKES THE REST
    let (mut constraints, mut panels) = match app.offline.is_empty()
    {
        true => (vec![Constraint::Min(3)], vec![Panel::Online]),

        false => (vec![Constraint::Length((online_rows(app) + 2).clamp(3, limit)), Constraint::Min(3)],
            vec![Panel::Online, Panel::Offline]),
    };

    if area.height >= consts::CHANNELS_MIN_HEIGHT && !app.channels.is_empty()
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
            Panel::Offline => draw_offline(frame, app, *area),
            Panel::Channels => draw_channels(frame, app, *area),
            Panel::Voice => draw_voice(frame, app, *area),
        }
    }
}

fn draw_online(frame: &mut Frame, app: &App, area: Rect)
{
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border())
        .title(Span::styled(format!(" {} ", t!("sidebar.online")), theme::title()))
        .title_top(Line::from(Span::styled(format!(" {} ", app.online.len()), theme::title())).right_aligned());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    //ID COLUMN, RIGHT-ALIGNED
    let width = app.online.iter().map(|user| user.id.to_string().len()).max().unwrap_or(1);
    let room = inner.width as usize;

    let me = app.username.clone();
    let mut lines = Vec::new();

    for section in app.online_sections()
    {
        //SECTION HEADER, ITS SIZE ON THE RIGHT
        let label = section_label(section[0].role);
        let count = section.len().to_string();
        let pad = room.saturating_sub(label.width() + count.width() + 1);

        lines.push(Line::from(vec!
        [
            Span::styled(label, theme::dim().add_modifier(Modifier::BOLD)),
            Span::styled(format!("{:pad$}{count} ", ""), theme::dim()),
        ]));

        lines.extend(section.iter().map(|user|
        {
            //OUR OWN ROW STAYS MARKED; EVERYBODY ELSE GETS THEIR COLOR
            let style = match user.username == me
            {
                true => theme::accent(),
                false => app.theme.style(user.username_color),
            }.add_modifier(Modifier::BOLD);

            //WHAT THEY ARE ON, IF THEY SHARE IT
            let device = app.devices.get(&user.username).map(|device| super::device_label(device)).unwrap_or_default();
            let reserved = if device.is_empty() { 0 } else { device.width() + 2 };

            //THE NAME GIVES WAY TO THE DEVICE
            let name = truncate(&user.username, room.saturating_sub(width + 2 + reserved));

            let mut spans = vec!
            [
                Span::styled(format!("{id:>width$}  ", id = user.id), theme::dim()),
                Span::styled(name.clone(), style),
            ];

            //DEVICE ON THE RIGHT EDGE
            if !device.is_empty()
            {
                let pad = room.saturating_sub(width + 3 + name.width() + device.width());

                spans.push(Span::styled(format!("{:pad$}{device} ", ""), theme::dim()));
            }

            Line::from(spans)
        }));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

fn online_rows(app: &App) -> u16 //USERS PLUS SECTION HEADERS
{
    (app.online.len() + app.online_sections().count()) as u16
}

fn section_label(role: Role) -> String //owner -> Owners
{
    let name = role.name();

    i18n::get(&format!("sidebar.roles.{name}")).map(str::to_owned)
        .unwrap_or_else(|| format!("{}{}s", name[..1].to_uppercase(), &name[1..]))
}

fn draw_offline(frame: &mut Frame, app: &App, area: Rect)
{
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border())
        .title(Span::styled(format!(" {} ", t!("sidebar.offline")), theme::title()))
        .title_top(Line::from(Span::styled(format!(" {} ", app.offline.len()), theme::title())).right_aligned());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let room = inner.width as usize;

    let lines = app.offline.iter().map(|(username, color)|
    {
        let name = truncate(username, room);

        //THEIR OWN COLOR, ELSE DIM
        Line::from(match color
        {
            Some(_) => Span::styled(name, app.theme.style(*color).add_modifier(Modifier::BOLD)),
            None => Span::styled(name, theme::dim().add_modifier(Modifier::BOLD)),
        })
    }).collect::<Vec<Line>>();

    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_channels(frame: &mut Frame, app: &App, area: Rect)
{
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border())
        .title(Span::styled(format!(" {} ", t!("sidebar.channels")), theme::title()))
        .title_top(Line::from(Span::styled(format!(" {} ", app.channels.len()), theme::title())).right_aligned());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let current = options::get_channel();

    let lines = app.channels.iter().map(|name|
    {
        let here = current == *name;

        Line::from(vec!
        [
            Span::styled(if here { "▸ " } else { "  " }, theme::accent()),
            Span::styled("#", theme::dim()),
            Span::styled(name.clone(), if here { theme::accent() } else { Style::default() }),
        ])
    }).collect::<Vec<Line>>();

    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_voice(frame: &mut Frame, app: &App, area: Rect)
{
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border())
        .title(Span::styled(format!(" {} ", t!("sidebar.voice")), theme::title()))
        .title_top(Line::from(Span::styled(format!(" {} ", app.voice.len()), theme::title())).right_aligned());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = app.voice.iter().map(|user|
    {
        //MUTE ONLY SHOWS WHILE WE LISTEN
        #[cfg(feature = "client_voice")]
        let muted = app.voice_enabled && options::is_muted(if user.is_local { None } else { Some(user.id) });

        #[cfg(not(feature = "client_voice"))]
        let muted = false;

        let marker = if muted { "✕" } else if user.is_speaking { "●" } else { "○" };
        let style = if muted
        {
            theme::error()
        } else if user.is_speaking
        {
            theme::speaking()
        } else
        {
            theme::dim()
        };

        //NO PING FOR SOMEBODY WE DO NOT RECEIVE
        let latency = match user.latency
        {
            Some(latency) => format!(" {latency}ms"),
            None => String::new(),
        };

        Line::from(vec!
        [
            Span::styled(format!("{marker} {}", user.username), style),
            Span::styled(latency, theme::dim()),
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
        .border_style(theme::border_active())
        .title_bottom(Line::from(Span::styled(left, theme::dim())))
        .title_bottom(Line::from(Span::styled(right_status(app), theme::dim())).right_aligned());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 { return; }

    //"> " GUTTER
    let [gutter, text_area] = Layout::horizontal([Constraint::Length(2), Constraint::Min(0)]).areas(inner);
    frame.render_widget(Paragraph::new(Span::styled("> ", theme::accent())), gutter);

    //SCROLL THE INPUT TO THE CURSOR
    let offset = cursor.1.saturating_sub(text_area.height.saturating_sub(1));
    frame.render_widget(Paragraph::new(lines).scroll((offset, 0)), text_area);

    //NO CARET WHILE AN OVERLAY HAS THE KEYBOARD
    if app.settings.open || app.tofu.is_some() || app.login.is_some() || app.account.is_some() { return; }

    frame.set_cursor_position(Position::new
    (
        text_area.x + cursor.0.min(text_area.width.saturating_sub(1)),
        text_area.y + cursor.1.saturating_sub(offset),
    ));
}

fn draw_palette(frame: &mut Frame, app: &mut App, area: Rect) -> Rect
{
    //ROW COUNT, VISIBLE ROWS AND LABELS
    let (total, selected, title) = match &app.palette.mode
    {
        PaletteMode::Hidden => return Rect::ZERO,

        PaletteMode::Menu(matches, selected) => (matches.len(), *selected, format!(" {} ", t!("palette.commands"))),

        //PARAMETER VALUE LIST
        PaletteMode::Values(values) =>
            (values.matches.len(), values.selected, format!(" {} ", capitalize(values.title()))),

        PaletteMode::Signature(..) => (1, 0, format!(" {} ", t!("palette.parameters"))),
    };

    let rows = total.min(consts::MAX_ROWS);

    //KEEP THE SELECTION IN VIEW
    let first = window(app.palette.offset, selected, total, rows);

    app.palette.offset = first;

    let height = rows as u16 + 2;

    if area.height < height || area.width < 10 { return Rect::ZERO; }

    //POPUP ABOVE THE INPUT
    let popup = Rect
    {
        x: area.x,
        y: area.y + area.height - height,
        width: area.width,
        height,
    };

    frame.render_widget(Clear, popup); //Clear RESETS THE CELLS

    frame.buffer_mut().set_style(popup, theme::base());

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border_active())
        .title(Span::styled(title, theme::title()));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let lines = match &app.palette.mode
    {
        PaletteMode::Values(values) => value_lines(values, rows, first),
        _ => entry_lines(app, rows, first, inner.width as usize),
    };

    frame.render_widget(Paragraph::new(lines), inner);

    draw_scrollbar(frame, popup, total, rows, first);

    popup
}

//ONE COLORED ROW PER ACCEPTED VALUE
fn value_lines(values: &Values, rows: usize, first: usize) -> Vec<Line<'static>>
{
    values.matches.iter().skip(first).take(rows).enumerate().map(|(row, value)|
    {
        let selected = first + row == values.selected;

        let mut spans = vec![Span::styled(if selected { "▌" } else { " " }, theme::accent())];

        //PAINT THE SWATCH AS A BACKGROUND
        if let Some(color) = values.swatch(value)
        {
            spans.push(Span::styled("    ", Style::new().bg(theme::ansi(Color::from_crossterm(color)))));
            spans.push(Span::raw(" "));
        }

        spans.push(Span::raw(value.clone()));

        let line = Line::from(spans);

        if selected { line.style(theme::selected()) } else { line }
    }).collect()
}

//ONE ROW PER COMMAND, OR THE PARAMETER HINT
fn entry_lines(app: &App, rows: usize, first: usize, width: usize) -> Vec<Line<'static>>
{
    //ROWS, PLUS WHICH ONE IS SELECTED
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

    //MEASURE COLUMNS ACROSS VISIBLE ROWS
    let signature_width = entries.iter().map(|(entry, _)| entry.width()).max().unwrap_or(0);
    let shortcut_width = entries.iter().map(|(entry, _)| entry.shortcut().width()).max().unwrap_or(0);

    entries.iter().enumerate().map(|(row, (entry, active))|
    {
        let mut spans = vec![Span::styled(if Some(row) == selected { "▌" } else { " " }, theme::accent())];

        //SHOW THE ACTIVE PARAMETER'S DESCRIPTION
        let description = active.and_then(|i| entry.args().get(i)).map_or(entry.description(), |arg| i18n::text(arg.description));

        spans.extend(entry.spans(*active));
        spans.push(Span::raw(" ".repeat(signature_width - entry.width() + 2)));
        spans.push(Span::styled(description.to_string(), theme::dim()));

        //SHORTCUTS IN THE RIGHT COLUMN
        if shortcut_width > 0
        {
            let used = 1 + signature_width + 2 + description.width();
            let shortcut = entry.shortcut();

            spans.push(Span::raw(" ".repeat(width.saturating_sub(used + shortcut_width + 1))));
            spans.push(Span::styled(format!("{shortcut:>shortcut_width$} "), theme::accent()));
        }

        let line = Line::from(spans);

        if Some(row) == selected { line.style(theme::selected()) } else { line }
    }).collect()
}

//"COLOR" -> "Color"
fn capitalize(name: &str) -> String
{
    let mut chars = name.chars();

    match chars.next()
    {
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
        None => String::new(),
    }
}

//THE /settings OVERLAY
fn draw_settings(frame: &mut Frame, state: &mut Settings, area: Rect, font: FontSize) -> Rect
{
    let width = consts::SETTINGS_WIDTH.min(area.width.saturating_sub(2)).max(1);
    let inner_width = width.saturating_sub(2) as usize;

    if area.height < 5 || inner_width < 12 { return Rect::ZERO; }

    //BOTH MODES SHARE THE BOX
    let (title, total, selected) = match &state.picker
    {
        Some(picker) => (format!(" {} ", picker.title), picker.entries.len(), picker.selected),
        None => (format!(" {} ", state.title()), state.rows.len(), state.selected),
    };

    //WRAP THE SELECTED KEY'S COMMENT
    let hint_lines = match state.picker.is_none().then(|| state.rows.get(state.selected)).flatten()
    {
        Some(row) => description_lines(state, row, inner_width as u16),
        None => Vec::new(),
    };

    //SIZE THE FOOT FOR THE LONGEST COMMENT
    let hint_height = match state.picker.is_some()
    {
        true => 0,
        false => state.rows.iter()
            .map(|row| description_lines(state, row, inner_width as u16).len())
            .max().unwrap_or(0),
    };

    //THE PICTURE CLAIMS ROWS AT THE TOP, AND THE ROWS GET WHAT IS LEFT
    let picture = match state.picker.is_some()
    {
        true => 0,
        false => state.picture_rows(),
    };

    let room = (area.height.saturating_sub(4) as usize).saturating_sub(picture as usize); //BORDERS PLUS A LINE OF AIR TOP AND BOTTOM

    //THE ROWS WIN WHEN THERE IS NO ROOM
    let footer = match hint_height { 0 => 0, height => height + 1 };
    let footer = if room > footer { footer } else { 0 };

    let rows_room = room - footer;

    let visible = match &state.picker
    {
        Some(_) => total.min(consts::MAX_PICKER_ROWS).min(rows_room),
        None => total.min(rows_room),
    }.max(1);

    //OFFSET AND VISIBLE ROW COUNT
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

    //VALUE COLUMN BEHIND THE LONGEST LABEL
    let label_width = state.rows.iter().filter_map(|row| match row
    {
        Row::Item(item) => Some(item.label.width()),
        Row::Header(_) | Row::Action(_) => None,
    }).max().unwrap_or(0).min(inner_width.saturating_sub(consts::SETTINGS_VALUE_WIDTH as usize + 3));

    //LABELS GIVE WAY FIRST ON A NARROW TERMINAL

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

    //THE PICTURE'S ROWS ARE RESERVED, NOT DRAWN INTO
    if picture > 0 { lines.splice(0..0, std::iter::repeat_n(Line::default(), picture as usize)); }

    //DESCRIPTION UNDER A RULE
    if footer > 0
    {
        lines.push(Line::from(Span::styled("\u{2500}".repeat(inner_width), theme::border())));

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

    frame.render_widget(Clear, popup); //Clear RESETS THE CELLS

    frame.buffer_mut().set_style(popup, theme::base());

    let hint = match (state.picker.is_some(), state.edit.is_some())
    {
        (true, _) => t!("hint.settings.picker"),
        (_, true) if state.editing_avatar() => t!("hint.settings.avatar"),
        (_, true) => t!("hint.settings.edit"),

        _ => match state.mode
        {
            Mode::Client => t!("hint.settings.client"),
            Mode::Server => t!("hint.settings.server"),
            Mode::Profile { own: true } => t!("hint.settings.own_profile"),

            //THE ONE THING A PROFILE THAT IS NOT OURS STILL DOES
            Mode::Profile { own: false } => match state.link().is_some()
            {
                true => t!("hint.settings.profile_link"),
                false => t!("hint.settings.profile"),
            },
        },
    };
    let hint = format!(" {hint} ");

    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border_active())
        .title(Span::styled(title, theme::title()))
        .title_bottom(Line::from(Span::styled(hint, theme::dim())).centered());

    //THE PROFILE'S RANK ON THE RIGHT
    if state.profile() && state.picker.is_none()
    {
        block = block.title_top(Line::from(Span::styled(format!(" {} ", state.role), theme::dim())).right_aligned());
    }

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    frame.render_widget(Paragraph::new(lines), inner);

    //WHERE THE PICTURE GOES, CENTRED IN THE ROWS IT CLAIMED
    state.picture_area = match state.picture.as_ref().filter(|_| picture > 0)
    {
        Some(ready) =>
        {
            let (width, height) = state::picture_cells(&ready.frames[ready.current].image,
                inner.width, consts::AVATAR_ROWS, font);

            Rect
            {
                x: inner.x + inner.width.saturating_sub(width) / 2,
                y: inner.y + consts::AVATAR_ROWS.saturating_sub(height) / 2,
                width,
                height,
            }
        },

        None => Rect::ZERO,
    };

    //THE TRACK IS THE ROWS' OWN HEIGHT, NOT THE BOX'S
    draw_scrollbar(frame, Rect { y: popup.y + picture, height: rows_height, ..popup }, total, visible, first);

    popup
}

//SERVER IDENTITY PROMPT
fn draw_tofu(frame: &mut Frame, prompt: &Prompt, area: Rect) -> Rect
{
    let width = consts::TOFU_WIDTH.min(area.width.saturating_sub(2)).max(1);
    let inner_width = width.saturating_sub(4); //BORDERS PLUS A COLUMN OF AIR EACH SIDE

    if area.height < 9 || inner_width < 20 { return Rect::ZERO; }

    let confirming = prompt.stage == Stage::Confirm;

    let warning = match (confirming, prompt.mismatch)
    {
        (true, _) => t!("tofu.warning.confirm"),
        (false, true) => t!("tofu.warning.changed"),
        (false, false) => t!("tofu.warning.unknown"),
    };

    //THE LABEL COLUMN FITS THE LONGEST LABEL
    let labels = [t!("tofu.server"), t!("tofu.pinned"), t!("tofu.new_key"), t!("tofu.key")];
    let label_width = labels.iter().map(|label| label.width()).max().unwrap_or(0) + 2;
    let column = |label: &str| format!("{label:<label_width$}");

    //WRAP THE BODY
    let mut lines = state::wrap_line(&Line::from(Span::styled(warning, theme::notice())), inner_width);

    lines.push(Line::default());
    lines.push(Line::from(vec!
    [
        Span::styled(column(t!("tofu.server")), theme::dim()),
        Span::raw(prompt.host.clone()),
    ]));

    //SHOW BOTH FINGERPRINTS ON A MISMATCH
    for (index, row) in prompt.pinned_fingerprint().into_iter().enumerate()
    {
        lines.push(Line::from(vec!
        [
            Span::styled(column(if index == 0 { t!("tofu.pinned") } else { "" }), theme::dim()),
            Span::styled(row, theme::dim()),
        ]));
    }

    let label = if prompt.mismatch { t!("tofu.new_key") } else { t!("tofu.key") };

    for (index, row) in prompt.fingerprint().into_iter().enumerate()
    {
        lines.push(Line::from(vec!
        [
            Span::styled(column(if index == 0 { label } else { "" }), theme::dim()),
            Span::styled(row, theme::accent()),
        ]));
    }

    lines.push(Line::default());

    if confirming
    {
        let typed = prompt.typed.chars().count();

        lines.append(&mut state::wrap_line(&Line::from(Span::styled(t!("tofu.type_to_replace", word = t!("tofu.challenge")),
            theme::text())), inner_width));

        lines.push(Line::from(vec!
        [
            Span::styled(prompt.typed.clone(), theme::accent()),
            Span::styled("_".repeat(t!("tofu.challenge").chars().count().saturating_sub(typed)), theme::dim()),
        ]).centered());

        if prompt.wrong
        {
            lines.push(Line::from(Span::styled(t!("tofu.type_to_confirm", word = t!("tofu.challenge")),
                theme::error())).centered());
        }
    } else
    {
        lines.push(Line::from(vec!
        [
            button(t!("tofu.reject"), !prompt.accept, theme::error()),
            Span::raw("  "),
            button(if prompt.mismatch { t!("tofu.replace") } else { t!("tofu.trust") }, prompt.accept, theme::ok()),
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

    frame.render_widget(Clear, popup); //Clear RESETS THE CELLS

    frame.buffer_mut().set_style(popup, theme::base());

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::error())
        .title(Span::styled(format!(" {} ", prompt.title()), theme::error()))
        .title_bottom(Line::from(Span::styled(format!(" {} ", if confirming
        {
            t!("hint.tofu.confirm")
        } else { t!("hint.tofu.warn") }), theme::dim())).centered());

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    //ONE COLUMN OF AIR EACH SIDE
    let [_, text_area, _] = Layout::horizontal
    ([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ]).areas(inner);

    frame.render_widget(Paragraph::new(lines), text_area);

    popup
}

fn draw_login(frame: &mut Frame, login: &Login, reconnect: &Reconnect, area: Rect) -> Rect
{
    //STATUS ROW
    let mut notes = vec![match (login.busy, login.error.as_deref(), login.hint.as_deref())
    {
        //A RETRY SAYS SO INSTEAD, SINCE NOBODY ASKED FOR IT
        (true, ..) => Line::from(Span::styled(reconnect.status()
            .unwrap_or_else(|| login.waiting().to_owned()), theme::accent())),
        (false, Some(error), _) => Line::from(Span::styled(error.to_string(), theme::error())),
        (false, None, Some(hint)) => Line::from(Span::styled(hint.to_string(), theme::dim())),
        (false, None, None) => Line::default(),
    }];

    //THE PROXY BELONGS TO THE ADDRESS STEP
    if login.stage == LoginStage::Address && options::socks5_enabled()
    {
        notes.push(Line::from(Span::styled(t!("login.socks5", address = config::read_config::<String>("socks5_addr")),
            theme::dim())));
    }

    let footer = match (login.stage, login.busy, login.cancellable())
    {
        (_, true, true) => t!("hint.login.cancel"),
        (_, true, false) => t!("hint.login.quit"),
        (LoginStage::Address, false, _) => t!("hint.login.connect"),
        (_, false, _) => t!("hint.login.continue"),
    };

    draw_form(frame, area, login.title(), &[(login.label(), &login.input)], login.masked(), notes, footer, (!login.busy).then_some(0))
}

fn draw_account(frame: &mut Frame, form: &Account, area: Rect) -> Rect
{
    let status = match (form.busy, form.armed, form.error.as_deref())
    {
        (true, ..) => Line::from(Span::styled(t!("login.waiting"), theme::accent())),
        (false, true, _) => Line::from(Span::styled(t!("account.delete_warning"), theme::error())),
        (false, false, Some(error)) => Line::from(Span::styled(error.to_string(), theme::error())),
        (false, false, None) => Line::default(),
    };

    let footer = match (form.busy, form.kind, form.armed)
    {
        (true, ..) => "",
        (false, Kind::Passwd, _) => t!("hint.account.passwd"),
        (false, Kind::Delete, false) => t!("hint.account.delete"),
        (false, Kind::Delete, true) => t!("hint.account.confirm"),
    };

    let fields: Vec<(&str, &InputBuffer)> = form.kind.labels().into_iter().zip(form.fields.iter()).collect();

    draw_form(frame, area, form.kind.title(), &fields, true, vec![status], footer, (!form.busy).then_some(form.focus))
}

//A CENTRED BOX WITH LABELLED FIELDS AND A STATUS UNDER THEM
#[allow(clippy::too_many_arguments)]
fn draw_form
(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    fields: &[(&str, &InputBuffer)],
    masked: bool,
    notes: Vec<Line<'static>>,
    footer: &str,
    focus: Option<usize>,
) -> Rect
{
    let width = consts::LOGIN_WIDTH.min(area.width.saturating_sub(2)).max(1);
    let inner_width = width.saturating_sub(4); //BORDERS PLUS A COLUMN OF AIR EACH SIDE
    let field_width = inner_width.saturating_sub(2); //"> " GUTTER

    if area.height < 8 || field_width < 8 { return Rect::ZERO; }

    let mut lines: Vec<Line> = Vec::new();
    let mut caret = None;

    for (index, (label, input)) in fields.iter().enumerate()
    {
        let (field, cursor) = input.render(field_width, masked);
        let focused = focus == Some(index);

        //A BLANK ROW BETWEEN FIELDS
        if index > 0 { lines.push(Line::default()); }

        if focused { caret = Some((cursor.0, lines.len() as u16 + consts::FIELD_ROW + cursor.1)); }

        lines.push(Line::from(Span::styled(*label, theme::dim())));

        for (row, line) in field.into_iter().enumerate()
        {
            let gutter = if row > 0 { "  " } else { "> " };
            let mut spans = vec![Span::styled(gutter, if focused { theme::accent() } else { theme::dim() })];
            spans.extend(line.spans);

            lines.push(Line::from(spans));
        }
    }

    lines.push(Line::default());

    //NOTES, WRAPPED
    for note in &notes { lines.extend(state::wrap_line(note, inner_width)); }

    let height = (lines.len() as u16 + 2).min(area.height);

    let popup = Rect
    {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, popup); //Clear RESETS THE CELLS

    frame.buffer_mut().set_style(popup, theme::base());

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border_active())
        .title(Span::styled(format!(" {title} "), theme::title()))
        .title_bottom(Line::from(Span::styled(if footer.is_empty() { String::new() } else { format!(" {footer} ") }, theme::dim())).centered());

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    //ONE COLUMN OF AIR EACH SIDE
    let [_, text_area, _] = Layout::horizontal
    ([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ]).areas(inner);

    frame.render_widget(Paragraph::new(lines), text_area);

    //THIS BOX KEEPS THE CARET
    if let Some((column, row)) = caret
    {
        frame.set_cursor_position(Position::new
        (
            text_area.x + 2 + column.min(field_width.saturating_sub(1)),
            text_area.y + row,
        ));
    }

    popup
}

fn button(label: &str, selected: bool, style: Style) -> Span<'static>
{
    let label = format!(" {label} ");

    if selected { Span::styled(label, style.patch(theme::selected())) } else { Span::styled(label, theme::dim()) }
}

//WRAP THE SELECTED ROW'S DESCRIPTION
fn description_lines(state: &Settings, row: &Row, width: u16) -> Vec<Line<'static>>
{
    let mut spans = Vec::new();

    //THE PATHS BESIDE A TYPED AVATAR
    if let Row::Item(item) = row && item.key == consts::AVATAR_KEY && state.editing_avatar() && !state.paths.is_empty()
    {
        let visible = state.paths.len().min(consts::MAX_ROWS);
        let first = window(0, state.path, state.paths.len(), visible);

        return state.paths.iter().enumerate().skip(first).take(visible).map(|(index, path)|
        {
            let selected = index == state.path;

            let line = Line::from(vec!
            [
                Span::styled(if selected { "▌ " } else { "  " }, theme::accent()),
                Span::styled(truncate(path, (width as usize).saturating_sub(2)), if selected { theme::accent() } else { theme::text() }),
            ]);

            if selected { line.style(theme::selected()) } else { line }
        }).collect();
    }

    match row
    {
        Row::Header(_) => return Vec::new(),

        //SAY WHAT A BUTTON DOES, OR WHY IT WILL NOT
        Row::Action(label) if **label == *consts::RESTART_LABEL =>
        {
            spans.push(Span::styled(t!("settings.restart_description"), theme::dim()));

            if state.unsaved() { spans.push(Span::styled(format!(" \u{b7} {}", t!("settings.save_first")), theme::notice())); }
            else if state.confirm { spans.push(Span::styled(format!(" \u{b7} {}", t!("settings.press_again_confirm")), theme::error())); }
        },

        Row::Action(_) if state.profile() =>
            spans.push(Span::styled(t!("settings.save_profile_description"), theme::dim())),

        Row::Action(_) => spans.push(Span::styled(t!("settings.save_description"), theme::dim())),

        //A FIELD IS PROSE OR A LINK, SO THE FOOT IS WHERE IT IS READ
        Row::Item(item) if state.profile() => match &item.value
        {
            Value::Avatar(Some(path)) if path.is_empty() =>
                spans.push(Span::styled(t!("profile.avatar.removed"), theme::notice())),

            Value::Avatar(Some(path)) => spans.push(Span::styled(t!("profile.avatar.uploaded", path), theme::text())),

            Value::Avatar(None) => spans.push(Span::styled(t!("profile.avatar.help",
                limit = chat_consts::MAX_IMAGE_SIZE / chat_consts::MEGABYTE), theme::dim())),

            Value::Text(text) if text.is_empty() =>
                spans.push(Span::styled(i18n::get(&format!("profile.empty.{}", item.key)).map(str::to_owned)
                    .unwrap_or_else(|| t!("profile.empty_field", field = item.label.to_lowercase())), theme::dim())),

            Value::Text(text) => spans.push(Span::styled(text.clone(), theme::text())),

            _ => {},
        },

        Row::Item(item) =>
        {
            if !item.hint.is_empty() { spans.push(Span::styled(item.hint.clone(), theme::dim())); }

            //MARK A STARTUP-ONLY KEY
            if item.restart
            {
                let note = match spans.is_empty() { true => t!("settings.restart_required").to_owned(), false => format!(" \u{b7} {}", t!("settings.restart_required")) };

                spans.push(Span::styled(note, theme::notice()));
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
        //SECTION HEADING WITH A RULE
        Row::Header(label) => return Line::from(vec!
        [
            Span::styled(format!(" {label} "), theme::title()),
            Span::styled("─".repeat(width.saturating_sub(label.width() + 2)), theme::border()),
        ]),

        //A BUTTON IS THE WHOLE ROW
        Row::Action(label) =>
        {
            //A BUTTON IS LIVE WHEN IT HAS SOMETHING TO DO
            let restart = **label == *consts::RESTART_LABEL;
            let live = if restart { !_state.unsaved() } else { _state.unsaved() };
            let armed = restart && _state.confirm;

            let style = match (armed, selected, live)
            {
                (true, _, _) => theme::error(),
                (_, true, _) => theme::accent(),
                (_, false, true) => theme::text(),
                (_, false, false) => theme::dim(),
            };

            let label = i18n::text(label);
            let text = match armed
            {
                true => format!("[ {label} \u{b7} {} ]", t!("settings.press_again")),
                false => format!("[ {label} ]"),
            };
            let padding = width.saturating_sub(text.width() + 1) / 2;

            let line = Line::from(vec!
            [
                Span::styled(if selected { "▌" } else { " " }, theme::accent()),
                Span::raw(" ".repeat(padding)),
                Span::styled(text, style),
            ]);

            return if selected { line.style(theme::selected()) } else { line };
        },

        Row::Item(item) => item,
    };

    let mut spans = vec!
    [
        Span::styled(if selected { "▌" } else { " " }, theme::accent()),
        Span::styled
        (
            format!(" {:<label_width$}  ", truncate(&item.label, label_width)),
            if selected { theme::accent() } else { theme::text() },
        ),
    ];

    let value_width = width.saturating_sub(label_width + 3);

    //SHOW THE TEXT BEING TYPED, CARET AND ALL
    match _state.edit.as_ref().filter(|_| selected)
    {
        Some(edit) => spans.push(Span::styled(format!("{}▏", truncate(edit, value_width.saturating_sub(1))), theme::accent())),
        None => spans.extend(value_spans(_state, &item.value, value_width)),
    }

    //MARK AN EDITED ROW
    if item.changed { spans.push(Span::styled(" ●", theme::notice())); }

    //MARK A ROW NEEDING A RESTART
    if item.restart { spans.push(Span::styled(" ↻", theme::dim())); }

    let line = Line::from(spans);

    if selected { line.style(theme::selected()) } else { line }
}

fn value_spans(_state: &Settings, value: &Value, _width: usize) -> Vec<Span<'static>>
{
    match value
    {
        Value::Toggle { on: true, .. } => vec![Span::styled(format!("● {}", t!("settings.on")), theme::ok())],
        Value::Toggle { on: false, .. } => vec![Span::styled(format!("○ {}", t!("settings.off")), theme::dim())],

        Value::Number(number) => vec![Span::styled(number.to_string(), theme::text())],

        Value::Text(text) if text.is_empty() => vec![Span::styled(format!("({})", t!("settings.empty")), theme::dim())],
        Value::Text(text) => vec![Span::styled(truncate(text, _width), theme::text())],

        Value::Avatar(None) if _state.avatar.is_some() => vec![Span::styled(t!("profile.avatar.set"), theme::text())],
        Value::Avatar(None) => vec![Span::styled(format!("({})", t!("profile.avatar.none")), theme::dim())],
        Value::Avatar(Some(path)) if path.is_empty() => vec![Span::styled(t!("profile.avatar.remove"), theme::notice())],
        Value::Avatar(Some(path)) => vec![Span::styled(truncate(path, _width), theme::text())],

        Value::Theme(index) => vec![Span::styled(format!("◂ {} ▸", theme::PALETTES[*index].name), theme::accent())],
        Value::Language(code) => vec![Span::styled(format!("◂ {} ▸", i18n::language_name(code)), theme::accent())],

        #[cfg(feature = "client_voice")]
        Value::Volume(percent) =>
        {
            //THE BAR IS THE WHOLE RANGE
            let filled = (*percent as usize * consts::SLIDER_WIDTH).div_ceil(voice_consts::VOLUME_MAX as usize);

            vec!
            [
                Span::styled("█".repeat(filled), theme::accent()),
                Span::styled("░".repeat(consts::SLIDER_WIDTH.saturating_sub(filled)), theme::border()),
                Span::styled(format!(" {percent:>3}%"), if *percent == 0 { theme::dim() } else { theme::text() }),
            ]
        },

        #[cfg(feature = "client_voice")]
        Value::Device { id, input } =>
        {
            if id.is_empty()
            {
                vec![Span::styled(t!("settings.default_device"), theme::dim())]
            } else
            {
                vec![Span::styled(truncate(&_state.device_label(id, *input), _width), theme::accent())]
            }
        },
    }
}

#[cfg(feature = "client_voice")]
fn picker_line(entry: &DeviceEntry, selected: bool, width: usize) -> Line<'static>
{
    //ENTRY 0 IS THE SYSTEM DEFAULT
    let (text, style) = if entry.id.is_empty()
    {
        (t!("settings.default_device").to_owned(), theme::dim())
    } else
    {
        (truncate(&entry.label, width.saturating_sub(3)), theme::text())
    };

    let line = Line::from(vec!
    [
        Span::styled(if selected { "▌" } else { " " }, theme::accent()),
        Span::styled(format!(" {text}"), style),
    ]);

    if selected { line.style(theme::selected()) } else { line }
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
        //0% IS OFF
        let off = options::is_muted(None) || voice_options::get_input_volume() == 0;

        parts.push(String::from(if off { t!("status.mic_off") } else { t!("status.mic_on") }));
    }

    parts.push(t!("status.settings").to_owned());

    format!(" {} ", parts.join(" │ "))
}

//THE CHANNEL'S VOICE ROSTER
fn voice_visible(app: &App) -> bool
{
    !app.voice.is_empty()
}


