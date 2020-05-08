use std::collections::HashMap;
use std::fs::File;
use std::io::{stdout, Read, Write};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode},
    queue,
    style::{Attribute, Print},
    terminal,
};

use roxmltree::{Document, Node};

struct Epub {
    container: zip::ZipArchive<File>,
}

impl Epub {
    fn new(path: &str) -> std::io::Result<Self> {
        let file = File::open(path)?;

        Ok(Epub {
            container: zip::ZipArchive::new(file)?,
        })
    }
    fn render(acc: &mut Vec<String>, n: Node) {
        if n.is_text() {
            let text = n.text().unwrap();
            if !text.trim().is_empty() {
                let last = acc.last_mut().unwrap();
                last.push_str(text);
            }
            return;
        }

        match n.tag_name().name() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                acc.push(String::from("\x1b\x5b1m"));
                for c in n.children() {
                    Self::render(acc, c);
                }
                acc.push(String::from("\x1b\x5b0m"));
            }
            "blockquote" | "p" => {
                acc.push(String::new());
                for c in n.children() {
                    Self::render(acc, c);
                }
                acc.push(String::new());
            }
            "li" => {
                acc.push(String::from("- "));
                for c in n.children() {
                    Self::render(acc, c);
                }
                acc.push(String::new());
            }
            "br" => acc.push(String::new()),
            _ => {
                for c in n.children() {
                    Self::render(acc, c);
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
    fn get_toc(&mut self) -> Vec<(String, String)> {
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
                manifest.insert(
                    n.attribute("id").unwrap(),
                    n.attribute("href").unwrap(),
                );
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
                let title = nav.remove(path).unwrap_or_else(|| i.to_string());
                let path = rootdir.join(path).to_str().unwrap().to_string();
                (title, path)
            })
            .collect()
    }
}

fn wrap(text: String, width: u16) -> Vec<String> {
    // XXX assumes a char is 1 unit wide
    let mut wrapped = Vec::new();

    let mut start = 0;
    let mut brk = 0;
    let mut line = 0;
    let mut word = 0;
    let mut skip = 0;

    for (i, c) in text.char_indices() {
        match c {
            ' ' => {
                brk = i;
                skip = 1;
                word = 0;
            }
            // https://www.unicode.org/reports/tr14/
            // https://en.wikipedia.org/wiki/Line_wrap_and_word_wrap
            // currently only break at hyphen and em-dash :shrug:
            '-' | '—' => {
                brk = i + c.len_utf8();
                skip = 0;
                word = 0;
            }
            _ => {
                word += 1;
            }
        }

        if line == width {
            wrapped.push(String::from(&text[start..brk]));
            start = brk + skip;
            line = word;
        } else {
            line += 1;
        }
    }

    wrapped.push(String::from(&text[start..]));
    wrapped
}

struct Position(String, usize, usize);

enum Direction {
    Forward,
    Backward,
}

trait View {
    fn run(&self, bk: &mut Bk, kc: KeyCode);
    fn render(&self, bk: &Bk) -> Vec<String>;
}

struct Help;
impl View for Help {
    fn run(&self, bk: &mut Bk, _: KeyCode) {
        bk.view = Some(&Page);
    }
    fn render(&self, _: &Bk) -> Vec<String> {
        let text = r#"
                   Esc q  Quit
                    F1 ?  Help
                       /  Search
                     Tab  Table of Contents

PageDown Right Space f l  Page Down
         PageUp Left b h  Page Up
                       d  Half Page Down
                       u  Half Page Up
                  Down j  Line Down
                    Up k  Line Up
                  Home g  Chapter Start
                   End G  Chapter End
                       [  Previous Chapter
                       ]  Next Chapter
                       n  Search Forward
                       N  Search Backward
                   "#;

        text.lines().map(String::from).collect()
    }
}

