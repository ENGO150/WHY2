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

//TeX MATH, LAID OUT IN CELLS

use std::iter;

use ratatui::
{
    style::Style,
    text::{ Line, Span },
};

use super::{ markup, theme };

//CONSTS
const INDENT: &str = "  "; //DISPLAY MATH IS SET IN FROM THE PANE, THE WAY A BLOCK IS
const MAX_DEPTH: usize = 32; //A BRACE THIS DEEP IS A BRACE

//STRUCTS
//A RECTANGLE OF CELLS PLUS ITS BASELINE ROW
#[derive(Clone)]
pub struct Block
{
    rows: Vec<String>,
    baseline: usize,
}

//ENUMS
enum Node
{
    Sym(String),
    List(Vec<Node>),
    Frac(Box<Node>, Box<Node>),
    Sqrt(Box<Node>, Option<String>),           //ROOT AND ITS DEGREE
    Delim(char, char, Box<Node>),              //\left .. \right, STRETCHED TO WHAT IS BETWEEN THEM
    Script { base: Box<Node>, sup: Option<Box<Node>>, sub: Option<Box<Node>> },
    Big(String),                               //AN OPERATOR WHOSE SCRIPTS GO OVER AND UNDER IT
}

//IMPLEMENTATIONS
impl Block
{
    fn text(text: &str) -> Self
    {
        Self { rows: vec![text.to_owned()], baseline: 0 }
    }

    fn empty() -> Self { Self::text("") }

    fn width(&self) -> usize { self.rows.iter().map(|row| markup::text_width(row)).max().unwrap_or(0) }
    fn height(&self) -> usize { self.rows.len() }
    fn is_flat(&self) -> bool { self.rows.len() == 1 }

    fn pad(&self, width: usize) -> Vec<String> //EVERY ROW OUT TO THE SAME WIDTH, SO COLUMNS STAY COLUMNS
    {
        self.rows.iter().map(|row|
        {
            let mut row = row.clone();

            row.extend(iter::repeat_n(' ', width.saturating_sub(markup::text_width(&row))));

            row
        }).collect()
    }

    fn centre(&self, width: usize) -> Self //CENTRED IN A WIDER BOX
    {
        let left = (width - self.width().min(width)) / 2;

        Self
        {
            rows: self.pad(self.width()).into_iter()
                .map(|row| format!("{}{row}", " ".repeat(left))).collect(),
            baseline: self.baseline,
        }
    }

    //SIDE BY SIDE ON A SHARED BASELINE
    fn beside(self, other: Self) -> Self
    {
        if self.rows.iter().all(String::is_empty) && self.rows.len() == 1 { return other; }

        let above = self.baseline.max(other.baseline);
        let below = (self.height() - self.baseline).max(other.height() - other.baseline);

        let left = self.stretch(above, below);
        let right = other.stretch(above, below);

        let left_width = left.width();

        Self
        {
            rows: left.pad(left_width).into_iter().zip(right.rows)
                .map(|(l, r)| format!("{l}{r}")).collect(),
            baseline: above,
        }
    }

    fn stretch(&self, above: usize, below: usize) -> Self //BLANK ROWS ONTO EITHER END
    {
        let width = self.width();

        let mut rows: Vec<String> = iter::repeat_n(" ".repeat(width), above - self.baseline).collect();

        rows.extend(self.pad(width));
        rows.extend(iter::repeat_n(" ".repeat(width), below - (self.height() - self.baseline)));

        Self { rows, baseline: above }
    }

    //STACKED, THE MIDDLE PIECE'S ROW AS BASELINE
    fn stack(above: Vec<Self>, middle: Self, below: Vec<Self>) -> Self
    {
        let width = above.iter().chain(iter::once(&middle)).chain(below.iter())
            .map(Self::width).max().unwrap_or(0);

        let mut rows: Vec<String> = Vec::new();

        for block in &above { rows.extend(block.centre(width).pad(width)); }

        let baseline = rows.len() + middle.baseline;

        rows.extend(middle.centre(width).pad(width));

        for block in &below { rows.extend(block.centre(width).pad(width)); }

        Self { rows, baseline }
    }
}

//FUNCTIONS
//INLINE MATH, SET LINEARLY
pub fn inline(source: &str, style: Style) -> Vec<Span<'static>>
{
    let nodes = parse(source);
    let block = layout(&nodes, false);

    let text = match block.is_flat()
    {
        true => block.rows[0].trim_end().to_owned(),
        false => linear(&nodes),
    };

    vec![Span::styled(text, math_style(style))]
}

