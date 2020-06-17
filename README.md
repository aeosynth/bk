# bk
bk is a WIP terminal Epub reader, written in Rust.

# Features
- Cross platform - Linux, macOS and Windows support
- Single binary, instant startup
- Epub 2/3 support
- Vim bindings
- Incremental search
- Bookmarks

# Install
Install from crates.io:

    cargo install bk

or from github:

    git clone https://github.com/aeosynth/bk
    cargo install --path bk

# Usage

    Usage: bk [<path>] [-w <width>]

    read a book

    Options:
      -w, --width       characters per line
      --help            display usage information

Running `bk` without a path will load the most recent Epub.

Type any function key (eg <kbd>F1</kbd>) to see the keybinds.

# Configuration alternatives

- Theming: theme your terminal
- Config file: create an alias with cli options

# TODO
- more configuration
- better html support
- test unicode
- github actions / ci
- css?
- mobi?

# Inspiration
<https://github.com/wustho/epr>
