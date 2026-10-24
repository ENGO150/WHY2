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

//! # Command Palette
//!
//! The popup above the input line, driven straight off
//! [`COMMAND_LIST`](why2_chat::command::COMMAND_LIST) — the trigger table is never duplicated
//! here. It has three modes ([`PaletteMode`]): a filtered command menu while the command word is
//! being typed, a list of the values the parameter under the caret accepts, and once those run
//! out, a one-line signature with that parameter highlighted. An `@name` at the end of the line
//! offers usernames instead.
//!
//! # Paths
//! A path parameter is completed off the disk ([`paths`]): everything past the last `/` is the
//! name being matched, the rest is the directory read. Directories are offered with their
//! separator, so Tab walks into them; dotfiles only once a `.` is typed; and `~` is the home
//! directory ([`expand_home`]). Values are matched case-insensitively but inserted as spelled on
//! disk.

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
/// What the palette is showing.
pub enum PaletteMode
{
    /// Nothing.
    Hidden,                          //NOTHING TO SHOW
    /// Matching commands or actions, and the selected one.
    Menu(Vec<Entry>, usize),         //MATCHING ENTRIES + SELECTION
    /// The values the parameter under the caret accepts.
    Values(Values),                  //THE ANSWERS A PARAMETER ACCEPTS
    /// One command's signature, and the parameter being typed.
    Signature(Entry, Option<usize>), //ONE ENTRY + THE PARAMETER BEING TYPED
}

//STRUCTS
//ONE POPUP LINE - A COMMAND OR ONE OF ITS ACTIONS
/// One palette row: a command, or one of its actions.
#[derive(Clone, Copy)]
pub struct Entry
{
    /// The command.
    pub info: &'static CommandInfo,
    /// The action, for a doorway command.
    pub sub: Option<&'static SubcommandInfo>,
}

//WHAT MAY GO IN THE PARAMETER THE CARET IS ON
/// What may go in the parameter under the caret.
pub struct Values
{
    /// The parameter; `None` for a mention.
    pub arg: Option<&'static CommandArg>, //NONE FOR A MENTION

    //NOT &'static str: MONITORS ARE RUNTIME-ONLY
    /// The matching values.
    pub matches: Vec<String>,
    /// The selected one.
    pub selected: usize,
    /// Character index where the half-typed value begins.
    pub start: usize, //CHAR INDEX WHERE THE HALF-TYPED VALUE BEGINS
}

/// The slash-command autocomplete.
pub struct Palette //SLASH-COMMAND AUTOCOMPLETE
{
    /// What it is showing.
    pub mode: PaletteMode,

    //FIRST VISIBLE ROW, WRITTEN BY THE DRAW PATH
    /// First visible row, written by the draw path.
    pub offset: usize,
}