//DISPLAY MATH, IN TWO DIMENSIONS
pub fn display(source: &str, width: u16) -> Vec<Line<'static>>
{
    let nodes = parse(source);
    let block = layout(&nodes, true);
    let style = theme::MATH;

    if block.width() + INDENT.len() > width.max(1) as usize
    {
        return super::state::wrap_line(&Line::from(Span::styled(linear(&nodes), style)), width);
    }

    block.rows.into_iter()
        .map(|row| Line::from(Span::styled(format!("{INDENT}{}", row.trim_end()), style)))
        .collect()
}

fn math_style(style: Style) -> Style //MATH KEEPS THE MESSAGE'S COLOUR WHERE IT HAS ONE
{
    match style.fg
    {
        Some(_) => style,
        None => theme::MATH,
    }
}

//LAYOUT
fn layout(nodes: &[Node], display: bool) -> Block
{
    nodes.iter().fold(Block::empty(), |block, node| block.beside(one(node, display)))
}

fn one(node: &Node, display: bool) -> Block
{
    match node
    {
        Node::Sym(text) => Block::text(text),
        Node::List(nodes) => layout(nodes, display),
        Node::Big(op) => Block::text(op),

        //THE RULE IS THE WIDER SIDE, AND THE BASELINE
        Node::Frac(num, den) =>
        {
            let num = one(num, display);
            let den = one(den, display);

            //DRAW A RULE WIDER THAN WHAT IT DIVIDES
            let stacked = !num.is_flat() || !den.is_flat();
            let width = num.width().max(den.width()) + if stacked { 2 } else { 0 };

            Block::stack(vec![num], Block::text(&"─".repeat(width)), vec![den])
        },

        Node::Sqrt(inner, degree) =>
        {
            let inner = one(inner, display);
            let root = degree.as_deref().and_then(superscript).unwrap_or_default();

            //A MULTI-ROW ROOT IS BRACKETED INSTEAD
            if !inner.is_flat()
            {
                return Block::text(&format!("{root}√")).beside(delimited('(', ')', inner));
            }

            let bar = format!("{}{}", " ".repeat(markup::text_width(&root) + 1), "─".repeat(inner.width()));
            let body = Block::text(&format!("{root}√{}", inner.rows[0]));

            Block::stack(vec![Block::text(&bar)], body, Vec::new())
        },

        Node::Delim(open, close, inner) => delimited(*open, *close, one(inner, display)),

        //LIMITS GO OVER AND UNDER, WHERE THERE IS ROOM
        Node::Script { base, sup, sub } =>
        {
            let big = matches!(**base, Node::Big(_));
            let base = one(base, display);

            if big && display
            {
                let above = sup.iter().map(|node| one(node, false)).collect();
                let below = sub.iter().map(|node| one(node, false)).collect();

                return Block::stack(above, base, below);
            }

            base.beside(scripts(sup.as_deref(), sub.as_deref(), display))
        },
    }
}

//A SCRIPT AS A SUPERSCRIPT, A RAISED ROW, OR ^(..)
fn scripts(sup: Option<&Node>, sub: Option<&Node>, display: bool) -> Block
{
    let sup = sup.map(|node| (one(node, false), linear_one(node)));
    let sub = sub.map(|node| (one(node, false), linear_one(node)));

    let small = |script: &Option<(Block, String)>, map: fn(&str) -> Option<String>|
    {
        script.as_ref().filter(|(block, _)| block.is_flat()).and_then(|(_, text)| map(text))
    };

    let (up, down) = (small(&sup, superscript), small(&sub, subscript));

    match (up, down)
    {
        (Some(up), Some(down)) => Block::text(&format!("{down}{up}")),
        (Some(up), None) if sub.is_none() => Block::text(&up),
        (None, Some(down)) if sup.is_none() => Block::text(&down),

        _ => match display
        {
            //sup, THE BASELINE'S OWN ROW, THEN sub
            true => Block::stack(sup.iter().map(|(block, _)| block.clone()).collect(),
                Block::empty(), sub.iter().map(|(block, _)| block.clone()).collect()),

            false =>
            {
                let mut text = String::new();

                if let Some((_, sub)) = sub.as_ref().filter(|(_, text)| !text.is_empty())
                {
                    text.push_str(&format!("_({sub})"));
                }

                if let Some((_, sup)) = sup.as_ref().filter(|(_, text)| !text.is_empty())
                {
                    text.push_str(&format!("^({sup})"));
                }

                Block::text(&text)
            },
        },
    }
}

