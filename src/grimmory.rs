use serde::Deserialize;
use std::collections::HashMap;

const BASE: &str = "http://10.0.10.10:6060/api/v1";

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

#[derive(Deserialize, Debug)]
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
    let annotations = client
        .get(format!("{BASE}/annotations/book/{book_id}"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<Annotation>>()
        .await?;

    Ok(annotations)
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
