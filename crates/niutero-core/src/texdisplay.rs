//! Display-only rendering of BibTeX/LaTeX field text to Unicode, with emphasis.
//!
//! BibTeX field values are LaTeX source: capitalized words are wrapped in
//! case-protection braces (`{{Beyond}} {{Morphology}}`), accents are escapes
//! (`M\"uller`, `Be\~nat`), and titles may use `\emph{...}` / `\textbf{...}`.
//! A UI shouldn't show that raw. [`to_runs`] turns it into styled text runs and
//! [`to_display`] flattens that to a plain Unicode string.
//!
//! This is **one-way and display-only** — it never touches the stored value, so
//! the `.bib` source of truth stays byte-stable. Editing and search must keep
//! operating on the raw field; only presentation goes through here.
//!
//! Scope (Tiers A+B): brace stripping, accent + special-character decoding,
//! dashes/quotes, and `\emph`/`\textbf`/`\textit` emphasis. Math (`$...$`,
//! `\(...\)`) is passed through **verbatim** — it is not yet typeset.

/// A run of text sharing one emphasis style. Adjacent same-style runs are merged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
}

/// Render LaTeX-ish field text to styled Unicode runs (Tiers A+B).
pub fn to_runs(input: &str) -> Vec<Run> {
    let chars: Vec<char> = input.chars().collect();
    let mut b = Builder::default();
    let mut i = 0;
    process(&chars, &mut i, false, false, false, &mut b);
    // Collapse runs of whitespace introduced by command stripping.
    for r in &mut b.runs {
        r.text = tidy(&r.text);
    }
    b.runs.retain(|r| !r.text.is_empty());
    b.runs
}

/// Render LaTeX-ish field text to a plain Unicode string (Tier A) — the runs
/// with their styling dropped.
pub fn to_display(input: &str) -> String {
    let mut out = String::new();
    for r in to_runs(input) {
        out.push_str(&r.text);
    }
    out
}

// ---------------------------------------------------------------- internals

#[derive(Default)]
struct Builder {
    runs: Vec<Run>,
}

impl Builder {
    fn push(&mut self, t: &str, bold: bool, italic: bool) {
        if t.is_empty() {
            return;
        }
        if let Some(last) = self.runs.last_mut() {
            if last.bold == bold && last.italic == italic {
                last.text.push_str(t);
                return;
            }
        }
        self.runs.push(Run {
            text: t.to_string(),
            bold,
            italic,
        });
    }
    fn ch(&mut self, c: char, bold: bool, italic: bool) {
        let mut buf = [0u8; 4];
        self.push(c.encode_utf8(&mut buf), bold, italic);
    }
}

/// Process `chars[*i..]` with the current `(bold, italic)` style. When
/// `stop_at_close` is set, returns on the matching unescaped `}` (consuming it);
/// otherwise runs to the end. `bold`/`italic` may change locally via in-group
/// declarations (`\bf`, `\itshape`, …), which persist until this scope ends.
fn process(
    chars: &[char],
    i: &mut usize,
    mut bold: bool,
    mut italic: bool,
    stop_at_close: bool,
    b: &mut Builder,
) {
    while *i < chars.len() {
        let c = chars[*i];
        match c {
            '}' if stop_at_close => {
                *i += 1;
                return;
            }
            '}' => {
                *i += 1; // stray close brace — drop
            }
            '{' => {
                *i += 1;
                // A plain group inherits the current style.
                process(chars, i, bold, italic, true, b);
            }
            '$' => {
                // Math: pass through verbatim, delimiters included (Tier C TODO).
                let start = *i;
                *i += 1;
                while *i < chars.len() && chars[*i] != '$' {
                    *i += 1;
                }
                if *i < chars.len() {
                    *i += 1; // closing $
                }
                let math: String = chars[start..*i].iter().collect();
                b.push(&math, bold, italic);
            }
            '\\' => {
                *i += 1;
                command(chars, i, &mut bold, &mut italic, b);
            }
            '~' => {
                *i += 1;
                b.ch('\u{a0}', bold, italic); // non-breaking space
            }
            '-' => {
                // -- → en dash, --- → em dash
                let dashes = chars[*i..].iter().take_while(|&&x| x == '-').count();
                match dashes {
                    n if n >= 3 => {
                        b.ch('\u{2014}', bold, italic);
                        *i += 3;
                    }
                    2 => {
                        b.ch('\u{2013}', bold, italic);
                        *i += 2;
                    }
                    _ => {
                        b.ch('-', bold, italic);
                        *i += 1;
                    }
                }
            }
            '`' => {
                if chars.get(*i + 1) == Some(&'`') {
                    b.ch('\u{201C}', bold, italic); // “
                    *i += 2;
                } else {
                    b.ch('\u{2018}', bold, italic); // ‘
                    *i += 1;
                }
            }
            '\'' => {
                if chars.get(*i + 1) == Some(&'\'') {
                    b.ch('\u{201D}', bold, italic); // ”
                    *i += 2;
                } else {
                    b.ch('\'', bold, italic); // keep a lone apostrophe
                    *i += 1;
                }
            }
            _ => {
                b.ch(c, bold, italic);
                *i += 1;
            }
        }
    }
}

