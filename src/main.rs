use std::io::{stdout, Write};
use std::{cmp::min, collections::HashMap, env, iter, process::exit};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode},
    queue,
    style::{Attribute, Print},
    terminal,
};

mod epub;

fn wrap(text: &str, width: usize) -> Vec<(usize, String)> {
    // XXX assumes a char is 1 unit wide
    let mut lines = Vec::new();

    let mut start = 0;
    let mut end = 0;
    let mut len = 0;
    let mut word = 0;
    let mut skip = 0;

    for (i, c) in text.char_indices() {
        len += 1;
        match c {
            ' ' => {
                end = i;
                skip = 1;
                word = 0;
            }
            '-' | '—' => {
                if len > width {
                    // `end = i + len` is a hack that breaks here
                    word += 1;
                } else {
                    end = i + c.len_utf8();
                    skip = 0;
                    word = 0;
                }
            }
            _ => {
                word += 1;
            }
        }
        if c == '\n' {
            lines.push((start, String::from(&text[start..i])));
            start = i + 1;
            len = 0;
        } else if len > width {
            let line = if word == len {
                &text[start..i]
            } else {
                &text[start..end]
            };
            lines.push((start, String::from(line)));
            start = end + skip;
            len = word;
        }
    }

    lines
}

#[derive(Clone)]
enum Direction {
    Forward,
    Backward,
}

trait View {
    fn run(&self, bk: &mut Bk, kc: KeyCode);
    fn render(&self, bk: &Bk) -> Vec<String>;
}

// TODO render something useful?
struct Mark;
impl View for Mark {
    fn run(&self, bk: &mut Bk, kc: KeyCode) {
        match kc {
            KeyCode::Char(c) => bk.mark(c),
            _ => (),
        }
        bk.view = Some(&Page)
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        Page::render(&Page, bk)
    }
}

struct Jump;
impl View for Jump {
    fn run(&self, bk: &mut Bk, kc: KeyCode) {
        match kc {
            KeyCode::Char(c) => bk.jump(c),
            _ => (),
        }
        bk.view = Some(&Page);
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        Page::render(&Page, bk)
    }
}

struct Help;
impl View for Help {
    fn run(&self, bk: &mut Bk, _: KeyCode) {
        bk.view = Some(&Page);
    }
    fn render(&self, _: &Bk) -> Vec<String> {
        let text = r#"
                   Esc q  Quit
                      Fn  Help
                       /  Search Forward
                       ?  Search Backward
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

                       n  Repeat search forward
                       N  Repeat search backward
                      mx  Set mark x
                      'x  Jump to mark x
                   "#;

        text.lines().map(String::from).collect()
    }
}

