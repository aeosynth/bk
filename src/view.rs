use crossterm::{
    event::{
        KeyCode::{self, *},
        MouseEvent,
    },
    style::Attribute,
};
use std::cmp::{max, min, Ordering};
use unicode_width::UnicodeWidthChar;

use crate::{get_line, Bk, Direction, SearchArgs};

pub trait View {
    fn render(&self, bk: &Bk) -> Vec<String>;
    fn on_key(&self, bk: &mut Bk, kc: KeyCode);
    fn on_mouse(&self, _: &mut Bk, _: MouseEvent) {}
}

// TODO render something useful?
struct Mark;
impl View for Mark {
    fn on_key(&self, bk: &mut Bk, kc: KeyCode) {
        if let Char(c) = kc {
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
    fn on_key(&self, bk: &mut Bk, kc: KeyCode) {
        if let Char(c) = kc {
            if let Some(&pos) = bk.mark.get(&c) {
                bk.jump(pos);
            }
        }
        bk.view = Some(&Page);
    }
    fn render(&self, bk: &Bk) -> Vec<String> {
        Page::render(&Page, bk)
    }
}

struct Metadata;
impl View for Metadata {
    fn on_key(&self, bk: &mut Bk, _: KeyCode) {
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
    fn on_key(&self, bk: &mut Bk, _: KeyCode) {
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

pub struct Nav;
impl Nav {
    fn start(&self, bk: &mut Bk) {
        bk.nav_top = bk.chapter.saturating_sub(bk.rows / 2);
        bk.mark('\'');
        bk.view = Some(&Self);
    }
    fn prev_chapter(&self, bk: &mut Bk) {
        if bk.chapter > 0 {
            if bk.chapter == bk.nav_top {
                bk.nav_top -= 1;
            }
            bk.chapter -= 1;
        }
    }
    fn next_chapter(&self, bk: &mut Bk) {
        if bk.chapter < bk.chapters.len() - 1 {
            bk.chapter += 1;
            if bk.chapter == bk.nav_top + bk.rows {
                bk.nav_top += 1;
            }
        }
    }
    fn scroll_down(&self, bk: &mut Bk, n: usize) {
        bk.nav_top = min(bk.nav_top + n, bk.chapters.len() - 1);
        bk.chapter = max(bk.chapter, bk.nav_top);
    }
    fn scroll_up(&self, bk: &mut Bk, n: usize) {
        bk.nav_top = bk.nav_top.saturating_sub(n);
        bk.chapter = min(bk.chapter, bk.nav_top + bk.rows - 1);
    }
    fn click(&self, bk: &mut Bk, row: usize) {
        if bk.nav_top + row < bk.chapters.len() {
            bk.chapter = bk.nav_top + row;
            bk.line = 0;
            bk.view = Some(&Page);
        }
    }
}

impl View for Nav {
    fn on_mouse(&self, bk: &mut Bk, e: MouseEvent) {
        match e {
            MouseEvent::Down(_, _, row, _) => self.click(bk, row as usize),
            MouseEvent::ScrollDown(_, _, _) => self.scroll_down(bk, 3),
            MouseEvent::ScrollUp(_, _, _) => self.scroll_up(bk, 3),
            _ => (),
        }
    }
    fn on_key(&self, bk: &mut Bk, kc: KeyCode) {
        match kc {
            Esc | Tab | Left | Char('h') | Char('q') => {
                bk.jump_reset();
                bk.view = Some(&Page);
            }
            Enter | Right | Char('l') => {
                bk.line = 0;
                bk.view = Some(&Page);
            }
            Down | Char('j') => self.next_chapter(bk),
            Up | Char('k') => self.prev_chapter(bk),
            Home | Char('g') => {
                bk.chapter = 0;
                bk.nav_top = 0;
            }
            End | Char('G') => {
                bk.chapter = bk.chapters.len() - 1;
                bk.nav_top = bk.chapters.len().saturating_sub(bk.rows);
            }
            PageDown | Char('f') => self.scroll_down(bk, bk.rows),
            PageUp | Char('b') => self.scroll_up(bk, bk.rows),
            Char('d') => self.scroll_down(bk, bk.rows / 2),
            Char('u') => self.scroll_up(bk, bk.rows / 2),
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

pub struct Page;
impl View for Page {
    fn on_mouse(&self, bk: &mut Bk, e: MouseEvent) {
        match e {
            MouseEvent::Down(_, col, row, _) => {
                let c = bk.chap();
                let line = bk.line + row as usize;

                if col < bk.pad() || line >= c.lines.len() {
                    return;
                }
                let (start, end) = c.lines[line];
                let line_col = (col - bk.pad()) as usize;

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
                    let mut link = bk.links.get(url);
                    // FIXME hack
                    if link.is_none() {
                        let url = url.split('#').next().unwrap();
                        link = bk.links.get(url);
                        if link.is_none() {
                            return;
                        }
                    }
                    let &(chapter, byte) = link.unwrap();
                    let line = get_line(&bk.chapters[chapter].lines, byte);
                    bk.jump((chapter, line));
                }
            }
            MouseEvent::ScrollDown(_, _, _) => bk.scroll_down(3),
            MouseEvent::ScrollUp(_, _, _) => bk.scroll_up(3),
            _ => (),
        }
    }
    fn on_key(&self, bk: &mut Bk, kc: KeyCode) {
        match kc {
            Esc | Char('q') => bk.view = None,
            Tab => Nav.start(bk),
            F(_) => bk.view = Some(&Help),
            Char('m') => bk.view = Some(&Mark),
            Char('\'') => bk.view = Some(&Jump),
            Char('i') => bk.view = Some(&Metadata),
            Char('?') => bk.start_search(Direction::Prev),
            Char('/') => bk.start_search(Direction::Next),
            Char('N') => {
                bk.search(SearchArgs {
                    dir: Direction::Prev,
                    skip: true,
                });
            }
            Char('n') => {
                bk.search(SearchArgs {
                    dir: Direction::Next,
                    skip: true,
                });
            }
            End | Char('G') => {
                bk.mark('\'');
                bk.line = bk.chap().lines.len().saturating_sub(bk.rows);
            }
            Home | Char('g') => {
                bk.mark('\'');
                bk.line = 0;
            }
            Char('d') => bk.scroll_down(bk.rows / 2),
            Char('u') => bk.scroll_up(bk.rows / 2),
            Up | Char('k') => bk.scroll_up(3),
            Left | PageUp | Char('b') | Char('h') => {
                bk.scroll_up(bk.rows);
            }
            Down | Char('j') => bk.scroll_down(3),
            Right | PageDown | Char('f') | Char('l') | Char(' ') => bk.scroll_down(bk.rows),
            Char('[') => bk.prev_chapter(),
            Char(']') => bk.next_chapter(),
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

pub struct Search;
impl View for Search {
    fn on_key(&self, bk: &mut Bk, kc: KeyCode) {
        match kc {
            Esc => {
                bk.jump_reset();
                bk.view = Some(&Page);
            }
            Enter => {
                bk.view = Some(&Page);
            }
            Backspace => {
                bk.query.pop();
                bk.jump_reset();
                bk.search(SearchArgs {
                    dir: bk.dir.clone(),
                    skip: false,
                });
            }
            Char(c) => {
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