/// Handle a `\` command at `chars[*i..]` (the backslash already consumed).
fn command(chars: &[char], i: &mut usize, bold: &mut bool, italic: &mut bool, b: &mut Builder) {
    if *i >= chars.len() {
        return;
    }
    let first = chars[*i];

    // Single-char (symbol) control sequences: `\&`, `\'e`, `\"{o}`, `\,`, …
    if !first.is_ascii_alphabetic() {
        // Accent escapes whose accent char is a symbol.
        if matches!(first, '\'' | '`' | '^' | '"' | '~' | '=' | '.') {
            *i += 1;
            let base = read_accent_target(chars, i);
            b.push(&apply_accent(first, &base), *bold, *italic);
            return;
        }
        // Escaped literals / spacing.
        *i += 1;
        let s: Option<&str> = match first {
            '&' => Some("&"),
            '%' => Some("%"),
            '$' => Some("$"),
            '#' => Some("#"),
            '_' => Some("_"),
            '{' => Some("{"),
            '}' => Some("}"),
            '\\' => Some(" "), // `\\` line break → space
            ' ' | '/' | ',' | ';' | ':' | '!' => Some(" "), // (thin) spaces
            '-' => Some(""),   // discretionary hyphen
            _ => Some(""),
        };
        if let Some(s) = s {
            b.push(s, *bold, *italic);
        }
        return;
    }

    // Multi-letter control word.
    let start = *i;
    while *i < chars.len() && chars[*i].is_ascii_alphabetic() {
        *i += 1;
    }
    let name: String = chars[start..*i].iter().collect();
    // LaTeX swallows one space after a control word.
    if chars.get(*i) == Some(&' ') {
        *i += 1;
    }

    // Letter-named accents: \u \v \H \r \c \k \b \d \t (take a target).
    if matches!(
        name.as_str(),
        "u" | "v" | "H" | "r" | "c" | "k" | "b" | "d" | "t"
    ) && wants_target(chars, *i)
    {
        let key = name.chars().next().unwrap();
        let base = read_accent_target(chars, i);
        b.push(&apply_accent(key, &base), *bold, *italic);
        return;
    }

    // Emphasis commands that take a braced argument.
    if let Some((nb, ni)) = arg_style(&name) {
        // Skip optional space already eaten; expect `{`.
        if chars.get(*i) == Some(&'{') {
            *i += 1;
            process(chars, i, *bold || nb, *italic || ni, true, b);
        }
        return;
    }

    // In-group style declarations: persist until the enclosing `}`.
    match name.as_str() {
        "bf" | "bfseries" => {
            *bold = true;
            return;
        }
        "it" | "itshape" | "em" | "sl" | "slshape" => {
            *italic = true;
            return;
        }
        "normalfont" | "rmfamily" | "sffamily" | "mdseries" | "upshape" => {
            *bold = false;
            *italic = false;
            return;
        }
        "tt" | "ttfamily" | "sc" | "scshape" | "rm" | "sf" => return, // family change: ignore
        _ => {}
    }

    // Named symbols / letters.
    if let Some(s) = named_symbol(&name) {
        b.push(s, *bold, *italic);
        return;
    }

    // Unknown command: keep a following braced argument's content (strip the
    // command), else drop it.
    if chars.get(*i) == Some(&'{') {
        *i += 1;
        process(chars, i, *bold, *italic, true, b);
    }
}

/// Does the position look like an accent target (so `\c{c}` is an accent but a
/// bare `\c` at a boundary isn't mis-eaten)?
fn wants_target(chars: &[char], i: usize) -> bool {
    matches!(chars.get(i), Some('{') | Some('\\'))
        || chars.get(i).is_some_and(|c| c.is_ascii_alphabetic())
}

