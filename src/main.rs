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

    for annotation in annotations.as_array().into_iter().flatten() {
        let Some(needle) = annotation["text"].as_str() else {
            continue;
        };
        println!("\ngrimmory: {}", annotation["cfi"]);

        for (i, item) in spine.iter().enumerate() {
            let source = epub.read_entry(&item.href)?;
            let flat = text::FlatText::build(&source)?;

            for hit in flat.find_all(needle) {
                println!(
                    "    ours: spine {i} ({}) /{:?}:{} .. /{:?}:{}",
                    item.idref, hit.start.path, hit.start.utf16_offset, hit.end.path, hit.end.utf16_offset
                );
            }
        }
    }

    let _entries = tolino::parse::parse_file("./notes.txt".to_string());

    Ok(())
}
