//! Flattens an XHTML content document into a single searchable string, keeping
//! a map back to CFI coordinates (step path + UTF-16 offset within the text
//! node) for every character that survives normalization.

/// One CFI child step, plus the element's `id` if it has one. The id becomes the
/// optional assertion a CFI can carry — `/2[p01]` — which lets a reader detect
/// that a document changed under a stored CFI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub index: usize,
    pub id: Option<String>,
}

/// A point inside a content document, in the coordinates a CFI needs:
/// `path` is the sequence of CFI child steps from the root element's children
/// down to a text node (so the last step is always odd), and `utf16_offset` is
/// the offset into that text node measured in UTF-16 code units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Point {
    pub path: Vec<Step>,
    pub utf16_offset: usize,
}

/// The span of a search hit, as two points. Start and end may sit in different
/// text nodes, which is what forces a range CFI later on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub start: Point,
    pub end: Point,
}

/// A run of buffer text copied verbatim from one text node. Within a segment
/// the buffer and the source agree character for character, so an offset can be
/// converted by re-counting the slice.
#[derive(Debug)]
struct Segment {
    buf_start: usize,
    buf_len: usize,
    path: Vec<Step>,
    utf16_start: usize,
}

pub struct FlatText {
    text: String,
    segments: Vec<Segment>,
}

impl FlatText {
    pub fn build(xhtml: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let doc = roxmltree::Document::parse(xhtml)?;

        let mut flat = FlatText {
            text: String::new(),
            segments: Vec::new(),
        };

        // CFI steps after the `!` are relative to the root element's children,
        // so the walk starts at <html> itself and numbers from there.
        flat.walk(doc.root_element(), &mut Vec::new());
        Ok(flat)
    }

    /// The normalized text, for searching or eyeballing.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Every occurrence of `needle`, normalized the same way the buffer was.
    /// Returns all hits rather than the first: a short highlight can legitimately
    /// appear more than once, and picking one silently would be a guess.
    pub fn find_all(&self, needle: &str) -> Vec<Match> {
        let needle = normalize(needle);
        let needle = needle.trim();
        if needle.is_empty() {
            return Vec::new();
        }

        self.text
            .match_indices(needle)
            .filter_map(|(at, hit)| {
                Some(Match {
                    start: self.point_at(at)?,
                    end: self.point_at(at + hit.len())?,
                })
            })
            .collect()
    }

    /// Maps a byte offset in the buffer back to source coordinates.
    fn point_at(&self, offset: usize) -> Option<Point> {
        // The last segment starting at or before `offset`. Offsets landing in
        // collapsed whitespace (which belongs to no segment) clamp to the end of
        // the preceding segment.
        let idx = self
            .segments
            .partition_point(|s| s.buf_start <= offset)
            .checked_sub(1)?;
        let segment = &self.segments[idx];

        let end = (segment.buf_start + segment.buf_len).min(offset);
        let consumed = self.text.get(segment.buf_start..end)?;

        Some(Point {
            path: segment.path.clone(),
            utf16_offset: segment.utf16_start + consumed.encode_utf16().count(),
        })
    }

    fn walk(&mut self, node: roxmltree::Node, path: &mut Vec<Step>) {
        let mut elements_seen = 0;
        // Adjacent text nodes (and text separated only by comments or PIs) form
        // a single logical text node for CFI purposes, so they accumulate here
        // and are flushed as one string.
        let mut pending: Vec<&str> = Vec::new();

        for child in node.children() {
            if child.is_text() {
                pending.push(child.text().unwrap_or(""));
                continue;
            }

            if !child.is_element() {
                continue; // comments and PIs are not addressable and do not split text
            }

            self.flush(&mut pending, 2 * elements_seen + 1, path);
            elements_seen += 1;

            path.push(Step {
                index: 2 * elements_seen,
                id: child.attribute("id").map(str::to_string),
            });
            if !matches!(child.tag_name().name(), "head" | "script" | "style") {
                self.walk(child, path);
            }
            path.pop();
        }

        self.flush(&mut pending, 2 * elements_seen + 1, path);
    }

