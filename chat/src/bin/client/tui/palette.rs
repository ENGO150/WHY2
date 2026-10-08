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

use std::
{
    ffi::OsStr,
    fs,
    iter,
    path::
    {
        Path,
        PathBuf,
    },
};

use image::ImageFormat;

use ratatui::text::Span;

use crossterm::style::Color;

use unicode_width::UnicodeWidthStr;

use crate::
{
    colors,
    i18n,
    options,
    role::Role,
    command::
    {
        self,
        ArgValues,
        CommandArg,
        CommandInfo,
        SubcommandInfo,
    },
};

use crate::t;

use super::
{
    consts,
    theme,
};

//ENUMS
pub enum PaletteMode
{
    Hidden,                          //NOTHING TO SHOW
    Menu(Vec<Entry>, usize),         //MATCHING ENTRIES + SELECTION
    Values(Values),                  //THE ANSWERS A PARAMETER ACCEPTS
    Signature(Entry, Option<usize>), //ONE ENTRY + THE PARAMETER BEING TYPED
}

//STRUCTS
//ONE POPUP LINE - A COMMAND OR ONE OF ITS ACTIONS
#[derive(Clone, Copy)]
pub struct Entry
{
    pub info: &'static CommandInfo,
    pub sub: Option<&'static SubcommandInfo>,
}

//WHAT MAY GO IN THE PARAMETER THE CARET IS ON
pub struct Values
{
    pub arg: Option<&'static CommandArg>, //NONE FOR A MENTION

    //NOT &'static str: MONITORS ARE RUNTIME-ONLY
    pub matches: Vec<String>,
    pub selected: usize,
    pub start: usize, //CHAR INDEX WHERE THE HALF-TYPED VALUE BEGINS
}

pub struct Palette //SLASH-COMMAND AUTOCOMPLETE
{
    pub mode: PaletteMode,