/// Read the base of an accent: a `{...}` group, a `\i`/`\j` dotless letter, or
/// the next single character.
fn read_accent_target(chars: &[char], i: &mut usize) -> String {
    // optional spaces
    while chars.get(*i) == Some(&' ') {
        *i += 1;
    }
    match chars.get(*i) {
        Some('{') => {
            *i += 1;
            let mut inner = String::new();
            // a brace group; recurse minimally (accents inside are uncommon)
            let mut depth = 1;
            while *i < chars.len() && depth > 0 {
                match chars[*i] {
                    '{' => {
                        depth += 1;
                        *i += 1;
                    }
                    '}' => {
                        depth -= 1;
                        *i += 1;
                    }
                    '\\' => {
                        // \i / \j dotless letters
                        *i += 1;
                        if let Some(&l) = chars.get(*i) {
                            inner.push(if l == 'i' {
                                'i'
                            } else if l == 'j' {
                                'j'
                            } else {
                                l
                            });
                            *i += 1;
                        }
                    }
                    other => {
                        inner.push(other);
                        *i += 1;
                    }
                }
            }
            inner
        }
        Some('\\') => {
            *i += 1;
            // \i or \j (dotless) used as accent base
            if let Some(&l) = chars.get(*i) {
                *i += 1;
                match l {
                    'i' => "i".into(),
                    'j' => "j".into(),
                    _ => l.to_string(),
                }
            } else {
                String::new()
            }
        }
        Some(&c) => {
            *i += 1;
            c.to_string()
        }
        None => String::new(),
    }
}

/// Emphasis commands that wrap a braced argument → (adds bold, adds italic).
fn arg_style(name: &str) -> Option<(bool, bool)> {
    match name {
        "textbf" | "mathbf" | "boldsymbol" => Some((true, false)),
        "textit" | "emph" | "textsl" | "mathit" | "textsc" => Some((false, true)),
        // keep content, no emphasis
        "textrm" | "texttt" | "textnormal" | "textmd" | "textup" | "mbox" | "text"
        | "ensuremath" | "mathrm" | "operatorname" => Some((false, false)),
        _ => None,
    }
}

/// Named-symbol and special-letter commands → their Unicode.
fn named_symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "ss" => "ß",
        "ae" => "æ",
        "AE" => "Æ",
        "oe" => "œ",
        "OE" => "Œ",
        "o" => "ø",
        "O" => "Ø",
        "aa" => "å",
        "AA" => "Å",
        "l" => "ł",
        "L" => "Ł",
        "i" => "ı", // dotless i
        "j" => "ȷ", // dotless j
        "dh" => "ð",
        "DH" => "Ð",
        "th" => "þ",
        "TH" => "Þ",
        "dag" => "†",
        "ddag" => "‡",
        "S" => "§",
        "P" => "¶",
        "copyright" => "©",
        "pounds" => "£",
        "textbackslash" => "\\",
        "textbar" => "|",
        "textasciitilde" => "~",
        "textquotesingle" => "'",
        "textquotedbl" => "\"",
        "textendash" => "–",
        "textemdash" => "—",
        "ldots" | "dots" | "textellipsis" => "…",
        "nobreakspace" => "\u{a0}",
        _ => return None,
    })
}

/// Combine an accent with its base character, preferring a precomposed Unicode
/// code point; falling back to base + a combining mark for rarer pairs.
fn apply_accent(accent: char, base: &str) -> String {
    let mut ch = base.chars();
    let (Some(b), None) = (ch.next(), ch.next()) else {
        // multi-char or empty base: just return it un-accented
        return base.to_string();
    };
    if let Some(c) = precomposed(accent, b) {
        return c.to_string();
    }
    // Fallback: base + combining diacritic (rendering depends on the font).
    let combining = match accent {
        '\'' => '\u{0301}',
        '`' => '\u{0300}',
        '^' => '\u{0302}',
        '"' => '\u{0308}',
        '~' => '\u{0303}',
        '=' => '\u{0304}',
        '.' => '\u{0307}',
        'u' => '\u{0306}',
        'v' => '\u{030C}',
        'H' => '\u{030B}',
        'r' => '\u{030A}',
        'c' => '\u{0327}',
        'k' => '\u{0328}',
        'd' => '\u{0323}',
        'b' => '\u{0331}',
        't' => '\u{0361}',
        _ => return b.to_string(),
    };
    let mut s = String::new();
    s.push(b);
    s.push(combining);
    s
}

