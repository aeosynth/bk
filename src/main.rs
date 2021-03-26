use anyhow::Result;
use crossterm::{
    cursor,
    event::{DisableMouseCapture, EnableMouseCapture, Event},
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
    process::exit,
};
use unicode_width::UnicodeWidthChar;

mod view;
use view::{Page, Toc, View};

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
    quit: bool,
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
    view: &'a dyn View,
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
                    .chain(std::iter::once('…'))
                    .collect();
            }
        }

        let mut bk = Bk {
            quit: false,
            chapters,
            chapter: args.chapter,
            line: 0,
            mark: HashMap::new(),
            links: epub.links,
            cols,
            rows: rows as usize,
            max_width: args.width,
            view: if args.toc { &Toc } else { &Page },
            cursor: 0,
            dir: Direction::Next,
            meta,
            query: String::new(),
        };

        bk.line = get_line(&bk.chap().lines, args.byte);
        bk.mark('\'');

        bk
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

        let mut render = |bk: &Bk| {
            queue!(
                stdout,
                terminal::Clear(terminal::ClearType::All),
                Print(style::Attribute::Reset)
            )
            .unwrap();
            for (i, line) in bk.view.render(bk).iter().enumerate() {
                queue!(stdout, cursor::MoveTo(bk.pad(), i as u16), Print(line)).unwrap();
            }
            queue!(stdout, cursor::MoveTo(bk.pad(), bk.cursor as u16)).unwrap();
            stdout.flush().unwrap();
        };

        render(self);
        loop {
            match crossterm::event::read()? {
                Event::Key(e) => self.view.on_key(self, e.code),
                Event::Mouse(e) => {
                    // XXX idk seems lame
                    if e.kind == crossterm::event::MouseEventKind::Moved {
                        continue;
                    }
                    self.view.on_mouse(self, e);
                }
                Event::Resize(cols, rows) => {
                    self.rows = rows as usize;
                    if cols != self.cols {
                        self.cols = cols;
                        let width = min(cols, self.max_width) as usize;
                        for c in &mut self.chapters {
                            c.lines = wrap(&c.text, width);
                        }
                    }
                    self.view.on_resize(self);
                    // XXX marks aren't updated
                }
            }
            if self.quit {
                break;
            }
            render(self);
        }

        queue!(
            stdout,
            terminal::LeaveAlternateScreen,
            cursor::Show,
            DisableMouseCapture
        )?;
        terminal::disable_raw_mode()
    }
    fn chap(&self) -> &Chapter {
        &self.chapters[self.chapter]
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
    fn mark(&mut self, c: char) {
        self.mark.insert(c, (self.chapter, self.line));
    }
    fn pad(&self) -> u16 {
        self.cols.saturating_sub(self.max_width) / 2
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
        (Err(_), None) => return Err(anyhow::anyhow!("no path arg and no valid save file")),
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
