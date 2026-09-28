# The Great Wave

`tgw` turns readable timing diagrams into compact, self-contained SVGs. Write aligned signals in a `.tgw` file; there are no braces, commas, or repeated `name`/`wave` keys. The Rust library and CLI have no dependencies.

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

## Use

```sh
cargo build --release
./target/release/tgw examples/transfer.tgw -o transfer.svg
./target/release/tgw < examples/transfer.tgw > transfer.svg
```

`-i/--input` also selects an input file; `-o/--output` writes a file. Omit the input, or use `-`, to read stdin. `--indent 2` formats the SVG. `--help` lists all options. Errors include the source line and column.

Existing WaveJSON/JSON5 files remain supported. Input syntax is detected automatically, or selected with `--format tgw|json5|auto`. Convert old diagrams once:

```sh
./target/release/tgw old-diagram.json5 --convert -o diagram.tgw
```

`--convert` also formats native diagrams with aligned waveform columns. Conversion preserves supported diagram data; comments and unsupported JSON properties are omitted.

## Live view

`tgw-view` opens one diagram in a window and redraws it whenever that file is saved. The default `tgw` build stays dependency-free; the viewer is an optional feature and needs Rust 1.85 or newer.

Build it, then open a `.tgw` or WaveJSON file:

```sh
cargo build --release --features view --bin tgw-view
./target/release/tgw-view examples/transfer.tgw
```

The same command works while editing. Save the file and the window updates. `tgw-view --help` prints usage. Pass one path only; stdin is not accepted.

```sh
cargo run --release --features view --bin tgw-view -- examples/transfer.tgw
```

Signal names stay fixed on the left. Tick numbers, the title, and the waveforms scroll together. Drag a scrollbar, click its track, or use the trackpad; hold Shift to move a vertical scroll sideways. A diagram wider than the window scrolls horizontally, and a taller one scrolls vertically. A tick label that would be cut by the edge is omitted until it fits. A diagram that already fits the window is enlarged and has no scrollbar. `@bounds` still limits the time range that is drawn.

A syntax error or a missing file keeps the last successful picture and shows `path:line:col: message` until the next good save. Command-W and Command-Q close the window.

```sh
cargo test --features view
cargo clippy --all-targets --features view -- -D warnings
```

## Native format

One signal per line: `name: wave`. Names can contain spaces and bus indices, such as `data [7:0]`. Blank lines are ignored; `---` inserts a blank lane. `#` starts a comment outside quotes. Indentation is cosmetic.

Separate data labels with spaces after `=>`. Quote a label containing spaces, `#`, `;`, quotes, or `=>`. Single and double quotes support `\n`, `\r`, `\t`, `\\`, escaped quotes, and `\uXXXX`; Unicode text also works directly.

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

Groups use `@group name` and `@end`, and can nest. `@group` without a name draws an unlabeled bracket. Lane options follow semicolons:

| Option | Meaning |
| --- | --- |
| `period=2` | Stretch the signal's time units; must be positive. |
| `phase=0.25` | Advance by a quarter cycle; negative values delay. |
| `node=..a..B` | Name event positions; uppercase nodes have no visible node label. |
| `over=1..0` / `under=2..0` | Colored annotation spans; `.` extends and `0` ends a span. |

The wave alphabet is `p n P N h l H L 0 1 x d u z = 2-9`. Clocks are `p/n`, with arrows for `P/N`; `x` is unknown, `z` is high impedance, and `=/2-9` are data buses. `.` repeats, `|` marks a gap, and `<...>` halves the duration of the enclosed level/bus symbols. Spaces within waveforms are ignored. A clock retains at least two half-cycle bricks; a level retains at least one. Fractional periods use this discrete half-cycle geometry, and annotations follow the resulting timing.

| Directive | Example |
| --- | --- |
| Title / footer | `@title Bus transfer`, `@footer Figure 1` |
| Cycle boundary numbers | `@tick 0` |
| Numbers between boundaries | `@tock 0` |
| Numeric start and step | `@tick 0 0.5` |
| Explicit tick labels | `@tick "idle" "request" "reply"` |
| Footer ticks | `@foot-tick 0`, `@foot-tock 0` |
| Label interval | `@every 2`, `@foot-every 2` |
| Horizontal scale | `@scale 2` (integer 1–100) |
| Visible cycle window | `@bounds 10 30` (end exclusive) |
| Grid visibility | `@grid off` or `@grid on` |
| Edge label size | `@arc-font 12` |
| Global gap marks | `@gaps . . 1 . 2` |
| Connection | `@edge a~>b propagation delay` |
| Explicit empty diagram | `@empty` |

