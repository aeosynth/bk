use std::collections::HashMap;
use std::fs::File;
use std::io::{stdout, Error, Read, Write};

use crossterm::{
    cursor,
    event::{
        read, DisableMouseCapture, EnableMouseCapture, Event, KeyCode,
        MouseEvent,
    },
    queue,
    style::{Attribute, Print},
    terminal,
};

use roxmltree::Document;

struct Epub {
    container: zip::ZipArchive<File>,
}

struct Bk {
    epub: Epub,
    cols: u16,
    chapter: Vec<String>,
    chapter_idx: usize,
    pos: usize,
    rows: usize,
    toc: Vec<String>,
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
    fn get_toc(&mut self) -> Vec<String> {
        // container.xml -> <rootfile> -> opf -> <spine> -> <manifest>
        let xml = self.get_text("META-INF/container.xml");
        let doc = Document::parse(&xml).unwrap();
        let path = doc
            .descendants()
            .find(|n| n.has_tag_name("rootfile"))
            .unwrap()
            .attribute("full-path")
            .unwrap();

        let mut manifest = HashMap::new();
        let xml = self.get_text(path);
        let doc = Document::parse(&xml).unwrap();
        doc.root_element()
            .children()
            .find(|n| n.has_tag_name("manifest"))
            .unwrap()
            .children()
            .filter(|n| n.is_element())
            .for_each(|n| {
                manifest.insert(
                    n.attribute("id").unwrap(),
                    n.attribute("href").unwrap(),
                );
            });

        let path = std::path::Path::new(&path).parent().unwrap();
        doc.root_element()
            .children()
            .find(|n| n.has_tag_name("spine"))
            .unwrap()
            .children()
            .filter(|n| n.is_element())
            .map(|n| {
                let name =
                    manifest.get(n.attribute("idref").unwrap()).unwrap();
                path.join(name).to_str().unwrap().to_string()
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
            line += 1;
            word += 1;
            if c == ' ' {
                space = i;
                word = 0;
            }
            if line == width {
                wrapped.push(String::from(&chunk[start..space]));
                start = space + 1;
                line = word;
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
            chapter: Vec::new(),
            chapter_idx,
            toc: epub.get_toc(),
            epub,
            pos,
            pad,
            cols,
            rows: rows as usize,
        };
        bk.get_chapter(chapter_idx);
        Ok(bk)
    }
    fn get_chapter(&mut self, idx: usize) {
        let mut chapter = Vec::new();
        let xml = self.epub.get_text(&self.toc[idx]);
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
                _ => (),
            }
        }
        chapter.pop(); //padding
        self.chapter = wrap(chapter, self.cols - (self.pad * 2));
        self.chapter_idx = idx;
    }
    fn scroll_down(&mut self, n: usize) {
        if self.rows < self.chapter.len() - self.pos {
            self.pos += n;
        } else if self.chapter_idx < self.toc.len() - 1 {
            self.get_chapter(self.chapter_idx + 1);
            self.pos = 0;
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
    fn run(&mut self, event: Event) -> bool {
        match event {
            Event::Key(e) => match e.code {
                KeyCode::Char('q') => return true,
                KeyCode::Char('p') => {
                    if self.chapter_idx > 0 {
                        self.get_chapter(self.chapter_idx - 1);
                        self.pos = 0;
                    }
                }
                KeyCode::Char('n') => {
                    if self.chapter_idx < self.toc.len() - 1 {
                        self.get_chapter(self.chapter_idx + 1);
                        self.pos = 0;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.scroll_up(1);
                }
                KeyCode::Left | KeyCode::PageUp | KeyCode::Char('h') => {
                    self.scroll_up(self.rows);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.scroll_down(1);
                }
                KeyCode::Right
                | KeyCode::PageDown
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
        false
    }
    fn render(&self) {
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

    let mut bk = Bk::new(&path, chapter, pos, 2).unwrap_or_else(|e| {
        println!("error reading epub: {}", e);
        std::process::exit(1);
    });

    let mut stdout = stdout();
    queue!(
        stdout,
        terminal::EnterAlternateScreen,
        cursor::Hide,
        EnableMouseCapture
    )?;
    terminal::enable_raw_mode()?;

    loop {
        bk.render();
        if bk.run(read()?) {
            break;
        }
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
        DisableMouseCapture
    )?;
    stdout.flush()?;
    terminal::disable_raw_mode()
}
