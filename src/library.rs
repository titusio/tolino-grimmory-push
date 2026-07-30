//! The books themselves, flattened into searchable text so a passage can be
//! turned back into the CFI that addresses it.

use crate::{cfi, epub, grimmory, text};

/// A passage found at one specific place in one specific book.
pub struct Hit {
    pub book_id: u32,
    pub cfi: String,
    pub chapter: Option<String>,
}

/// One content document of a book, flattened, with everything a CFI needs.
struct Document {
    step: usize,
    chapter: Option<String>,
    flat: text::FlatText,
}

pub struct LoadedBook {
    id: u32,
    spine_step: usize,
    documents: Vec<Document>,
}

impl LoadedBook {
    pub async fn load(
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

/// Every place this passage occurs anywhere in the library.
///
/// Searching the whole library at once is what assigns an entry to a book: a
/// passage belongs to whichever book contains it, so entries from books that
/// are not in grimmory simply never match. That avoids having to reconcile
/// tolino's title strings with grimmory's metadata.
pub fn locate(library: &[LoadedBook], passage: &str) -> Vec<Hit> {
    library.iter().flat_map(|book| book.locate(passage)).collect()
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
