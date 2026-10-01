# The Great Wave

<img src="assets/icon/tgw.svg" width="88" height="88" align="right" alt="The Great Wave icon">

`tgw` draws timing diagrams. A `.tgw` file is a list of signals, one per line, and the program turns it into a compact SVG. The same binary can keep that SVG current while you edit, and can show the diagram in a window. The rendering library has no dependencies.

```text
@title Bus transfer
@tick 0

clk:         P.....|...
data [7:0]:  x.345x|=.x => head body tail "read data"
request:     0.1..0|1.0
---
acknowledge: 1.....|01.
```

![Bus transfer](examples/transfer.svg)

## Render

```sh
cargo build --release
./target/release/tgw examples/transfer.tgw -o transfer.svg
./target/release/tgw < examples/transfer.tgw > transfer.svg
```

`-i` selects the input. `-o` selects the output. Omit the input, or pass `-`, to read stdin. Omit `-o`, or pass `-`, to write stdout. `--indent 2` pretty-prints the SVG. `tgw --help` lists every flag.

A render refuses to replace the diagram with the SVG. The same path, a symlink, and a hard link are all the same file. On Windows, a different case of the name is the same file too.

Errors name the file, line, and column.

## Convert

WaveJSON and JSON5 still render. The syntax is detected automatically, or chosen with `--format tgw`, `json5`, or `auto`.

```sh
./target/release/tgw old-diagram.json5 --convert -o diagram.tgw
```

`--convert` rewrites a native diagram into aligned columns as well. Supported data is kept. A `#` comment stays with the construct it precedes, including an end-of-line note. JSON comments and unsupported JSON properties are left out.

## Watch and view

```sh
./target/release/tgw examples/transfer.tgw -o transfer.svg --view   # window and file
./target/release/tgw examples/transfer.tgw --view                   # window only
./target/release/tgw examples/transfer.tgw -o transfer.svg --watch  # file only
./target/release/tgw examples/asm.tgw --view                        # an ASM chart, same window
```

`--view` opens a window. `--watch` does not. Both keep running, and every save of the input refreshes the picture. With `-o`, the file is refreshed too.

Live mode needs a real input file. `--watch` also needs `-o`. `--convert` cannot be combined with either. A directory is rejected, and the output directory must already exist.

The output is the same bytes as a one-off render with the same `--indent` and `--format`. An unchanged save does not rewrite it. A new picture is published by renaming a temporary file into place. When a rename is refused, as when Windows is holding the file open, the picture is written in place. Editors that save by renaming a sibling are followed. The file is also polled five times a second, so a save is still seen on a network drive. On Windows the poll compares names without regard to case, and uses the file index so a replaced file is seen even when the size and timestamp do not change.

A syntax error, a missing file, or a write that fails leaves the last good picture in place. A diagram error is reported as `path:line:col: message`. A write error is the operating-system error. The window shows it in the status bar; `--watch` prints it on stderr. A failed write is tried again at the next poll. Ctrl-W or Ctrl-Q closes the window (Command on macOS). `--watch` stops on Ctrl-C.

In the window, signal names stay fixed. The title, tick numbers, and waveforms scroll together. An `@asm` chart is the same window with no name column: the whole picture scrolls, and both bars appear when it does not fit. Use the wheel or trackpad, and hold Shift to scroll sideways. Drag a scrollbar thumb, or click the track to move by most of the view. Bars appear only when the diagram overflows. A diagram that fits is enlarged until it fills the window, with the same small padding on every edge. A short diagram opens in a window snug to the drawing. A later resize is kept, and the extra room becomes padding. A waveform wider than the window scrolls horizontally. A taller diagram scrolls vertically. A tick label that would be cut in half is left out until it fits. `@bounds` still decides which cycles are drawn.

The window follows the system appearance. Default light is the palette written into the SVG. Default dark draws the same diagram in a dark palette. Ctrl-Shift-L (Command-Shift-L on macOS) switches between them and keeps the choice. The saved file stays on the light palette.

The saved SVG stays compact: a long clock is one pattern, and an unknown value is a small hatch pattern. The window paints only the visible slice. It repeats a clock or the grid as strokes. An unknown hatch is a set of continuous slashes, carried through a bus transition, so the marks meet from one side to the other. A bus fill overlaps the edge it shares with that transition, covering the page along the seam.

## Builds

| Command | What you get |
| --- | --- |
| `cargo build --release` | Renderer, `--watch`, and `--view` |
| `cargo build --release --no-default-features --features watch` | Renderer and `--watch`, no GUI libraries |
| `cargo build --release --no-default-features` | Renderer only, no dependencies |

