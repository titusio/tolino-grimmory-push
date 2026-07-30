use std::collections::HashMap;
use std::io::{Cursor, Read};

/// One reading-order entry of the spine, with its content document resolved to
/// a path inside the zip.
#[derive(Debug)]
pub struct SpineItem {
    pub idref: String,
    pub href: String,
}

/// An opened epub container. Owning the archive keeps every `by_name` borrow
/// scoped to a single method call.
pub struct Epub {
    archive: zip::ZipArchive<Cursor<Vec<u8>>>,
}

impl Epub {
    pub fn open(bytes: Vec<u8>) -> Result<Self, Box<dyn std::error::Error>> {
        let archive = zip::ZipArchive::new(Cursor::new(bytes))?;
        Ok(Self { archive })
    }

    /// Reads a single archive entry into an owned String.
    fn read_entry(&mut self, name: &str) -> Result<String, Box<dyn std::error::Error>> {
        let mut file = self.archive.by_name(name)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(contents)
    }

    /// Path of the package document, as declared in META-INF/container.xml.
    fn opf_path(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        let container = self.read_entry("META-INF/container.xml")?;
        let doc = roxmltree::Document::parse(&container)?;

        let Some(rootfile) = doc
            .descendants()
            .find(|n| n.tag_name().name() == "rootfile")
        else {
            return Err("no <rootfile> found in container.xml".into());
        };

        let Some(path) = rootfile.attribute("full-path") else {
            return Err("<rootfile> has no full-path attribute".into());
        };

        Ok(path.to_string())
    }

    /// The package document (.opf) contents.
    pub fn opf(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        let path = self.opf_path()?;
        self.read_entry(&path)
    }

    /// The spine in reading order, each entry resolved through the manifest to
    /// a zip path ready for `read_entry`.
    pub fn spine(&mut self) -> Result<Vec<SpineItem>, Box<dyn std::error::Error>> {
        let opf_path = self.opf_path()?;
        let opf = self.read_entry(&opf_path)?;
        let doc = roxmltree::Document::parse(&opf)?;

        // Manifest hrefs are relative to the directory holding the .opf.
        let base = opf_path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");

        let manifest: HashMap<&str, &str> = doc
            .descendants()
            .filter(|n| in_parent(n, "manifest") && n.tag_name().name() == "item")
            .filter_map(|n| Some((n.attribute("id")?, n.attribute("href")?)))
            .collect();

        let mut items = Vec::new();
        for itemref in doc
            .descendants()
            .filter(|n| in_parent(n, "spine") && n.tag_name().name() == "itemref")
        {
            let Some(idref) = itemref.attribute("idref") else {
                return Err("<itemref> has no idref attribute".into());
            };

            let Some(href) = manifest.get(idref) else {
                return Err(format!("spine references unknown manifest id `{idref}`").into());
            };

            items.push(SpineItem {
                idref: idref.to_string(),
                href: resolve_href(base, href),
            });
        }

        Ok(items)
    }
}

fn in_parent(node: &roxmltree::Node, parent: &str) -> bool {
    node.parent()
        .is_some_and(|p| p.tag_name().name() == parent)
}

/// Joins a manifest href onto the .opf's directory, collapsing `.` and `..`.
fn resolve_href(base: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or(href);

    let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    for segment in href.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }

    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::resolve_href;

    #[test]
    fn resolves_relative_to_opf_dir() {
        assert_eq!(resolve_href("OEBPS", "chap1.xhtml"), "OEBPS/chap1.xhtml");
        assert_eq!(resolve_href("", "chap1.xhtml"), "chap1.xhtml");
        assert_eq!(
            resolve_href("OEBPS/text", "../images/cover.xhtml"),
            "OEBPS/images/cover.xhtml"
        );
        assert_eq!(resolve_href("OEBPS", "./chap1.xhtml"), "OEBPS/chap1.xhtml");
        assert_eq!(resolve_href("OEBPS", "chap1.xhtml#intro"), "OEBPS/chap1.xhtml");
    }
}
