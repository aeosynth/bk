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
    bk [<path>] [-w <width>]

Running `bk` without a path will load the most recent Epub.

Type any function key (eg <kbd>F1</kbd>) to see the keybinds.

# TODO
- more configuration
- better html support
- better unicode support
- css?
- mobi?

# Inspiration
<https://github.com/wustho/epr>
