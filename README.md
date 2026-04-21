# txpaint

txpaint is an application for editing "ASCII" art. It supports using CodePage437 fonts and works with .xp(rexpaint) files. It is created with Rust, using [egui](https://github.com/emilk/egui) and [wgpu](https://github.com/gfx-rs/wgpu). I made this because Rexpaint is only for windows and it was annoying to use with wine, and I am allergic to windows.

It is also essentially 100% vibecoded with Claude, if you have questions about the code don't ask me because I don't know the answer. I made this because I needed a cross-platform open source solution for this tool that does what I needed. Feel free to steal it.

I'll accept PRs but I don't intend to publish builds of this anywhere at the moment, you can run it by cloning the repo and using `cargo run` or have cargo install it.

## Features

### Tools

Pencil Tool - "paint" tiles by clicking

Selection Tools

There is a selection system, which you can fill/erase the selection.

Rectangle Select - Select a rectangular area, hold shift to add to current selection, control to subtract from current selection. Not holding a modifier will replace selection.

### Glyph Palettes

There is a standard glyph palette arranged in a typical CodePage437 font layout, however you can also create custom glyph palette, which align the glyphs on a grid however you want. This can make it easier to pull out certain glyphs for different types of more specific work. The glyph palette can also be navigated with WASD or arrow keys to change the active glyph.

Custom glyph palettes can be saved and loaded to a custom `.gpal` format.