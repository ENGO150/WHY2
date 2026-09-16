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
    style::{ Modifier, Style },
    text::{ Line, Span },
};

use unicode_width::UnicodeWidthChar;

use super::
{
    math,
    theme,
    state,
    consts,
};

//STRUCTS
struct Emphasis //AN OPEN RUN AND WHERE IT WAS FOUND TO CLOSE
{
    run: usize,
    end: usize,
    modifier: Modifier,
}

struct Marker<'a> //WHAT A ROW OPENS WITH ONCE ITS LINE MARKER IS OFF
{
    spans: Vec<Span<'static>>,   //THE MARKER, AS IT IS DRAWN
    hanging: Vec<Span<'static>>, //WHAT ITS WRAPPED ROWS OPEN WITH
    style: Style,                //A HEADING RESTYLES THE WHOLE ROW
    rest: &'a str,
}

//ENUMS
//WHAT A MESSAGE IS MADE OF, MARKUP OFF
enum Segment
{
    Text(String),
    Raw(String),                                  //A CHARACTER THE BACKSLASH TOOK THE MARKUP OFF
    Code(String),                                 //INLINE `code`
    Block { lang: Option<String>, body: String }, //FENCED ```code```
    Math(String),                                 //INLINE $math$
    Display(String),                              //$$math$$
    Link { text: String, url: String },           //[text](url)
    Open(Modifier),                               //EMPHASIS OPENS
    Close(Modifier),                              //AND CLOSES
}

