use std::collections::HashMap;
use std::fs::File;
use std::io::{stdout, Error, Read, Write};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, MouseEvent},
    queue,
    style::{Attribute, Print},
    terminal,
};

use roxmltree::Document;

enum Mode {
    Help,
    Nav,
    Read,
}

struct Epub {
    container: zip::ZipArchive<File>,
}

struct Bk {
    mode: Mode,
    epub: Epub,
    cols: u16,
    chapter: Vec<String>,
    chapter_idx: usize,
    nav_idx: usize,
    pos: usize,
    rows: usize,
    toc: Vec<(String, String)>,
    pad: u16,
}

impl Epub {
    fn new(path: &str) -> Result<Self, Error> {
        let file = File::open(path)?;

        Ok(Epub {
            container: zip::ZipArchive::new(file)?,
        })
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

        // manifest - paths
        // spine - order
        // toc - names
        let mut id_path = HashMap::new();
        let xml = self.get_text(path);
        let doc = Document::parse(&xml).unwrap();
        doc.root_element()
            .children()
            .find(|n| n.has_tag_name("manifest"))
            .unwrap()
            .children()
            .filter(|n| n.is_element())
            .for_each(|n| {
                id_path.insert(
                    n.attribute("id").unwrap(),
                    n.attribute("href").unwrap(),
                );
            });
        let dirname = std::path::Path::new(&path).parent().unwrap();
        let paths: Vec<&str> = doc
            .root_element()
            .children()
            .find(|n| n.has_tag_name("spine"))
            .unwrap()
            .children()
            .filter(|n| n.is_element())
            .map(|n| id_path.remove(n.attribute("idref").unwrap()).unwrap())
            .collect();

        // epub2: item id="ncx" (spine toc=id)
        // epub3: item properties="nav"
        // TODO is epub3 toc usable w/o spine?
        let toc: Vec<_> = {
            if let Some(path) = id_path.get("ncx") {
                let xml = self.get_text(dirname.join(path).to_str().unwrap());
                let doc = Document::parse(&xml).unwrap();

                doc.descendants()
                    .find(|n| n.has_tag_name("navMap"))
                    .unwrap()
                    .descendants()
                    .filter(|n| n.has_tag_name("navPoint"))
                    .map(|n| {
                        (
                            n.descendants()
                                .find(|n| n.has_tag_name("content"))
                                .unwrap()
                                .attribute("src")
                                .unwrap()
                                .to_string(),
                            n.descendants()
                                .find(|n| n.has_tag_name("text"))
                                .unwrap()
                                .text()
                                .unwrap()
                                .to_string(),
                        )
                    })
                    .collect()
            } else if let Some(path) = id_path.get("toc.xhtml") {
                let xml = self.get_text(dirname.join(path).to_str().unwrap());
                let doc = Document::parse(&xml).unwrap();

                doc.descendants()
                    .find(|n| n.has_tag_name("nav"))
                    .unwrap()
                    .descendants()
                    .filter(|n| n.has_tag_name("a"))
                    .map(|n| {
                        let path = n.attribute("href").unwrap().to_string();
                        let text = n
                            .descendants()
                            .filter(|n| n.is_text())
                            .map(|n| n.text().unwrap())
                            .collect();
                        (path, text)
                    })
                    .collect()
            } else {
                panic!("can't read epub");
            }
        };

        // playOrder is not a thing
        let mut toc_idx = 0;
        paths
            .into_iter()
            .enumerate()
            .map(|(i, path)| {
                let zip_path =
                    dirname.join(path).to_str().unwrap().to_string();
                let name = match toc.get(toc_idx) {
                    Some(point) if point.0 == path => {
                        toc_idx += 1;
                        point.1.to_string()
                    }
                    _ => i.to_string(),
                };
                (name, zip_path)
            })
            .collect()
    }
}

fn wrap(text: Vec<String>, width: u16) -> Vec<String> {
    // XXX assumes a char is 1 unit wide
    let mut wrapped = Vec::with_capacity(text.len() * 2);

    for chunk in text {
        let mut start = 0;
        let mut space = 0;
        let mut line = 0;
        let mut word = 0;

        for (i, c) in chunk.char_indices() {
            if c == ' ' {
                space = i;
                word = 0;
            } else {
                word += 1;
            }
            if line == width {
                wrapped.push(String::from(&chunk[start..space]));
                start = space + 1;
                line = word;
            } else {
                line += 1;
            }
        }
        wrapped.push(String::from(&chunk[start..]));
    }
    wrapped
}