    //FIRST VISIBLE ROW, WRITTEN BY THE DRAW PATH
    pub offset: usize,
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
        i18n::text(self.sub.map_or(self.info.description, |sub| sub.description))
    }

    //ONLY WHOLE COMMANDS CARRY A SHORTCUT
    pub fn shortcut(&self) -> String
    {
        match self.sub
        {
            Some(_) => String::new(),
            None => self.info.shortcut.map(|s| t!("keys.ctrl", key = s.to_ascii_uppercase())).unwrap_or_default(),
        }
    }

    //WHAT THE USER TYPES TO GET HERE
    pub fn name(&self) -> String
    {
        let mut name = format!("{}{}", command::COMMAND_PREFIX, self.info.triggers[0].to_lowercase());

        if let Some(sub) = self.sub { name.push_str(&format!(" {}", sub.triggers[0].to_lowercase())); }

        name
    }

    //FULL SIGNATURE AS PLAIN TEXT, FOR MEASURING
    pub fn signature(&self) -> String
    {
        let args = self.args().iter().map(format_arg).collect::<Vec<String>>().join(" ");
        let separator = if args.is_empty() { "" } else { " " };

        format!("{}{separator}{args}", self.name())
    }

    pub fn width(&self) -> usize { self.signature().width() }

    //THE SAME SIGNATURE, STYLED
    pub fn spans(&self, active: Option<usize>) -> Vec<Span<'static>>
    {
        let mut spans = vec![Span::styled(self.name(), theme::title())];

        for (i, arg) in self.args().iter().enumerate()
        {
            let style = if active == Some(i)
            {
                theme::arg_active()
            } else if arg.required
            {
                theme::arg_required()
            } else
            {
                theme::arg_optional()
            };

            spans.push(Span::raw(" "));
            spans.push(Span::styled(format_arg(arg), style));
        }

        spans
    }

    //ALREADY SPELLED OUT, SO Enter SENDS IT
    pub fn typed(&self, input: &str) -> bool
    {
        let Some(rest) = input.trim().strip_prefix(command::COMMAND_PREFIX) else { return false };

        match self.sub
        {
            None => self.info.triggers.iter().any(|t| t.eq_ignore_ascii_case(rest)),

            //BOTH WORDS HAVE TO BE THERE
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
    pub fn selection(&self) -> Option<&str> { self.matches.get(self.selected).map(String::as_str) }

    //ALREADY SPELLED OUT, SO Enter SENDS IT
    pub fn typed(&self, input: &str) -> bool
    {
        let typed = input.chars().skip(self.start).collect::<String>();

        self.selection().is_some_and(|value| value.eq_ignore_ascii_case(typed.trim()))
    }

    //THE SWATCH DRAWN BESIDE A ROW
    pub fn swatch(&self, value: &str) -> Option<Color>
    {
        match self.arg.map(|arg| arg.values)
        {
            Some(ArgValues::Colors) => colors::by_name(value),
            _ => None,
        }
    }

    pub fn title(&self) -> &'static str
    {
        self.arg.map_or(t!("palette.mentions"), |arg| i18n::text(arg.name))
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
        Self { mode: PaletteMode::Hidden, offset: 0 }
    }

    //A MENU IS OPEN (NAVIGABLE + COMPLETABLE)
    pub fn is_active(&self) -> bool { matches!(self.mode, PaletteMode::Menu(..) | PaletteMode::Values(..)) }

    pub fn values(&self) -> Option<&Values>
    {
        match &self.mode
        {
            PaletteMode::Values(values) => Some(values),
            _ => None,
        }
    }

    //ANYTHING AT ALL IS ON SCREEN
    pub fn is_visible(&self) -> bool { !matches!(self.mode, PaletteMode::Hidden) }

    //RECOMPUTE FROM THE CURRENT INPUT
    pub fn update(&mut self, input: &str, role: Role, users: &[String])
    {
        //THE LOGIN PROMPT OWNS THE LINE UNTIL AUTH
        if !options::get_sending_messages()
        {
            self.dismiss();
            return;
        }

        if self.mention(input, users) { return; }

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

            //COMMAND WORD FINISHED - HAND OVER THE REST
            Some(split) =>
            {
                let (word, tail) = rest.split_at(split);

                let Some(info) = command::COMMAND_LIST.iter()
                    .find(|info| info.available(role) && info.triggers.iter().any(|t| t.eq_ignore_ascii_case(word))) else
                {
                    self.dismiss();
                    return;
                };

                //AN ACTION OWNS EVERYTHING PAST IT
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

    //THE ACTION WORD OF /command <action> ...
    fn action(&mut self, info: &'static CommandInfo, tail: &str, role: Role, input: &str)
    {
        match tail.find(char::is_whitespace)
        {
            //STILL TYPING THE ACTION
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

                //AN ACTION OUT OF OUR REACH IS NOT HINTED
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

    //AN @NAME BEING TYPED AT THE END OF THE LINE
    fn mention(&mut self, input: &str, users: &[String]) -> bool
    {
        let typed = partial(input);

        let Some(name) = typed.strip_prefix('@') else { return false };

        if !name.chars().all(mention_char) { return false; }

        let candidate = typed.to_lowercase();

        let matches = iter::once(consts::MENTION_EVERYONE).chain(users.iter().map(String::as_str))
            .map(|user| format!("@{user}"))
            .filter(|value| value.to_lowercase().starts_with(&candidate))
            .collect::<Vec<String>>();

        if matches.is_empty() { return false; }

        self.offer(None, matches, typed, input);

        true
    }

    //THE PARAMETER THE CARET IS ON
    fn hint(&mut self, entry: Entry, tail: &str, input: &str)
    {
        let args = entry.args();
        let active = active_arg(args, tail);

        if let Some(arg) = active.and_then(|i| args.get(i)) && arg.values != ArgValues::Free
        {
            let typed = partial(tail);
            let candidate = typed.to_lowercase();

            let matches = vocabulary(arg.values, typed).into_iter()
                .filter(|value| value.to_lowercase().starts_with(&candidate)).collect::<Vec<String>>();

            //A TYPO STILL LEAVES THE SIGNATURE HINT
            if !matches.is_empty()
            {
                self.offer(Some(arg), matches, typed, input);
                return;
            }
        }

        self.mode = PaletteMode::Signature(entry, active);
    }

    //SHOW matches FOR THE HALF-TYPED typed, KEEPING THE SELECTION
    fn offer(&mut self, arg: Option<&'static CommandArg>, matches: Vec<String>, typed: &str, input: &str)
    {
        //A FULLY TYPED VALUE WINS THE SELECTION
        let exact = matches.iter().position(|value| value.eq_ignore_ascii_case(typed));

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
    }

    //SHOW matches, KEEPING THE SELECTION
    fn menu(&mut self, matches: Vec<Entry>, typed: &str)
    {
        if matches.is_empty()
        {
            self.dismiss();
            return;
        }

        //A FULLY TYPED WORD WINS THE SELECTION
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
        self.offset = 0;
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

    //A TRAILING SPACE MEANS THE NEXT PARAMETER
    let index = if tail.ends_with(char::is_whitespace) { given } else { given.saturating_sub(1) };

    //THE LAST PARAMETER SWALLOWS THE REST
    Some(index.min(args.len() - 1))
}

//THE HALF-TYPED VALUE THE CARET IS ON
fn partial(tail: &str) -> &str
{
    if tail.ends_with(char::is_whitespace) { "" } else { tail.split_whitespace().next_back().unwrap_or("") }
}

//A CHARACTER A USERNAME MAY HOLD
pub fn mention_char(c: char) -> bool
{
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

//WHETHER text MENTIONS username OR EVERYONE
pub fn mentions(text: &str, username: &str) -> bool
{
    if username.is_empty() { return false; }

    let mut rest = text;
    let mut before = None;

    while let Some(at) = rest.find('@')
    {
        let boundary = rest[..at].chars().next_back().or(before).is_none_or(|c| !mention_char(c));

        let name = &rest[at + 1..];
        let word = &name[..name.find(|c| !mention_char(c)).unwrap_or(name.len())];

        if boundary && (word.eq_ignore_ascii_case(username) || word.eq_ignore_ascii_case(consts::MENTION_EVERYONE)) { return true; }

        before = Some('@');
        rest = name;
    }

    false
}

//WHETHER A FILE IS A PICTURE THIS CLIENT DECODES
fn decodable(name: &str) -> bool
{
    let Some(extension) = Path::new(name).extension().and_then(OsStr::to_str) else { return false; };

    ImageFormat::from_extension(extension).is_some_and(|format| format.reading_enabled())
}

//WHAT SITS BESIDE THE HALF-TYPED PATH
pub fn paths(typed: &str, images: bool) -> Vec<String>
{
    //A BARE ~ IS THE HOME DIRECTORY
    if typed == "~" { return vec![String::from("~/")]; }

    //EVERYTHING PAST THE LAST SEPARATOR IS THE NAME BEING TYPED
    let (dir, prefix) = match typed.rfind('/')
    {
        Some(cut) => (&typed[..=cut], &typed[cut + 1..]),
        None => ("", typed),
    };

    let target = if dir.is_empty() { PathBuf::from(".") } else { expand_home(dir) };

    let Ok(entries) = fs::read_dir(target) else { return Vec::new() };

    let prefix = prefix.to_lowercase();

    let mut matches = entries.flatten().filter_map(|entry|
    {
        let name = entry.file_name().into_string().ok()?;

        if !name.to_lowercase().starts_with(&prefix) { return None; }

        //A DOTFILE IS OFFERED ONLY WHEN ASKED FOR
        if name.starts_with('.') && !prefix.starts_with('.') { return None; }

        //A DIRECTORY CARRIES ITS SEPARATOR, SO Tab WALKS INTO IT
        let directory = entry.path().is_dir();

        if images && !directory && !decodable(&name) { return None; }

        let separator = if directory { "/" } else { "" };

        Some(format!("{dir}{name}{separator}"))
    }).collect::<Vec<String>>();

    matches.sort_unstable();
    matches.truncate(consts::MAX_PATHS);

    matches
}

//THE ANSWERS, READ WHERE THEY ARE DEFINED
fn vocabulary(values: ArgValues, typed: &str) -> Vec<String>
{
    match values
    {
        //THE FILESYSTEM, READ AT EVERY KEYSTROKE
        ArgValues::Paths => paths(typed, false),

        //THE SAME, PICTURES ONLY
        ArgValues::Images => paths(typed, true),

        ArgValues::Colors => colors::offered().into_iter().map(str::to_string).collect(),

        //THE MONITORS OF THIS MACHINE, READ AT RUNTIME
        #[cfg(feature = "client_screen")]
        ArgValues::Monitors => crate::screen::capture::monitor_names(),

        #[cfg(not(feature = "client_screen"))]
        ArgValues::Monitors => Vec::new(),

        //THE ROLES, OFFERED BY NAME
        ArgValues::Roles => Role::ALL.iter().map(Role::to_string).collect(),

        ArgValues::Bools => vec![ "true".to_string(), "false".to_string() ],

        ArgValues::Free => Vec::new(),
    }
}

//PUBLIC
pub fn format_arg(arg: &command::CommandArg) -> String //<REQUIRED> / [OPTIONAL]
{
    if arg.required
    {
        format!("<{}>", i18n::text(arg.name).to_lowercase())
    } else
    {
        format!("[{}]", i18n::text(arg.name).to_lowercase())
    }
}

pub fn expand_home(path: &str) -> PathBuf //A LEADING ~ AS THE HOME DIRECTORY
{
    let Some(rest) = path.strip_prefix('~') else { return PathBuf::from(path) };

    let Some(home) = dirs::home_dir() else { return PathBuf::from(path) };

    match rest.strip_prefix('/')
    {
        Some(rest) => home.join(rest),
        None if rest.is_empty() => home,

        //~name IS SOMEBODY ELSE'S HOME, NOT OURS
        None => PathBuf::from(path),
    }
}