/// Precomposed Unicode for the common (accent, base) pairs found in `.bib`
/// author names and titles. Returns `None` for anything not tabulated (the
/// caller then falls back to a combining mark).
fn precomposed(accent: char, base: char) -> Option<char> {
    let r = match (accent, base) {
        // acute
        ('\'', 'a') => 'á',
        ('\'', 'e') => 'é',
        ('\'', 'i') => 'í',
        ('\'', 'o') => 'ó',
        ('\'', 'u') => 'ú',
        ('\'', 'y') => 'ý',
        ('\'', 'c') => 'ć',
        ('\'', 'n') => 'ń',
        ('\'', 's') => 'ś',
        ('\'', 'z') => 'ź',
        ('\'', 'r') => 'ŕ',
        ('\'', 'l') => 'ĺ',
        ('\'', 'g') => 'ǵ',
        ('\'', 'A') => 'Á',
        ('\'', 'E') => 'É',
        ('\'', 'I') => 'Í',
        ('\'', 'O') => 'Ó',
        ('\'', 'U') => 'Ú',
        ('\'', 'Y') => 'Ý',
        ('\'', 'C') => 'Ć',
        ('\'', 'N') => 'Ń',
        ('\'', 'S') => 'Ś',
        ('\'', 'Z') => 'Ź',
        // grave
        ('`', 'a') => 'à',
        ('`', 'e') => 'è',
        ('`', 'i') => 'ì',
        ('`', 'o') => 'ò',
        ('`', 'u') => 'ù',
        ('`', 'n') => 'ǹ',
        ('`', 'A') => 'À',
        ('`', 'E') => 'È',
        ('`', 'I') => 'Ì',
        ('`', 'O') => 'Ò',
        ('`', 'U') => 'Ù',
        // circumflex
        ('^', 'a') => 'â',
        ('^', 'e') => 'ê',
        ('^', 'i') => 'î',
        ('^', 'o') => 'ô',
        ('^', 'u') => 'û',
        ('^', 'w') => 'ŵ',
        ('^', 'y') => 'ŷ',
        ('^', 'c') => 'ĉ',
        ('^', 'g') => 'ĝ',
        ('^', 's') => 'ŝ',
        ('^', 'A') => 'Â',
        ('^', 'E') => 'Ê',
        ('^', 'I') => 'Î',
        ('^', 'O') => 'Ô',
        ('^', 'U') => 'Û',
        // diaeresis / umlaut
        ('"', 'a') => 'ä',
        ('"', 'e') => 'ë',
        ('"', 'i') => 'ï',
        ('"', 'o') => 'ö',
        ('"', 'u') => 'ü',
        ('"', 'y') => 'ÿ',
        ('"', 'A') => 'Ä',
        ('"', 'E') => 'Ë',
        ('"', 'I') => 'Ï',
        ('"', 'O') => 'Ö',
        ('"', 'U') => 'Ü',
        // tilde
        ('~', 'a') => 'ã',
        ('~', 'o') => 'õ',
        ('~', 'n') => 'ñ',
        ('~', 'u') => 'ũ',
        ('~', 'A') => 'Ã',
        ('~', 'O') => 'Õ',
        ('~', 'N') => 'Ñ',
        // macron
        ('=', 'a') => 'ā',
        ('=', 'e') => 'ē',
        ('=', 'i') => 'ī',
        ('=', 'o') => 'ō',
        ('=', 'u') => 'ū',
        ('=', 'A') => 'Ā',
        ('=', 'O') => 'Ō',
        ('=', 'U') => 'Ū',
        // caron / háček
        ('v', 'c') => 'č',
        ('v', 's') => 'š',
        ('v', 'z') => 'ž',
        ('v', 'r') => 'ř',
        ('v', 'e') => 'ě',
        ('v', 'n') => 'ň',
        ('v', 'd') => 'ď',
        ('v', 't') => 'ť',
        ('v', 'l') => 'ľ',
        ('v', 'g') => 'ǧ',
        ('v', 'C') => 'Č',
        ('v', 'S') => 'Š',
        ('v', 'Z') => 'Ž',
        ('v', 'R') => 'Ř',
        ('v', 'N') => 'Ň',
        ('v', 'E') => 'Ě',
        // ring
        ('r', 'a') => 'å',
        ('r', 'u') => 'ů',
        ('r', 'A') => 'Å',
        // breve
        ('u', 'a') => 'ă',
        ('u', 'g') => 'ğ',
        ('u', 'u') => 'ŭ',
        ('u', 'e') => 'ĕ',
        ('u', 'A') => 'Ă',
        ('u', 'G') => 'Ğ',
        // dot above
        ('.', 'z') => 'ż',
        ('.', 'e') => 'ė',
        ('.', 'c') => 'ċ',
        ('.', 'g') => 'ġ',
        ('.', 'Z') => 'Ż',
        ('.', 'I') => 'İ',
        // cedilla
        ('c', 'c') => 'ç',
        ('c', 's') => 'ş',
        ('c', 'g') => 'ģ',
        ('c', 't') => 'ţ',
        ('c', 'C') => 'Ç',
        ('c', 'S') => 'Ş',
        // ogonek
        ('k', 'a') => 'ą',
        ('k', 'e') => 'ę',
        ('k', 'A') => 'Ą',
        ('k', 'E') => 'Ę',
        // double acute (Hungarian)
        ('H', 'o') => 'ő',
        ('H', 'u') => 'ű',
        ('H', 'O') => 'Ő',
        ('H', 'U') => 'Ű',
        _ => return None,
    };
    Some(r)
}