impl Bk {
    fn new(
        path: &str,
        chapter_idx: usize,
        pos: usize,
        pad: u16,
    ) -> Result<Self, Error> {
        let (cols, rows) = terminal::size().unwrap();
        let mut epub = Epub::new(path)?;
        let mut bk = Bk {
            mode: Mode::Read,
            chapter: Vec::new(),
            chapter_idx,
            nav_idx: 0,
            toc: epub.get_toc(),
            epub,
            pos,
            pad,
            cols,
            rows: rows as usize,
        };
        bk.get_chapter(chapter_idx);
        bk.pos = pos;
        Ok(bk)
    }
    fn get_chapter(&mut self, idx: usize) {
        let mut chapter = Vec::new();
        let xml = self.epub.get_text(&self.toc[idx].1);
        let doc = Document::parse(&xml).unwrap();

        for n in doc.descendants() {
            match n.tag_name().name() {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    let text: String = n
                        .descendants()
                        .filter(|n| n.is_text())
                        .map(|n| n.text().unwrap())
                        .collect();
                    chapter.push(format!(
                        "{}{}{}",
                        Attribute::Bold,
                        text,
                        Attribute::Reset
                    ));
                    chapter.push(String::from(""));
                }
                "p" => {
                    chapter.push(
                        n.descendants()
                            .filter(|n| n.is_text())
                            .map(|n| n.text().unwrap())
                            .collect(),
                    );
                    chapter.push(String::from(""));
                }
                "li" => {
                    chapter.push(
                        n.descendants()
                            .filter(|n| n.is_text())
                            .map(|n| format!("- {}", n.text().unwrap()))
                            .collect(),
                    );
                    chapter.push(String::from(""));
                }
                _ => (),
            }
        }
        chapter.pop(); //padding
        self.chapter = wrap(chapter, self.cols - (self.pad * 2));
        self.chapter_idx = idx;
        self.pos = 0;
    }
    fn scroll_down(&mut self, n: usize) {
        if self.rows < self.chapter.len() - self.pos {
            self.pos += n;
        } else if self.chapter_idx < self.toc.len() - 1 {
            self.get_chapter(self.chapter_idx + 1);
        }
    }
    fn scroll_up(&mut self, n: usize) {
        if self.pos > 0 {
            self.pos = self.pos.saturating_sub(n);
        } else if self.chapter_idx > 0 {
            self.get_chapter(self.chapter_idx - 1);
            self.pos = (self.chapter.len() / self.rows) * self.rows;
        }
    }
    fn run(&mut self, e: Event) -> bool {
        match self.mode {
            Mode::Read => return self.run_read(e),
            Mode::Nav => self.run_nav(e),
            Mode::Help => self.mode = Mode::Read,
        }
        true
    }
    fn run_nav(&mut self, e: Event) {
        match e {
            Event::Key(e) => match e.code {
                KeyCode::Esc | KeyCode::Tab | KeyCode::Char('q') => {
                    self.mode = Mode::Read
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if self.nav_idx < self.toc.len() - 1 {
                        self.nav_idx += 1;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if self.nav_idx > 0 {
                        self.nav_idx -= 1;
                    }
                }
                KeyCode::Enter => {
                    self.get_chapter(self.nav_idx);
                    self.mode = Mode::Read;
                }
                KeyCode::Home | KeyCode::Char('g') => self.nav_idx = 0,
                KeyCode::End | KeyCode::Char('G') => {
                    self.nav_idx = self.toc.len() - 1
                }
                _ => (),
            },
            Event::Mouse(e) => match e {
                MouseEvent::ScrollDown(_, _, _) => {
                    if self.nav_idx < self.toc.len() - 1 {
                        self.nav_idx += 1;
                    }
                }
                MouseEvent::ScrollUp(_, _, _) => {
                    if self.nav_idx > 0 {
                        self.nav_idx -= 1;
                    }
                }
                MouseEvent::Down(event::MouseButton::Left, _, row, _) => {
                    self.get_chapter(row as usize);
                    self.mode = Mode::Read;
                }
                _ => (),
            },
            _ => (),
        }
    }
    fn run_read(&mut self, e: Event) -> bool {
        match e {
            Event::Key(e) => match e.code {
                KeyCode::Esc | KeyCode::Char('q') => return false,
                KeyCode::Tab => self.start_nav(),
                KeyCode::F(1) | KeyCode::Char('?') => self.mode = Mode::Help,
                KeyCode::Char('p') => {
                    if self.chapter_idx > 0 {
                        self.get_chapter(self.chapter_idx - 1);
                    }
                }
                KeyCode::Char('n') => {
                    if self.chapter_idx < self.toc.len() - 1 {
                        self.get_chapter(self.chapter_idx + 1);
                    }
                }
                KeyCode::End | KeyCode::Char('G') => {
                    self.pos = (self.chapter.len() / self.rows) * self.rows;
                }
                KeyCode::Home | KeyCode::Char('g') => self.pos = 0,
                KeyCode::Char('d') => {
                    self.scroll_down(self.rows / 2);
                }
                KeyCode::Char('u') => {
                    self.scroll_up(self.rows / 2);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.scroll_up(1);
                }
                KeyCode::Left
                | KeyCode::PageUp
                | KeyCode::Char('b')
                | KeyCode::Char('h') => {
                    self.scroll_up(self.rows);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.scroll_down(1);
                }
                KeyCode::Right
                | KeyCode::PageDown
                | KeyCode::Char('f')
                | KeyCode::Char('l')
                | KeyCode::Char(' ') => {
                    self.scroll_down(self.rows);
                }
                _ => (),
            },
            Event::Mouse(e) => match e {
                MouseEvent::ScrollDown(_, _, _) => self.scroll_down(3),
                MouseEvent::ScrollUp(_, _, _) => self.scroll_up(3),
                _ => (),
            },
            Event::Resize(cols, rows) => {
                self.cols = cols;
                self.rows = rows as usize;
                self.get_chapter(self.chapter_idx);
            }
        }
        true
    }
    fn render(&self) {
        match self.mode {
            Mode::Read => self.render_read(),
            Mode::Help => self.render_help(),
            Mode::Nav => self.render_nav(),
        }
    }
    fn start_nav(&mut self) {
        self.nav_idx = self.chapter_idx;
        self.mode = Mode::Nav;
    }
    fn render_nav(&self) {
        let mut stdout = stdout();
        queue!(stdout, terminal::Clear(terminal::ClearType::All),).unwrap();
        let end = std::cmp::min(self.rows, self.toc.len());
        for i in 0..end {
            if i == self.nav_idx {
                queue!(
                    stdout,
                    cursor::MoveTo(0, i as u16),
                    Print(format!(
                        "{}{}{}",
                        Attribute::Reverse,
                        &self.toc[i].0,
                        Attribute::Reset
                    ))
                )
                .unwrap();
            } else {
                queue!(
                    stdout,
                    cursor::MoveTo(0, i as u16),
                    Print(&self.toc[i].0)
                )
                .unwrap();
            }
        }

        stdout.flush().unwrap();
    }
    fn render_help(&self) {
        let text = r#"
                   Esc q  Quit
                    F1 ?  Help
                     Tab  Table of Contents
PageDown Right Space f l  Page Down
         PageUp Left b h  Page Up
                       d  Half Page Down
                       u  Half Page Up
                  Down j  Line Down
                    Up k  Line Up
                       n  Next Chapter
                       p  Previous Chapter
                  Home g  Chapter Start
                   End G  Chapter End
                   "#;

        let mut stdout = stdout();
        queue!(stdout, terminal::Clear(terminal::ClearType::All),).unwrap();
        for (i, line) in text.lines().enumerate() {
            queue!(stdout, cursor::MoveTo(0, i as u16), Print(line)).unwrap();
        }
        stdout.flush().unwrap();
    }
    fn render_read(&self) {
        let mut stdout = stdout();
        queue!(
            stdout,
            terminal::Clear(terminal::ClearType::All),
            cursor::MoveTo(self.pad, 0),
        )
        .unwrap();

        let end = std::cmp::min(self.pos + self.rows, self.chapter.len());
        for line in self.pos..end {
            queue!(
                stdout,
                Print(&self.chapter[line]),
                cursor::MoveToNextLine(1),
                cursor::MoveRight(self.pad)
            )
            .unwrap();
        }
        stdout.flush().unwrap();
    }
}

