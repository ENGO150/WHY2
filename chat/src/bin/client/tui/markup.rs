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

use std::mem;

use ratatui::
{
    style::Style,
    text::{ Line, Span },
};

use unicode_width::UnicodeWidthChar;

use super::
{
    math,
    theme,
    state,
};

//CONSTS
const GUTTER: u16 = 2;      //THE BAR AND THE SPACE AFTER IT
const TAB: usize = 4;       //A TAB IS EXPANDED, SINCE A CELL GRID HAS NO TAB STOPS
const MAX_LANG: usize = 20; //LONGER THAN THIS AND THE FIRST WORD IS CODE

//ENUMS
//WHAT A MESSAGE IS MADE OF, MARKUP OFF
enum Segment
{
    Text(String),
    Code(String),                                 //INLINE `code`
    Block { lang: Option<String>, body: String }, //FENCED ```code```
    Math(String),                                 //INLINE $math$
    Display(String),                              //$$math$$
}

//FUNCTIONS
//ONE MESSAGE, AS THE ROWS IT DRAWS AS
pub fn render(prefix: Vec<Span<'static>>, text: &str, style: Style, width: u16, math: bool)
    -> Vec<Line<'static>>
{
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut current = prefix;
    let mut open = true; //A LINE IS BEING BUILT

    for segment in parse(text, math)
    {
        match segment
        {
            //THE ONLY LINE BREAK INSIDE TEXT
            Segment::Text(text) => for (i, part) in text.split('\n').enumerate()
            {
                if i > 0
                {
                    flush(&mut out, &mut current, &mut open, width);
                    open = true;
                }

                if !part.is_empty() { current.push(Span::styled(part.to_owned(), style)); }
            },

            //A NEWLINE INSIDE INLINE CODE IS A SPACE
            Segment::Code(code) =>
            {
                current.push(Span::styled(code.replace('\n', " "), theme::CODE));
                open = true;
            },

            Segment::Math(source) =>
            {
                current.extend(math::inline(&source, style));
                open = true;
            },

            //BOTH OWN THEIR ROWS, SO CLOSE THE CURRENT RUN
            Segment::Block { lang, body } =>
            {
                close(&mut out, &mut current, &mut open, width);
                block(&mut out, lang.as_deref(), &body, width);
            },

            Segment::Display(source) =>
            {
                close(&mut out, &mut current, &mut open, width);
                out.extend(math::display(&source, width));
            },
        }
    }

    //A MESSAGE THAT ENDS ON A BLOCK ENDS THERE
    if open || out.is_empty() { flush(&mut out, &mut current, &mut open, width); }

    out
}

fn flush(out: &mut Vec<Line<'static>>, current: &mut Vec<Span<'static>>, open: &mut bool, width: u16)
{
    if !*open { return; } //NOTHING IS BEING BUILT

    out.extend(state::wrap_line(&Line::from(mem::take(current)), width));

    *open = false;
}

fn close(out: &mut Vec<Line<'static>>, current: &mut Vec<Span<'static>>, open: &mut bool, width: u16)
{
    match current.is_empty()
    {
        true => *open = false,
        false => flush(out, current, open, width),
    }
}

//A FENCED BLOCK, PADDED AND NEVER WORD-WRAPPED
fn block(out: &mut Vec<Line<'static>>, lang: Option<&str>, body: &str, width: u16)
{
    let inner = width.saturating_sub(GUTTER).max(1) as usize;

    //NOTHING HIGHLIGHTS, SO THE LANGUAGE IS SHOWN
    if let Some(lang) = lang.filter(|lang| !lang.is_empty())
    {
        out.push(row(pad(lang, inner), theme::CODE_LANG));
    }

    for line in body.split('\n')
    {
        for chunk in split_cells(&expand_tabs(line), inner)
        {
            out.push(row(pad(&chunk, inner), theme::CODE_BLOCK));
        }
    }
}

fn row(content: String, style: Style) -> Line<'static>
{
    Line::from(vec![Span::styled("▏ ", theme::CODE_BAR), Span::styled(content, style)])
}

fn pad(text: &str, width: usize) -> String
{
    let mut out = text.to_owned();

    out.extend(std::iter::repeat_n(' ', width.saturating_sub(text_width(text))));

    out
}

fn expand_tabs(line: &str) -> String
{
    let mut out = String::new();

    for c in line.chars()
    {
        match c
        {
            '\t' => out.extend(std::iter::repeat_n(' ', TAB - out.chars().count() % TAB)),
            '\r' => {},
            _ => out.push(c),
        }
    }

    out
}

fn split_cells(text: &str, width: usize) -> Vec<String> //HARD-BREAK ONE SOURCE LINE EVERY width CELLS
{
    let mut out = Vec::new();
    let mut chunk = String::new();
    let mut column = 0usize;

    for c in text.chars()
    {
        let w = c.width().unwrap_or(0);

        if column + w > width && column > 0
        {
            out.push(mem::take(&mut chunk));
            column = 0;
        }

        chunk.push(c);
        column += w;
    }

    out.push(chunk);
    out
}

pub fn text_width(text: &str) -> usize
{
    text.chars().map(|c| c.width().unwrap_or(0)).sum()
}

//THE PARSER; IT NEVER CONSUMES AN UNCLOSED RUN
fn parse(text: &str, math: bool) -> Vec<Segment>
{
    let chars: Vec<char> = text.chars().collect();

    let mut out: Vec<Segment> = Vec::new();
    let mut buf = String::new();
    let mut i = 0usize;

    //A DELIMITER NOT FOUND ONCE IS NOT SEARCHED AGAIN
    let mut missing = [false; 5];

    while i < chars.len()
    {
        //A BACKSLASH TAKES THE MARKUP OFF WHAT FOLLOWS
        if chars[i] == '\\' && matches!(chars.get(i + 1), Some('`' | '$' | '\\'))
        {
            buf.push(chars[i + 1]);
            i += 2;

            continue;
        }

        let taken = match chars[i]
        {
            '`' => backtick(&chars, i, &mut out, &mut buf, &mut missing),
            '$' if math => dollar(&chars, i, &mut out, &mut buf, &mut missing),
            _ => None,
        };

        match taken
        {
            Some(next) => i = next,
            None =>
            {
                buf.push(chars[i]);
                i += 1;
            },
        }
    }

    if !buf.is_empty() { out.push(Segment::Text(buf)); }

    out
}

//THREE BACKTICKS OPEN A FENCE, ONE OR TWO INLINE
fn backtick(chars: &[char], i: usize, out: &mut Vec<Segment>, buf: &mut String, missing: &mut [bool; 5])
    -> Option<usize>
{
    let run = chars[i..].iter().take_while(|c| **c == '`').count();
    let kind = run.min(3) - 1;

    if missing[kind] { return None; }

    let (segment, next) = match run >= 3
    {
        true =>
        {
            let start = i + 3;
            let end = seen(find(chars, start, &['`', '`', '`']), &mut missing[kind])?;
            let inner: String = chars[start..end].iter().collect();

            (fence(&inner), end + 3)
        },

        false =>
        {
            let start = i + run;
            let close = vec!['`'; run];
            let end = seen(find_escaped(chars, start, &close), &mut missing[kind])?;
            let inner: String = chars[start..end].iter().collect();

            if inner.is_empty() { return None; }

            (Segment::Code(inner), end + run)
        },
    };

    flush_text(out, buf);
    out.push(segment);

    Some(next)
}

//DISCORD'S RULE: A LONE FIRST WORD IS THE LANGUAGE
fn fence(inner: &str) -> Segment
{
    let (lang, body) = match inner.split_once('\n')
    {
        Some((first, rest)) if is_language(first) => (Some(first.trim().to_owned()), rest),
        _ => (None, inner.strip_prefix('\n').unwrap_or(inner)),
    };

    Segment::Block { lang, body: body.strip_suffix('\n').unwrap_or(body).to_owned() }
}

fn is_language(word: &str) -> bool
{
    let word = word.trim();

    !word.is_empty() && word.len() <= MAX_LANG
        && word.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '-' | '_' | '.'))
}

