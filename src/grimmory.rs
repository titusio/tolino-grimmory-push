use serde::Deserialize;
use std::collections::HashMap;
use std::io::{Cursor, Read};
use zip::read::ZipFile;

use crate::tolino;

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
    id: u32,
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
        .post("http://10.0.10.10:6060/api/v1/auth/login")
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
        .get("http://10.0.10.10:6060/api/v1/books")
        .bearer_auth(token)
        .send()
        .await?
        .json::<Vec<Book>>()
        .await?;

    Ok(books)
}

pub async fn download_book(
    book: &Book,
    token: &String,
) -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!(
            "http://10.0.10.10:6060/api/v1/books/{}/download",
            book.id
        ))
        .bearer_auth(&token)
        .send()
        .await?;

    dbg!(&response);

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

    let cursor = std::io::Cursor::new(bytes);
    let Ok(mut archive) = zip::ZipArchive::new(cursor) else {
        return Err("Failed to construct zip archive".into());
    };

    let mut file = archive.by_name("META-INF/container.xml")?;

    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let doc = roxmltree::Document::parse(&contents)?;

    let Some(rootfile) = doc
        .descendants()
        .find(|n| n.tag_name().name() == "rootfile")
    else {
        return Err("no <rootfile> found in container.xml".into());
    };

    let Some(opf_path) = rootfile.attribute("full-path") else {
        return Err("<rootfile> has no full-path attribute".into());
    };

    // let contents_file = archive.by_name(opf_path)?.read_to_string(&mut contents)?;

    return Ok("".to_string());
}