struct Nav;
impl View for Nav {
    fn run(&self, bk: &mut Bk, kc: KeyCode) {
        match kc {
            KeyCode::Esc
            | KeyCode::Tab
            | KeyCode::Left
            | KeyCode::Char('h')
            | KeyCode::Char('q') => {
                bk.jump_reset();
                bk.view = Some(&Page);
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                bk.line = 0;
                bk.view = Some(&Page);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if bk.chapter < bk.chapters.len() - 1 {
                    bk.chapter += 1;
                    if bk.chapter == bk.nav_top + bk.rows {
                        bk.nav_top += 1;
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if bk.chapter > 0 {
                    if bk.chapter == bk.nav_top {
                        bk.nav_top -= 1;
                    }
                    bk.chapter -= 1;
                }
            }
            KeyCode::Home | KeyCode::Char('g') => {
                bk.chapter = 0;
                bk.nav_top = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                bk.chapter = bk.chapters.len() - 1;
                bk.nav_top = bk.chapters.len().saturating_sub(bk.rows);
            }
            _ => (),
        }
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        let end = min(bk.nav_top + bk.rows, bk.chapters.len());

        bk.chapters[bk.nav_top..end]
            .iter()
            .enumerate()
            .map(|(i, chapter)| {
                if bk.chapter == bk.nav_top + i {
                    format!(
                        "{}{}{}",
                        Attribute::Reverse,
                        chapter.title,
                        Attribute::Reset
                    )
                } else {
                    chapter.title.to_string()
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
                bk.nav_top = bk.chapter.saturating_sub(bk.rows - 1);
                bk.mark('\'');
                bk.view = Some(&Nav);
            }
            KeyCode::F(_) => bk.view = Some(&Help),
            KeyCode::Char('?') => bk.start_search(Direction::Backward),
            KeyCode::Char('/') => bk.start_search(Direction::Forward),
            KeyCode::Char('m') => bk.view = Some(&Mark),
            KeyCode::Char('\'') => bk.view = Some(&Jump),
            KeyCode::Char('N') => {
                bk.search(Direction::Backward);
            }
            KeyCode::Char('n') => {
                // FIXME
                bk.scroll_down(1);
                bk.search(Direction::Forward);
            }
            KeyCode::End | KeyCode::Char('G') => {
                bk.mark('\'');
                bk.line = bk.lines().len().saturating_sub(bk.rows);
            }
            KeyCode::Home | KeyCode::Char('g') => {
                bk.mark('\'');
                bk.line = 0;
            }
            KeyCode::Char('d') => {
                bk.scroll_down(bk.rows / 2);
            }
            KeyCode::Char('u') => {
                bk.scroll_up(bk.rows / 2);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                bk.scroll_up(2);
            }
            KeyCode::Left | KeyCode::PageUp | KeyCode::Char('b') | KeyCode::Char('h') => {
                bk.scroll_up(bk.rows);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                bk.scroll_down(2);
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
        let end = min(bk.line + bk.rows, bk.lines().len());
        bk.lines()[bk.line..end].iter().map(String::from).collect()
    }
}

struct Search;
impl View for Search {
    fn run(&self, bk: &mut Bk, kc: KeyCode) {
        match kc {
            KeyCode::Esc => {
                bk.jump_reset();
                bk.view = Some(&Page);
            }
            KeyCode::Enter => {
                bk.view = Some(&Page);
            }
            KeyCode::Backspace => {
                bk.query.pop();
                bk.jump_reset();
                bk.search(bk.dir.clone());
            }
            KeyCode::Char(c) => {
                bk.query.push(c);
                if !bk.search(bk.dir.clone()) {
                    bk.jump_reset();
                }
            }
            _ => (),
        }
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        let end = min(bk.line + bk.rows - 1, bk.lines().len());
        let mut buf = Vec::with_capacity(bk.rows);

        for line in bk.lines()[bk.line..end].iter() {
            if let Some(i) = line.find(&bk.query) {
                buf.push(format!(
                    "{}{}{}{}{}",
                    &line[..i],
                    Attribute::Reverse,
                    &bk.query,
                    Attribute::Reset,
                    &line[i + bk.query.len()..],
                ));
            } else {
                buf.push(String::from(line));
            }
        }

        for _ in buf.len()..bk.rows - 1 {
            buf.push(String::new());
        }
        let prefix = match bk.dir {
            Direction::Forward => '/',
            Direction::Backward => '?',
        };
        buf.push(format!("{}{}", prefix, bk.query));
        buf
    }
}

// search the text to find the byte index of the query, then find the containing line
// ideally we could use string slices as pointers, but self referential structs are hard
struct Chapter {
    title: String,
    text: String,
    bytes: Vec<usize>,
    lines: Vec<String>,
}

struct Bk<'a> {
    chapters: Vec<Chapter>,
    // position in the book
    chapter: usize,
    line: usize,
    mark: HashMap<char, (usize, usize)>,
    // terminal
    cols: u16,
    rows: usize,
    // user config
    max_width: u16,
    // view state
    view: Option<&'a dyn View>,
    dir: Direction,
    nav_top: usize,
    query: String,
}

impl Bk<'_> {
    fn new(epub: epub::Epub, chapter: usize, line: usize, max_width: u16) -> Self {
        let (cols, rows) = terminal::size().unwrap();
        let width = min(cols, max_width) as usize;

        let mut chapters = Vec::with_capacity(epub.chapters.len());
        for (title, text) in epub.chapters {
            let title = if title.chars().count() > width {
                title
                    .chars()
                    .take(width - 1)
                    .chain(iter::once('…'))
                    .collect()
            } else {
                title
            };
            let wrap = wrap(&text, width);
            let mut lines = Vec::with_capacity(wrap.len());
            let mut bytes = Vec::with_capacity(wrap.len());

            for (byte, line) in wrap {
                lines.push(line);
                bytes.push(byte);
            }
            chapters.push(Chapter {
                title,
                text,
                lines,
                bytes,
            });
        }

        Bk {
            chapters,
            chapter,
            line,
            mark: HashMap::new(),
            cols,
            rows: rows as usize,
            max_width,
            view: Some(&Page),
            dir: Direction::Forward,
            nav_top: 0,
            query: String::new(),
        }
    }
    fn run(&mut self) -> crossterm::Result<()> {
        let mut stdout = stdout();
        queue!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
        terminal::enable_raw_mode()?;

        while let Some(view) = self.view {
            let pad = self.cols.saturating_sub(self.max_width) / 2;

            queue!(stdout, terminal::Clear(terminal::ClearType::All))?;
            for (i, line) in view.render(self).iter().enumerate() {
                queue!(stdout, cursor::MoveTo(pad, i as u16), Print(line))?;
            }
            stdout.flush().unwrap();

            match event::read()? {
                Event::Key(e) => view.run(self, e.code),
                Event::Resize(cols, rows) => {
                    self.cols = cols;
                    self.rows = rows as usize;
                }
                // TODO
                Event::Mouse(_) => (),
            }
        }

        queue!(stdout, terminal::LeaveAlternateScreen, cursor::Show)?;
        terminal::disable_raw_mode()
    }
    fn mark(&mut self, c: char) {
        self.mark.insert(c, (self.chapter, self.line));
    }
    fn jump(&mut self, c: char) {
        if let Some(&(c, l)) = self.mark.get(&c) {
            let jump = (self.chapter, self.line);
            self.chapter = c;
            self.line = l;
            self.mark.insert('\'', jump);
        }
    }
    fn jump_reset(&mut self) {
        let &(c, l) = self.mark.get(&'\'').unwrap();
        self.chapter = c;
        self.line = l;
    }
    fn lines(&self) -> &Vec<String> {
        &self.chapters[self.chapter].lines
    }
    fn next_chapter(&mut self) {
        if self.chapter < self.chapters.len() - 1 {
            self.chapter += 1;
            self.line = 0;
        }
    }
    fn prev_chapter(&mut self) {
        if self.chapter > 0 {
            self.chapter -= 1;
            self.line = 0;
        }
    }
    fn scroll_down(&mut self, n: usize) {
        if self.line + self.rows < self.lines().len() {
            self.line += n;
        } else {
            self.next_chapter();
        }
    }
    fn scroll_up(&mut self, n: usize) {
        if self.line > 0 {
            self.line = self.line.saturating_sub(n);
        } else {
            self.prev_chapter();
            self.line = self.lines().len().saturating_sub(self.rows);
        }
    }
    fn start_search(&mut self, dir: Direction) {
        self.mark('\'');
        self.query.clear();
        self.dir = dir;
        self.view = Some(&Search);
    }
    fn search(&mut self, dir: Direction) -> bool {
        // https://doc.rust-lang.org/std/vec/struct.Vec.html#method.binary_search
        // If the value is not found then Result::Err is returned, containing the index where a matching element
        // could be inserted while maintaining sorted order.
        let get_line = |bytes: &Vec<usize>, byte: usize| -> usize {
            match bytes.binary_search(&byte) {
                Ok(n) => n,
                Err(n) => n - 1,
            }
        };
        let head = (self.chapter, self.chapters[self.chapter].bytes[self.line]);
        match dir {
            Direction::Forward => {
                let tail = (self.chapter + 1..self.chapters.len() - 1).map(|n| (n, 0));
                for (c, byte) in iter::once(head).chain(tail) {
                    if let Some(index) = self.chapters[c].text[byte..].find(&self.query) {
                        self.line = get_line(&self.chapters[c].bytes, index + byte);
                        self.chapter = c;
                        return true;
                    }
                }
                false
            }
            Direction::Backward => {
                let tail = (0..self.chapter - 1)
                    .rev()
                    .map(|c| (c, self.chapters[c].text.len()));
                for (c, byte) in iter::once(head).chain(tail) {
                    if let Some(index) = self.chapters[c].text[..byte].rfind(&self.query) {
                        self.line = get_line(&self.chapters[c].bytes, index);
                        self.chapter = c;
                        return true;
                    }
                }
                false
            }
        }
    }
}

fn restore(save_path: &str) -> Option<(String, usize, usize)> {
    let path = env::args().nth(1);
    let save = std::fs::read_to_string(save_path);

    let get_save = |s: String| {
        let mut lines = s.lines();
        (
            lines.next().unwrap().to_string(),
            lines.next().unwrap().parse::<usize>().unwrap(),
            lines.next().unwrap().parse::<usize>().unwrap(),
        )
    };

    let canon = |s: String| {
        std::fs::canonicalize(s)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    };

    match (save, path) {
        (Err(_), None) => None,
        (Err(_), Some(path)) => Some((canon(path), 0, 0)),
        (Ok(save), None) => Some(get_save(save)),
        (Ok(save), Some(path)) => {
            let save = get_save(save);
            if path == save.0 {
                Some(save)
            } else {
                Some((canon(path), 0, 0))
            }
        }
    }
}

fn main() {
    let save_path = if cfg!(windows) {
        format!("{}\\bk", env::var("APPDATA").unwrap())
    } else {
        format!("{}/.local/share/bk", env::var("HOME").unwrap())
    };
    let (path, chapter, line) = restore(&save_path).unwrap_or_else(|| {
        println!("usage: bk path");
        exit(1);
    });

    let epub = epub::Epub::new(&path).unwrap_or_else(|e| {
        println!("error reading epub: {}", e);
        exit(1);
    });

    let mut bk = Bk::new(epub, chapter, line, 75);
    // crossterm really shouldn't error
    bk.run().unwrap();

    std::fs::write(save_path, format!("{}\n{}\n{}", path, bk.chapter, bk.line)).unwrap_or_else(
        |e| {
            println!("error saving position: {}", e);
            exit(1);
        },
    );
}
