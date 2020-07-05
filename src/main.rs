use std::io::{stdout, Write};
use std::{cmp::min, collections::HashMap, env, fs, iter, process::exit};

use argh::FromArgs;

use crossterm::{
    cursor,
    event::{self, Event, KeyCode},
    queue,
    style::{Attribute, Print},
    terminal,
};

mod epub;

// XXX assumes a char is i unit wide
fn wrap(text: &str, width: usize) -> Vec<(usize, usize)> {
    let mut lines = Vec::new();
    // bytes
    let mut start = 0;
    let mut end = 0;
    // chars after the break
    let mut after = 0;
    // chars in unbroken line
    let mut len = 0;
    // are we breaking on whitespace?
    let mut skip = false;

    for (i, c) in text.char_indices() {
        len += 1;
        match c {
            '\n' => {
                after = 0;
                end = i;
                skip = true;
                len = width + 1;
            }
            ' ' => {
                after = 0;
                end = i;
                skip = true;
            }
            '-' | '—' if len <= width => {
                after = 0;
                end = i + c.len_utf8();
                skip = false;
            }
            _ => after += 1,
        }
        if len > width {
            if len == after {
                after = 1;
                end = i;
                skip = false;
            }
            lines.push((start, end));
            start = end;
            if skip {
                start += 1;
            }
            len = after;
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
        if let KeyCode::Char(c) = kc {
            bk.mark(c)
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
        if let KeyCode::Char(c) = kc {
            bk.jump(c)
        }
        bk.view = Some(&Page);
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        Page::render(&Page, bk)
    }
}

struct Metadata;
impl View for Metadata {
    fn run(&self, bk: &mut Bk, _: KeyCode) {
        bk.view = Some(&Page);
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        let lines: Vec<usize> = bk.chapters.iter().map(|c| c.lines.len()).collect();
        let current = lines[..bk.chapter].iter().sum::<usize>() + bk.line;
        let total = lines.iter().sum::<usize>();
        let progress = current as f32 / total as f32 * 100.0;

        let pages = lines[bk.chapter] / bk.rows;
        let page = bk.line / bk.rows;

        let mut vec = vec![
            format!("chapter: {}/{}", page, pages),
            format!("total: {:.0}%", progress),
            String::new(),
        ];
        vec.extend_from_slice(&bk.meta);
        vec
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
                     Tab  Table of Contents
                       i  Progress and Metadata

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

                       /  Search Forward
                       ?  Search Backward
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
            KeyCode::Char('m') => bk.view = Some(&Mark),
            KeyCode::Char('\'') => bk.view = Some(&Jump),
            KeyCode::Char('i') => bk.view = Some(&Metadata),
            KeyCode::Char('?') => bk.start_search(Direction::Backward),
            KeyCode::Char('/') => bk.start_search(Direction::Forward),
            // XXX temporarily broken (well needing to manually advance before searching)
            KeyCode::Char('N') => {
                bk.search(Direction::Backward);
            }
            KeyCode::Char('n') => {
                bk.search(Direction::Forward);
            }
            KeyCode::End | KeyCode::Char('G') => {
                bk.mark('\'');
                bk.line = bk.chap().lines.len().saturating_sub(bk.rows);
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
        let c = bk.chap();
        let end = min(bk.line + bk.rows, c.lines.len());
        c.lines[bk.line..end]
            .iter()
            .map(|&(a, b)| String::from(&c.text[a..b]))
            .collect()
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
        let c = bk.chap();
        let end = min(bk.line + bk.rows - 1, c.lines.len());
        let mut buf = Vec::with_capacity(bk.rows);

        for &(a, b) in c.lines[bk.line..end].iter() {
            let line = String::from(&c.text[a..b]);
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
                buf.push(line);
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

struct Chapter {
    title: String,
    // a single string for searching
    text: String,
    // byte indexes
    lines: Vec<(usize, usize)>,
}

struct Bk<'a> {
    meta: Vec<String>,
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
    fn new(epub: epub::Epub, args: Props) -> Self {
        let (cols, rows) = terminal::size().unwrap();
        let width = min(cols, args.width) as usize;
        let meta = wrap(&epub.meta, width)
            .into_iter()
            .map(|(a, b)| String::from(&epub.meta[a..b]))
            .collect();

        let mut chapters = Vec::with_capacity(epub.chapters.len());
        for (text, title) in epub.chapters {
            let title = if title.chars().count() > width {
                title
                    .chars()
                    .take(width - 1)
                    .chain(iter::once('…'))
                    .collect()
            } else {
                title
            };
            let lines = wrap(&text, width);
            chapters.push(Chapter { title, text, lines });
        }

        let mut mark = HashMap::new();
        let view: &dyn View = if args.toc {
            // need an initial mark to reset to
            mark.insert('\'', (args.chapter, args.line));
            &Nav
        } else {
            &Page
        };

        Bk {
            meta,
            line: min(
                args.line,
                chapters[args.chapter]
                    .lines
                    .len()
                    .saturating_sub(rows as usize),
            ),
            chapters,
            chapter: args.chapter,
            mark,
            cols,
            rows: rows as usize,
            max_width: args.width,
            view: Some(view),
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
        } else {
            self.prev_chapter();
            self.line = self.chap().lines.len().saturating_sub(self.rows);
        }
    }
    fn start_search(&mut self, dir: Direction) {
        self.mark('\'');
        self.query.clear();
        self.dir = dir;
        self.view = Some(&Search);
    }
    fn search(&mut self, dir: Direction) -> bool {
        let get_line = |lines: &Vec<(usize, usize)>, byte: usize| -> usize {
            match lines.binary_search_by_key(&byte, |&(a, _)| a) {
                Ok(n) => n,
                Err(n) => n - 1,
            }
        };
        let (start, end) = self.chap().lines[self.line];
        match dir {
            Direction::Forward => {
                let head = (self.chapter, start);
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
            Direction::Backward => {
                let head = (self.chapter, end);
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

#[derive(FromArgs)]
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
    line: usize,
    width: u16,
    toc: bool,
}

fn init(save_path: &str) -> Option<(String, bool, Props)> {
    let args: Args = argh::from_env();
    // TODO nice error message instead of panic
    let path = args
        .path
        .map(|s| fs::canonicalize(s).unwrap().to_str().unwrap().to_string());
    let save = fs::read_to_string(save_path).and_then(|s| {
        let mut lines = s.lines();
        Ok((
            lines.next().unwrap().to_string(),
            lines.next().unwrap().parse::<usize>().unwrap(),
            lines.next().unwrap().parse::<usize>().unwrap(),
        ))
    });

    let (path, chapter, line) = match (save, path) {
        (Err(_), None) => return None,
        (Err(_), Some(path)) => (path, 0, 0),
        (Ok(save), None) => save,
        (Ok(save), Some(path)) => {
            if save.0 == path {
                save
            } else {
                (path, 0, 0)
            }
        }
    };

    Some((
        path,
        args.meta,
        Props {
            chapter,
            line,
            width: args.width,
            toc: args.toc,
        },
    ))
}

fn main() {
    let save_path = if cfg!(windows) {
        format!("{}\\bk", env::var("APPDATA").unwrap())
    } else {
        format!("{}/.local/share/bk", env::var("HOME").unwrap())
    };

    let (path, meta, args) = init(&save_path).unwrap_or_else(|| {
        println!("error: need a path");
        exit(1);
    });

    let mut epub = epub::Epub::new(&path).unwrap_or_else(|e| {
        println!("error reading epub: {}", e);
        exit(1);
    });

    if meta {
        println!("{}", epub.meta);
        exit(0);
    }

    epub.get_chapters();
    let mut bk = Bk::new(epub, args);
    // i have never seen crossterm error
    bk.run().unwrap();

    fs::write(save_path, format!("{}\n{}\n{}", path, bk.chapter, bk.line)).unwrap_or_else(|e| {
        println!("error saving state: {}", e);
        exit(1);
    });
}
