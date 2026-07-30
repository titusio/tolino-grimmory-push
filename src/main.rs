pub mod cfi;
pub mod epub;
pub mod grimmory;
pub mod library;
pub mod sync;
pub mod text;
pub mod tolino;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Writing is the default; --dry-run reports what would happen instead.
    let dry_run = std::env::args().any(|arg| arg == "--dry-run");

    let token = grimmory::login().await?;
    let books = grimmory::get_books(&token).await?;

    let mut sync = sync::Sync::load(&books, &token, dry_run).await?;
    sync.verify();
    sync.migrate_notes().await;

    let entries = tolino::parse::parse_file("./notes.txt".to_string())?;
    sync.run(&entries).await;

    println!(
        "\n{}{} (of {} entries)",
        if dry_run { "dry run: " } else { "" },
        sync.report,
        entries.len()
    );

    Ok(())
}