`tgw --help` says when a flag is missing from the build. The window uses [GPUI](https://www.gpui.rs/). `rust-toolchain.toml` pins the compiler. A program that only renders can depend on the library with `default-features = false`.

## Platforms

- **macOS.** The Dock shows the tgw icon while a window is open.
- **Windows.** The icon is embedded in `tgw.exe` for Explorer, the taskbar, and the title bar. That needs the Visual Studio resource compiler, or `windres` for the GNU toolchain. Without one, the build warns and continues. CRLF sources render the same picture as LF. A sharing or lock violation while an editor is still saving is treated as transient, and the next poll reads the finished file.
- **Linux.** The window runs on Wayland or X11. With neither `WAYLAND_DISPLAY` nor `DISPLAY` set, `--view` exits and names `--watch` as the alternative. Building the window needs:

```sh
sudo apt install pkg-config libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  libxcb1-dev libx11-xcb-dev libfontconfig-dev libfreetype-dev
```

X11 takes the icon from the binary. Wayland looks up the desktop entry:

```sh
cargo install --path .
install -Dm644 assets/linux/tgw.desktop ~/.local/share/applications/tgw.desktop
install -Dm644 assets/icon/tgw.svg ~/.local/share/icons/hicolor/scalable/apps/tgw.svg
```

## Icon

`assets/icon/tgw.svg` is the master: a square pulse that becomes Hokusai's wave. The Dock PNG and the Windows ICO are generated from it.

```sh
cargo run --example icon
```

## Diagram language

One signal per line, `name: wave`. Names may contain spaces and ranges, as in `data [7:0]`. A blank line is ignored. `---` inserts an empty lane. `#` starts a comment outside quotes. Indentation does not matter.

Labels come after `=>`, separated by spaces. Quote a label that contains a space, `#`, `;`, a quote, or `=>`. Quotes accept `\n`, `\r`, `\t`, `\\`, escaped quotes, and `\uXXXX`. Unicode can also be written directly.

```text
@group Master
  clk:     p....... ; period=2
  address: x3.x4..x => 0x10 0x20 ; phase=0.25
  @group Control
    write: 01.0.... ; node=.a.b....
    read:  0...1..0
  @end
@end

@edge a<->b write pulse
```

A node is one letter in the mask: `.` skips a cycle, and `[setup]` names that one cycle with a word. An uppercase letter is drawn without a label. `@edge setup~>hold "tSU"` connects two names. A missing name or an unknown connector is an error, and the message lists the nodes or the connectors.

`@group Name` and `@end` nest, up to 64 deep. `@group` with no name draws an unlabeled bracket.

| Option | Effect |
| --- | --- |
| `period=2` | Make each symbol that many cycles long. Must be positive. |
| `phase=0.25` | Shift the signal earlier by a quarter cycle. Negative values start it later. |
| `node=.a.B.[setup].` | Name positions on the wave. One letter is one cycle. `[setup]` is one cycle with a word. `.` skips a cycle. An uppercase letter has no label. |
| `over=1..0` | A colored bar above the lane. `1`–`9` choose the color, `.` continues, `0` ends. |
| `under=2..0` | The same bar below the lane. |

The wave symbols are `p n P N h l H L 0 1 x d u z = 2-9`.

| Symbols | Meaning |
| --- | --- |
| `p` `n` | Clock, rising or falling. |
| `P` `N` | The same clocks, with an arrow on the edge. |
| `h` `l` `H` `L` `0` `1` | High and low levels. `H` and `L` carry an arrow. |
| `x` | Unknown. |
| `d` `u` | Weak low and weak high, drawn dashed. |
| `z` | High impedance, a line at mid level. |
| `=` `2`–`9` | A data bus. The digit selects the color. |
| `.` | Repeat the previous symbol. |
| `\|` | A gap in the trace. |
| `<...>` | Draw the enclosed symbols at half duration. |

Spaces inside a wave are ignored. A clock is at least two half-cycles. A level is at least one. A fractional period still lands on that half-cycle grid, and nodes, bars, and edges follow it.

| Directive | Example |
| --- | --- |
| Title and footer | `@title Bus transfer`, `@footer Figure 1` |
| Tick on each cycle | `@tick 0` |
| Tick between cycles | `@tock 0` |
| Start and step | `@tick 0 0.5` |
| Named ticks | `@tick "idle" "request" "reply"` |
| Footer ticks | `@foot-tick 0`, `@foot-tock 9` |
| Keep every Nth label | `@every 2`, `@foot-every 2` |
| Cycle width | `@scale 2`, an integer from 1 to 100 |
| Drawn range | `@bounds 2 6`, end exclusive |
| Grid | `@grid on`, `@grid off` |
| Edge label size | `@arc-font 12` |
| Gaps along the whole diagram | `@gaps . . 1 . \|` |
| An edge between nodes | `@edge a~>b propagation delay`, `@edge setup~>hold "tSU"` |
| A diagram with no lanes | `@empty` |

Any tick directive accepts `off`. For a numeric series, the first number is the start and the optional second is the step, which also sets the decimal places. A series with step 1 follows `@bounds`. Any other series keeps the start you wrote. Labels are thinned when they would collide, and a series stops at 10,000 labels.

An analog trace is an SVG path. X is in cycles. Y runs from 0 at the low rail to 1 at the high rail. Curves, arcs, relative commands, and exponents are accepted. `period`, `phase`, and `@scale` apply. The stroke stays one pixel wide after scaling.

```text
analog: path M0,0 L1,0 C1.5,0 1.5,1 2,1 H3 A0.5,0.5 0 0 1 4,1 L5,0
```

Register and assign diagrams are not supported. JSON5 input accepts the subset usual for these diagrams: bare or quoted keys, either quote style, comments, trailing commas, arrays, objects, finite numbers, booleans, and null.

A file whose first directive is `@asm` is an algorithmic state machine chart. Every other file stays a timing diagram. Indentation matters only after `@asm`. `#` comments, `@title`, and `@footer` work as they do above. `--convert` reprints the chart at a 2-space indent.

```text
@asm
@title Bus handshake

idle:
  req=0
  ack=0
  ? start
    0 idle
    1 (req=1) wait

wait:
  ack=1
  ? done
    0 wait
    1 idle
```

`name:` opens a state. Later lines that are not `?` or `>` are outputs, one line each. `? condition` is a decision with exits `0` and `1`, in either order. `1 (req=1) wait` draws a conditional-output box on that exit, then links to `wait`. `> next` is an unconditional exit. A state with no exit is terminal. An exit may be a nested `?` instead of a state name.

Inside a block, `0` continues down and `1` leaves to the right. Blocks stack in source order. A link into the block directly below is a straight arrow. Any other link runs in a side channel: forward on the right, back on the left. The text does not carry coordinates.

The window shows the chart as one picture, with no name column. When the chart fits, it is scaled by a whole number so each SVG pixel stays a whole device pixel. When it does not fit, the scale stays 1 and both scrollbars appear.

## Picture

Bus transitions cross at the same point, and a label is centered on the visible part of its value. A gap clears the trace underneath it. Each SVG has a title, a description, and paint ids that belong to that document only. There is no global stylesheet, so several diagrams can sit on one page.

A long clock is one shared pattern. A long hold is one stroke. `@bounds` drops geometry outside the window, including annotations, instead of emitting it and hiding it. `render_into` reuses the caller's buffer. Text is measured against a conservative sans-serif, so a label's exact width can differ slightly by platform.

```sh
cargo run --release --example long_wave
```

That example reports median and p95 render times, SVG sizes, and native versus JSON5 for long signals.

## Library

```rust
let svg = tgw::render("clk: P...\ndata: x3.4 => ready done")?;
let mut buffer = Vec::new();
tgw::render_into("clk: p...", &mut buffer)?;
let editable = tgw::to_tgw("{signal:[{name:'clk',wave:'p...'}]}")?;
# Ok::<(), tgw::Error>(())
```

`render_opts` indents the SVG. `render_with_format` and `to_tgw_with_format` take `InputFormat::Auto`, `Tgw`, or `Json5`. `render_themed` takes `scheme::LIGHT` or `scheme::DARK`. `render` and `render_with_format` always write `scheme::LIGHT`, which is also the palette of a saved file. On error the output buffer is cleared. Invalid paths, excessive nesting, non-finite timing, and out-of-range geometry return `Error` with a byte offset and a message.

## Checks

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --all-targets --no-default-features --features watch -- -D warnings
cargo clippy --locked --all-targets --no-default-features -- -D warnings
cargo test --locked
cargo test --locked --no-default-features
cargo test --locked --no-default-features --features watch
cargo test --locked --no-default-features --release
cargo test --locked --test watch -- --ignored   # opens a window
cargo doc --locked --no-deps
cargo build --locked --release
```

The three Clippy lines, and the matching tests, are the default build, the headless watcher, and the renderer alone. The renderer is also tested in release.

`tests/fixtures` holds native diagrams and their SVG snapshots. The original eight also have JSON5 twins in `tests/fixtures/legacy` and must render identically, including under CRLF. Further snapshots cover the wave alphabet, fractional ticks, period and phase, markup, Unicode, a cropped bus, SVG paths, an empty diagram, and nested groups. `tests/watch.rs` drives the real binary through in-place saves, a broken save, deletion, and a rename, then through every edge-case picture, for `--watch` and, when ignored tests run, for `--view`. The window tests also slice and raster those pictures the way the viewer does.

```sh
UPDATE_SNAPSHOTS=1 cargo test --test render snapshots
```

Browser QA is optional and uses a Playwright install that is not part of the Rust build:

```sh
node scripts/visual-check.cjs /tmp/tgw-visual
```

Set `PLAYWRIGHT_MODULE` and `BROWSER_EXECUTABLE` when they are not on the default paths. The script checks SVG references and text bounds, then saves 1× and 2× screenshots.

## License

The Great Wave is released under the MIT License. Copyright (c) 2026 eaSyMaxSU.

Wave rules, lane semantics, and the default bus palette originate in [WaveDrom](https://wavedrom.com/) by Aliaksei Chapyzhenka (Copyright 2011–2026), which is MIT licensed ([source](https://github.com/wavedrom/wavedrom), [WaveJSON](https://github.com/wavedrom/schema)). Legacy input uses the [JSON5](https://spec.json5.org/) subset customary for those diagrams ([project](https://github.com/json5/json5), MIT). SVG arc geometry follows the [SVG 2 arc implementation notes](https://www.w3.org/TR/SVG2/implnote.html#ArcImplementationNotes).
