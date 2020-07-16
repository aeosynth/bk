use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode},
    queue,
    style::{Attribute, Print},
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

mod epub;
use epub::Chapter;

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

struct SearchArgs {
    dir: Direction,
    skip: bool,
}

#[derive(Clone)]
enum Direction {
    Next,
    Prev,
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
            KeyCode::Char('?') => bk.start_search(Direction::Prev),
            KeyCode::Char('/') => bk.start_search(Direction::Next),
            KeyCode::Char('N') => {
                bk.search(SearchArgs {
                    dir: Direction::Prev,
                    skip: true,
                });
            }
            KeyCode::Char('n') => {
                bk.search(SearchArgs {
                    dir: Direction::Next,
                    skip: true,
                });
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
        let line_end = min(bk.line + bk.rows, c.lines.len());

        let attrs = {
            let text_start = c.lines[bk.line].0;
            let text_end = c.lines[line_end - 1].1;

            let qlen = bk.query.len();
            let mut search = Vec::new();
            if qlen > 0 {
                for (pos, _) in c.text[text_start..text_end].match_indices(&bk.query) {
                    search.push((text_start + pos, Attribute::Reverse));
                    search.push((text_start + pos + qlen, Attribute::NoReverse));
                }
            }
            let mut search_iter = search.into_iter().peekable();

            let mut merged = Vec::new();
            let attr_start = match c
                .attrs
                .binary_search_by_key(&text_start, |&(pos, _, _)| pos)
            {
                Ok(n) => n,
                Err(n) => n - 1,
            };
            let mut attrs_iter = c.attrs[attr_start..].iter();
            let (_, _, attr) = attrs_iter.next().unwrap();
            if attr.has(Attribute::Bold) {
                merged.push((text_start, Attribute::Bold));
            }
            if attr.has(Attribute::Italic) {
                merged.push((text_start, Attribute::Italic));
            }
            if attr.has(Attribute::Underlined) {
                merged.push((text_start, Attribute::Underlined));
            }
            let mut attrs_iter = attrs_iter
                .map(|&(pos, a, _)| (pos, a))
                .take_while(|(pos, _)| pos <= &text_end)
                .peekable();

            // use itertools?
            loop {
                match (search_iter.peek(), attrs_iter.peek()) {
                    (None, None) => break,
                    (Some(_), None) => {
                        merged.extend(search_iter);
                        break;
                    }
                    (None, Some(_)) => {
                        merged.extend(attrs_iter);
                        break;
                    }
                    (Some(&s), Some(&a)) => {
                        if s.0 < a.0 {
                            merged.push(s);
                            search_iter.next();
                        } else {
                            merged.push(a);
                            attrs_iter.next();
                        }
                    }
                }
            }

            merged
        };

        let mut buf = Vec::new();
        let mut iter = attrs.into_iter().peekable();
        for &(mut start, end) in &c.lines[bk.line..line_end] {
            let mut s = String::new();
            while let Some(&(pos, attr)) = iter.peek() {
                if pos > end {
                    break;
                }
                s.push_str(&c.text[start..pos]);
                s.push_str(&attr.to_string());
                start = pos;
                iter.next();
            }
            s.push_str(&c.text[start..end]);
            buf.push(s);
        }
        buf
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
                bk.search(SearchArgs {
                    dir: bk.dir.clone(),
                    skip: false,
                });
            }
            KeyCode::Char(c) => {
                bk.query.push(c);
                let args = SearchArgs {
                    dir: bk.dir.clone(),
                    skip: false,
                };
                if !bk.search(args) {
                    bk.jump_reset();
                }
            }
            _ => (),
        }
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        let mut buf = Page::render(&Page, bk);
        if buf.len() == bk.rows {
            buf.pop();
        } else {
            for _ in buf.len()..bk.rows - 1 {
                buf.push(String::new());
            }
        }
        let prefix = match bk.dir {
            Direction::Next => '/',
            Direction::Prev => '?',
        };
        buf.push(format!("{}{}", prefix, bk.query));
        buf
    }
}

struct Bk<'a> {
    chapters: Vec<epub::Chapter>,
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
    meta: Vec<String>,
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

        let line = match chapters[args.chapter]
            .lines
            .binary_search_by_key(&args.byte, |&(a, _)| a)
        {
            Ok(n) => n,
            Err(n) => n - 1,
        };

        let mut mark = HashMap::new();
        let view: &dyn View = if args.toc {
            // need an initial mark to reset to
            mark.insert('\'', (args.chapter, line));
            &Nav
        } else {
            &Page
        };

        Bk {
            chapters,
            chapter: args.chapter,
            line,
            mark,
            cols,
            rows: rows as usize,
            max_width: args.width,
            view: Some(view),
            dir: Direction::Next,
            meta,
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

            queue!(
                stdout,
                terminal::Clear(terminal::ClearType::All),
                Print(Attribute::Reset)
            )?;
            for (i, line) in view.render(self).iter().enumerate() {
                queue!(stdout, cursor::MoveTo(pad, i as u16), Print(line))?;
            }
            stdout.flush().unwrap();

            match event::read()? {
                Event::Key(e) => view.run(self, e.code),
                Event::Resize(cols, rows) => {
                    self.rows = rows as usize;
                    if cols != self.cols {
                        self.cols = cols;
                        let width = min(cols, self.max_width) as usize;
                        for c in &mut self.chapters {
                            c.lines = wrap(&c.text, width);
                        }
                    }
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
    fn search(&mut self, args: SearchArgs) -> bool {
        let get_line = |lines: &Vec<(usize, usize)>, byte: usize| -> usize {
            match lines.binary_search_by_key(&byte, |&(a, _)| a) {
                Ok(n) => n,
                Err(n) => n - 1,
            }
        };
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
        .map_err(|e| anyhow::Error::new(e))
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
