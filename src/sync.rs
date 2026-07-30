//! Turning tolino entries into grimmory records.
//!
//! Each entry becomes up to two records at the same location: a highlight for
//! the passage, and — when the reader typed something — a note. grimmory keeps
//! those in separate tables with separate uniqueness, which is what makes a
//! note show up as a note in its reader and notebook rather than as a comment
//! hanging off a highlight.

use crate::grimmory::{self, NewAnnotation, NewNote, limits};
use crate::library::{self, Hit, LoadedBook};
use crate::text;
use crate::tolino::parse::Entry;
use std::fmt;

#[derive(Default)]
pub struct Report {
    highlights: u32,
    notes: u32,
    migrated: u32,
    duplicate: u32,
    ambiguous: u32,
    unfound: u32,
    failed: u32,
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} highlights created, {} notes created, {} notes migrated, \
             {} already present, {} ambiguous, {} not found, {} failed",
            self.highlights,
            self.notes,
            self.migrated,
            self.duplicate,
            self.ambiguous,
            self.unfound,
            self.failed
        )
    }
}

pub struct Sync<'a> {
    token: &'a str,
    dry_run: bool,
    library: Vec<LoadedBook>,
    annotations: Vec<grimmory::Annotation>,
    notes: Vec<grimmory::Note>,
    /// Copied from an existing annotation to keep the library looking
    /// consistent. `None` leaves grimmory to apply its own defaults, which is
    /// also where an existing value that fails its validation lands.
    color: Option<String>,
    style: Option<String>,
    pub report: Report,
}

impl<'a> Sync<'a> {
    /// Downloads every book and everything already recorded against it.
    pub async fn load(
        books: &[grimmory::Book],
        token: &'a str,
        dry_run: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut library = Vec::new();
        let mut annotations = Vec::new();
        let mut notes = Vec::new();

        for book in books {
            annotations.extend(grimmory::get_annotations(book.id, token).await?);
            notes.extend(grimmory::get_notes(book.id, token).await?);
            library.push(LoadedBook::load(book, token).await?);
        }

        println!(
            "{} books, {} existing annotations, {} existing notes",
            books.len(),
            annotations.len(),
            notes.len()
        );

        let template = annotations.first();
        let color = template
            .map(|a| a.color.clone())
            .filter(|c| limits::is_hex_color(c));
        let style = template
            .map(|a| a.style.clone())
            .filter(|s| limits::STYLES.contains(&s.as_str()));

        Ok(Sync {
            token,
            dry_run,
            library,
            annotations,
            notes,
            color,
            style,
            report: Report::default(),
        })
    }

    /// Sanity gate: if we cannot reproduce a CFI grimmory itself wrote, our
    /// coordinates are off and anything we create would land in the wrong place.
    pub fn verify(&self) {
        for annotation in &self.annotations {
            let ours: Vec<String> = library::locate(&self.library, &annotation.text)
                .into_iter()
                .map(|hit| hit.cfi)
                .collect();

            if !ours.contains(&annotation.cfi) {
                println!("warning: cannot reproduce {} (got {ours:?})", annotation.cfi);
            }
        }
    }

    /// Earlier runs wrote notes into the annotation's own `note` field, where
    /// grimmory's reader never shows them. Moves those across to real notes and
    /// blanks the field — passing an empty string, since grimmory ignores a null.
    pub async fn migrate_notes(&mut self) {
        // Taken up front so the loop can extend the note list as it goes.
        let carrying: Vec<grimmory::Annotation> = self
            .annotations
            .iter()
            .filter(|a| a.note.as_deref().is_some_and(|n| !n.trim().is_empty()))
            .cloned()
            .collect();

        for annotation in carrying {
            let note = annotation.note.as_deref().unwrap_or_default();

            if self.dry_run {
                self.report.migrated += 1;
                println!("would migrate note at {} {:?}", annotation.cfi, preview(note));
                continue;
            }

            // Creating first means a failure part way through leaves the note
            // where it was rather than losing it.
            let moved = if self.find_note(annotation.book_id, &annotation.cfi).is_some() {
                Ok(())
            } else {
                self.create_note(&NewNote {
                    book_id: annotation.book_id,
                    cfi: &annotation.cfi,
                    note_content: note,
                    selected_text: Some(annotation.text.as_str())
                        .filter(|t| limits::fits(t, limits::TEXT)),
                    color: None,
                    chapter_title: annotation.chapter_title.as_deref(),
                })
                .await
            };

            let cleared = grimmory::set_annotation_note(annotation.id, "", self.token).await;
            match moved.and(cleared) {
                Ok(()) => {
                    self.report.migrated += 1;
                    println!("migrated note at {}", annotation.cfi);
                }
                Err(err) => {
                    self.report.failed += 1;
                    println!("failed migrating {}: {err}", annotation.cfi);
                }
            }
        }
    }

