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

    Usage: bk [<path>] [-m] [-t] [-w <width>]

    read a book

    Options:
      -m, --meta        print metadata and exit
      -t, --toc         start with table of contents open
      -w, --width       characters per line
      --help            display usage information

Running `bk` without a path will load the most recent Epub.

Type any function key (eg <kbd>F1</kbd>) to see the keybinds.

# Configuration alternatives

- Theming: theme your terminal
- Config file: create an alias with cli options

# Comparison
|   | bk | epr/epy |
| - | - | - |
| language | rust | python |
| runtime deps | none | python, curses |
| additional formats | none | FictionBook |
| incremental search | :heavy_check_mark: | :x: |
| multi line search | :heavy_check_mark: | :x: |
| regex search | :x: | :heavy_check_mark: |
| links | :x: | :x: |
| images | :x: | :heavy_check_mark: |
| themes | :x: | :heavy_check_mark: |
| choose file from history | :x: | :heavy_check_mark: |
| external integration | none | dictionary |

# Inspiration
<https://github.com/wustho/epr>