//A BRACKET AS TALL AS WHAT IT HOLDS
fn delimited(open: char, close: char, inner: Block) -> Block
{
    if inner.is_flat() || open == '.' && close == '.'
    {
        let (open, close) = (if open == '.' { ' ' } else { open }, if close == '.' { ' ' } else { close });

        return Block::text(&open.to_string()).beside(inner).beside(Block::text(&close.to_string()));
    }

    let bar = |c: char| Block { rows: stretch_delim(c, inner.height()), baseline: inner.baseline };
    let (left, right) = (bar(open), bar(close));

    left.beside(inner).beside(right)
}

fn stretch_delim(delim: char, height: usize) -> Vec<String>
{
    let pieces = match delim
    {
        '(' => ['⎛', '⎜', '⎝'],
        ')' => ['⎞', '⎟', '⎠'],
        '[' => ['⎡', '⎢', '⎣'],
        ']' => ['⎤', '⎥', '⎦'],
        '{' => ['⎧', '⎪', '⎩'],
        '}' => ['⎫', '⎪', '⎭'],
        '.' => [' ', ' ', ' '],
        _ => [delim, delim, delim],
    };

    (0..height).map(|row| match row
    {
        0 => pieces[0].to_string(),
        _ if row == height - 1 => pieces[2].to_string(),
        _ => pieces[1].to_string(),
    }).collect()
}

//THE ONE-ROW FALLBACK
fn linear(nodes: &[Node]) -> String
{
    nodes.iter().map(linear_one).collect()
}

fn linear_one(node: &Node) -> String
{
    match node
    {
        Node::Sym(text) => text.clone(),
        Node::Big(op) => op.clone(),
        Node::List(nodes) => linear(nodes),
        Node::Frac(num, den) => format!("{}/{}", bracketed(num), bracketed(den)),
        Node::Sqrt(inner, degree) => format!("{}√({})",
            degree.as_deref().and_then(superscript).unwrap_or_default(), linear_one(inner)),

        Node::Delim(open, close, inner) => format!("{}{}{}",
            if *open == '.' { ' ' } else { *open }, linear_one(inner),
            if *close == '.' { ' ' } else { *close }),

        Node::Script { base, sup, sub } =>
        {
            let mut out = linear_one(base);

            //AN EMPTY SCRIPT IS NOTHING
            if let Some(text) = sub.as_ref().map(|node| linear_one(node)).filter(|text| !text.is_empty())
            {
                out.push_str(&subscript(&text).unwrap_or(format!("_({text})")));
            }

            if let Some(text) = sup.as_ref().map(|node| linear_one(node)).filter(|text| !text.is_empty())
            {
                out.push_str(&superscript(&text).unwrap_or(format!("^({text})")));
            }

            out
        },
    }
}

//a/b NEEDS NO BRACKETS, (a + b)/2 DOES
fn bracketed(node: &Node) -> String
{
    let text = linear_one(node);

    match text.chars().count() > 1 && !matches!(node, Node::Delim(..))
    {
        true => format!("({text})"),
        false => text,
    }
}

//SCRIPTS, ALL OR NOTHING
fn superscript(text: &str) -> Option<String>
{
    map_chars(text, "0123456789+-=()abcdefghijklmnoprstuvwxyz",
        "⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾ᵃᵇᶜᵈᵉᶠᵍʰⁱʲᵏˡᵐⁿᵒᵖʳˢᵗᵘᵛʷˣʸᶻ")
}

fn subscript(text: &str) -> Option<String>
{
    map_chars(text, "0123456789+-=()aehijklmnoprstuvx",
        "₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎ₐₑₕᵢⱼₖₗₘₙₒₚᵣₛₜᵤᵥₓ")
}

fn map_chars(text: &str, from: &str, to: &str) -> Option<String>
{
    if text.is_empty() { return None; }

    text.chars().map(|c| from.chars().position(|f| f == c).and_then(|i| to.chars().nth(i)))
        .collect()
}

//THE PARSER; AN UNKNOWN WORD IS SET AS ITSELF
struct Parser
{
    chars: Vec<char>,
    pos: usize,
    depth: usize, //NOTHING BOUNDS A MESSAGE'S NESTING BUT THIS
}