    /// Locates every entry and writes whatever is missing at that location.
    pub async fn run(&mut self, entries: &[Entry]) {
        for entry in entries {
            let Some(quote) = entry.quote() else {
                continue; // a note with no selected passage has nothing to anchor to
            };

            let hits = library::locate(&self.library, quote);
            let hit = match hits.as_slice() {
                [hit] => hit,
                [] => {
                    self.report.unfound += 1;
                    println!("not found: {:?} p{}", entry.title(), entry.page());
                    continue;
                }
                _ => {
                    self.report.ambiguous += 1;
                    println!(
                        "{} matches, skipped: {:?} p{} {:?}",
                        hits.len(),
                        entry.title(),
                        entry.page(),
                        preview(quote)
                    );
                    continue;
                }
            };

            let body = text::normalize(quote);
            if !limits::fits(&body, limits::TEXT) || !limits::fits(&hit.cfi, limits::CFI) {
                self.report.failed += 1;
                println!("too long for grimmory: {:?} p{}", entry.title(), entry.page());
                continue;
            }

            self.highlight(hit, &body).await;
            if let Some(note) = entry.note() {
                self.note(hit, &body, note).await;
            }
        }
    }

    /// Records the passage itself, unless a highlight already sits there.
    async fn highlight(&mut self, hit: &Hit, body: &str) {
        if self
            .annotations
            .iter()
            .any(|a| a.book_id == hit.book_id && a.cfi == hit.cfi)
        {
            self.report.duplicate += 1;
            return;
        }

        if self.dry_run {
            self.report.highlights += 1;
            println!("would create {} {:?}", hit.cfi, preview(body));
            return;
        }

        let new = NewAnnotation {
            book_id: hit.book_id,
            cfi: &hit.cfi,
            text: body,
            color: self.color.as_deref(),
            style: self.style.as_deref(),
            chapter_title: chapter(hit),
        };

        // One rejected record should not strand the rest of the run.
        match grimmory::create_annotation(&new, self.token).await {
            Ok(annotation) => {
                self.report.highlights += 1;
                println!("created {}", hit.cfi);
                self.annotations.push(annotation);
            }
            Err(err) => {
                self.report.failed += 1;
                println!("failed {}: {err}", hit.cfi);
            }
        }
    }

    /// Records the reader's own words at the same location as the passage.
    async fn note(&mut self, hit: &Hit, body: &str, note: &str) {
        // A note already here is left alone — but if it says something else,
        // the two sides have drifted apart and silence would hide it.
        if let Some(theirs) = self.find_note(hit.book_id, &hit.cfi) {
            if theirs.note_content.trim() != note.trim() {
                println!(
                    "note differs at {}: grimmory {:?}, tolino {:?}",
                    hit.cfi,
                    preview(&theirs.note_content),
                    preview(note)
                );
            }
            return;
        }

        if self.dry_run {
            self.report.notes += 1;
            println!("would note   {} {:?}", hit.cfi, preview(note));
            return;
        }

        let new = NewNote {
            book_id: hit.book_id,
            cfi: &hit.cfi,
            note_content: note,
            selected_text: Some(body),
            color: None,
            chapter_title: chapter(hit),
        };

        match self.create_note(&new).await {
            Ok(()) => {
                self.report.notes += 1;
                println!("noted   {}", hit.cfi);
            }
            Err(err) => {
                self.report.failed += 1;
                println!("failed noting {}: {err}", hit.cfi);
            }
        }
    }

    fn find_note(&self, book_id: u32, cfi: &str) -> Option<&grimmory::Note> {
        self.notes
            .iter()
            .find(|n| n.book_id == book_id && n.cfi == cfi)
    }

    /// Creates a note and remembers it, so later entries at the same location
    /// see it without another round trip.
    async fn create_note(&mut self, new: &NewNote<'_>) -> Result<(), Box<dyn std::error::Error>> {
        let created = grimmory::create_note(new, self.token).await?;
        self.notes.push(created);
        Ok(())
    }
}

/// An over-long chapter title would sink the whole request, and it is only a
/// label, so it goes rather than the record it belongs to.
fn chapter(hit: &Hit) -> Option<&str> {
    hit.chapter
        .as_deref()
        .filter(|t| limits::fits(t, limits::CHAPTER))
}

/// Shortened for log lines.
fn preview(s: &str) -> String {
    match s.char_indices().nth(60) {
        Some((at, _)) => format!("{}…", &s[..at]),
        None => s.to_string(),
    }
}
