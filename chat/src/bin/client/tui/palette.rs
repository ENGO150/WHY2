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

use crossterm::style::Color;

use unicode_width::UnicodeWidthStr;

use crate::
{
    colors,
    options,
    command::
    {
        self,
        ArgValues,
        CommandArg,
        CommandInfo,
        SubcommandInfo,
    },
};

use super::theme;

//CONSTS
pub const MAX_ROWS: usize = 8; //VISIBLE POPUP ROWS

//ENUMS
pub enum PaletteMode
{
    Hidden,                          //NOTHING TO SHOW
    Menu(Vec<Entry>, usize),         //MATCHING ENTRIES + SELECTION
    Values(Values),                  //THE ANSWERS A PARAMETER ACCEPTS, WHERE THERE IS A KNOWN LIST OF THEM
    Signature(Entry, Option<usize>), //ONE ENTRY + THE PARAMETER BEING TYPED
}

//STRUCTS
//ONE POPUP LINE - A COMMAND, OR ONE ACTION OF A COMMAND THAT TAKES ONE (/server mute).
//AN ACTION SPEAKS FOR ITSELF FROM HERE ON: ITS OWN ARGUMENTS, ITS OWN DESCRIPTION, ITS OWN ROLE
#[derive(Clone, Copy)]
pub struct Entry
{
    pub info: &'static CommandInfo,
    pub sub: Option<&'static SubcommandInfo>,
}

//WHAT MAY GO IN THE PARAMETER THE CARET IS ON. THE COLOR COMMANDS ARE THE REASON THIS EXISTS: THE VOCABULARY IS
//crossterm'S OWN AND IS NOWHERE ON THE SCREEN, SO WITHOUT IT THE ONLY WAY TO LEARN A NAME IS TO GUESS ONE AND BE TOLD NO
pub struct Values
{
    pub arg: &'static CommandArg,
    pub matches: Vec<&'static str>,
    pub selected: usize,
    pub start: usize, //CHAR INDEX WHERE THE HALF-TYPED VALUE BEGINS - COMPLETING REPLACES EVERYTHING FROM HERE ON
}

pub struct Palette //SLASH-COMMAND AUTOCOMPLETE
{
    pub mode: PaletteMode,
}

