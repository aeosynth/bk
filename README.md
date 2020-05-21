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

    bk path/to/epub

Type any function key (eg <kbd>F1</kbd>) to see the commands.

Running `bk` without an argument will load the most recent Epub.

# TODO
- configuration
- better html support
- better unicode support
- mobi?
- images?
- css?
- gui?

# Inspiration
<https://github.com/wustho/epr>