impl Parser
{
    fn list(&mut self, group: bool) -> Vec<Node>
    {
        let mut out = Vec::new();

        while self.pos < self.chars.len()
        {
            if self.chars[self.pos] == '}'
            {
                if group { break; }

                self.pos += 1;

                continue;
            }

            //\right BELONGS TO WHOEVER OPENED THE \left
            if self.command_here().as_deref() == Some("right") { break; }

            out.push(self.atom());
        }

        out
    }

    fn atom(&mut self) -> Node
    {
        let base = self.primary();

        let (mut sup, mut sub) = (None, None);

        while let Some(kind) = self.chars.get(self.pos).copied().filter(|c| matches!(c, '^' | '_'))
        {
            self.pos += 1;

            let script = Box::new(self.primary());

            match kind
            {
                '^' => sup = Some(script),
                _ => sub = Some(script),
            }
        }

        match (&sup, &sub)
        {
            (None, None) => base,
            _ => Node::Script { base: Box::new(base), sup, sub },
        }
    }

    fn primary(&mut self) -> Node
    {
        match self.chars.get(self.pos).copied()
        {
            None => Node::Sym(String::new()),

            Some('{') if self.depth < MAX_DEPTH =>
            {
                self.pos += 1;
                self.depth += 1;

                let inner = self.list(true);

                self.depth -= 1;
                self.eat('}');

                Node::List(inner)
            },

            Some('\\') => self.command(),

            //A SCRIPT WITH NO BASE GETS AN EMPTY ONE
            Some('^' | '_') => Node::Sym(String::new()),

            Some(c) =>
            {
                self.pos += 1;

                Node::Sym(c.to_string())
            },
        }
    }

    fn command(&mut self) -> Node
    {
        let Some(name) = self.command_here() else
        {
            self.pos += 1;

            return Node::Sym(String::new());
        };

        self.pos += 1 + name.chars().count().max(1);

        match name.as_str()
        {
            "frac" | "dfrac" | "tfrac" =>
                Node::Frac(Box::new(self.argument()), Box::new(self.argument())),

            "sqrt" =>
            {
                let degree = self.optional();

                Node::Sqrt(Box::new(self.argument()), degree)
            },

            //THE CONTENT IS WORDS, TAKEN AS TYPED
            "text" | "textrm" | "mathrm" | "mathbf" | "mathit" | "operatorname" =>
                Node::Sym(self.raw_argument()),

            "mathbb" => Node::Sym(alphabet(&self.raw_argument(), BLACKBOARD)),
            "mathcal" | "mathscr" => Node::Sym(alphabet(&self.raw_argument(), SCRIPT_CAPS)),

            "left" if self.depth < MAX_DEPTH =>
            {
                self.depth += 1;

                let open = self.delimiter();
                let inner = self.list(false);

                //A MISSING \right LEAVES THE BRACKET UNPARTNERED
                let close = match self.command_here().as_deref() == Some("right")
                {
                    true =>
                    {
                        self.pos += "\\right".len();

                        self.delimiter()
                    },

                    false => '.',
                };

                self.depth -= 1;

                Node::Delim(open, close, Box::new(Node::List(inner)))
            },

            "right" => Node::Sym(String::new()),

            _ if BIG.contains(&name.as_str()) => Node::Big(symbol(&name).unwrap_or(name)),

            _ => match symbol(&name)
            {
                Some(sym) => Node::Sym(sym),
                None => Node::Sym(name),
            },
        }
    }

    fn argument(&mut self) -> Node //{...}, OR THE SINGLE THING AFTER IT
    {
        self.skip_spaces();

        self.primary()
    }

    fn raw_argument(&mut self) -> String //THE BRACES' CONTENT, UNPARSED
    {
        self.skip_spaces();

        if self.chars.get(self.pos) != Some(&'{')
        {
            return match self.chars.get(self.pos).copied()
            {
                Some(c) => { self.pos += 1; c.to_string() },
                None => String::new(),
            };
        }

        self.pos += 1;

        let start = self.pos;
        let mut depth = 1usize;

        while self.pos < self.chars.len() && depth > 0
        {
            match self.chars[self.pos]
            {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {},
            }

            self.pos += 1;
        }

        self.chars[start..self.pos.saturating_sub(1)].iter().collect()
    }

