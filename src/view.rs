use crossterm::{
    event::{
        KeyCode::{self, *},
        MouseEvent, MouseEventKind,
    },
    style::Attribute,
};
use std::{
    cmp::{min, Ordering},
    iter,
};
use unicode_width::UnicodeWidthChar;

use crate::{Bk, Direction, SearchArgs};

pub trait View {
    fn render(&self, bk: &Bk) -> Vec<String>;
    fn on_key(&self, bk: &mut Bk, kc: KeyCode);
    fn on_mouse(&self, _: &mut Bk, _: MouseEvent) {}
    fn on_resize(&self, _: &mut Bk) {}
}

// TODO render something useful?
struct Mark;
impl View for Mark {
    fn on_key(&self, bk: &mut Bk, kc: KeyCode) {
        if let Char(c) = kc {
            bk.mark(c)
        }
        bk.view = &Page
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        Page::render(&Page, bk)
    }
}

struct Jump;
impl View for Jump {
    fn on_key(&self, bk: &mut Bk, kc: KeyCode) {
        if let Char(c) = kc {
            if let Some(&pos) = bk.mark.get(&c) {
                bk.jump(pos);
            }
        }
        bk.view = &Page;
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        Page::render(&Page, bk)
    }
}

struct Metadata;
impl View for Metadata {
    fn on_key(&self, bk: &mut Bk, _: KeyCode) {
        bk.view = &Page;
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        let lines: Vec<usize> = bk.chapters.iter().map(|c| c.lines.len()).collect();
        let current = lines[..bk.chapter].iter().sum::<usize>() + bk.line;
        let total = lines.iter().sum::<usize>();
        let progress = current as f32 / total as f32 * 100.0;

        let pages = (lines[bk.chapter] as f32 / bk.rows as f32).ceil() as usize;
        // if the last line is visible we're on the last page. first page is the short one
        let page = pages - (lines[bk.chapter] - 1 - bk.line) / bk.rows;

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
    fn on_key(&self, bk: &mut Bk, _: KeyCode) {
        bk.view = &Page;
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

pub struct Toc;
impl Toc {
    fn prev(&self, bk: &mut Bk, n: usize) {
        bk.chapter = bk.chapter.saturating_sub(n);
        self.cursor(bk);
    }
    fn next(&self, bk: &mut Bk, n: usize) {
        bk.chapter = min(bk.chapters.len() - 1, bk.chapter + n);
        self.cursor(bk);
    }
    fn cursor(&self, bk: &mut Bk) {
        bk.cursor = min(bk.rows / 2, bk.chapter);
    }
    fn click(&self, bk: &mut Bk, row: usize) {
        let start = bk.chapter - bk.cursor;
        if start + row < bk.chapters.len() {
            bk.chapter = start + row;
            bk.line = 0;
            bk.view = &Page;
        }
    }
}
impl View for Toc {
    fn on_resize(&self, bk: &mut Bk) {
        self.cursor(bk);
    }
    fn on_mouse(&self, bk: &mut Bk, e: MouseEvent) {
        match e.kind {
            MouseEventKind::Down(_) => self.click(bk, e.row as usize),
            MouseEventKind::ScrollDown => self.next(bk, 3),
            MouseEventKind::ScrollUp => self.prev(bk, 3),
            _ => (),
        }
    }
    fn on_key(&self, bk: &mut Bk, kc: KeyCode) {
        match kc {
            Esc | Tab | Left | Char('h') | Char('q') => {
                bk.jump_reset();
                bk.cursor = 0;
                bk.view = &Page;
            }
            Enter | Right | Char('l') => {
                bk.line = 0;
                bk.cursor = 0;
                bk.view = &Page;
            }
            Down | Char('j') => self.next(bk, 1),
            Up | Char('k') => self.prev(bk, 1),
            Home | Char('g') => self.prev(bk, bk.chapters.len()),
            End | Char('G') => self.next(bk, bk.chapters.len()),
            PageDown | Char('f') => self.next(bk, bk.rows),
            PageUp | Char('b') => self.prev(bk, bk.rows),
            Char('d') => self.next(bk, bk.rows / 2),
            Char('u') => self.prev(bk, bk.rows / 2),
            _ => (),
        }
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        let start = bk.chapter - bk.cursor;
        let end = min(bk.chapters.len(), start + bk.rows);

        let mut arr = bk.chapters[start..end]
            .iter()
            .map(|c| c.title.clone())
            .collect::<Vec<String>>();
        arr[bk.cursor] = format!(
            "{}{}{}",
            Attribute::Reverse,
            arr[bk.cursor],
            Attribute::Reset
        );
        arr
    }
}

pub struct Page;
impl Page {
    fn next_chapter(&self, bk: &mut Bk) {
        if bk.chapter < bk.chapters.len() - 1 {
            bk.chapter += 1;
            bk.line = 0;
        }
    }
    fn prev_chapter(&self, bk: &mut Bk) {
        if bk.chapter > 0 {
            bk.chapter -= 1;
            bk.line = 0;
        }
    }
    fn scroll_down(&self, bk: &mut Bk, n: usize) {
        if bk.line + bk.rows < bk.chap().lines.len() {
            bk.line += n;
        } else {
            self.next_chapter(bk);
        }
    }
    fn scroll_up(&self, bk: &mut Bk, n: usize) {
        if bk.line > 0 {
            bk.line = bk.line.saturating_sub(n);
        } else if bk.chapter > 0 {
            bk.chapter -= 1;
            bk.line = bk.chap().lines.len().saturating_sub(bk.rows);
        }
    }
    fn click(&self, bk: &mut Bk, e: MouseEvent) {
        let c = bk.chap();
        let line = bk.line + e.row as usize;

        if e.column < bk.pad() || line >= c.lines.len() {
            return;
        }
        let (start, end) = c.lines[line];
        let line_col = (e.column - bk.pad()) as usize;

        let mut cols = 0;
        let mut found = false;
        let mut byte = start;
        for (i, c) in c.text[start..end].char_indices() {
            cols += c.width().unwrap();
            if cols > line_col {
                byte += i;
                found = true;
                break;
            }
        }

        if !found {
            return;
        }

        let r = c.links.binary_search_by(|&(start, end, _)| {
            if start > byte {
                Ordering::Greater
            } else if end <= byte {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        });

        if let Ok(i) = r {
            let url = &c.links[i].2;
            let &(chapter, byte) = bk.links.get(url).unwrap();
            let line = super::get_line(&bk.chapters[chapter].lines, byte);
            bk.jump((chapter, line));
        }
    }
    fn start_search(&self, bk: &mut Bk, dir: Direction) {
        bk.mark('\'');
        bk.query.clear();
        bk.dir = dir;
        bk.view = &Search;
    }
}
impl View for Page {
    fn on_mouse(&self, bk: &mut Bk, e: MouseEvent) {
        match e.kind {
            MouseEventKind::Down(_) => self.click(bk, e),
            MouseEventKind::ScrollDown => self.scroll_down(bk, 3),
            MouseEventKind::ScrollUp => self.scroll_up(bk, 3),
            _ => (),
        }
    }
    fn on_key(&self, bk: &mut Bk, kc: KeyCode) {
        match kc {
            Esc | Char('q') => bk.quit = true,
            Tab => {
                bk.mark('\'');
                Toc.cursor(bk);
                bk.view = &Toc;
            }
            F(_) => bk.view = &Help,
            Char('m') => bk.view = &Mark,
            Char('\'') => bk.view = &Jump,
            Char('i') => bk.view = &Metadata,
            Char('?') => self.start_search(bk, Direction::Prev),
            Char('/') => self.start_search(bk, Direction::Next),
            Char('N') => {
                Search::search(
                    &Search,
                    bk,
                    SearchArgs {
                        dir: Direction::Prev,
                        skip: true,
                    },
                );
            }
            Char('n') => {
                Search::search(
                    &Search,
                    bk,
                    SearchArgs {
                        dir: Direction::Next,
                        skip: true,
                    },
                );
            }
            End | Char('G') => {
                bk.mark('\'');
                bk.line = bk.chap().lines.len().saturating_sub(bk.rows);
            }
            Home | Char('g') => {
                bk.mark('\'');
                bk.line = 0;
            }
            Char('d') => self.scroll_down(bk, bk.rows / 2),
            Char('u') => self.scroll_up(bk, bk.rows / 2),
            Up | Char('k') => self.scroll_up(bk, 3),
            Left | PageUp | Char('b') | Char('h') => {
                self.scroll_up(bk, bk.rows);
            }
            Down | Char('j') => self.scroll_down(bk, 3),
            Right | PageDown | Char('f') | Char('l') | Char(' ') => self.scroll_down(bk, bk.rows),
            Char('[') => self.prev_chapter(bk),
            Char(']') => self.next_chapter(bk),
            _ => (),
        }
    }
    fn on_resize(&self, bk: &mut Bk) {
        // lazy
        bk.line = min(bk.line, bk.chap().lines.len() - 1);
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        let c = bk.chap();
        let last_line = min(bk.line + bk.rows, c.lines.len());
        let text_start = c.lines[bk.line].0;
        let text_end = c.lines[last_line - 1].1;

        let mut base = {
            let start = match c
                .attrs
                .binary_search_by_key(&text_start, |&x| x.0)
            {
                Ok(n) => n,
                Err(n) => n - 1,
            };

            let attr = c.attrs[start].2;
            let mut head = Vec::new();
            if attr.has(Attribute::Bold) {
                head.push((text_start, Attribute::Bold));
            }
            if attr.has(Attribute::Italic) {
                head.push((text_start, Attribute::Italic));
            }
            if attr.has(Attribute::Underlined) {
                head.push((text_start, Attribute::Underlined));
            }
            let tail = c.attrs[start + 1..]
                        .iter()
                        .take_while(|x| x.0 < text_end)
                        .map(|x| (x.0, x.1));
            head.into_iter().chain(tail).peekable()
        };

        let mut search = Vec::new();
        if !bk.query.is_empty() {
            let len = bk.query.len();
            for (pos, _) in c.text[text_start..text_end].match_indices(&bk.query) {
                search.push((text_start + pos, Attribute::Reverse));
                search.push((text_start + pos + len, Attribute::NoReverse));
            }
        }
        let mut search = search.into_iter().peekable();

        let mut attrs = Vec::new();
        loop {
            match (search.peek(), base.peek()) {
                (None, None) => break,
                (Some(_), None) => {
                    attrs.extend(search);
                    break;
                }
                (None, Some(_)) => {
                    attrs.extend(base);
                    break;
                }
                (Some(&s), Some(&b)) => {
                    if s.0 < b.0 {
                        attrs.push(s);
                        search.next();
                    } else {
                        attrs.push(b);
                        base.next();
                    }
                }
            }
        }
        let mut attrs = attrs.into_iter().peekable();

        // itertools: peeking take while
        let mut buf = Vec::with_capacity(last_line - bk.line);
        for &(mut pos, line_end) in &c.lines[bk.line..last_line] {
            let mut s = String::new();
            while let Some(&(attr_pos, attr)) = attrs.peek() {
                if attr_pos > line_end {
                    break;
                }
                s.push_str(&c.text[pos..attr_pos]);
                s.push_str(&attr.to_string());
                pos = attr_pos;
                attrs.next();
            }
            s.push_str(&c.text[pos..line_end]);
            buf.push(s);
        }

        buf
    }
}

pub struct Search;
impl Search {
    fn search(&self, bk: &mut Bk, args: SearchArgs) -> bool {
        let (start, end) = bk.chap().lines[bk.line];
        match args.dir {
            Direction::Next => {
                let byte = if args.skip { end } else { start };
                let head = (bk.chapter, byte);
                let tail = (bk.chapter + 1..bk.chapters.len() - 1).map(|n| (n, 0));
                for (c, byte) in iter::once(head).chain(tail) {
                    if let Some(index) = bk.chapters[c].text[byte..].find(&bk.query) {
                        bk.line = super::get_line(&bk.chapters[c].lines, index + byte);
                        bk.chapter = c;
                        return true;
                    }
                }
                false
            }
            Direction::Prev => {
                let byte = if args.skip { start } else { end };
                let head = (bk.chapter, byte);
                let tail = (0..bk.chapter)
                    .rev()
                    .map(|c| (c, bk.chapters[c].text.len()));
                for (c, byte) in iter::once(head).chain(tail) {
                    if let Some(index) = bk.chapters[c].text[..byte].rfind(&bk.query) {
                        bk.line = super::get_line(&bk.chapters[c].lines, index);
                        bk.chapter = c;
                        return true;
                    }
                }
                false
            }
        }
    }
}
impl View for Search {
    fn on_key(&self, bk: &mut Bk, kc: KeyCode) {
        match kc {
            Esc => {
                bk.jump_reset();
                bk.query.clear();
                bk.view = &Page;
            }
            Enter => {
                bk.view = &Page;
            }
            Backspace => {
                bk.query.pop();
                bk.jump_reset();
                self.search(
                    bk,
                    SearchArgs {
                        dir: bk.dir.clone(),
                        skip: false,
                    },
                );
            }
            Char(c) => {
                bk.query.push(c);
                let args = SearchArgs {
                    dir: bk.dir.clone(),
                    skip: false,
                };
                if !self.search(bk, args) {
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