//FUNCTIONS
//ONE MESSAGE, AS THE ROWS IT DRAWS AS
pub fn render(prefix: Vec<Span<'static>>, text: &str, style: Style, width: u16, math: bool)
    -> Vec<Line<'static>>
{
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut current = prefix;
    let mut open = true;  //A LINE IS BEING BUILT
    let mut start = true; //AND NOTHING IS ON IT YET

    let mut line = style;                             //WHAT A HEADING RESTYLES THE ROW TO
    let mut hanging: Vec<Span<'static>> = Vec::new(); //AND WHAT ITS WRAPPED ROWS OPEN WITH
    let mut stack: Vec<Modifier> = Vec::new();

    for segment in parse(text, math)
    {
        match segment
        {
            //THE ONLY LINE BREAK INSIDE TEXT
            Segment::Text(text) => for (i, part) in text.split('\n').enumerate()
            {
                let mut part = part;

                if i > 0
                {
                    flush(&mut out, &mut current, &mut open, width, &hanging);

                    open = true;
                    start = true;
                    line = style;

                    hanging.clear();
                }

                //A LINE OF NOTHING BUT DASHES IS A RULE
                if start && is_rule(part)
                {
                    open = true;

                    current.push(rule(&current, width));
                    flush(&mut out, &mut current, &mut open, width, &hanging);

                    start = false;

                    continue;
                }

                if start && let Some(found) = marker(part, style)
                {
                    current.extend(found.spans);

                    hanging = found.hanging;
                    line = found.style;
                    part = found.rest;
                    start = false;
                    open = true;
                }

                if !part.is_empty()
                {
                    current.push(Span::styled(part.to_owned(), line.add_modifier(active(&stack))));

                    open = true;
                    start = false;
                }
            },

            Segment::Raw(text) =>
            {
                current.push(Span::styled(text, line.add_modifier(active(&stack))));

                open = true;
                start = false;
            },

            //A NEWLINE INSIDE INLINE CODE IS A SPACE
            Segment::Code(code) =>
            {
                current.push(Span::styled(code.replace('\n', " "), theme::CODE.add_modifier(active(&stack))));

                open = true;
                start = false;
            },

            //SHOWN AS ITS TEXT, WITH THE TARGET BESIDE IT
            Segment::Link { text, url } =>
            {
                current.push(Span::styled(text.clone(), theme::LINK));

                if url != text { current.push(Span::styled(format!(" ({url})"), theme::DIM)); }

                open = true;
                start = false;
            },

            Segment::Math(source) =>
            {
                current.extend(math::inline(&source, line.add_modifier(active(&stack))));

                open = true;
                start = false;
            },

            //BOTH OWN THEIR ROWS, SO CLOSE THE CURRENT RUN
            Segment::Block { lang, body } =>
            {
                close(&mut out, &mut current, &mut open, width, &hanging);
                block(&mut out, lang.as_deref(), &body, width);

                line = style;
                start = true;

                hanging.clear();
            },

            Segment::Display(source) =>
            {
                close(&mut out, &mut current, &mut open, width, &hanging);
                out.extend(math::display(&source, width));

                line = style;
                start = true;

                hanging.clear();
            },

            Segment::Open(modifier) => stack.push(modifier),
            Segment::Close(modifier) => { stack.pop_if(|open| *open == modifier); },
        }
    }

    //A MESSAGE THAT ENDS ON A BLOCK ENDS THERE
    if open || out.is_empty() { flush(&mut out, &mut current, &mut open, width, &hanging); }

    out
}

fn active(stack: &[Modifier]) -> Modifier //EVERY EMPHASIS THE TEXT IS INSIDE OF
{
    stack.iter().fold(Modifier::empty(), |all, modifier| all | *modifier)
}

fn flush(out: &mut Vec<Line<'static>>, current: &mut Vec<Span<'static>>, open: &mut bool, width: u16,
    hanging: &[Span<'static>])
{
    if !*open { return; } //NOTHING IS BEING BUILT

    let hang = hanging.iter().map(|span| text_width(&span.content)).sum::<usize>() as u16;
    let rows = state::wrap_line(&Line::from(mem::take(current)), width.saturating_sub(hang));

    //A WRAPPED ROW OPENS UNDER THE MARKER, NOT UNDER THE PANE
    for (i, row) in rows.into_iter().enumerate()
    {
        match i > 0 && !hanging.is_empty()
        {
            true =>
            {
                let mut spans = hanging.to_vec();

                spans.extend(row.spans);
                out.push(Line::from(spans));
            },

            false => out.push(row),
        }
    }

    *open = false;
}

fn close(out: &mut Vec<Line<'static>>, current: &mut Vec<Span<'static>>, open: &mut bool, width: u16,
    hanging: &[Span<'static>])
{
    match current.is_empty()
    {
        true => *open = false,
        false => flush(out, current, open, width, hanging),
    }
}

//A FENCED BLOCK, PADDED AND NEVER WORD-WRAPPED
fn block(out: &mut Vec<Line<'static>>, lang: Option<&str>, body: &str, width: u16)
{
    let inner = width.saturating_sub(consts::GUTTER).max(1) as usize;

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
            '\t' => out.extend(std::iter::repeat_n(' ', consts::TAB - out.chars().count() % consts::TAB)),
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

//THE LINE-LEVEL MARKDOWN, TAKEN OFF THE FRONT OF A ROW
fn marker(part: &str, style: Style) -> Option<Marker<'_>>
{
    let spaces = part.len() - part.trim_start_matches(' ').len();
    let body = &part[spaces..];
    let indent = || Span::raw(" ".repeat(spaces));

    //A HEADING, WHOSE MARKER IS NOT DRAWN AT ALL
    let hashes = body.chars().take_while(|c| *c == '#').count();

    if (1..=consts::MAX_HEADING).contains(&hashes) && body[hashes..].starts_with(' ')
    {
        return Some(Marker
        {
            spans: vec![indent()],
            hanging: vec![indent()],
            style: heading(style, hashes),
            rest: body[hashes..].trim_start_matches(' '),
        });
    }

    if body.starts_with("> ") || body == ">"
    {
        return Some(Marker
        {
            spans: vec![indent(), Span::styled(consts::QUOTE, theme::QUOTE)],
            hanging: vec![indent(), Span::styled(consts::QUOTE, theme::QUOTE)],
            style,
            rest: body[1..].strip_prefix(' ').unwrap_or(""),
        });
    }

    if matches!(body.get(..2), Some("- " | "* " | "+ "))
    {
        return Some(Marker
        {
            spans: vec![indent(), Span::styled(consts::BULLET, theme::BULLET)],
            hanging: vec![Span::raw(" ".repeat(spaces + text_width(consts::BULLET)))],
            style,
            rest: body[2..].trim_start_matches(' '),
        });
    }

    //AN ORDERED LIST KEEPS THE NUMBER IT WAS TYPED WITH
    let digits = body.chars().take_while(char::is_ascii_digit).count();

    if (1..=consts::MAX_ORDINAL).contains(&digits) && matches!(body.chars().nth(digits), Some('.' | ')'))
        && body[digits + 1..].starts_with(' ')
    {
        return Some(Marker
        {
            spans: vec![indent(), Span::styled(body[..digits + 2].to_owned(), theme::BULLET)],
            hanging: vec![Span::raw(" ".repeat(spaces + digits + 2))],
            style,
            rest: body[digits + 2..].trim_start_matches(' '),
        });
    }

    None
}

fn heading(style: Style, level: usize) -> Style //A HEADING KEEPS THE MESSAGE'S COLOUR WHERE IT HAS ONE
{
    let style = match style.fg
    {
        Some(_) => style,
        None => style.patch(theme::HEADING),
    };

    match level
    {
        1 => style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        _ => style.add_modifier(Modifier::BOLD),
    }
}

fn is_rule(text: &str) -> bool //THREE OR MORE OF THE SAME MARKER, AND NOTHING ELSE
{
    let text = text.trim();
    let Some(first) = text.chars().next() else { return false };

    matches!(first, '-' | '*' | '_') && text.chars().count() >= consts::MIN_RULE
        && text.chars().all(|c| c == first)
}

fn rule(current: &[Span<'static>], width: u16) -> Span<'static> //FILLING WHAT IS LEFT OF THE ROW
{
    let used: usize = current.iter().map(|span| text_width(&span.content)).sum();

    Span::styled(consts::RULE.to_string().repeat((width as usize).saturating_sub(used)), theme::RULE)
}

//THE PARSER; IT NEVER CONSUMES AN UNCLOSED RUN
fn parse(text: &str, math: bool) -> Vec<Segment>
{
    let chars: Vec<char> = text.chars().collect();

    let mut out: Vec<Segment> = Vec::new();
    let mut buf = String::new();
    let mut stack: Vec<Emphasis> = Vec::new();
    let mut i = 0usize;

    //A DELIMITER NOT FOUND ONCE IS NOT SEARCHED AGAIN
    let mut missing = [false; consts::MARKUP_KINDS];

    while i < chars.len()
    {
        //A BACKSLASH TAKES THE MARKUP OFF WHAT FOLLOWS
        if chars[i] == '\\' && chars.get(i + 1).is_some_and(|c| consts::ESCAPABLE.contains(*c))
        {
            flush_text(&mut out, &mut buf);
            out.push(Segment::Raw(chars[i + 1].to_string()));

            i += 2;

            continue;
        }

        let taken = match chars[i]
        {
            '`' => backtick(&chars, i, &mut out, &mut buf, &mut missing),
            '$' if math => dollar(&chars, i, &mut out, &mut buf, &mut missing),
            '[' => link(&chars, i, &mut out, &mut buf, &mut missing),
            '*' | '_' | '~' => emphasis(&chars, i, &mut out, &mut buf, &mut stack, &mut missing, math),
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
fn backtick(chars: &[char], i: usize, out: &mut Vec<Segment>, buf: &mut String,
    missing: &mut [bool; consts::MARKUP_KINDS]) -> Option<usize>
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

    !word.is_empty() && word.len() <= consts::MAX_LANG
        && word.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '-' | '_' | '.'))
}

//MATH, WITH THE GUARDS THAT KEEP PRICES OUT OF IT
fn dollar(chars: &[char], i: usize, out: &mut Vec<Segment>, buf: &mut String,
    missing: &mut [bool; consts::MARKUP_KINDS]) -> Option<usize>
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

//A LINK, WHOSE TARGET HAS TO BE ONE WORD
fn link(chars: &[char], i: usize, out: &mut Vec<Segment>, buf: &mut String,
    missing: &mut [bool; consts::MARKUP_KINDS]) -> Option<usize>
{
    let kind = kind('[');

    if missing[kind] { return None; }

    let label = seen(find_escaped(chars, i + 1, &[']']), &mut missing[kind])?;

    if chars.get(label + 1) != Some(&'(') { return None; }

    let end = find(chars, label + 2, &[')'])?;
    let text: String = chars[i + 1..label].iter().collect();
    let url: String = chars[label + 2..end].iter().collect();

    if text.is_empty() || url.is_empty() || url.chars().any(char::is_whitespace) { return None; }

    flush_text(out, buf);
    out.push(Segment::Link { text, url });

    Some(end + 1)
}

//EMPHASIS; A RUN ONLY OPENS IF ITS CLOSE IS ALREADY IN SIGHT
fn emphasis(chars: &[char], i: usize, out: &mut Vec<Segment>, buf: &mut String, stack: &mut Vec<Emphasis>,
    missing: &mut [bool; consts::MARKUP_KINDS], math: bool) -> Option<usize>
{
    //WHATEVER IS OPEN CLOSES HERE
    if stack.last().is_some_and(|open| open.end == i)
    {
        let open = stack.pop()?;

        flush_text(out, buf);
        out.push(Segment::Close(open.modifier));

        return Some(i + open.run);
    }

    let c = chars[i];
    let run = chars[i..].iter().take_while(|x| **x == c).take(consts::MAX_RUN).count();
    let modifier = modifier(c, run)?;
    let kind = kind(c);

    if missing[kind] { return None; }

    let start = i + run;

    if chars.get(start).is_none_or(|c| c.is_whitespace()) { return None; }
    if c == '_' && i > 0 && chars[i - 1].is_alphanumeric() { return None; } //snake_case IS A WORD

    let end = seen(find_run(chars, start, c, run, math), &mut missing[kind])?;

    if c == '_' && chars.get(end + run).is_some_and(|c| c.is_alphanumeric()) { return None; }

    flush_text(out, buf);
    stack.push(Emphasis { run, end, modifier });
    out.push(Segment::Open(modifier));

    Some(start)
}

fn modifier(c: char, run: usize) -> Option<Modifier> //WHAT A RUN OF THAT LENGTH MEANS
{
    match (c, run)
    {
        ('*' | '_', 1) => Some(Modifier::ITALIC),
        ('*', 2) => Some(Modifier::BOLD),
        ('*', _) => Some(Modifier::BOLD | Modifier::ITALIC),
        ('_', 2) => Some(Modifier::UNDERLINED),
        ('_', _) => Some(Modifier::UNDERLINED | Modifier::ITALIC),
        ('~', 1) => None,
        ('~', _) => Some(Modifier::CROSSED_OUT),
        _ => None,
    }
}

fn kind(c: char) -> usize //ITS SLOT IN THE MISSING TABLE
{
    match c
    {
        '*' => 5,
        '_' => 6,
        '~' => 7,
        _ => 8,
    }
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

//AND THE RUN THAT COULD CLOSE AN EMPHASIS, PAST CODE AND MATH
fn find_run(chars: &[char], from: usize, needle: char, run: usize, math: bool) -> Option<usize>
{
    let mut i = from;

    while i < chars.len()
    {
        if chars[i] == '\\' { i += 2; continue; }

        if let Some(next) = span(chars, i, math) { i = next; continue; }

        let len = chars[i..].iter().take_while(|c| **c == needle).take(run).count();

        if len == 0 { i += 1; continue; }
        if len >= run && !chars[i - 1].is_whitespace() { return Some(i); } //A CLOSE HANGS ON THE WORD BEFORE IT

        i += len;
    }

    None
}

fn span(chars: &[char], i: usize, math: bool) -> Option<usize> //HOW FAR A CODE OR MATH SPAN AT i REACHES
{
    let delim = match chars[i]
    {
        '`' => '`',
        '$' if math => '$',
        _ => return None,
    };

    let run = chars[i..].iter().take_while(|c| **c == delim).count().min(3);

    find(chars, i + run, &vec![delim; run]).map(|end| end + run)
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