struct Nav;
impl View for Nav {
    fn run(&self, bk: &mut Bk, kc: KeyCode) {
        match kc {
            KeyCode::Esc | KeyCode::Char('h') | KeyCode::Char('q') => {
                bk.view = Some(&Page);
            }
            KeyCode::Enter | KeyCode::Tab | KeyCode::Char('l') => {
                bk.get_chapter(bk.nav_idx);
                bk.pos = 0;
                bk.view = Some(&Page);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if bk.nav_idx < bk.toc.len() - 1 {
                    bk.nav_idx += 1;
                    if bk.nav_idx == bk.nav_top + bk.rows {
                        bk.nav_top += 1;
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if bk.nav_idx > 0 {
                    if bk.nav_idx == bk.nav_top {
                        bk.nav_top -= 1;
                    }
                    bk.nav_idx -= 1;
                }
            }
            KeyCode::Home | KeyCode::Char('g') => {
                bk.nav_idx = 0;
                bk.nav_top = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                bk.nav_idx = bk.toc.len() - 1;
                bk.nav_top = bk.toc.len().saturating_sub(bk.rows);
            }
            _ => (),
        }
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        let end = std::cmp::min(bk.nav_top + bk.rows, bk.toc.len());

        bk.toc[bk.nav_top..end]
            .iter()
            .enumerate()
            .map(|(i, line)| {
                if bk.nav_idx == bk.nav_top + i {
                    format!(
                        "{}{}{}",
                        Attribute::Reverse,
                        line.0,
                        Attribute::Reset
                    )
                } else {
                    line.0.to_string()
                }
            })
            .collect()
    }
}

struct Page;
impl View for Page {
    fn run(&self, bk: &mut Bk, kc: KeyCode) {
        match kc {
            KeyCode::Esc | KeyCode::Char('q') => bk.view = None,
            KeyCode::Tab => {
                bk.nav_idx = bk.chapter_idx;
                bk.nav_top = bk.nav_idx.saturating_sub(bk.rows - 1);
                bk.view = Some(&Nav);
            }
            KeyCode::F(1) | KeyCode::Char('?') => bk.view = Some(&Help),
            KeyCode::Char('/') => {
                bk.search = String::new();
                bk.view = Some(&Search);
            }
            KeyCode::Char('N') => {
                bk.search(Direction::Backward);
            }
            KeyCode::Char('n') => {
                bk.search(Direction::Forward);
            }
            KeyCode::End | KeyCode::Char('G') => {
                bk.pos = (bk.chapter.len() / bk.rows) * bk.rows;
            }
            KeyCode::Home | KeyCode::Char('g') => bk.pos = 0,
            KeyCode::Char('d') => {
                bk.scroll_down(bk.rows / 2);
            }
            KeyCode::Char('u') => {
                bk.scroll_up(bk.rows / 2);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                bk.scroll_up(1);
            }
            KeyCode::Left
            | KeyCode::PageUp
            | KeyCode::Char('b')
            | KeyCode::Char('h') => {
                bk.scroll_up(bk.rows);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                bk.scroll_down(1);
            }
            KeyCode::Right
            | KeyCode::PageDown
            | KeyCode::Char('f')
            | KeyCode::Char('l')
            | KeyCode::Char(' ') => {
                bk.scroll_down(bk.rows);
            }
            KeyCode::Char('[') => bk.prev_chapter(),
            KeyCode::Char(']') => bk.next_chapter(),
            _ => (),
        }
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        let end = std::cmp::min(bk.pos + bk.rows, bk.chapter.len());
        bk.chapter[bk.pos..end].iter().map(String::from).collect()
    }
}

struct Search;
impl View for Search {
    fn run(&self, bk: &mut Bk, kc: KeyCode) {
        match kc {
            KeyCode::Esc => {
                bk.search = String::new();
                bk.view = Some(&Page);
            }
            KeyCode::Enter => {
                bk.view = Some(&Page);
            }
            KeyCode::Backspace => {
                bk.search.pop();
            }
            KeyCode::Char(c) => {
                bk.search.push(c);
                bk.search(Direction::Forward);
            }
            _ => (),
        }
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        let end = std::cmp::min(bk.pos + bk.rows - 1, bk.chapter.len());
        let mut buf = Vec::with_capacity(bk.rows);

        for line in bk.chapter[bk.pos..end].iter() {
            if let Some(i) = line.find(&bk.search) {
                buf.push(format!(
                    "{}{}{}{}{}",
                    &line[..i],
                    Attribute::Reverse,
                    &bk.search,
                    Attribute::Reset,
                    &line[i + bk.search.len()..],
                ));
            } else {
                buf.push(String::from(line));
            }
        }

        for _ in buf.len()..bk.rows - 1 {
            buf.push(String::new());
        }
        buf.push(format!("/{}", bk.search));
        buf
    }
}

struct Bk<'a> {
    view: Option<&'a dyn View>,
    epub: Epub,
    cols: u16,
    chapter: Vec<String>,
    chapter_idx: usize,
    nav_idx: usize,
    nav_top: usize,
    pos: usize,
    rows: usize,
    toc: Vec<(String, String)>,
    pad: u16,
    search: String,
}

impl Bk<'_> {
    fn new(mut epub: Epub, pos: &Position, pad: u16) -> Self {
        let (cols, rows) = terminal::size().unwrap();
        let mut bk = Bk {
            view: Some(&Page),
            chapter: Vec::new(),
            chapter_idx: 0,
            nav_idx: 0,
            nav_top: 0,
            toc: epub.get_toc(),
            epub,
            pos: pos.2,
            pad,
            cols,
            rows: rows as usize,
            search: String::new(),
        };
        bk.get_chapter(pos.1);
        bk
    }
    fn run(&mut self) -> crossterm::Result<()> {
        let mut stdout = stdout();
        queue!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
        terminal::enable_raw_mode()?;

        while let Some(view) = self.view {
            queue!(stdout, terminal::Clear(terminal::ClearType::All))?;
            for (i, line) in view.render(self).iter().enumerate() {
                queue!(
                    stdout,
                    cursor::MoveTo(self.pad, i as u16),
                    Print(line)
                )?;
            }
            stdout.flush().unwrap();

            match event::read()? {
                Event::Key(e) => view.run(self, e.code),
                Event::Resize(cols, rows) => {
                    self.cols = cols;
                    self.rows = rows as usize;
                    self.get_chapter(self.chapter_idx);
                }
                // TODO
                Event::Mouse(_) => (),
            }
        }

        queue!(stdout, terminal::LeaveAlternateScreen, cursor::Show)?;
        terminal::disable_raw_mode()
    }
    fn get_chapter(&mut self, idx: usize) {
        let xml = self.epub.get_text(&self.toc[idx].1);
        let doc = Document::parse(&xml).unwrap();
        let body = doc.root_element().last_element_child().unwrap();
        let mut chapter = Vec::new();
        Epub::render(&mut chapter, body);

        let width = self.cols - (self.pad * 2);
        self.chapter = Vec::with_capacity(chapter.len() * 2);
        for line in chapter {
            self.chapter.append(&mut wrap(line, width))
        }
        self.chapter_idx = idx;
    }
    fn next_chapter(&mut self) {
        if self.chapter_idx < self.toc.len() - 1 {
            self.get_chapter(self.chapter_idx + 1);
            self.pos = 0;
        }
    }
    fn prev_chapter(&mut self) {
        if self.chapter_idx > 0 {
            self.get_chapter(self.chapter_idx - 1);
            self.pos = 0;
        }
    }
    fn scroll_down(&mut self, n: usize) {
        if self.rows < self.chapter.len() - self.pos {
            self.pos += n;
        } else {
            self.next_chapter();
        }
    }
    fn scroll_up(&mut self, n: usize) {
        if self.pos > 0 {
            self.pos = self.pos.saturating_sub(n);
        } else {
            self.prev_chapter();
            self.pos = (self.chapter.len() / self.rows) * self.rows;
        }
    }
    fn search(&mut self, dir: Direction) {
        match dir {
            Direction::Forward => {
                if let Some(i) = self.chapter[self.pos..]
                    .iter()
                    .position(|s| s.contains(&self.search))
                {
                    self.pos += i;
                }
            }
            Direction::Backward => {
                if let Some(i) = self.chapter[..self.pos]
                    .iter()
                    .rposition(|s| s.contains(&self.search))
                {
                    self.pos = i;
                }
            }
        }
    }
}