    /// Appends one logical text node to the buffer, recording a segment per run
    /// of non-whitespace.
    fn flush(&mut self, pending: &mut Vec<&str>, step: usize, path: &[Step]) {
        if pending.is_empty() {
            return;
        }

        let merged: String = pending.concat();
        pending.clear();

        let mut node_path = path.to_vec();
        node_path.push(Step {
            index: step,
            id: None, // text nodes cannot carry an assertion
        });

        let mut utf16_pos = 0;
        let mut open = false;

        for ch in merged.chars() {
            if ch.is_whitespace() {
                open = false;
                if !self.text.is_empty() && !self.text.ends_with(' ') {
                    self.text.push(' ');
                }
            } else {
                if !open {
                    self.segments.push(Segment {
                        buf_start: self.text.len(),
                        buf_len: 0,
                        path: node_path.clone(),
                        utf16_start: utf16_pos,
                    });
                    open = true;
                }
                self.text.push(ch);

                let last = self.segments.last_mut().expect("segment just pushed");
                last.buf_len = self.text.len() - last.buf_start;
            }

            utf16_pos += ch.len_utf16();
        }
    }
}

/// Collapses every run of whitespace to a single space. Applied to both the
/// document and the search needle so that source line wrapping, `&nbsp;` and
/// indentation cannot prevent a match.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{FlatText, Point, Step};

    fn flat(xhtml: &str) -> FlatText {
        FlatText::build(xhtml).expect("parses")
    }

    /// Step indices only, for assertions that don't care about ids.
    fn indices(path: &[Step]) -> Vec<usize> {
        path.iter().map(|s| s.index).collect()
    }

    #[test]
    fn skips_head_but_still_counts_it() {
        let f = flat("<html><head><title>Title</title></head><body><p>Hello world</p></body></html>");
        assert_eq!(f.text(), "Hello world");

        let m = &f.find_all("world")[0];
        // body is html's 2nd element child (/4), p is body's 1st (/2), the text
        // node precedes any element inside p (/1).
        assert_eq!(indices(&m.start.path), vec![4, 2, 1]);
        assert_eq!(m.start.utf16_offset, 6);
        assert_eq!(m.end.utf16_offset, 11);
    }

    #[test]
    fn match_spans_inline_elements() {
        let f = flat("<html><body><p>the quick <i>brown</i> fox</p></body></html>");
        assert_eq!(f.text(), "the quick brown fox");

        let m = &f.find_all("quick brown fox")[0];
        // no <head> here, so <body> is html's first element child: /2, not /4
        assert_eq!(indices(&m.start.path), vec![2, 2, 1]);
        assert_eq!(m.start.utf16_offset, 4);
        // ends in the text node after <i>, which is step 3 of <p>
        assert_eq!(indices(&m.end.path), vec![2, 2, 3]);
        assert_eq!(m.end.utf16_offset, 4);
    }

    #[test]
    fn records_element_ids_as_assertions() {
        let f = flat(r#"<html><body><div id="p01"><p>Hello</p></div></body></html>"#);
        let m = &f.find_all("Hello")[0];

        assert_eq!(
            m.start.path[1],
            Step {
                index: 2,
                id: Some("p01".to_string())
            }
        );
        // text nodes and id-less elements carry no assertion
        assert_eq!(m.start.path[0].id, None);
        assert_eq!(m.start.path[2].id, None);
    }

    #[test]
    fn point_equality_covers_ids() {
        let a = Point {
            path: vec![Step {
                index: 2,
                id: None,
            }],
            utf16_offset: 0,
        };
        let b = Point {
            path: vec![Step {
                index: 2,
                id: Some("x".to_string()),
            }],
            utf16_offset: 0,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn offsets_are_utf16_not_bytes() {
        let f = flat("<html><body><p>Größe test</p></body></html>");
        let m = &f.find_all("test")[0];
        // "Größe " is 8 UTF-8 bytes but 6 UTF-16 units
        assert_eq!(m.start.utf16_offset, 6);
    }

    #[test]
    fn collapses_source_whitespace() {
        let f = flat("<html><body><p>one\n   two</p></body></html>");
        assert_eq!(f.text(), "one two");

        let m = &f.find_all("two")[0];
        // the source node still has the newline and spaces in it
        assert_eq!(m.start.utf16_offset, 7);
    }

    #[test]
    fn needle_whitespace_is_normalized_too() {
        let f = flat("<html><body><p>one two</p></body></html>");
        assert_eq!(f.find_all("one\n  two").len(), 1);
    }

    #[test]
    fn reports_every_occurrence() {
        let f = flat("<html><body><p>echo</p><p>echo</p></body></html>");
        let hits = f.find_all("echo");
        assert_eq!(hits.len(), 2);
        assert_eq!(indices(&hits[0].start.path), vec![2, 2, 1]);
        assert_eq!(indices(&hits[1].start.path), vec![2, 4, 1]);
    }
}