    fn optional(&mut self) -> Option<String> //\sqrt[3]{..}
    {
        if self.chars.get(self.pos) != Some(&'[') { return None; }

        let start = self.pos + 1;
        let end = (start..self.chars.len()).find(|i| self.chars[*i] == ']')?;

        self.pos = end + 1;

        Some(self.chars[start..end].iter().collect())
    }

    fn delimiter(&mut self) -> char
    {
        self.skip_spaces();

        match self.command_here()
        {
            //\left\{ IS THE BRACKET, NOT THE COMMAND
            Some(name) =>
            {
                self.pos += 1 + name.chars().count().max(1);

                match name.as_str()
                {
                    "{" | "lbrace" => '{',
                    "}" | "rbrace" => '}',
                    "|" | "vert" | "Vert" => '|',
                    _ => '.',
                }
            },

            None => match self.chars.get(self.pos).copied()
            {
                Some(c) => { self.pos += 1; c },
                None => '.',
            },
        }
    }

    fn command_here(&self) -> Option<String> //THE COMMAND STARTING AT THE CURSOR, WITHOUT CONSUMING IT
    {
        if self.chars.get(self.pos) != Some(&'\\') { return None; }

        let name: String = self.chars[self.pos + 1..].iter().take_while(|c| c.is_ascii_alphabetic()).collect();

        match name.is_empty()
        {
            true => self.chars.get(self.pos + 1).map(char::to_string),
            false => Some(name),
        }
    }

    fn skip_spaces(&mut self)
    {
        while self.chars.get(self.pos).is_some_and(|c| *c == ' ') { self.pos += 1; }
    }

    fn eat(&mut self, c: char)
    {
        if self.chars.get(self.pos) == Some(&c) { self.pos += 1; }
    }
}

fn parse(source: &str) -> Vec<Node>
{
    Parser { chars: source.chars().collect(), pos: 0, depth: 0 }.list(false)
}

fn alphabet(text: &str, table: &[(char, char)]) -> String
{
    text.chars().map(|c| table.iter().find(|(from, _)| *from == c).map(|(_, to)| *to).unwrap_or(c)).collect()
}

fn symbol(name: &str) -> Option<String>
{
    SYMBOLS.iter().find(|(command, _)| *command == name).map(|(_, sym)| (*sym).to_owned())
}

//CONSTS
//OPERATORS WHOSE SCRIPTS ARE LIMITS
const BIG: [&str; 15] = ["sum", "prod", "coprod", "bigcup", "bigcap", "bigoplus", "bigotimes", "bigvee",
    "bigwedge", "lim", "limsup", "liminf", "max", "min", "sup"];

const BLACKBOARD: &[(char, char)] = &[('A', '𝔸'), ('B', '𝔹'), ('C', 'ℂ'), ('D', '𝔻'), ('E', '𝔼'),
    ('F', '𝔽'), ('G', '𝔾'), ('H', 'ℍ'), ('I', '𝕀'), ('J', '𝕁'), ('K', '𝕂'), ('L', '𝕃'), ('M', '𝕄'),
    ('N', 'ℕ'), ('O', '𝕆'), ('P', 'ℙ'), ('Q', 'ℚ'), ('R', 'ℝ'), ('S', '𝕊'), ('T', '𝕋'), ('U', '𝕌'),
    ('V', '𝕍'), ('W', '𝕎'), ('X', '𝕏'), ('Y', '𝕐'), ('Z', 'ℤ')];

const SCRIPT_CAPS: &[(char, char)] = &[('A', '𝒜'), ('B', 'ℬ'), ('C', '𝒞'), ('D', '𝒟'), ('E', 'ℰ'),
    ('F', 'ℱ'), ('G', '𝒢'), ('H', 'ℋ'), ('I', 'ℐ'), ('J', '𝒥'), ('K', '𝒦'), ('L', 'ℒ'), ('M', 'ℳ'),
    ('N', '𝒩'), ('O', '𝒪'), ('P', '𝒫'), ('Q', '𝒬'), ('R', 'ℛ'), ('S', '𝒮'), ('T', '𝒯'), ('U', '𝒰'),
    ('V', '𝒱'), ('W', '𝒲'), ('X', '𝒳'), ('Y', '𝒴'), ('Z', '𝒵')];

