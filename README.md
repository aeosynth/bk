# bk
bk is a WIP terminal Epub reader, written in Rust.

bk supports Linux and macOS. Windows runs but doesn't save position on exit.

# Usage

    cargo install --path .
    bk path/to/epub

Type <kbd>F1</kbd> or <kbd>?</kbd> to see the commands.

Running `bk` without an argument will load the most recent Epub.

# Features
- Single binary, instant startup
- Epub 2/3 support
- Incremental search
- Vim bindings

# TODO
- fix Windows
- configuration
- links
- better unicode support
- better html rendering
- mobi?
- images?
- css?
- gui?

# Inspiration
<https://github.com/wustho/epr>
