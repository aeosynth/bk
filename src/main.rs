use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    queue,
    style::{self, Print},
    terminal,
};
use serde::{Deserialize, Serialize};
use std::{
    cmp::min,
    collections::HashMap,
    env, fs,
    io::{stdout, Write},
    iter,
    process::exit,
};
use unicode_width::UnicodeWidthChar;

mod view;
use view::{Nav, Page, Search, View};

mod epub;
use epub::Chapter;

fn wrap(text: &str, max_cols: usize) -> Vec<(usize, usize)> {
    let mut lines = Vec::new();
    // bytes
    let mut start = 0;
    let mut end = 0;
    // cols after the break
    let mut after = 0;
    // cols of unbroken line
    let mut cols = 0;
    // are we breaking on whitespace?
    let mut space = false;

    // should probably use unicode_segmentation grapheme_indices
    for (i, c) in text.char_indices() {
        // https://github.com/unicode-rs/unicode-width/issues/6
        let char_cols = c.width().unwrap_or(0);
        cols += char_cols;
        match c {
            '\n' => {
                after = 0;
                end = i;
                space = true;
                cols = max_cols + 1;
            }
            ' ' => {
                after = 0;
                end = i;
                space = true;
            }
            '-' | '—' if cols <= max_cols => {
                after = 0;
                end = i + c.len_utf8();
                space = false;
            }
            _ => after += char_cols,
        }
        if cols > max_cols {
            // break a single long word
            if cols == after {
                after = char_cols;
                end = i;
                space = false;
            }
            lines.push((start, end));
            start = end;
            if space {
                start += 1;
            }
            cols = after;
        }
    }

    lines
}

fn get_line(lines: &[(usize, usize)], byte: usize) -> usize {
    match lines.binary_search_by_key(&byte, |&(a, _)| a) {
        Ok(n) => n,
        Err(n) => n - 1,
    }
}

struct SearchArgs {
    dir: Direction,
    skip: bool,
}

#[derive(Clone)]
enum Direction {
    Next,
    Prev,
}

pub struct Bk<'a> {
    chapters: Vec<epub::Chapter>,
    // position in the book
    chapter: usize,
    line: usize,
    mark: HashMap<char, (usize, usize)>,
    links: HashMap<String, (usize, usize)>,
    // layout
    cols: u16,
    rows: usize,
    max_width: u16,
    // view state
    view: Option<&'a dyn View>,
    cursor: usize,
    dir: Direction,
    meta: Vec<String>,
    query: String,
}

impl Bk<'_> {
    fn new(epub: epub::Epub, args: Props) -> Self {
        let (cols, rows) = terminal::size().unwrap();
        let width = min(cols, args.width) as usize;
        let meta = wrap(&epub.meta, width)
            .into_iter()
            .map(|(a, b)| String::from(&epub.meta[a..b]))
            .collect();

        let mut chapters = epub.chapters;
        for c in &mut chapters {
            c.lines = wrap(&c.text, width);
            if c.title.chars().count() > width {
                c.title = c
                    .title
                    .chars()
                    .take(width - 1)
                    .chain(iter::once('…'))
                    .collect();
            }
        }

        let mut bk = Bk {
            chapters,
            chapter: args.chapter,
            line: 0,
            mark: HashMap::new(),
            links: epub.links,
            cols,
            rows: rows as usize,
            max_width: args.width,
            view: Some(if args.toc { &Nav } else { &Page }),
            cursor: 0,
            dir: Direction::Next,
            meta,
            query: String::new(),
        };

        bk.line = get_line(&bk.chap().lines, args.byte);
        bk.mark('\'');

        bk
    }
    fn pad(&self) -> u16 {
        self.cols.saturating_sub(self.max_width) / 2
    }
    fn run(&mut self) -> crossterm::Result<()> {
        let mut stdout = stdout();
        queue!(
            stdout,
            terminal::EnterAlternateScreen,
            cursor::Hide,
            EnableMouseCapture
        )?;
        terminal::enable_raw_mode()?;

        while let Some(view) = self.view {
            queue!(
                stdout,
                terminal::Clear(terminal::ClearType::All),
                Print(style::Attribute::Reset)
            )?;
            for (i, line) in view.render(self).iter().enumerate() {
                queue!(stdout, cursor::MoveTo(self.pad(), i as u16), Print(line))?;
            }
            queue!(stdout, cursor::MoveTo(self.pad(), self.cursor as u16))?;
            stdout.flush().unwrap();

            match event::read()? {
                Event::Key(e) => view.on_key(self, e.code),
                Event::Mouse(e) => view.on_mouse(self, e),
                Event::Resize(cols, rows) => {
                    self.rows = rows as usize;
                    if cols != self.cols {
                        self.cols = cols;
                        let width = min(cols, self.max_width) as usize;
                        for c in &mut self.chapters {
                            c.lines = wrap(&c.text, width);
                        }
                    }
                    view.on_resize(self);
                    // XXX marks aren't updated
                }
            }
        }

        queue!(
            stdout,
            terminal::LeaveAlternateScreen,
            cursor::Show,
            DisableMouseCapture
        )?;
        terminal::disable_raw_mode()
    }
    fn mark(&mut self, c: char) {
        self.mark.insert(c, (self.chapter, self.line));
    }
    fn jump(&mut self, (c, l): (usize, usize)) {
        self.mark('\'');
        self.chapter = c;
        self.line = l;
    }
    fn jump_reset(&mut self) {
        let &(c, l) = self.mark.get(&'\'').unwrap();
        self.chapter = c;
        self.line = l;
    }
    fn chap(&self) -> &Chapter {
        &self.chapters[self.chapter]
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
        if self.line + self.rows < self.chap().lines.len() {
            self.line += n;
        } else {
            self.next_chapter();
        }
    }
    fn scroll_up(&mut self, n: usize) {
        if self.line > 0 {
            self.line = self.line.saturating_sub(n);
        } else if self.chapter > 0 {
            self.chapter -= 1;
            self.line = self.chap().lines.len().saturating_sub(self.rows);
        }
    }
    fn start_search(&mut self, dir: Direction) {
        self.mark('\'');
        self.query.clear();
        self.dir = dir;
        self.view = Some(&Search);
    }
    fn search(&mut self, args: SearchArgs) -> bool {
        let (start, end) = self.chap().lines[self.line];
        match args.dir {
            Direction::Next => {
                let byte = if args.skip { end } else { start };
                let head = (self.chapter, byte);
                let tail = (self.chapter + 1..self.chapters.len() - 1).map(|n| (n, 0));
                for (c, byte) in iter::once(head).chain(tail) {
                    if let Some(index) = self.chapters[c].text[byte..].find(&self.query) {
                        self.line = get_line(&self.chapters[c].lines, index + byte);
                        self.chapter = c;
                        return true;
                    }
                }
                false
            }
            Direction::Prev => {
                let byte = if args.skip { start } else { end };
                let head = (self.chapter, byte);
                let tail = (0..self.chapter)
                    .rev()
                    .map(|c| (c, self.chapters[c].text.len()));
                for (c, byte) in iter::once(head).chain(tail) {
                    if let Some(index) = self.chapters[c].text[..byte].rfind(&self.query) {
                        self.line = get_line(&self.chapters[c].lines, index);
                        self.chapter = c;
                        return true;
                    }
                }
                false
            }
        }
    }
}