Tick directives accept `off`. Numeric `start` is the displayed starting value, and optional `step` controls the increment and decimal precision. Unit ticks follow `@bounds`; explicitly scaled series retain their chosen start. Tick labels are thinned when needed to avoid collisions, with at most 10,000 numeric labels per series.

Native SVG paths support analog or custom shapes. X coordinates are cycles; Y coordinates run from 0 (low) to 1 (high). Standard SVG commands, curves, arcs, relative coordinates, and exponents are accepted. `period`, `phase`, and `@scale` apply to paths too; strokes keep a constant width.

```text
analog: path M0,0 L1,0 C1.5,0 1.5,1 2,1 H3 L4,0
```

Register and assign diagrams are outside the supported format. WaveJSON uses the JSON5 subset customary for timing diagrams: quoted or bare keys, single/double-quoted strings, comments, trailing commas, arrays, objects, finite numbers, booleans, and null.

## Rendering and performance

Clocks use shared, padded SVG patterns with complete arrowheads and consistent stroke weight. Bus transitions meet at the same crossing, labels center on the visible value, and gap marks clear the trace underneath. SVGs include a title, description, document-scoped paint IDs, and no global CSS, so different diagrams can be embedded on the same page.

Rendering scales with source length and visible runs. A 10,000-cycle clock is still one clock pattern and a constant number of drawing elements. Repeated holds remain one stroke. Horizontal cropping skips hidden geometry, including annotations, instead of emitting an entire long diagram. `render_into` reuses the caller's output allocation. Fonts use system fallbacks; metrics reserve space conservatively, so appearance may vary slightly between platforms.

Run the warmed-buffer benchmark for median and p95 timings, SVG sizes, and native/JSON5 comparisons:

```sh
cargo run --release --example long_wave
```

## Library

```rust
let svg = tgw::render("clk: P...\ndata: x3.4 => ready done")?;
let mut buffer = Vec::new();
tgw::render_into("clk: p...", &mut buffer)?;
let editable = tgw::to_tgw("{signal:[{name:'clk',wave:'p...'}]}")?;
# Ok::<(), tgw::Error>(())
```

`render_opts` adds SVG indentation; `render_with_format` selects `InputFormat::Auto`, `Tgw`, or `Json5`. `to_tgw_with_format` does the same for conversion. Rendering clears the output buffer on an error. Invalid paths, excessive nesting, nonfinite timing, and out-of-range geometry return errors.

## Checks

```sh
cargo test
cargo test --release
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

The native fixtures in `tests/fixtures` are compared with their legacy JSON5 sources in `tests/fixtures/legacy`. Regression tests cover geometry, cropping, Unicode, malformed inputs, conversion, and compact long signals. SVG snapshots only change on explicit request:

```sh
UPDATE_SNAPSHOTS=1 cargo test --test render snapshots
```

Optional browser QA uses Playwright installed separately from the Rust project:

```sh
node scripts/visual-check.cjs /tmp/tgw-visual
```

Set `PLAYWRIGHT_MODULE` to a local Playwright package and `BROWSER_EXECUTABLE` to a Chromium browser if they are outside the defaults. The script verifies SVG references and layout, then saves screenshots at 1× and 2× resolution.

## License

The Great Wave is released under the MIT License. Copyright (c) 2026 eaSyMaxSU.

Wave rules, lane semantics, and the default bus palette originate in [WaveDrom](https://wavedrom.com/) by Aliaksei Chapyzhenka (Copyright 2011–2026), which is MIT licensed ([source](https://github.com/wavedrom/wavedrom), [WaveJSON](https://github.com/wavedrom/schema)). Legacy input uses the [JSON5](https://spec.json5.org/) subset customary for those diagrams ([project](https://github.com/json5/json5), MIT). SVG arc geometry follows the [SVG 2 arc implementation notes](https://www.w3.org/TR/SVG2/implnote.html#ArcImplementationNotes).
