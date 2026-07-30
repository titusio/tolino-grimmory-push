pub mod cfi;
pub mod epub;
pub mod grimmory;
pub mod text;
pub mod tolino;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = grimmory::login().await?;
    let books = grimmory::get_books(&token).await?;
    println!("Got {} books", books.len());

    let Some(book) = books.first() else {
        return Err("library is empty".into());
    };

    let annotations = grimmory::get_annotations(book.id, &token).await?;

    let bytes = grimmory::download_book(book, &token).await?;
    let mut epub = epub::Epub::open(bytes)?;
    let spine = epub.spine()?;

    // Flatten every spine item once; each note is then searched against all of them.
    let mut documents = Vec::new();
    for item in &spine.items {
        let source = epub.read_entry(&item.href)?;
        documents.push((item, text::FlatText::build(&source)?));
    }

    /// Every CFI the passage resolves to, across the whole book.
    fn locate(
        documents: &[(&epub::SpineItem, text::FlatText)],
        spine_step: usize,
        passage: &str,
    ) -> Vec<String> {
        documents
            .iter()
            .flat_map(|(item, flat)| {
                flat.find_all(passage)
                    .into_iter()
                    .map(|hit| cfi::range(spine_step, item.step, &hit))
            })
            .collect()
    }

    for annotation in &annotations {
        let ours = locate(&documents, spine.step, &annotation.text);
        let agrees = ours.len() == 1 && ours[0] == annotation.cfi;
        println!(
            "grimmory {} -> {}",
            annotation.cfi,
            if agrees {
                "match".to_string()
            } else {
                format!("MISMATCH {ours:?}")
            }
        );
    }

    let entries = tolino::parse::parse_file("./notes.txt".to_string())?;
    let (mut located, mut ambiguous, mut unfound, mut no_quote) = (0, 0, 0, 0);

    for entry in &entries {
        let Some(quote) = entry.quote() else {
            no_quote += 1;
            continue;
        };

        match locate(&documents, spine.step, quote).as_slice() {
            [] => {
                unfound += 1;
                println!("  not found: {:?} p{}", entry.title(), entry.page());
            }
            [_] => located += 1,
            hits => {
                ambiguous += 1;
                println!("  {} hits (p{}): {quote}", hits.len(), entry.page());
            }
        }
    }

    println!(
        "\n{located} located, {ambiguous} ambiguous, {unfound} not found, {no_quote} without a passage (of {} notes)",
        entries.len()
    );

    Ok(())
}
