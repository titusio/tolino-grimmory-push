pub mod cfi;
pub mod epub;
pub mod grimmory;
pub mod text;
pub mod tolino;

use grimmory::NewAnnotation;

/// One content document of a book, flattened, with everything a CFI needs.
struct Document {
    step: usize,
    chapter: Option<String>,
    flat: text::FlatText,
}

/// A passage found at one specific place in one specific book.
struct Hit {
    book_id: u32,
    cfi: String,
    chapter: Option<String>,
}

struct LoadedBook {
    id: u32,
    spine_step: usize,
    documents: Vec<Document>,
}

impl LoadedBook {
    async fn load(
        book: &grimmory::Book,
        token: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let bytes = grimmory::download_book(book, token).await?;
        let mut epub = epub::Epub::open(bytes)?;
        let spine = epub.spine()?;

        let mut documents = Vec::new();
        for item in &spine.items {
            let source = epub.read_entry(&item.href)?;
            documents.push(Document {
                step: item.step,
                chapter: chapter_title(&source),
                flat: text::FlatText::build(&source)?,
            });
        }

        Ok(LoadedBook {
            id: book.id,
            spine_step: spine.step,
            documents,
        })
    }

    /// Every place this passage occurs in the book.
    fn locate(&self, passage: &str) -> Vec<Hit> {
        self.documents
            .iter()
            .flat_map(|doc| {
                doc.flat.find_all(passage).into_iter().map(move |hit| Hit {
                    book_id: self.id,
                    cfi: cfi::range(self.spine_step, doc.step, &hit),
                    chapter: doc.chapter.clone(),
                })
            })
            .collect()
    }
}

/// The `<title>` of a content document, which grimmory shows as the chapter name.
fn chapter_title(xhtml: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(xhtml).ok()?;
    let title = doc
        .descendants()
        .find(|n| n.tag_name().name() == "title")?
        .text()?
        .trim();

    (!title.is_empty()).then(|| title.to_string())
}

/// Shortened for log lines.
fn preview(s: &str) -> String {
    match s.char_indices().nth(60) {
        Some((at, _)) => format!("{}…", &s[..at]),
        None => s.to_string(),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Writing is the default; --dry-run reports what would happen instead.
    let dry_run = std::env::args().any(|arg| arg == "--dry-run");

    let token = grimmory::login().await?;
    let books = grimmory::get_books(&token).await?;

    // Notes are assigned to books by content, not by title: a passage belongs to
    // whichever book it is found in, so notes from books outside the library
    // simply never match. That avoids having to reconcile tolino's title strings
    // with grimmory's metadata.
    let mut library = Vec::new();
    let mut existing = Vec::new();
    for book in &books {
        existing.extend(grimmory::get_annotations(book.id, &token).await?);
        library.push(LoadedBook::load(book, &token).await?);
    }
    println!("{} books, {} existing annotations", books.len(), existing.len());

    // grimmory treats these as free-form strings, so rather than guessing valid
    // values we reuse the ones it already accepted.
    let Some(template) = existing.first() else {
        return Err(
            "no existing annotation to copy color and style from — create one in grimmory first"
                .into(),
        );
    };
    let (color, style) = (template.color.clone(), template.style.clone());

    // Sanity gate: if we cannot reproduce a CFI grimmory itself wrote, our
    // coordinates are off and anything we create would land in the wrong place.
    for annotation in &existing {
        let ours: Vec<String> = library
            .iter()
            .flat_map(|book| book.locate(&annotation.text))
            .map(|hit| hit.cfi)
            .collect();

        if !ours.contains(&annotation.cfi) {
            println!("warning: cannot reproduce {} (got {ours:?})", annotation.cfi);
        }
    }

    let entries = tolino::parse::parse_file("./notes.txt".to_string())?;
    let (mut created, mut duplicate, mut ambiguous, mut unfound, mut failed) = (0, 0, 0, 0, 0);

    for entry in &entries {
        let Some(quote) = entry.quote() else {
            continue; // a note with no selected passage has nothing to anchor to
        };

        let hits: Vec<Hit> = library.iter().flat_map(|book| book.locate(quote)).collect();
        let hit = match hits.as_slice() {
            [hit] => hit,
            [] => {
                unfound += 1;
                println!("not found: {:?} p{}", entry.title(), entry.page());
                continue;
            }
            _ => {
                ambiguous += 1;
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

        // Re-running must not double anything, so an annotation already sitting
        // at this exact CFI counts as done.
        if existing
            .iter()
            .any(|a| a.book_id == hit.book_id && a.cfi == hit.cfi)
        {
            duplicate += 1;
            continue;
        }

        let body = text::normalize(quote);
        let new = NewAnnotation {
            book_id: hit.book_id,
            cfi: &hit.cfi,
            text: &body,
            color: &color,
            style: &style,
            note: entry.note(),
            chapter_title: hit.chapter.as_deref(),
        };

        if dry_run {
            created += 1;
            println!("would create {} {:?}", hit.cfi, preview(&body));
            continue;
        }

        // One rejected annotation should not strand the rest of the run.
        match grimmory::create_annotation(&new, &token).await {
            Ok(_) => {
                created += 1;
                println!("created {}", hit.cfi);
            }
            Err(err) => {
                failed += 1;
                println!("failed {}: {err}", hit.cfi);
            }
        }
    }

    println!(
        "\n{}{created} created, {duplicate} already present, {ambiguous} ambiguous, \
         {unfound} not found, {failed} failed (of {} notes)",
        if dry_run { "dry run: " } else { "" },
        entries.len()
    );

    Ok(())
}
