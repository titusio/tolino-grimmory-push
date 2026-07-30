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

    for annotation in &annotations {
        println!("\ngrimmory: {}", annotation.cfi);

        for item in &spine.items {
            let source = epub.read_entry(&item.href)?;
            let flat = text::FlatText::build(&source)?;

            for hit in flat.find_all(&annotation.text) {
                let ours = cfi::range(spine.step, item.step, &hit);
                let verdict = if ours == annotation.cfi { "==" } else { "!=" };
                println!("    ours: {ours} {verdict}");
            }
        }
    }

    let _entries = tolino::parse::parse_file("./notes.txt".to_string());

    Ok(())
}