//MATH, WITH THE GUARDS THAT KEEP PRICES OUT OF IT
fn dollar(chars: &[char], i: usize, out: &mut Vec<Segment>, buf: &mut String, missing: &mut [bool; 5])
    -> Option<usize>
{
    let display = chars.get(i + 1) == Some(&'$');
    let close: Vec<char> = if display { vec!['$', '$'] } else { vec!['$'] };
    let start = i + close.len();
    let kind = 2 + close.len();

    if missing[kind] { return None; }
    if chars.get(start).is_none_or(|c| c.is_whitespace()) { return None; }

    let end = seen(find_escaped(chars, start, &close), &mut missing[kind])?;

    if chars[end - 1].is_whitespace() { return None; }
    if !display && chars.get(end + 1).is_some_and(char::is_ascii_digit) { return None; }

    let inner: String = chars[start..end].iter().collect();

    flush_text(out, buf);
    out.push(if display { Segment::Display(inner) } else { Segment::Math(inner) });

    Some(end + close.len())
}

fn find(chars: &[char], from: usize, needle: &[char]) -> Option<usize> //FIRST needle AT OR AFTER from
{
    (from..chars.len().saturating_sub(needle.len() - 1))
        .find(|i| chars[*i..*i + needle.len()] == *needle)
}

//THE SAME, PAST BACKSLASHES
fn find_escaped(chars: &[char], from: usize, needle: &[char]) -> Option<usize>
{
    let mut i = from;

    while i + needle.len() <= chars.len()
    {
        if chars[i] == '\\' { i += 2; continue; }
        if chars[i..i + needle.len()] == *needle { return Some(i); }

        i += 1;
    }

    None
}

fn seen(found: Option<usize>, missing: &mut bool) -> Option<usize> //A SEARCH THAT FAILED IS NOT REPEATED
{
    *missing = found.is_none();

    found
}

fn flush_text(out: &mut Vec<Segment>, buf: &mut String)
{
    if !buf.is_empty() { out.push(Segment::Text(mem::take(buf))); }
}
