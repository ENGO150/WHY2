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
const GUTTER: u16 = 2;      //THE BAR AND THE SPACE AFTER IT, WHICH A BLOCK'S CONTENT DOES NOT GET
const TAB: usize = 4;       //A TAB IS EXPANDED, SINCE A CELL GRID HAS NO TAB STOPS
const MAX_LANG: usize = 20; //LONGER THAN THIS AND THE FIRST WORD IS CODE, NOT A LANGUAGE NAME

//ENUMS
//WHAT A MESSAGE IS MADE OF ONCE THE MARKUP IS OFF IT. EVERYTHING THAT IS NOT ONE OF THESE IS Text,
//INCLUDING MARKUP THAT NEVER CLOSED - AN UNTERMINATED FENCE IS BACKTICKS SOMEBODY TYPED, NOT A BLOCK
enum Segment
{
    Text(String),
    Code(String),                                 //INLINE `code`
    Block { lang: Option<String>, body: String }, //FENCED ```code```
    Math(String),                                 //INLINE $math$
    Display(String),                              //$$math$$
}

//FUNCTIONS
//ONE MESSAGE, WRAPPED. THE LINES COME BACK READY TO DRAW RATHER THAN AS ONE LOGICAL LINE, BECAUSE A
//FENCED BLOCK IS ROWS AND NOT TEXT: IT IS PADDED TO THE PANE, SO IT HAS TO BE BROKEN WHERE IT IS BUILT
//RATHER THAN HANDED TO THE WORD-WRAPPER AFTERWARDS.
//math IS render_math: WITH IT OFF A DOLLAR SIGN IS A DOLLAR SIGN, SO THE FORMULA IS SHOWN AS IT WAS
//TYPED RATHER THAN AS AN APPROXIMATION OF ITSELF - CODE IS NOT AFFECTED, IT IS A SEPARATE SETTING'S WORTH
pub fn render(prefix: Vec<Span<'static>>, text: &str, style: Style, width: u16, math: bool)
    -> Vec<Line<'static>>
{
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut current = prefix;
    let mut open = true; //A LINE IS BEING BUILT - AN EMPTY ONE IS STILL A LINE THE USER TYPED

    for segment in parse(text, math)
    {
        match segment
        {
            //THE ONLY PLACE A LINE BREAK COMES FROM INSIDE TEXT: THE INPUT BAR IS MULTI-LINE, SO A
            //MESSAGE CAN CARRY ONE
            Segment::Text(text) => for (i, part) in text.split('\n').enumerate()
            {
                if i > 0
                {
                    flush(&mut out, &mut current, &mut open, width);
                    open = true;
                }

                if !part.is_empty() { current.push(Span::styled(part.to_owned(), style)); }
            },

            //A NEWLINE INSIDE INLINE CODE IS A SPACE - IT IS ONE RUN OF TEXT, AND A FENCE IS WHAT SPANS ROWS
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

            //BOTH OF THESE OWN THE ROWS THEY SIT ON, SO WHATEVER IS BEING BUILT GOES OUT FIRST - BUT AN
            //EMPTY LINE IS NOT ONE OF THEM: THE NEWLINE IN FRONT OF A FENCE IS THE FENCE'S OWN
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

    //A MESSAGE THAT ENDS ON A BLOCK ENDS THERE - THE BLANK LINE UNDER IT WOULD BE A ROW OF THE PANE
    if open || out.is_empty() { flush(&mut out, &mut current, &mut open, width); }

    out
}

fn flush(out: &mut Vec<Line<'static>>, current: &mut Vec<Span<'static>>, open: &mut bool, width: u16)
{
    if !*open { return; } //NOTHING IS BEING BUILT - A BLOCK JUST ENDED, AND ITS LAST ROW IS THE LAST ROW

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

//A FENCED BLOCK, AS ROWS PADDED TO THE PANE. THE PADDING IS WHAT MAKES IT A BOX RATHER THAN A RAGGED
//RUN OF HIGHLIGHTED WORDS, AND IT IS ALSO WHY THESE ROWS ARE NEVER WORD-WRAPPED: CODE IS BROKEN WHERE
//IT RUNS OUT OF CELLS, NOT AT THE LAST SPACE BEFORE IT
fn block(out: &mut Vec<Line<'static>>, lang: Option<&str>, body: &str, width: u16)
{
    let inner = width.saturating_sub(GUTTER).max(1) as usize;

    //THE LANGUAGE IS NOT HIGHLIGHTED WITH (NOTHING HERE HIGHLIGHTS), SO IT IS SHOWN INSTEAD OF USED
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

//THE PARSER. IT NEVER FAILS AND NEVER CONSUMES ANYTHING IT CANNOT CLOSE - MARKUP THAT DOES NOT
//TERMINATE IS THE CHARACTERS THAT WERE TYPED, WHICH IS THE ONLY BEHAVIOUR THAT CANNOT SWALLOW A MESSAGE
fn parse(text: &str, math: bool) -> Vec<Segment>
{
    let chars: Vec<char> = text.chars().collect();

    let mut out: Vec<Segment> = Vec::new();
    let mut buf = String::new();
    let mut i = 0usize;

    //A DELIMITER THAT WAS NOT FOUND ONCE IS NOT THERE AT ALL: THE SEARCH ONLY EVER STARTS LATER IN THE
    //MESSAGE, SO IT CANNOT SUCCEED AFTERWARDS. REMEMBERING THAT IS WHAT KEEPS A MESSAGE OF NOTHING BUT
    //BACKTICKS FROM COSTING A SEARCH PER BACKTICK
    let mut missing = [false; 5];

    while i < chars.len()
    {
        //A BACKSLASH TAKES THE MARKUP OFF WHATEVER FOLLOWS IT, AND OFF NOTHING ELSE
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

//A RUN OF THREE OR MORE BACKTICKS OPENS A FENCE, ONE OR TWO OPEN INLINE CODE THAT THE SAME RUN CLOSES
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

//DISCORD'S RULE: A FIRST WORD ON A LINE OF ITS OWN IS THE LANGUAGE, ANYTHING ELSE IS THE FIRST LINE OF
//CODE. THE LEADING NEWLINE GOES EITHER WAY - IT IS THE FENCE'S, NOT THE CODE'S
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

//MATH. THE GUARDS ARE WHAT KEEPS PRICES OUT OF IT: AN OPENING $ IS NOT FOLLOWED BY A SPACE, A CLOSING
//ONE IS NOT PRECEDED BY ONE AND NOT FOLLOWED BY A DIGIT, SO "$5 AND $10 LEFT" IS THREE WORDS AND NOT MATH
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

//THE SAME, PAST BACKSLASHES. A FENCE IS SEARCHED FOR WITHOUT THIS: WHAT IS INSIDE ONE IS VERBATIM, SO A
//LINE OF CODE ENDING IN A BACKSLASH CANNOT BE ALLOWED TO SWALLOW THE FENCE THAT CLOSES IT
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
