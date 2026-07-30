use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const BASE: &str = "http://10.0.10.10:6060/api/v1";
/// Notes are the one feature grimmory serves from a different API version.
const BASE_V2: &str = "http://10.0.10.10:6060/api/v2";

/// The validation grimmory applies to what we send. Mirroring it here keeps a
/// value it would reject from sinking an otherwise good request — the limits
/// are counted the way Java counts a string, in UTF-16 code units.
pub mod limits {
    pub const TEXT: usize = 5000;
    pub const CFI: usize = 1000;
    pub const CHAPTER: usize = 500;

    pub const STYLES: [&str; 4] = ["highlight", "underline", "strikethrough", "squiggly"];

    /// Whether a string is short enough for the column it is headed for.
    pub fn fits(s: &str, limit: usize) -> bool {
        s.encode_utf16().count() <= limit
    }

    /// Whether a colour matches the `#rrggbb` pattern grimmory insists on.
    pub fn is_hex_color(color: &str) -> bool {
        color.len() == 7
            && color.starts_with('#')
            && color[1..].chars().all(|c| c.is_ascii_hexdigit())
    }
}

/// Reads a response, keeping grimmory's own message when it rejects a request.
/// Its validation failures arrive as a body — `error_for_status` alone would
/// reduce "Color must be a valid hex color" to a bare 400.
async fn parse<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, Box<dyn std::error::Error>> {
    let status = response.status();
    let body = response.text().await?;

    if !status.is_success() {
        return Err(format!("{status}: {body}").into());
    }

    Ok(serde_json::from_str(&body)?)
}

#[derive(Deserialize, Debug)]
#[serde(rename_all(deserialize = "camelCase"))]
#[allow(dead_code)]
pub struct LoginResponse {
    access_token: String,
    expires: u32,
    is_default_password: bool,
    refresh_token: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all(deserialize = "camelCase"))]
#[allow(dead_code)]
pub struct Book {
    pub id: u32,
    library_id: u32,
    library_name: String,
    metadata: Metadata,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all(deserialize = "camelCase"))]
#[allow(dead_code)]
pub struct Metadata {
    book_id: u32,
    title: String,
    authors: Vec<String>,
    publisher: Option<String>,
    isbn13: Option<String>,
    language: Option<String>,
}

pub async fn login() -> Result<String, Box<dyn std::error::Error>> {
    println!("Logging In!");

    let username = std::env::var("SYNC_USERNAME")?;
    let password = std::env::var("SYNC_PASSWORD")?;

    let mut map = HashMap::new();
    map.insert("username", username);
    map.insert("password", password);

    let client = reqwest::Client::new();
    let res = client
        .post(format!("{BASE}/auth/login"))
        .json(&map)
        .send()
        .await?
        .json::<LoginResponse>()
        .await?;

    Ok(res.access_token)
}

pub async fn get_books(token: &String) -> Result<Vec<Book>, Box<dyn std::error::Error>> {
    println!("Getting books");
    let client = reqwest::Client::new();
    let books = client
        .get(format!("{BASE}/books"))
        .bearer_auth(token)
        .send()
        .await?
        .json::<Vec<Book>>()
        .await?;

    Ok(books)
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all(deserialize = "camelCase"))]
#[allow(dead_code)]
pub struct Annotation {
    pub id: u32,
    pub book_id: u32,
    pub cfi: String,
    pub text: String,
    pub note: Option<String>,
    pub chapter_title: Option<String>,
    pub color: String,
    pub style: String,
    created_at: String,
    updated_at: String,
    user_id: u32,
}

/// Fetches the annotations grimmory holds for a book.
pub async fn get_annotations(
    book_id: u32,
    token: &str,
) -> Result<Vec<Annotation>, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{BASE}/annotations/book/{book_id}"))
        .bearer_auth(token)
        .send()
        .await?;

    parse(response).await
}

/// The body grimmory expects when creating an annotation.
///
/// Everything optional here is omitted rather than guessed: the server fills in
/// `#FFFF00` and `highlight`, and validates both against a hex pattern and a
/// four-value enum, so sending a value we are unsure of only risks a 400.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NewAnnotation<'a> {
    pub book_id: u32,
    pub cfi: &'a str,
    pub text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter_title: Option<&'a str>,
}

/// Creates a highlight and returns it as grimmory stored it. A CFI already
/// annotated comes back as 409: the server holds a unique index on
/// (user, book, cfi).
pub async fn create_annotation(
    annotation: &NewAnnotation<'_>,
    token: &str,
) -> Result<Annotation, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{BASE}/annotations"))
        .bearer_auth(token)
        .json(annotation)
        .send()
        .await?;

    parse(response).await
}

#[derive(Serialize)]
struct NoteUpdate<'a> {
    note: &'a str,
}

/// Overwrites an annotation's inline note.
///
/// grimmory applies the field only when it is non-null, so an empty string is
/// the only way to clear one — which is what moving a note out to a real note
/// record needs.
pub async fn set_annotation_note(
    annotation_id: u32,
    note: &str,
    token: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let response = client
        .put(format!("{BASE}/annotations/{annotation_id}"))
        .bearer_auth(token)
        .json(&NoteUpdate { note })
        .send()
        .await?;

    parse::<Annotation>(response).await?;
    Ok(())
}

/// A note: the reader's own words anchored to a place in the book. Stored apart
/// from annotations, which is what makes it show up as a note in grimmory
/// rather than as a comment hanging off a highlight.
#[derive(Deserialize, Debug)]
#[serde(rename_all(deserialize = "camelCase"))]
#[allow(dead_code)]
pub struct Note {
    pub id: u32,
    pub book_id: u32,
    pub cfi: String,
    pub selected_text: Option<String>,
    pub note_content: String,
    pub color: Option<String>,
    pub chapter_title: Option<String>,
    created_at: String,
    updated_at: String,
    user_id: u32,
}

/// Fetches the notes grimmory holds for a book.
pub async fn get_notes(book_id: u32, token: &str) -> Result<Vec<Note>, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{BASE_V2}/book-notes/book/{book_id}"))
        .bearer_auth(token)
        .send()
        .await?;

    parse(response).await
}

/// The body grimmory expects when creating a note. Only `note_content` is
/// required; the passage it hangs on is context, and the colour defaults to
/// `#FFC107`.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NewNote<'a> {
    pub book_id: u32,
    pub cfi: &'a str,
    pub note_content: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_text: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter_title: Option<&'a str>,
}

/// Creates a note and returns it as grimmory stored it. Notes are keyed by
/// (user, book, cfi) independently of annotations, so a note and a highlight
/// can share a location.
pub async fn create_note(
    note: &NewNote<'_>,
    token: &str,
) -> Result<Note, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{BASE_V2}/book-notes"))
        .bearer_auth(token)
        .json(note)
        .send()
        .await?;

    parse(response).await
}

/// Fetches the raw epub bytes for a book, verifying we actually got a zip back.
pub async fn download_book(
    book: &Book,
    token: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{BASE}/books/{}/download", book.id))
        .bearer_auth(token)
        .send()
        .await?;

    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = response.bytes().await?;

    if !status.is_success() || !bytes.starts_with(b"PK\x03\x04") {
        return Err(format!(
            "expected epub, got status={} content-type={} len={}",
            status,
            content_type,
            bytes.len()
        )
        .into());
    }

    Ok(bytes.to_vec())
}
