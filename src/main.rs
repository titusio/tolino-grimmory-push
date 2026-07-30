pub mod grimmory;
pub mod tolino;

#[tokio::main]
async fn main() {
    let token = grimmory::login().await.unwrap();
    let books = grimmory::get_books(&token).await.unwrap();
    println!("Got {} books", books.len());

    grimmory::download_book(books.get(0).unwrap(), &token).await;

    let entries = tolino::parse::parse_file("./notes.txt".to_string());
    for e in entries {
        // println!("{:?}", e);
    }
}