#[derive(argh::FromArgs)]
/// read a book
struct Args {
    #[argh(positional)]
    path: Option<String>,

    /// print metadata and exit
    #[argh(switch, short = 'm')]
    meta: bool,

    /// start with table of contents open
    #[argh(switch, short = 't')]
    toc: bool,

    /// characters per line
    #[argh(option, short = 'w', default = "75")]
    width: u16,
}

struct Props {
    chapter: usize,
    byte: usize,
    width: u16,
    toc: bool,
}

#[derive(Default, Deserialize, Serialize)]
struct Save {
    last: String,
    files: HashMap<String, (usize, usize)>,
}

struct State {
    save: Save,
    save_path: String,
    path: String,
    meta: bool,
    bk: Props,
}

fn init() -> Result<State> {
    let save_path = if cfg!(windows) {
        format!("{}\\bk", env::var("APPDATA")?)
    } else {
        format!("{}/.local/share/bk", env::var("HOME")?)
    };
    // XXX will silently create a new default save if ron errors but path arg works.
    // revisit if/when stabilizing. ez file format upgrades
    let save = fs::read_to_string(&save_path)
        .map_err(anyhow::Error::new)
        .and_then(|s| {
            let save: Save = ron::from_str(&s)?;
            Ok(save)
        });
    let args: Args = argh::from_env();

    let mut path = args.path;
    // abort on path error
    if path.is_some() {
        path = Some(
            fs::canonicalize(path.unwrap())?
                .to_str()
                .unwrap()
                .to_string(),
        );
    }

    let (path, chapter, byte) = match (&save, &path) {
        (Err(_), None) => return Err(anyhow::anyhow!("no path arg and no or invalid save file")),
        (Err(_), Some(p)) => (p, 0, 0),
        (Ok(save), None) => {
            let &(chapter, byte) = save.files.get(&save.last).unwrap();
            (&save.last, chapter, byte)
        }
        (Ok(save), Some(p)) => {
            if save.files.contains_key(p) {
                let &(chapter, byte) = save.files.get(p).unwrap();
                (p, chapter, byte)
            } else {
                (p, 0, 0)
            }
        }
    };

    Ok(State {
        save_path,
        path: path.clone(),
        save: save.unwrap_or_default(),
        meta: args.meta,
        bk: Props {
            chapter,
            byte,
            width: args.width,
            toc: args.toc,
        },
    })
}

fn main() {
    let mut state = init().unwrap_or_else(|e| {
        println!("init error: {}", e);
        exit(1);
    });
    let epub = epub::Epub::new(&state.path, state.meta).unwrap_or_else(|e| {
        println!("epub error: {}", e);
        exit(1);
    });
    if state.meta {
        println!("{}", epub.meta);
        exit(0);
    }
    let mut bk = Bk::new(epub, state.bk);
    bk.run().unwrap_or_else(|e| {
        println!("run error: {}", e);
        exit(1);
    });

    let byte = bk.chap().lines[bk.line].0;
    state
        .save
        .files
        .insert(state.path.clone(), (bk.chapter, byte));
    state.save.last = state.path;
    let serialized = ron::to_string(&state.save).unwrap();
    fs::write(state.save_path, serialized).unwrap_or_else(|e| {
        println!("error saving state: {}", e);
        exit(1);
    });
}