fn restore() -> Option<(String, usize, usize)> {
    let path = std::env::args().nth(1);
    let save_path =
        format!("{}/.local/share/bk", std::env::var("HOME").unwrap());
    let save = std::fs::read_to_string(save_path);

    let get_save = |s: String| {
        let mut lines = s.lines();
        (
            lines.next().unwrap().to_string(),
            lines.next().unwrap().parse::<usize>().unwrap(),
            lines.next().unwrap().parse::<usize>().unwrap(),
        )
    };

    match (save, path) {
        (Err(_), None) => None,
        (Err(_), Some(path)) => Some((path, 0, 0)),
        (Ok(save), None) => Some(get_save(save)),
        (Ok(save), Some(path)) => {
            let save = get_save(save);
            if save.0 == path {
                Some(save)
            } else {
                Some((path, 0, 0))
            }
        }
    }
}

fn main() -> crossterm::Result<()> {
    let (path, chapter, pos) = restore().unwrap_or_else(|| {
        println!("usage: bk path");
        std::process::exit(1);
    });

    let mut bk = Bk::new(&path, chapter, pos, 3).unwrap_or_else(|e| {
        println!("error reading epub: {}", e);
        std::process::exit(1);
    });

    let mut stdout = stdout();
    queue!(
        stdout,
        terminal::EnterAlternateScreen,
        cursor::Hide,
        event::EnableMouseCapture
    )?;
    terminal::enable_raw_mode()?;

    bk.render();
    while bk.run(event::read()?) {
        bk.render();
    }

    std::fs::write(
        format!("{}/.local/share/bk", std::env::var("HOME").unwrap()),
        format!("{}\n{}\n{}", path, bk.chapter_idx, bk.pos),
    )
    .unwrap();

    queue!(
        stdout,
        terminal::LeaveAlternateScreen,
        cursor::Show,
        event::DisableMouseCapture
    )?;
    stdout.flush()?;
    terminal::disable_raw_mode()
}
