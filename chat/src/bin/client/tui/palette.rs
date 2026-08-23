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

use ratatui::text::Span;

use unicode_width::UnicodeWidthStr;

use crate::command::{ self, CommandInfo };

use super::theme;

//CONSTS
pub const MAX_ROWS: usize = 8; //VISIBLE POPUP ROWS

//ENUMS
pub enum PaletteMode
{
    Hidden,                                         //NOTHING TO SHOW
    Menu(Vec<&'static CommandInfo>, usize),         //MATCHING COMMANDS + SELECTION
    Signature(&'static CommandInfo, Option<usize>), //ONE COMMAND + THE PARAMETER BEING TYPED
}

//STRUCTS
pub struct Palette //SLASH-COMMAND AUTOCOMPLETE
{
    pub mode: PaletteMode,
}

//IMPLEMENTATIONS
impl Default for Palette
{
    fn default() -> Self { Self::new() }
}

impl Palette
{
    pub fn new() -> Self
    {
        Self { mode: PaletteMode::Hidden }
    }

    //THE COMMAND MENU IS OPEN (NAVIGABLE + COMPLETABLE)
    pub fn is_active(&self) -> bool { matches!(self.mode, PaletteMode::Menu(..)) }

    //ANYTHING AT ALL IS ON SCREEN (MENU OR PARAMETER HINT)
    pub fn is_visible(&self) -> bool { !matches!(self.mode, PaletteMode::Hidden) }

    //RECOMPUTE FROM THE CURRENT INPUT
    pub fn update(&mut self, input: &str)
    {
        let Some(rest) = input.strip_prefix(command::COMMAND_PREFIX) else
        {
            self.dismiss();
            return;
        };

        match rest.find(char::is_whitespace)
        {
            //STILL TYPING THE COMMAND WORD - FILTER THE LIST
            None =>
            {
                let candidate = rest.to_lowercase();

                let matches = command::COMMAND_LIST.iter()
                    .filter(|info| info.triggers.iter().any(|t| t.to_lowercase().starts_with(&candidate)))
                    .collect::<Vec<&'static CommandInfo>>();

                if matches.is_empty()
                {
                    self.dismiss();
                    return;
                }

                //A FULLY TYPED COMMAND WINS THE SELECTION, OTHERWISE KEEP IT WHERE IT WAS.
                //WITHOUT THIS, "/screens" HIGHLIGHTS "/screen" AND Enter RUNS THE WRONG COMMAND.
                let exact = matches.iter()
                    .position(|info| info.triggers.iter().any(|t| t.eq_ignore_ascii_case(rest)));

                let selected = match (exact, &self.mode)
                {
                    (Some(exact), _) => exact,
                    (None, PaletteMode::Menu(_, selected)) => (*selected).min(matches.len() - 1),
                    (None, _) => 0,
                };

                self.mode = PaletteMode::Menu(matches, selected);
            },

            //COMMAND WORD IS FINISHED - HINT THE PARAMETER THE USER IS ON
            Some(split) =>
            {
                let (word, tail) = rest.split_at(split);

                let Some(info) = command::COMMAND_LIST.iter()
                    .find(|info| info.triggers.iter().any(|t| t.eq_ignore_ascii_case(word))) else
                {
                    self.dismiss();
                    return;
                };

                if info.args.is_empty()
                {
                    self.dismiss();
                    return;
                }

                self.mode = PaletteMode::Signature(info, active_arg(info, tail));
            },
        }
    }

    pub fn dismiss(&mut self)
    {
        self.mode = PaletteMode::Hidden;
    }

    pub fn next(&mut self)
    {
        if let PaletteMode::Menu(matches, selected) = &mut self.mode
        {
            *selected = (*selected + 1) % matches.len();
        }
    }

    pub fn previous(&mut self)
    {
        if let PaletteMode::Menu(matches, selected) = &mut self.mode
        {
            *selected = if *selected == 0 { matches.len() - 1 } else { *selected - 1 };
        }
    }

    pub fn selection(&self) -> Option<&'static CommandInfo>
    {
        match &self.mode
        {
            PaletteMode::Menu(matches, selected) => matches.get(*selected).copied(),
            _ => None,
        }
    }
}

//FUNCTIONS
//PRIVATE
//WHICH PARAMETER THE CARET IS SITTING ON; None ONCE EVERY PARAMETER HAS BEEN GIVEN
fn active_arg(info: &CommandInfo, tail: &str) -> Option<usize>
{
    let given = tail.split_whitespace().count();

    //A TRAILING SPACE MEANS THE USER MOVED ON TO THE NEXT PARAMETER
    let index = if tail.ends_with(char::is_whitespace) { given } else { given.saturating_sub(1) };

    //THE LAST PARAMETER SWALLOWS THE REST OF THE LINE (E.G. A PRIVATE MESSAGE)
    if index >= info.args.len()
    {
        if tail.ends_with(char::is_whitespace) { None } else { Some(info.args.len() - 1) }
    } else
    {
        Some(index)
    }
}

//PUBLIC
pub fn format_arg(arg: &command::CommandArg) -> String //<REQUIRED> / [OPTIONAL]
{
    if arg.required
    {
        format!("<{}>", arg.name.to_lowercase())
    } else
    {
        format!("[{}]", arg.name.to_lowercase())
    }
}

pub fn format_args(info: &CommandInfo) -> String
{
    info.args.iter().map(format_arg).collect::<Vec<String>>().join(" ")
}

//FULL COMMAND SIGNATURE AS PLAIN TEXT - USE THIS TO MEASURE THE COLUMN
pub fn signature(info: &CommandInfo) -> String
{
    let args = format_args(info);
    let separator = if args.is_empty() { "" } else { " " };

    format!("{}{}{separator}{args}", command::COMMAND_PREFIX, info.triggers[0].to_lowercase())
}

pub fn signature_width(info: &CommandInfo) -> usize
{
    signature(info).width()
}

//SAME SIGNATURE, STYLED: REQUIRED PARAMETERS STAND OUT, THE ACTIVE ONE MORE SO
pub fn signature_spans(info: &CommandInfo, active: Option<usize>) -> Vec<Span<'static>>
{
    let mut spans = vec![Span::styled(format!("{}{}", command::COMMAND_PREFIX, info.triggers[0].to_lowercase()), theme::TITLE)];

    for (i, arg) in info.args.iter().enumerate()
    {
        let style = if active == Some(i)
        {
            theme::ARG_ACTIVE
        } else if arg.required
        {
            theme::ARG_REQUIRED
        } else
        {
            theme::ARG_OPTIONAL
        };

        spans.push(Span::raw(" "));
        spans.push(Span::styled(format_arg(arg), style));
    }

    spans
}

pub fn format_shortcut(info: &CommandInfo) -> String
{
    info.shortcut.map(|s| format!("Ctrl+{}", s.to_ascii_uppercase())).unwrap_or_default()
}