/// Collapse internal runs of ASCII whitespace to single spaces (command and
/// brace stripping can leave doubled spaces). Leading/trailing space is kept so
/// run boundaries don't lose word separation.
fn tidy(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        let is_space = c == ' ' || c == '\t' || c == '\n' || c == '\r';
        if is_space {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> String {
        to_display(s)
    }

    #[test]
    fn strips_case_protection_braces() {
        assert_eq!(d("{{Beyond}} {{Morphology}}"), "Beyond Morphology");
        assert_eq!(
            d("{Core} {Syntax:} a {Minimalist} {Approach}"),
            "Core Syntax: a Minimalist Approach"
        );
        assert_eq!(d("{DNA} sequencing"), "DNA sequencing");
    }

    #[test]
    fn decodes_accents() {
        assert_eq!(d(r#"M\"uller"#), "Müller");
        assert_eq!(d(r#"Be\~nat"#), "Beñat");
        assert_eq!(d(r#"Erd\H{o}s"#), "Erdős");
        assert_eq!(d(r#"\v{S}m\'{\i}d"#), "Šmíd");
        assert_eq!(d(r#"Fran\c{c}ois"#), "François");
        assert_eq!(d(r#"G\"odel, Kurt"#), "Gödel, Kurt");
        assert_eq!(d(r#"{\AA}str\"om"#), "Åström");
    }

    #[test]
    fn decodes_specials_and_dashes() {
        assert_eq!(d(r"Tom \& Jerry"), "Tom & Jerry");
        assert_eq!(d("pages 10--20"), "pages 10–20");
        assert_eq!(d("a---b"), "a—b");
        // `~` is a non-breaking space (keeps "et al." together).
        assert_eq!(d("Sch\\\"on~et~al."), "Schön\u{a0}et\u{a0}al.");
        assert_eq!(d("``quoted''"), "“quoted”");
        assert_eq!(d(r"50\% off"), "50% off");
        assert_eq!(d(r"\ss"), "ß");
    }

    #[test]
    fn emphasis_runs() {
        let r = to_runs(r"A \emph{sparse} model");
        assert_eq!(
            r,
            vec![
                Run {
                    text: "A ".into(),
                    bold: false,
                    italic: false
                },
                Run {
                    text: "sparse".into(),
                    bold: false,
                    italic: true
                },
                Run {
                    text: " model".into(),
                    bold: false,
                    italic: false
                },
            ]
        );
        let r = to_runs(r"{\bf Bold} then normal");
        assert_eq!(
            r[0],
            Run {
                text: "Bold".into(),
                bold: true,
                italic: false
            }
        );
        assert_eq!(
            r[1],
            Run {
                text: " then normal".into(),
                bold: false,
                italic: false
            }
        );
    }

    #[test]
    fn math_is_passed_through_verbatim() {
        assert_eq!(d(r"Complexity $O(n^2)$ bound"), "Complexity $O(n^2)$ bound");
        assert_eq!(d(r"$\alpha$-decay"), "$\\alpha$-decay");
    }

    #[test]
    fn unknown_command_keeps_arg_drops_name() {
        assert_eq!(d(r"\url{http://x}"), "http://x");
        assert_eq!(d(r"\foobar baz"), "baz");
    }

    #[test]
    fn plain_text_unchanged() {
        assert_eq!(
            d("Scaling and Evaluating Sparse Autoencoders"),
            "Scaling and Evaluating Sparse Autoencoders"
        );
        assert_eq!(d(""), "");
    }
}
