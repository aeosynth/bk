# bk
bk is a WIP terminal Epub reader, written in Rust.

# Features
- Cross platform - Linux, macOS and Windows support
- Single binary, instant startup
- Epub 2/3 support
- Vim bindings
- Incremental search
- Bookmarks

# Usage

Install from crates.io:

    cargo install bk

or from github:

    git clone https://github.com/aeosynth/bk
    cargo install --path bk

then run:

    bk [flags] [path]

Running `bk` without a path will load the most recent Epub.

The `-w` flag sets the line width.

Type any function key (eg <kbd>F1</kbd>) to see the commands.

# TODO
- configuration
- better html support
- better unicode support
- mobi?
- css?
- gui?

# Inspiration
<https://github.com/wustho/epr>