//IMPLEMENTATIONS
impl Entry
{
    /// Returns a row for a command.
    pub fn command(info: &'static CommandInfo) -> Self { Self { info, sub: None } }

    /// Returns a row for one action of a doorway command.
    pub fn action(info: &'static CommandInfo, sub: &'static SubcommandInfo) -> Self { Self { info, sub: Some(sub) } }

    /// Returns the parameters of the command or action.
    pub fn args(&self) -> &'static [CommandArg]
    {
        self.sub.map_or(self.info.args, |sub| sub.args)
    }

    /// Returns the description, translated.
    pub fn description(&self) -> &'static str
    {
        i18n::text(self.sub.map_or(self.info.description, |sub| sub.description))
    }

    //ONLY WHOLE COMMANDS CARRY A SHORTCUT
    /// Returns the shortcut label; only whole commands carry one.
    pub fn shortcut(&self) -> String
    {
        match self.sub
        {
            Some(_) => String::new(),
            None => self.info.shortcut.map(|s| t!("keys.ctrl", key = s.to_ascii_uppercase())).unwrap_or_default(),
        }
    }

    //WHAT THE USER TYPES TO GET HERE
    /// Returns what the user types to get here.
    pub fn name(&self) -> String
    {
        let mut name = format!("{}{}", command::COMMAND_PREFIX, self.info.triggers[0].to_lowercase());

        if let Some(sub) = self.sub { name.push_str(&format!(" {}", sub.triggers[0].to_lowercase())); }

        name
    }

    //FULL SIGNATURE AS PLAIN TEXT, FOR MEASURING
    /// Returns the full signature as plain text, for measuring.
    pub fn signature(&self) -> String
    {
        let args = self.args().iter().map(format_arg).collect::<Vec<String>>().join(" ");
        let separator = if args.is_empty() { "" } else { " " };

        format!("{}{separator}{args}", self.name())
    }

    /// Returns the signature's display width.
    pub fn width(&self) -> usize { self.signature().width() }

    //THE SAME SIGNATURE, STYLED
    /// Returns the signature, styled, with the active parameter highlighted.
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
    /// Returns whether the input already spells this row out, so Enter sends rather than completes.
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
    /// Returns the selected value.
    pub fn selection(&self) -> Option<&str> { self.matches.get(self.selected).map(String::as_str) }

    //ALREADY SPELLED OUT, SO Enter SENDS IT
    /// Returns whether the input already spells the selected value out.
    pub fn typed(&self, input: &str) -> bool
    {
        let typed = input.chars().skip(self.start).collect::<String>();

        self.selection().is_some_and(|value| value.eq_ignore_ascii_case(typed.trim()))
    }

    //THE SWATCH DRAWN BESIDE A ROW
    /// Returns the color swatch drawn beside a value, for a color parameter.
    pub fn swatch(&self, value: &str) -> Option<Color>
    {
        match self.arg.map(|arg| arg.values)
        {
            Some(ArgValues::Colors) => colors::by_name(value),
            _ => None,
        }
    }

    /// Returns the list's title.
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
    /// Creates a hidden palette.
    pub fn new() -> Self
    {
        Self { mode: PaletteMode::Hidden, offset: 0 }
    }

    //A MENU IS OPEN (NAVIGABLE + COMPLETABLE)
    /// Returns whether a navigable list is open.
    pub fn is_active(&self) -> bool { matches!(self.mode, PaletteMode::Menu(..) | PaletteMode::Values(..)) }

    /// Returns the values list, if that is what is showing.
    pub fn values(&self) -> Option<&Values>
    {
        match &self.mode
        {
            PaletteMode::Values(values) => Some(values),
            _ => None,
        }
    }

    //ANYTHING AT ALL IS ON SCREEN
    /// Returns whether anything is showing.
    pub fn is_visible(&self) -> bool { !matches!(self.mode, PaletteMode::Hidden) }

    //RECOMPUTE FROM THE CURRENT INPUT
    /// Recomputes the palette from the input line.
    ///
    /// # Parameters
    /// - `input`: The input line.
    /// - `role`: Our role; commands and actions above it are not offered.
    /// - `users`: Usernames, for mentions.
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
    /// Shows the actions of a doorway command, or the parameter after the action.
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
    /// Offers usernames for an `@name` at the end of the line.
    ///
    /// # Returns
    /// `true` if a mention is being typed.
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
    /// Shows the values or the signature for the parameter under the caret.
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
    /// Shows values matching the half-typed one, keeping the selection where possible.
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
    /// Shows matching rows, keeping the selection where possible.
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

    /// Hides the palette.
    pub fn dismiss(&mut self)
    {
        self.mode = PaletteMode::Hidden;
        self.offset = 0;
    }

    /// Selects the next row, wrapping.
    pub fn next(&mut self)
    {
        match &mut self.mode
        {
            PaletteMode::Menu(matches, selected) => *selected = (*selected + 1) % matches.len(),
            PaletteMode::Values(values) => values.selected = (values.selected + 1) % values.matches.len(),

            _ => {},
        }
    }

    /// Selects the previous row, wrapping.
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

    /// Returns the selected row of the command menu.
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
/// Returns which parameter the caret is on.
fn active_arg(args: &'static [CommandArg], tail: &str) -> Option<usize>
{
    let given = tail.split_whitespace().count();

    //A TRAILING SPACE MEANS THE NEXT PARAMETER
    let index = if tail.ends_with(char::is_whitespace) { given } else { given.saturating_sub(1) };

    //THE LAST PARAMETER SWALLOWS THE REST
    Some(index.min(args.len() - 1))
}

//THE HALF-TYPED VALUE THE CARET IS ON
/// Returns the half-typed value the caret is on.
fn partial(tail: &str) -> &str
{
    if tail.ends_with(char::is_whitespace) { "" } else { tail.split_whitespace().next_back().unwrap_or("") }
}

//A CHARACTER A USERNAME MAY HOLD
/// Returns whether a character may be part of a username in a mention.
pub fn mention_char(c: char) -> bool
{
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

//WHETHER text MENTIONS username OR EVERYONE
/// Returns whether text mentions `username` or everyone.
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
/// Returns whether a file name has an image extension this client decodes.
fn decodable(name: &str) -> bool
{
    let Some(extension) = Path::new(name).extension().and_then(OsStr::to_str) else { return false; };

    ImageFormat::from_extension(extension).is_some_and(|format| format.reading_enabled())
}

//WHAT SITS BESIDE THE HALF-TYPED PATH
/// Returns the entries beside a half-typed path, directories with their separator.
///
/// # Parameters
/// - `typed`: The half-typed path.
/// - `images`: Offer decodable pictures only (directories are always offered).
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
/// Returns the values a parameter kind offers.
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
/// Formats a parameter as `<required>` or `[optional]`.
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

/// Expands a leading `~` to the home directory.
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