const SYMBOLS: &[(&str, &str)] =
&[
    //GREEK
    ("alpha", "α"), ("beta", "β"), ("gamma", "γ"), ("delta", "δ"), ("epsilon", "ε"),
    ("varepsilon", "ε"), ("zeta", "ζ"), ("eta", "η"), ("theta", "θ"), ("vartheta", "ϑ"),
    ("iota", "ι"), ("kappa", "κ"), ("lambda", "λ"), ("mu", "μ"), ("nu", "ν"), ("xi", "ξ"),
    ("pi", "π"), ("varpi", "ϖ"), ("rho", "ρ"), ("varrho", "ϱ"), ("sigma", "σ"), ("varsigma", "ς"),
    ("tau", "τ"), ("upsilon", "υ"), ("phi", "φ"), ("varphi", "φ"), ("chi", "χ"), ("psi", "ψ"),
    ("omega", "ω"), ("Gamma", "Γ"), ("Delta", "Δ"), ("Theta", "Θ"), ("Lambda", "Λ"), ("Xi", "Ξ"),
    ("Pi", "Π"), ("Sigma", "Σ"), ("Upsilon", "Υ"), ("Phi", "Φ"), ("Psi", "Ψ"), ("Omega", "Ω"),

    //OPERATORS AND RELATIONS
    ("times", "×"), ("div", "÷"), ("pm", "±"), ("mp", "∓"), ("cdot", "⋅"), ("ast", "∗"),
    ("star", "⋆"), ("circ", "∘"), ("bullet", "∙"), ("oplus", "⊕"), ("ominus", "⊖"),
    ("otimes", "⊗"), ("odot", "⊙"), ("leq", "≤"), ("le", "≤"), ("geq", "≥"), ("ge", "≥"),
    ("neq", "≠"), ("ne", "≠"), ("approx", "≈"), ("equiv", "≡"), ("sim", "∼"), ("simeq", "≃"),
    ("cong", "≅"), ("propto", "∝"), ("ll", "≪"), ("gg", "≫"), ("subset", "⊂"), ("subseteq", "⊆"),
    ("supset", "⊃"), ("supseteq", "⊇"), ("in", "∈"), ("notin", "∉"), ("ni", "∋"), ("cup", "∪"),
    ("cap", "∩"), ("setminus", "∖"), ("emptyset", "∅"), ("varnothing", "∅"), ("forall", "∀"),
    ("exists", "∃"), ("nexists", "∄"), ("neg", "¬"), ("lnot", "¬"), ("land", "∧"), ("lor", "∨"),
    ("wedge", "∧"), ("vee", "∨"), ("perp", "⊥"), ("parallel", "∥"), ("angle", "∠"),
    ("therefore", "∴"), ("because", "∵"), ("mid", "∣"), ("nmid", "∤"),

    //SIGNS
    ("infty", "∞"), ("partial", "∂"), ("nabla", "∇"), ("hbar", "ℏ"), ("ell", "ℓ"), ("Re", "ℜ"),
    ("Im", "ℑ"), ("aleph", "ℵ"), ("deg", "°"), ("prime", "′"), ("dots", "…"), ("ldots", "…"),
    ("cdots", "⋯"), ("vdots", "⋮"), ("ddots", "⋱"), ("checkmark", "✓"), ("dagger", "†"),

    //ARROWS
    ("to", "→"), ("gets", "←"), ("rightarrow", "→"), ("leftarrow", "←"), ("Rightarrow", "⇒"),
    ("Leftarrow", "⇐"), ("leftrightarrow", "↔"), ("Leftrightarrow", "⇔"), ("mapsto", "↦"),
    ("uparrow", "↑"), ("downarrow", "↓"), ("implies", "⟹"), ("iff", "⟺"), ("hookrightarrow", "↪"),

    //BIG OPERATORS
    ("sum", "∑"), ("prod", "∏"), ("coprod", "∐"), ("int", "∫"), ("iint", "∬"), ("iiint", "∭"),
    ("oint", "∮"), ("bigcup", "⋃"), ("bigcap", "⋂"), ("bigoplus", "⨁"), ("bigotimes", "⨂"),
    ("bigvee", "⋁"), ("bigwedge", "⋀"),

    //SPACING AND ESCAPES
    ("quad", "  "), ("qquad", "    "), (",", " "), (";", " "), (":", " "), ("!", ""), (" ", " "),
    ("{", "{"), ("}", "}"), ("%", "%"), ("&", "&"), ("#", "#"), ("_", "_"), ("$", "$"),
];
