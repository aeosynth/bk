# bk
bk is a WIP terminal Epub reader, written in Rust.

# Features
- Cross platform - Linux, macOS and Windows support
- Single binary, instant startup
- Epub 2/3 support
- Incremental search
- Vim bindings

# Usage

Install from crates.io:

    cargo install bk

or from github:

    git clone https://github.com/aeosynth/bk
    cargo install --path .

then run:

    bk path/to/epub

Type <kbd>F1</kbd> or <kbd>?</kbd> to see the commands.

Running `bk` without an argument will load the most recent Epub.

# TODO
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
