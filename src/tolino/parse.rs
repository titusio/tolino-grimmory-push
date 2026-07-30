use chrono::NaiveDateTime;
use regex::Regex;
use std::fs;
use std::sync::OnceLock;

#[derive(Debug)]
pub enum Kind {
    Highlight { quote: String },
    Note { note: String, quote: Option<String> },
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct Entry {
    title: String,
    author: Option<String>,
    page: u32,
    kind: Kind,
    added: NaiveDateTime,
}

impl Entry {
    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn page(&self) -> u32 {
        self.page
    }

    /// The passage from the book, if this entry has one. A note the reader typed
    /// without selecting any text has nothing to locate.
    pub fn quote(&self) -> Option<&str> {
        match &self.kind {
            Kind::Highlight { quote } => Some(quote),
            Kind::Note { quote, .. } => quote.as_deref(),
        }
    }
}

// Quote characters we accept as delimiters (straight, German, guillemets).
const OPEN: &[char] = &['"', '„', '“', '«', '»'];
const CLOSE: &[char] = &['"', '”', '“', '«', '»'];

// Regexes compiled once on first use.
fn marker_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(Markierung|Notiz) auf Seite (\d+):\s*(.*)$").unwrap())
}

fn title_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(.*?)\s*\(([^)]+)\)\s*$").unwrap())
}

/// Remove a single surrounding pair of quote characters, if present.
fn strip_outer_quotes(s: &str) -> String {
    let t = s.trim();
    let t = t.strip_prefix(OPEN).unwrap_or(t);
    let t = t.strip_suffix(CLOSE).unwrap_or(t);
    t.trim().to_string()
}

/// "Title (Author)" -> (title, Some(author)); a line without parens -> (line, None).
fn parse_title(line: &str) -> (String, Option<String>) {
    match title_re().captures(line) {
        Some(c) => (c[1].trim().to_string(), Some(c[2].trim().to_string())),
        None => (line.trim().to_string(), None),
    }
}

/// "Newport, Cal" -> "Cal Newport"; leaves anything without ", " untouched.
fn normalize_author(a: &str) -> String {
    match a.split_once(',') {
        Some((last, first)) => format!("{} {}", first.trim(), last.trim()),
        None => a.to_string(),
    }
}

/// Parse one record (the lines of a single block, blanks already removed).
fn parse_block(block: &[&str]) -> Option<Entry> {
    let (title, author) = parse_title(block.first()?);
    let author = author.map(|a| normalize_author(&a));

    // Locate the "Markierung/Notiz auf Seite N: ..." line.
    let marker_idx = block.iter().position(|l| marker_re().is_match(l))?;
    let caps = marker_re().captures(block[marker_idx])?;
    let kind_str = &caps[1];
    let page: u32 = caps[2].parse().ok()?;
    let rest = caps[3].trim();

    let added = block
        .iter()
        .find_map(|l| l.trim().strip_prefix("Hinzugefügt am "))?;

    let kind = match kind_str {
        // Highlight: the text after the colon IS the quoted passage.
        "Markierung" => Kind::Highlight {
            quote: strip_outer_quotes(rest),
        },
        // Note: text after the colon is the user's note; the passage (if any) is
        // on the following line(s), which we join and unquote.
        "Notiz" => {
            let region: Vec<&str> = block[marker_idx + 1..]
                .iter()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty() && !l.starts_with("Hinzugefügt"))
                .collect();
            let quote = (!region.is_empty()).then(|| strip_outer_quotes(&region.join(" ")));
            Kind::Note {
                note: rest.to_string(),
                quote,
            }
        }
        _ => return None,
    };

    let added = NaiveDateTime::parse_from_str(added, "%d.%m.%Y | %H:%M").ok()?;

    Some(Entry {
        title,
        author,
        page,
        kind,
        added,
    })
}

pub fn parse_file(file_path: String) -> Result<Vec<Entry>, Box<dyn std::error::Error>> {
    // The tolino export separates "Markierung / auf / Seite / N" and the date with
    // non-breaking spaces (U+00A0). Normalize them so the patterns below can use
    // ordinary spaces.
    let content = fs::read_to_string(file_path)?.replace('\u{A0}', " ");

    let is_separator = |line: &&str| {
        let t = line.trim();
        !t.is_empty() && t.chars().all(|c| c == '-')
    };

    let entries: Vec<Entry> = content
        .lines()
        .collect::<Vec<_>>()
        .split(is_separator)
        .filter_map(|block| {
            let cleaned: Vec<&str> = block
                .iter()
                .copied()
                .filter(|l| !l.trim().is_empty())
                .collect();
            (!cleaned.is_empty())
                .then(|| parse_block(&cleaned))
                .flatten()
        })
        .collect();

    println!("found {} entries", entries.len());

    Ok(entries)
}

#[cfg(test)]
mod tmp_tests {
    #[test]
    fn smoke() {
        let e = super::parse_file("./notes.txt".to_string()).unwrap();
        for x in e.iter().take(4) { println!("{x:?}"); }
        assert!(!e.is_empty());
    }
}
