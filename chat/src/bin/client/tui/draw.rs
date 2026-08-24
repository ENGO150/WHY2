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
    command::CommandInfo,
    options::{ self, LoginState },
};

#[cfg(feature = "client_voice")]
use crate::network::voice::client::options as voice_options;

use super::
{
    theme,
    state::{ self, App },
    palette::{ self, PaletteMode },
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
};

//CONSTS
const SIDEBAR_WIDTH: u16          = 24;
const SIDEBAR_MIN_TERM_WIDTH: u16 = 70; //BELOW THIS THE SIDEBAR IS DROPPED AND MESSAGES GO FULL-WIDTH
const INPUT_MIN_HEIGHT: u16       = 3;
const INPUT_MAX_HEIGHT: u16       = 8;
const CHANNELS_MIN_HEIGHT: u16    = 12; //SIDEBAR ROWS NEEDED BEFORE THE CHANNEL LIST IS WORTH SHOWING
const SETTINGS_WIDTH: u16         = 62; //SETTINGS OVERLAY, CAPPED TO THE TERMINAL
const TOFU_WIDTH: u16             = 64; //SERVER IDENTITY OVERLAY, CAPPED TO THE TERMINAL
const SETTINGS_VALUE_WIDTH: u16   = 20; //NARROWEST THE VALUE COLUMN MAY GET (BAR + PERCENTAGE)

#[cfg(feature = "client_voice")]
const SLIDER_WIDTH: usize         = 14; //CELLS OF VOLUME BAR

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

    //THE SETTINGS OVERLAY COVERS EVERYTHING ELSE (THE INPUT CURSOR IS SUPPRESSED IN draw_input)
    if app.settings.open { draw_settings(frame, &app.settings, area); }

    //...AND THE IDENTITY PROMPT COVERS THE SETTINGS OVERLAY, BECAUSE IT IS THE ONLY THING THE USER MAY ANSWER
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

    //NO CARET WHILE AN OVERLAY OWNS THE KEYBOARD
    if app.settings.open || app.tofu.is_some() { return; }

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
        PaletteMode::Hidden => return,

        PaletteMode::Menu(matches, selected) =>
        {
            //KEEP THE SELECTION IN VIEW
            let first = selected.saturating_sub(palette::MAX_ROWS.saturating_sub(1));

            let entries = matches.iter().copied()
                .skip(first)
                .take(palette::MAX_ROWS)
                .map(|info| (info, None))
                .collect::<Vec<(&'static CommandInfo, Option<usize>)>>();

            (entries, Some(selected - first), " Commands ")
        },

        PaletteMode::Signature(info, active) => (vec![(*info, *active)], None, " Parameters "),
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

        //THE ACTIVE PARAMETER'S OWN DESCRIPTION TAKES OVER THE COLUMN WHILE IT'S BEING TYPED
        let description = active.and_then(|i| info.args.get(i)).map_or(info.description, |arg| arg.description);

        spans.extend(palette::signature_spans(info, *active));
        spans.push(Span::raw(" ".repeat(signature_width - palette::signature_width(info) + 2)));
        spans.push(Span::styled(description.to_string(), theme::DIM));

        //SHORTCUTS HUG THE RIGHT EDGE, IN THEIR OWN COLUMN
        if shortcut_width > 0
        {
            let used = 1 + signature_width + 2 + description.width();
            let shortcut = palette::format_shortcut(info);

            spans.push(Span::raw(" ".repeat((inner.width as usize).saturating_sub(used + shortcut_width + 1))));
            spans.push(Span::styled(format!("{shortcut:>shortcut_width$} "), theme::ACCENT));
        }

        let line = Line::from(spans);

        if Some(row) == selected { line.style(theme::SELECTED) } else { line }
    }).collect::<Vec<Line>>();

    frame.render_widget(Paragraph::new(lines), inner);
}

//THE /settings OVERLAY - ONE CENTERED BOX, EITHER THE SETTING ROWS OR THE DEVICE PICKER ON TOP OF THEM
fn draw_settings(frame: &mut Frame, state: &Settings, area: Rect)
{
    let width = SETTINGS_WIDTH.min(area.width.saturating_sub(2)).max(1);
    let inner_width = width.saturating_sub(2) as usize;

    if area.height < 5 || inner_width < 12 { return; }

    //THE PICKER BORROWS THE SAME BOX, SO BOTH MODES ARE MEASURED THE SAME WAY
    let (title, total, selected) = match &state.picker
    {
        Some(picker) => (picker.title, picker.entries.len(), picker.selected),
        None => (" Settings ", state.rows.len(), state.selected),
    };

    let room = area.height.saturating_sub(4) as usize; //BORDERS PLUS A LINE OF AIR TOP AND BOTTOM
    let visible = match &state.picker
    {
        Some(_) => total.min(settings::MAX_PICKER_ROWS).min(room),
        None => total.min(room),
    }.max(1);

    //KEEP THE SELECTION IN VIEW
    let first = selected.saturating_sub(visible.saturating_sub(1)).min(total.saturating_sub(visible));

    //THE VALUE COLUMN STARTS RIGHT BEHIND THE LONGEST LABEL, NOT AT SOME GUESSED OFFSET
    let label_width = state.rows.iter().filter_map(|row| match row
    {
        Row::Item(item) => Some(item.label.width()),
        Row::Header(_) => None,
    }).max().unwrap_or(0).min(inner_width.saturating_sub(SETTINGS_VALUE_WIDTH as usize + 3));

    //ON A NARROW TERMINAL THE LABELS GIVE WAY FIRST - THE VALUES ARE WHAT THE USER IS HERE FOR

    let lines = match &state.picker
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

    let hint = match &state.picker
    {
        Some(_) => " ↑↓ select │ ⏎ apply │ Esc back ",
        None => " ↑↓ move │ ←→ change │ ⏎ select │ Esc close ",
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::BORDER_ACTIVE)
        .title(Span::styled(title, theme::TITLE))
        .title_bottom(Line::from(Span::styled(hint, theme::DIM)).centered());

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    frame.render_widget(Paragraph::new(lines), inner);
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

    let height = lines.len() as u16 + 2;

    let popup = Rect
    {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height: height.min(area.height),
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

fn button(label: &'static str, selected: bool, style: Style) -> Span<'static>
{
    if selected { Span::styled(label, style.patch(theme::SELECTED)) } else { Span::styled(label, theme::DIM) }
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

        Row::Item(item) => item,
    };

    let mut spans = vec!
    [
        Span::styled(if selected { "▌" } else { " " }, theme::ACCENT),
        Span::styled
        (
            format!(" {:<label_width$}  ", truncate(item.label, label_width)),
            if selected { theme::ACCENT } else { theme::TEXT },
        ),
    ];

    spans.extend(value_spans(_state, &item.value, width.saturating_sub(label_width + 3)));

    let line = Line::from(spans);

    if selected { line.style(theme::SELECTED) } else { line }
}

fn value_spans(_state: &Settings, value: &Value, _width: usize) -> Vec<Span<'static>>
{
    match value
    {
        Value::Toggle { on: true, .. } => vec![Span::styled("● on", theme::OK)],
        Value::Toggle { on: false, .. } => vec![Span::styled("○ off", theme::DIM)],

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

fn voice_visible(app: &App) -> bool
{
    app.voice_enabled && !app.voice.is_empty()
}