fn restore() -> Option<Position> {
    let path = std::env::args().nth(1);
    let save_path =
        format!("{}/.local/share/bk", std::env::var("HOME").unwrap());
    let save = std::fs::read_to_string(save_path);

    let get_save = |s: String| {
        let mut lines = s.lines();
        Position(
            lines.next().unwrap().to_string(),
            lines.next().unwrap().parse::<usize>().unwrap(),
            lines.next().unwrap().parse::<usize>().unwrap(),
        )
    };

    match (save, path) {
        (Err(_), None) => None,
        (Err(_), Some(path)) => Some(Position(path, 0, 0)),
        (Ok(save), None) => Some(get_save(save)),
        (Ok(save), Some(path)) => {
            let save = get_save(save);
            if save.0 == path {
                Some(save)
            } else {
                Some(Position(path, 0, 0))
            }
        }
    }
}

fn main() {
    let pos = restore().unwrap_or_else(|| {
        println!("usage: bk path");
        std::process::exit(1);
    });

    let epub = Epub::new(&pos.0).unwrap_or_else(|e| {
        println!("error reading epub: {}", e);
        std::process::exit(1);
    });

    let mut bk = Bk::new(epub, &pos, 3);
    // crossterm really shouldn't error
    bk.run().unwrap();

    std::fs::write(
        format!("{}/.local/share/bk", std::env::var("HOME").unwrap()),
        format!("{}\n{}\n{}", pos.0, bk.chapter_idx, bk.pos),
    )
    .unwrap_or_else(|e| {
        println!("error saving position: {}", e);
        std::process::exit(1);
    });
}
