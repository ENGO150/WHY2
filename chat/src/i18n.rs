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
    fs,
    fmt::
    {
        Display,
        Write,
    },
    sync::LazyLock,
    collections::HashMap,
};

use toml_edit::
{
    DocumentMut,
    Item,
    Table,
};

use crate::
{
    config,
    consts,
    misc,
};

//STRUCTS
struct Locale //ONE LANGUAGE'S TEXT, FLATTENED TO DOTTED KEYS
{
    text: HashMap<String, String>,
    plurals: String,
}

//CONSTS
const ENGLISH: &str = include_str!("../locales/en.toml");

const BUILTIN: &[(&str, &str)] = //LANGUAGES SHIPPED IN THE BINARY
&[
    ("en", ENGLISH),
];

//GLOBAL VARIABLES
static FALLBACK: LazyLock<Locale> = LazyLock::new(|| parse(ENGLISH).expect("Parsing the English locale failed"));

static ACTIVE: LazyLock<Option<Locale>> = LazyLock::new(||
{
    let code = config::read_config::<String>("language");
    let code = code.trim();

    //A FILE IN THE CONFIG DIR WINS OVER A BUILT-IN ONE
    let path = format!("{}{}/{code}.toml", misc::get_why2_dir(), consts::LOCALES_DIR);

    fs::read_to_string(path).ok().and_then(|content| parse(&content))
        .or_else(|| BUILTIN.iter().find(|(name, _)| *name == code).and_then(|(_, content)| parse(content)))
});

//MACROS
#[macro_export]
macro_rules! t //TRANSLATED TEXT, {name} PLACEHOLDERS FILLED
{
    (@value $name:ident = $value:expr) => { $value };
    (@value $name:ident) => { $name };

    ($key:literal) => { $crate::i18n::text($key) };

    ($key:literal $(, $name:ident $(= $value:expr)?)+ $(,)?) =>
    {
        $crate::i18n::format($crate::i18n::text($key),
            &[$((stringify!($name), &$crate::t!(@value $name $(= $value)?) as &dyn ::std::fmt::Display)),+])
    };
}

#[macro_export]
macro_rules! tn //PLURAL TEXT, THE COUNT IS {count}
{
    ($key:literal, $count:expr $(, $name:ident $(= $value:expr)?)* $(,)?) =>
    {{
        let count = $count;

        $crate::i18n::format($crate::i18n::plural($key, count as u64),
            &[("count", &count as &dyn ::std::fmt::Display) $(, (stringify!($name), &$crate::t!(@value $name $(= $value)?) as &dyn ::std::fmt::Display))*])
    }};
}

//FUNCTIONS
//PRIVATE
fn parse(content: &str) -> Option<Locale>
{
    let document = content.parse::<DocumentMut>().ok()?;
    let mut text = HashMap::new();

    flatten(document.as_table(), "", &mut text);

    let plurals = text.remove("meta.plurals").unwrap_or_else(|| String::from("one-other"));

    Some(Locale { text, plurals })
}

fn flatten(table: &Table, prefix: &str, out: &mut HashMap<String, String>) //NESTED TABLES TO DOTTED KEYS
{
    for (key, item) in table
    {
        let key = format!("{prefix}{key}");

        match item
        {
            Item::Table(table) => flatten(table, &format!("{key}."), out),
            Item::Value(value) => if let Some(value) = value.as_str() { out.insert(key, value.to_owned()); },
            _ => {},
        }
    }
}

fn category(rule: &str, count: u64) -> &'static str //WHICH PLURAL FORM A COUNT TAKES
{
    match rule
    {
        "other" => "other",

        //CZECH, SLOVAK
        "one-few-other" => match count
        {
            1 => "one",
            2..=4 => "few",
            _ => "other",
        },

        //RUSSIAN, UKRAINIAN, SERBIAN, CROATIAN
        "one-few-many" => match (count % 10, count % 100)
        {
            (1, rest) if rest != 11 => "one",
            (2..=4, rest) if !(12..=14).contains(&rest) => "few",
            _ => "many",
        },

        _ => if count == 1 { "one" } else { "other" },
    }
}

fn lookup(locale: &'static Locale, key: &str) -> Option<&'static str>
{
    locale.text.get(key).map(String::as_str)
}

fn plural_in(locale: &'static Locale, key: &str, count: u64) -> Option<&'static str>
{
    lookup(locale, &format!("{key}.{}", category(&locale.plurals, count)))
        .or_else(|| lookup(locale, &format!("{key}.other")))
}

//PUBLIC
pub fn get(key: &str) -> Option<&'static str> //TEXT FOR key, IF ANY LOCALE HAS IT
{
    ACTIVE.as_ref().and_then(|locale| lookup(locale, key)).or_else(|| lookup(&FALLBACK, key))
}

pub fn text(key: &'static str) -> &'static str //TEXT FOR key, OR THE KEY ITSELF
{
    get(key).unwrap_or(key)
}

pub fn plural(key: &'static str, count: u64) -> &'static str //THE FORM OF key FOR count
{
    ACTIVE.as_ref().and_then(|locale| plural_in(locale, key, count))
        .or_else(|| plural_in(&FALLBACK, key, count))
        .unwrap_or(key)
}

pub fn format(template: &str, args: &[(&str, &dyn Display)]) -> String //FILL {name} PLACEHOLDERS
{
    let mut out = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(at) = rest.find(['{', '}'])
    {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];

        //DOUBLED BRACES ARE LITERAL
        if tail.starts_with("{{") || tail.starts_with("}}")
        {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }

        if tail.starts_with('{')
            && let Some(close) = tail.find('}')
            && let Some((_, value)) = args.iter().find(|(name, _)| *name == &tail[1..close])
        {
            write!(out, "{value}").ok();
            rest = &tail[close + 1..];
            continue;
        }

        out.push_str(&tail[..1]);
        rest = &tail[1..];
    }

    out.push_str(rest);
    out
}

pub fn split<'a>(template: &'a str, name: &str) -> (&'a str, &'a str) //THE TEXT EITHER SIDE OF {name}
{
    template.split_once(&format!("{{{name}}}")).unwrap_or((template, ""))
}
