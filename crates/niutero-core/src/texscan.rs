//! LaTeX citation scanning: which library keys a set of `.tex`/`.aux` files
//! cite, and how that compares to the library. Pure logic, no IO.

use serde::Serialize;
use std::collections::BTreeSet;

/// Keys cited by some LaTeX source, plus whether `\nocite{*}` was seen.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Cited {
    pub keys: BTreeSet<String>,
    pub cite_all: bool,
}

/// Used / missing / unused split of cited keys against a library.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct TexReport {
    /// Cited and present in the library.
    pub used: Vec<String>,
    /// Cited but not in the library (undefined references).
    pub missing: Vec<String>,
    /// In the library but never cited (empty when `\nocite{*}` is present).
    pub unused: Vec<String>,
    pub cite_all: bool,
}

/// Extract cited keys from LaTeX/aux source. Recognizes any `\…cite…{…}`
/// command (`\cite`, `\citep`, `\citet`, `\Cite`, `\nocite`, …) and the `.aux`
/// forms `\citation{…}` / `\bibcite{…}`; honors a starred command and `[…]`
/// option groups; `\nocite{*}` sets `cite_all`. Unescaped `%`-to-end-of-line
/// comments are ignored.
pub fn cited_keys(src: &str) -> Cited {
    let cleaned = strip_comments(src);
    let b = cleaned.as_bytes();
    let n = b.len();
    let mut out = Cited::default();
    let mut i = 0;
    while i < n {
        if b[i] != b'\\' {
            i += 1;
            continue;
        }
        let name_start = i + 1;
        let mut j = name_start;
        while j < n && b[j].is_ascii_alphabetic() {
            j += 1;
        }
        let name = cleaned[name_start..j].to_ascii_lowercase();
        if !(name.contains("cite") || name == "citation") {
            i = (i + 1).max(j);
            continue;
        }
        // Skip an optional `*` then any number of `[...]` option groups.
        let mut k = j;
        if k < n && b[k] == b'*' {
            k += 1;
        }
        loop {
            while k < n && b[k].is_ascii_whitespace() {
                k += 1;
            }
            if k < n && b[k] == b'[' {
                while k < n && b[k] != b']' {
                    k += 1;
                }
                k += 1; // consume ']' (or pass end)
            } else {
                break;
            }
        }
        if k < n && b[k] == b'{' {
            let key_start = k + 1;
            let mut m = key_start;
            while m < n && b[m] != b'}' {
                m += 1;
            }
            let is_nocite = name == "nocite";
            for key in cleaned[key_start..m].split(',') {
                let key = key.trim();
                if key.is_empty() {
                    continue;
                }
                if is_nocite && key == "*" {
                    out.cite_all = true;
                } else {
                    out.keys.insert(key.to_string());
                }
            }
            i = m + 1;
        } else {
            i = k.max(i + 1);
        }
    }
    out
}

/// Compute used / missing / unused. With `\nocite{*}`, every library key counts
/// as used, so `unused` is empty.
pub fn report(lib_keys: &BTreeSet<String>, cited: &Cited) -> TexReport {
    let used: Vec<String> = cited.keys.intersection(lib_keys).cloned().collect();
    let missing: Vec<String> = cited.keys.difference(lib_keys).cloned().collect();
    let unused: Vec<String> = if cited.cite_all {
        Vec::new()
    } else {
        lib_keys.difference(&cited.keys).cloned().collect()
    };
    TexReport {
        used,
        missing,
        unused,
        cite_all: cited.cite_all,
    }
}

fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        let bytes = line.as_bytes();
        let mut cut = line.len();
        for i in 0..bytes.len() {
            if bytes[i] == b'%' && (i == 0 || bytes[i - 1] != b'\\') {
                cut = i;
                break;
            }
        }
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(src: &str) -> Vec<String> {
        cited_keys(src).keys.into_iter().collect()
    }

    #[test]
    fn basic_and_comma_separated() {
        assert_eq!(keys(r"\cite{a,b, c}"), vec!["a", "b", "c"]);
    }

    #[test]
    fn variants_options_and_star() {
        let src = r"\citep[see][p.~5]{x} \citet{y} \Cite{z} \cite*{w}";
        assert_eq!(keys(src), vec!["w", "x", "y", "z"]);
    }

    #[test]
    fn nocite_star_sets_cite_all() {
        let c = cited_keys(r"\nocite{a} \nocite{*}");
        assert_eq!(c.keys.into_iter().collect::<Vec<_>>(), vec!["a"]);
        assert!(c.cite_all);
    }

    #[test]
    fn comments_are_ignored() {
        let src = "% \\cite{ignored}\n\\cite{real} % \\cite{also_ignored}";
        assert_eq!(keys(src), vec!["real"]);
    }

    #[test]
    fn aux_citation_form() {
        assert_eq!(keys("\\citation{k1}\n\\citation{k2}\n"), vec!["k1", "k2"]);
    }

    #[test]
    fn report_splits_used_missing_unused() {
        let lib: BTreeSet<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let cited = cited_keys(r"\cite{b,d}");
        let r = report(&lib, &cited);
        assert_eq!(r.used, vec!["b"]);
        assert_eq!(r.missing, vec!["d"]);
        assert_eq!(r.unused, vec!["a", "c"]);
        assert!(!r.cite_all);
    }

    #[test]
    fn report_with_cite_all_has_no_unused() {
        let lib: BTreeSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let cited = cited_keys(r"\nocite{*}");
        let r = report(&lib, &cited);
        assert!(r.used.is_empty());
        assert!(r.missing.is_empty());
        assert!(r.unused.is_empty());
        assert!(r.cite_all);
    }
}
