use anyhow::Result;
use crossterm::style::{Attribute, Attributes};
use roxmltree::{Document, Node};
use std::{collections::HashMap, fs::File, io::Read};

pub struct Chapter {
    pub title: String,
    // single string for search
    pub text: String,
    pub lines: Vec<(usize, usize)>,
    // crossterm gives us a bitset but doesn't let us diff it, so store the state transition
    pub attrs: Vec<(usize, Attribute, Attributes)>,
}

pub struct Epub {
    container: zip::ZipArchive<File>,
    pub chapters: Vec<Chapter>,
    pub meta: String,
}

impl Epub {
    pub fn new(path: &str, meta: bool) -> Result<Self> {
        let file = File::open(path)?;
        let mut epub = Epub {
            container: zip::ZipArchive::new(file)?,
            chapters: Vec::new(),
            meta: String::new(),
        };
        let chapters = epub.get_rootfile()?;
        if !meta {
            epub.get_chapters(chapters);
        }
        Ok(epub)
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
    fn get_chapters(&mut self, chapters: Vec<(String, String)>) {
        self.chapters = chapters
            .into_iter()
            .filter_map(|(title, path)| {
                let xml = self.get_text(&path);
                // https://github.com/RazrFalcon/roxmltree/issues/12
                // UnknownEntityReference for HTML entities
                let doc = Document::parse(&xml).unwrap();
                let body = doc.root_element().last_element_child().unwrap();
                let attrs = Attributes::default();
                let mut c = Chapter {
                    title,
                    text: String::new(),
                    lines: Vec::new(),
                    attrs: vec![(0, Attribute::Reset, attrs)],
                };
                render(body, &mut c, attrs);
                if c.text.is_empty() {
                    None
                } else {
                    Some(c)
                }
            })
            .collect();
    }
    fn get_rootfile(&mut self) -> Result<Vec<(String, String)>> {
        let xml = self.get_text("META-INF/container.xml");
        let doc = Document::parse(&xml)?;
        let path = doc
            .descendants()
            .find(|n| n.has_tag_name("rootfile"))
            .unwrap()
            .attribute("full-path")
            .unwrap();
        let xml = self.get_text(path);
        let doc = Document::parse(&xml)?;

        // zip expects unix path even on windows
        let rootdir = match path.rfind('/') {
            Some(n) => &path[..=n],
            None => "",
        };
        let mut manifest = HashMap::new();
        let mut nav = HashMap::new();
        let mut children = doc.root_element().children().filter(Node::is_element);
        let meta_node = children.next().unwrap();
        let manifest_node = children.next().unwrap();
        let spine_node = children.next().unwrap();

        meta_node.children().filter(Node::is_element).for_each(|n| {
            let name = n.tag_name().name();
            let text = n.text();
            if text.is_some() && name != "meta" {
                self.meta
                    .push_str(&format!("{}: {}\n", name, text.unwrap()));
            }
        });
        manifest_node
            .children()
            .filter(Node::is_element)
            .for_each(|n| {
                manifest.insert(n.attribute("id").unwrap(), n.attribute("href").unwrap());
            });
        if doc.root_element().attribute("version") == Some("3.0") {
            let path = manifest_node
                .children()
                .find(|n| n.attribute("properties") == Some("nav"))
                .unwrap()
                .attribute("href")
                .unwrap();
            let xml = self.get_text(&format!("{}{}", rootdir, path));
            let doc = Document::parse(&xml)?;
            epub3(doc, &mut nav);
        } else {
            let id = spine_node.attribute("toc").unwrap_or("ncx");
            let path = manifest.get(id).unwrap();
            let xml = self.get_text(&format!("{}{}", rootdir, path));
            let doc = Document::parse(&xml)?;
            epub2(doc, &mut nav);
        }
        Ok(spine_node
            .children()
            .filter(Node::is_element)
            .enumerate()
            .map(|(i, n)| {
                let id = n.attribute("idref").unwrap();
                let path = manifest.remove(id).unwrap();
                let label = nav.remove(path).unwrap_or_else(|| i.to_string());
                let path = format!("{}{}", rootdir, path);
                (label, path)
            })
            .collect())
    }
}

fn render(n: Node, c: &mut Chapter, attrs: Attributes) {
    if n.is_text() {
        let text = n.text().unwrap();
        if !text.trim().is_empty() {
            c.text.push_str(text);
        }
        return;
    }

    // fuck this gay earth
    match n.tag_name().name() {
        "a" => {
            let a = Attribute::Underlined;
            c.attrs.push((c.text.len(), a, attrs | a));
            for child in n.children() {
                render(child, c, attrs | a);
            }
            c.attrs.push((c.text.len(), Attribute::NoUnderline, attrs));
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            c.text.push('\n');
            let a = Attribute::Bold;
            c.attrs.push((c.text.len(), a, attrs | a));
            for child in n.children() {
                render(child, c, attrs | a);
            }
            c.attrs.push((c.text.len(), Attribute::NoBold, attrs));
            c.text.push('\n');
        }
        "em" => {
            let a = Attribute::Italic;
            c.attrs.push((c.text.len(), a, attrs | a));
            for child in n.children() {
                render(child, c, attrs | a);
            }
            c.attrs.push((c.text.len(), Attribute::NoItalic, attrs));
        }
        "strong" => {
            let a = Attribute::Bold;
            c.attrs.push((c.text.len(), a, attrs | a));
            for child in n.children() {
                render(child, c, attrs | a);
            }
            c.attrs.push((c.text.len(), Attribute::NoBold, attrs));
        }
        "blockquote" | "p" | "tr" => {
            c.text.push('\n');
            for child in n.children() {
                render(child, c, attrs);
            }
            c.text.push('\n');
        }
        "li" => {
            c.text.push_str("\n- ");
            for child in n.children() {
                render(child, c, attrs);
            }
            c.text.push('\n');
        }
        "br" => c.text.push('\n'),
        "hr" => c.text.push_str("\n* * *\n"),
        _ => {
            for child in n.children() {
                render(child, c, attrs);
            }
        }
    }
}

fn epub2(doc: Document, nav: &mut HashMap<String, String>) {
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
        });
}
fn epub3(doc: Document, nav: &mut HashMap<String, String>) {
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
        });
}
