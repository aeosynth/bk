use std::collections::HashMap;
use std::fs::File;
use std::io::Read;

use roxmltree::{Document, Node};

pub struct Epub {
    container: zip::ZipArchive<File>,
    pub nav: Vec<String>,
    pub chapters: Vec<String>,
}

impl Epub {
    pub fn new(path: &str) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let mut epub = Epub {
            container: zip::ZipArchive::new(file)?,
            nav: Vec::new(),
            chapters: Vec::new(),
        };
        let nav = epub.get_nav();
        epub.nav.reserve_exact(nav.len());
        epub.chapters.reserve_exact(nav.len());
        for (path, label) in nav {
            epub.nav.push(label);
            let xml = epub.get_text(&path);
            // https://github.com/RazrFalcon/roxmltree/issues/12
            // UnknownEntityReference for HTML entities
            let doc = Document::parse(&xml).unwrap();
            let body = doc.root_element().last_element_child().unwrap();
            // XXX no initial string, buf.last is none
            let mut chapter = String::new();
            Epub::render(&mut chapter, body);
            epub.chapters.push(chapter);
        }
        Ok(epub)
    }
    fn render(buf: &mut String, n: Node) {
        if n.is_text() {
            let text = n.text().unwrap();
            if !text.trim().is_empty() {
                buf.push_str(text);
            }
            return;
        }

        match n.tag_name().name() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                buf.push_str("\n\x1b\x5b1m");
                for c in n.children() {
                    Self::render(buf, c);
                }
                buf.push_str("\x1b\x5b0m\n");
            }
            "blockquote" | "p" => {
                buf.push('\n');
                for c in n.children() {
                    Self::render(buf, c);
                }
                buf.push('\n');
            }
            "li" => {
                buf.push_str("\n- ");
                for c in n.children() {
                    Self::render(buf, c);
                }
                buf.push('\n');
            }
            "br" => buf.push('\n'),
            _ => {
                for c in n.children() {
                    Self::render(buf, c);
                }
            }
        }
    }
    fn get_text(&mut self, name: &str) -> String {
        let mut text = String::new();
        self.container
            .by_name(name)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        text
    }
    fn get_nav(&mut self) -> Vec<(String, String)> {
        let xml = self.get_text("META-INF/container.xml");
        let doc = Document::parse(&xml).unwrap();
        let path = doc
            .descendants()
            .find(|n| n.has_tag_name("rootfile"))
            .unwrap()
            .attribute("full-path")
            .unwrap();

        let xml = self.get_text(path);
        let doc = Document::parse(&xml).unwrap();
        let rootdir = std::path::Path::new(&path).parent().unwrap();

        let mut manifest = HashMap::new();
        doc.root_element()
            .children()
            .find(|n| n.has_tag_name("manifest"))
            .unwrap()
            .children()
            .filter(Node::is_element)
            .for_each(|n| {
                manifest.insert(n.attribute("id").unwrap(), n.attribute("href").unwrap());
            });

        let mut nav = HashMap::new();
        if doc.root_element().attribute("version") == Some("3.0") {
            let path = doc
                .root_element()
                .children()
                .find(|n| n.has_tag_name("manifest"))
                .unwrap()
                .children()
                .find(|n| n.attribute("properties") == Some("nav"))
                .unwrap()
                .attribute("href")
                .unwrap();
            let xml = self.get_text(rootdir.join(path).to_str().unwrap());
            let doc = Document::parse(&xml).unwrap();

            doc.descendants()
                .find(|n| n.has_tag_name("nav"))
                .unwrap()
                .descendants()
                .filter(|n| n.has_tag_name("a"))
                .for_each(|n| {
                    let path = n.attribute("href").unwrap().to_string();
                    let text = n
                        .descendants()
                        .filter(Node::is_text)
                        .map(|n| n.text().unwrap())
                        .collect();
                    nav.insert(path, text);
                })
        } else {
            let path = manifest.get("ncx").unwrap();
            let xml = self.get_text(rootdir.join(path).to_str().unwrap());
            let doc = Document::parse(&xml).unwrap();

            doc.descendants()
                .find(|n| n.has_tag_name("navMap"))
                .unwrap()
                .descendants()
                .filter(|n| n.has_tag_name("navPoint"))
                .for_each(|n| {
                    let path = n
                        .descendants()
                        .find(|n| n.has_tag_name("content"))
                        .unwrap()
                        .attribute("src")
                        .unwrap()
                        .to_string();
                    let text = n
                        .descendants()
                        .find(|n| n.has_tag_name("text"))
                        .unwrap()
                        .text()
                        .unwrap()
                        .to_string();
                    nav.insert(path, text);
                })
        }

        doc.root_element()
            .children()
            .find(|n| n.has_tag_name("spine"))
            .unwrap()
            .children()
            .filter(Node::is_element)
            .enumerate()
            .map(|(i, n)| {
                let id = n.attribute("idref").unwrap();
                let path = manifest.remove(id).unwrap();
                let label = nav.remove(path).unwrap_or_else(|| i.to_string());
                let path = rootdir.join(path).to_str().unwrap().to_string();
                (path, label)
            })
            .collect()
    }
}

#[test]
fn test_dir() {
    let path = "/mnt/lit/read";
    for entry in std::fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        let s = path.to_str().unwrap();
        println!("testing: {}", s);
        Epub::new(s).unwrap();
    }
}