//IMPLEMENTATIONS
impl Entry
{
    pub fn command(info: &'static CommandInfo) -> Self { Self { info, sub: None } }

    pub fn action(info: &'static CommandInfo, sub: &'static SubcommandInfo) -> Self { Self { info, sub: Some(sub) } }

    pub fn args(&self) -> &'static [CommandArg]
    {
        self.sub.map_or(self.info.args, |sub| sub.args)
    }

    pub fn description(&self) -> &'static str
    {
        self.sub.map_or(self.info.description, |sub| sub.description)
    }

    //ONLY WHOLE COMMANDS CARRY A SHORTCUT - A KEY THAT LANDED ON HALF OF ONE WOULD HAVE NOTHING TO RUN
    pub fn shortcut(&self) -> String
    {
        match self.sub
        {
            Some(_) => String::new(),
            None => self.info.shortcut.map(|s| format!("Ctrl+{}", s.to_ascii_uppercase())).unwrap_or_default(),
        }
    }

    //WHAT THE USER TYPES TO GET HERE, WITHOUT THE PARAMETERS (/server mute)
    pub fn name(&self) -> String
    {
        let mut name = format!("{}{}", command::COMMAND_PREFIX, self.info.triggers[0].to_lowercase());

        if let Some(sub) = self.sub { name.push_str(&format!(" {}", sub.triggers[0].to_lowercase())); }

        name
    }

    //FULL SIGNATURE AS PLAIN TEXT - USE THIS TO MEASURE THE COLUMN
    pub fn signature(&self) -> String
    {
        let args = self.args().iter().map(format_arg).collect::<Vec<String>>().join(" ");
        let separator = if args.is_empty() { "" } else { " " };

        format!("{}{separator}{args}", self.name())
    }

    pub fn width(&self) -> usize { self.signature().width() }

    //SAME SIGNATURE, STYLED: REQUIRED PARAMETERS STAND OUT, THE ACTIVE ONE MORE SO
    pub fn spans(&self, active: Option<usize>) -> Vec<Span<'static>>
    {
        let mut spans = vec![Span::styled(self.name(), theme::TITLE)];

        for (i, arg) in self.args().iter().enumerate()
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

    //THE ENTRY IS ALREADY SPELLED OUT ON THE LINE, SO Enter SENDS IT INSTEAD OF COMPLETING IT
    pub fn typed(&self, input: &str) -> bool
    {
        let Some(rest) = input.trim().strip_prefix(command::COMMAND_PREFIX) else { return false };

        match self.sub
        {
            None => self.info.triggers.iter().any(|t| t.eq_ignore_ascii_case(rest)),

            //BOTH WORDS HAVE TO BE THERE - THE COMMAND WORD ALONE IS NOT THIS ENTRY
            Some(sub) => match rest.split_once(char::is_whitespace)
            {
                Some((word, action)) => self.info.triggers.iter().any(|t| t.eq_ignore_ascii_case(word)) &&
                    sub.triggers.iter().any(|t| t.eq_ignore_ascii_case(action.trim())),

                None => false,
            },
        }
    }
}

impl Values
{
    pub fn selection(&self) -> Option<&'static str> { self.matches.get(self.selected).copied() }

    //THE HIGHLIGHTED VALUE IS ALREADY SPELLED OUT ON THE LINE, SO Enter SENDS IT INSTEAD OF COMPLETING IT
    pub fn typed(&self, input: &str) -> bool
    {
        let typed = input.chars().skip(self.start).collect::<String>();

        self.selection().is_some_and(|value| value.eq_ignore_ascii_case(typed.trim()))
    }

    //THE STYLE OF THE SWATCH DRAWN BESIDE A ROW - A NAME IS NOT WORTH MUCH IF IT DOES NOT SHOW ITS OWN COLOR
    pub fn swatch(&self, value: &str) -> Option<Color>
    {
        match self.arg.values
        {
            ArgValues::Colors => colors::by_name(value),
            ArgValues::Free => None,
        }
    }
}

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

    //A MENU IS OPEN (NAVIGABLE + COMPLETABLE) - EITHER OF COMMANDS OR OF WHAT ONE PARAMETER ACCEPTS
    pub fn is_active(&self) -> bool { matches!(self.mode, PaletteMode::Menu(..) | PaletteMode::Values(..)) }

    pub fn values(&self) -> Option<&Values>
    {
        match &self.mode
        {
            PaletteMode::Values(values) => Some(values),
            _ => None,
        }
    }

    //ANYTHING AT ALL IS ON SCREEN (MENU OR PARAMETER HINT)
    pub fn is_visible(&self) -> bool { !matches!(self.mode, PaletteMode::Hidden) }

    //RECOMPUTE FROM THE CURRENT INPUT (role HIDES WHAT WE ARE NOT ALLOWED TO RUN)
    pub fn update(&mut self, input: &str, role: usize)
    {
        //THE INPUT LINE BELONGS TO THE LOGIN PROMPT UNTIL AUTH IS DONE - COMMANDS ARE NOT DISPATCHED YET EITHER
        if !options::get_sending_messages()
        {
            self.dismiss();
            return;
        }

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
                    .filter(|info| info.available(role) && info.triggers.iter().any(|t| t.to_lowercase().starts_with(&candidate)))
                    .map(Entry::command).collect::<Vec<Entry>>();

                self.menu(matches, rest);
            },

            //COMMAND WORD IS FINISHED - HAND THE REST OF THE LINE TO ITS ACTIONS, OR HINT THE PARAMETER THE USER IS ON
            Some(split) =>
            {
                let (word, tail) = rest.split_at(split);

                let Some(info) = command::COMMAND_LIST.iter()
                    .find(|info| info.available(role) && info.triggers.iter().any(|t| t.eq_ignore_ascii_case(word))) else
                {
                    self.dismiss();
                    return;
                };

                //A COMMAND THAT TAKES AN ACTION HAS NOTHING OF ITS OWN TO HINT - THE ACTION OWNS EVERYTHING PAST IT
                if !info.subcommands.is_empty()
                {
                    self.action(info, tail.trim_start(), role, input);
                    return;
                }

                if info.args.is_empty()
                {
                    self.dismiss();
                    return;
                }

                self.hint(Entry::command(info), tail, input);
            },
        }
    }

    //THE ACTION WORD OF /command <action> ... - A MENU WHILE IT IS BEING TYPED, ITS PARAMETERS ONCE IT IS DONE
    fn action(&mut self, info: &'static CommandInfo, tail: &str, role: usize, input: &str)
    {
        match tail.find(char::is_whitespace)
        {
            //STILL TYPING THE ACTION - FILTER WHAT OUR ROLE MAY RUN
            None =>
            {
                let candidate = tail.to_lowercase();

                let matches = info.actions(role)
                    .filter(|sub| sub.triggers.iter().any(|t| t.to_lowercase().starts_with(&candidate)))
                    .map(|sub| Entry::action(info, sub)).collect::<Vec<Entry>>();

                self.menu(matches, tail);
            },

            Some(split) =>
            {
                let (action, tail) = tail.split_at(split);

                //AN ACTION OUT OF OUR REACH IS NOT HINTED EITHER - IT IS NOT SUPPOSED TO BE THERE AT ALL
                let Some(sub) = info.action(action).filter(|sub| sub.available(role)) else
                {
                    self.dismiss();
                    return;
                };

                if sub.args.is_empty()
                {
                    self.dismiss();
                    return;
                }

                self.hint(Entry::action(info, sub), tail, input);
            },
        }
    }

    //THE PARAMETER THE CARET IS ON: ITS OWN ANSWERS WHERE IT HAS A CLOSED SET OF THEM, OTHERWISE THE PLAIN SIGNATURE HINT
    fn hint(&mut self, entry: Entry, tail: &str, input: &str)
    {
        let args = entry.args();
        let active = active_arg(args, tail);

        if let Some(arg) = active.and_then(|i| args.get(i)) && arg.values != ArgValues::Free
        {
            let typed = partial(tail).to_lowercase();

            let matches = vocabulary(arg.values).into_iter()
                .filter(|value| value.starts_with(&typed)).collect::<Vec<&'static str>>();

            //A TYPO IS NOT A REASON TO GO BLANK - THE SIGNATURE HINT BELOW STILL SAYS WHAT THE PARAMETER IS
            if !matches.is_empty()
            {
                //A FULLY TYPED VALUE WINS THE SELECTION, OTHERWISE KEEP IT WHERE IT WAS (SAME RULE AS THE COMMAND MENU)
                let exact = matches.iter().position(|value| value.eq_ignore_ascii_case(&typed));

                let selected = match (exact, &self.mode)
                {
                    (Some(exact), _) => exact,
                    (None, PaletteMode::Values(values)) => values.selected.min(matches.len() - 1),
                    (None, _) => 0,
                };

                self.mode = PaletteMode::Values(Values
                {
                    arg,
                    matches,
                    selected,
                    start: input.chars().count() - typed.chars().count(),
                });

                return;
            }
        }

        self.mode = PaletteMode::Signature(entry, active);
    }

    //SHOW matches, KEEPING THE SELECTION WHERE IT WAS UNLESS typed SPELLS ONE OF THEM OUT IN FULL
    fn menu(&mut self, matches: Vec<Entry>, typed: &str)
    {
        if matches.is_empty()
        {
            self.dismiss();
            return;
        }

        //A FULLY TYPED WORD WINS THE SELECTION, OTHERWISE KEEP IT WHERE IT WAS.
        //WITHOUT THIS, "/screens" HIGHLIGHTS "/screen" AND Enter RUNS THE WRONG COMMAND.
        let exact = matches.iter().position(|entry| match entry.sub
        {
            Some(sub) => sub.triggers.iter().any(|t| t.eq_ignore_ascii_case(typed)),
            None => entry.info.triggers.iter().any(|t| t.eq_ignore_ascii_case(typed)),
        });

        let selected = match (exact, &self.mode)
        {
            (Some(exact), _) => exact,
            (None, PaletteMode::Menu(_, selected)) => (*selected).min(matches.len() - 1),
            (None, _) => 0,
        };

        self.mode = PaletteMode::Menu(matches, selected);
    }

    pub fn dismiss(&mut self)
    {
        self.mode = PaletteMode::Hidden;
    }

    pub fn next(&mut self)
    {
        match &mut self.mode
        {
            PaletteMode::Menu(matches, selected) => *selected = (*selected + 1) % matches.len(),
            PaletteMode::Values(values) => values.selected = (values.selected + 1) % values.matches.len(),

            _ => {},
        }
    }

    pub fn previous(&mut self)
    {
        match &mut self.mode
        {
            PaletteMode::Menu(matches, selected) =>
                *selected = if *selected == 0 { matches.len() - 1 } else { *selected - 1 },

            PaletteMode::Values(values) =>
                values.selected = if values.selected == 0 { values.matches.len() - 1 } else { values.selected - 1 },

            _ => {},
        }
    }

    pub fn selection(&self) -> Option<Entry>
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
//WHICH PARAMETER THE CARET IS SITTING ON
fn active_arg(args: &'static [CommandArg], tail: &str) -> Option<usize>
{
    let given = tail.split_whitespace().count();

    //A TRAILING SPACE MEANS THE USER MOVED ON TO THE NEXT PARAMETER
    let index = if tail.ends_with(char::is_whitespace) { given } else { given.saturating_sub(1) };

    //THE LAST PARAMETER SWALLOWS THE REST OF THE LINE (E.G. A PRIVATE MESSAGE), SO THERE IS
    //NEVER A PARAMETER BEYOND IT TO ADVANCE TO - KEEP IT ACTIVE NO MATTER HOW MUCH MORE IS TYPED
    Some(index.min(args.len() - 1))
}

//THE HALF-TYPED VALUE THE CARET IS ON - EMPTY ONCE THE USER HAS MOVED ON TO THE NEXT PARAMETER
fn partial(tail: &str) -> &str
{
    if tail.ends_with(char::is_whitespace) { "" } else { tail.split_whitespace().next_back().unwrap_or("") }
}

//THE ANSWERS THEMSELVES, READ WHERE THEY ARE ALREADY DEFINED RATHER THAN SPELLED OUT A SECOND TIME
fn vocabulary(values: ArgValues) -> Vec<&'static str>
{
    match values
    {
        ArgValues::Colors => colors::COLORS.iter().map(|(name, _)| *name).collect(),
        ArgValues::Free => Vec::new(),
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

