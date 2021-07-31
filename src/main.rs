use crossterm::{
    cursor,
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    queue,
    style::{self, Color::Rgb, Colors, Print, SetColors},
    terminal,
};
use serde::{Deserialize, Serialize};
use std::{
    cmp::min,
    collections::HashMap,
    env, fs,
    io::{self, Write},
    iter,
    process::exit,
};
use unicode_width::UnicodeWidthChar;

mod view;
use view::{Page, Toc, View};

mod epub;

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
    colors: Colors,
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
            chapter: 0,
            line: 0,
            mark: HashMap::new(),
            links: epub.links,
            colors: args.colors,
            cols,
            rows: rows as usize,
            max_width: args.width,
            view: if args.toc { &Toc } else { &Page },
            cursor: 0,
            dir: Direction::Next,
            meta,
            query: String::new(),
        };

        bk.jump_byte(args.chapter, args.byte);
        bk.mark('\'');

        bk
    }
    fn run(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        queue!(
            stdout,
            terminal::EnterAlternateScreen,
            cursor::Hide,
            EnableMouseCapture,
        )?;
        terminal::enable_raw_mode()?;

        let mut render = |bk: &Bk| {
            queue!(
                stdout,
                Print(style::Attribute::Reset),
                SetColors(bk.colors),
                terminal::Clear(terminal::ClearType::All),
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
            match event::read()? {
                Event::Key(e) => self.view.on_key(self, e.code),
                Event::Mouse(e) => {
                    // XXX idk seems lame
                    if e.kind == event::MouseEventKind::Moved {
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
    fn jump(&mut self, (c, l): (usize, usize)) {
        self.mark('\'');
        self.chapter = c;
        self.line = l;
    }
    fn jump_byte(&mut self, c: usize, byte: usize) {
        self.chapter = c;
        self.line = match self.chapters[c]
            .lines
            .binary_search_by_key(&byte, |&(a, _)| a)
        {
            Ok(n) => n,
            Err(n) => n - 1,
        }
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
    fn search(&mut self, args: SearchArgs) -> bool {
        let (start, end) = self.chapters[self.chapter].lines[self.line];
        match args.dir {
            Direction::Next => {
                let byte = if args.skip { end } else { start };
                let head = (self.chapter, byte);
                let tail = (self.chapter + 1..self.chapters.len() - 1).map(|n| (n, 0));
                for (c, byte) in iter::once(head).chain(tail) {
                    if let Some(index) = self.chapters[c].text[byte..].find(&self.query) {
                        self.jump_byte(c, index + byte);
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
                        self.jump_byte(c, index);
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

    /// background color (eg 282a36)
    #[argh(option)]
    bg: Option<String>,

    /// foreground color (eg f8f8f2)
    #[argh(option)]
    fg: Option<String>,

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
    colors: Colors,
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

fn init() -> Result<State, Box<dyn std::error::Error>> {
    let save_path = if cfg!(windows) {
        format!("{}\\bk", env::var("APPDATA")?)
    } else {
        format!("{}/.local/share/bk", env::var("HOME")?)
    };
    // XXX will silently create a new default save if ron errors but path arg works.
    // revisit if/when stabilizing. ez file format upgrades
    let save: io::Result<Save> = fs::read_to_string(&save_path).and_then(|s| {
        ron::from_str(&s)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid save file"))
    });
    let args: Args = argh::from_env();

    let path = match args.path {
        Some(p) => Some(fs::canonicalize(p)?.to_str().unwrap().to_string()),
        None => None,
    };

    let (path, save, chapter, byte) = match (save, path) {
        (Err(e), None) => return Err(Box::new(e)),
        (Err(_), Some(p)) => (p, Save::default(), 0, 0),
        (Ok(s), None) => {
            let &(chapter, byte) = s.files.get(&s.last).unwrap();
            (s.last.clone(), s, chapter, byte)
        }
        (Ok(s), Some(p)) => {
            if s.files.contains_key(&p) {
                let &(chapter, byte) = s.files.get(&p).unwrap();
                (p, s, chapter, byte)
            } else {
                (p, s, 0, 0)
            }
        }
    };

    // XXX oh god what
    let fg = args
        .fg
        .map(|s| Rgb {
            r: u8::from_str_radix(&s[0..2], 16).unwrap(),
            g: u8::from_str_radix(&s[2..4], 16).unwrap(),
            b: u8::from_str_radix(&s[4..6], 16).unwrap(),
        })
        .unwrap_or(style::Color::Reset);
    let bg = args
        .bg
        .map(|s| Rgb {
            r: u8::from_str_radix(&s[0..2], 16).unwrap(),
            g: u8::from_str_radix(&s[2..4], 16).unwrap(),
            b: u8::from_str_radix(&s[4..6], 16).unwrap(),
        })
        .unwrap_or(style::Color::Reset);

    Ok(State {
        path,
        save,
        save_path,
        meta: args.meta,
        bk: Props {
            colors: Colors::new(fg, bg),
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

    let byte = bk.chapters[bk.chapter].lines[bk.line].0;
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
