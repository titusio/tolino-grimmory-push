pub mod epub;
pub mod grimmory;
pub mod tolino;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = grimmory::login().await?;
    let books = grimmory::get_books(&token).await?;
    println!("Got {} books", books.len());

    let Some(book) = books.first() else {
        return Err("library is empty".into());
    };

    let bytes = grimmory::download_book(book, &token).await?;
    let mut epub = epub::Epub::open(bytes)?;
    for (i, item) in epub.spine()?.iter().enumerate() {
        println!("{i}: {} -> {}", item.idref, item.href);
    }

    let _entries = tolino::parse::parse_file("./notes.txt".to_string());

    Ok(())
}
