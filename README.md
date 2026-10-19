# The Great Wave

<img src="assets/icon/tgw.svg" width="88" height="88" align="right" alt="The Great Wave icon">

`tgw` turns a text file into a compact SVG. One binary draws four pictures. The rendering library has no dependencies.

| First directive | Picture | SVG root class | Example |
| --- | --- | --- | --- |
| `@wvf` | Timing diagram | `class="tgw"` | `examples/transfer.tgw` |
| `@asm` | Algorithmic state machine chart | `class="tgw asm"` | `examples/asm.tgw` |
| `@hls` | High-level synthesis schedule | `class="tgw hls"` | `examples/hls.tgw` |
| `@gtl` | Gate-level netlist | `class="tgw gtl"` | `examples/gtl.tgw` |

Use timing diagrams to explain signal relationships, ASM charts to describe controller behavior, HLS schedules to show when arithmetic runs, and gate netlists to show combinational connections. The input is UTF-8 text and the output is SVG: keep the source in version control, review changes as text, and embed the rendered picture in documentation. tgw draws the description you supply; it does not simulate a circuit, choose an HLS schedule, or synthesize RTL.

## Contents

- [Download and run](#download-and-run)
- [Quick start](#quick-start)
- [Input format](#input-format) and [writing a file](#writing-a-file)
- [Render](#render), [command-line reference](#command-line-reference), and [convert](#convert)
- [Watch and view](#watch-and-view)
- Language reference: [timing diagrams](#timing-diagrams), [ASM charts](#asm-charts), [HLS schedules](#hls-schedules), and [gate netlists](#gate-netlists)
- [Example cookbook](#example-cookbook)
- [Picture](#picture) and [library integration](#library)
- [Builds](#builds), [platforms](#platforms), and [troubleshooting](#troubleshooting)
- [Icon](#icon), [checks](#checks), and [license](#license)

## Download and run

Download the matching archive from [GitHub Releases](https://github.com/eaSyMaxSU/TheGreatWave/releases/latest). Every download contains the **combined renderer, viewer, and watcher**; no Rust installation or source checkout is needed. Extract it and run `tgw` from a terminal, or copy the executable to a directory on your `PATH`. Examples, this README, and the license are included.

| Platform | Archive suffix | Requirements |
| --- | --- | --- |
| macOS Apple Silicon (ARM64) | `aarch64-apple-darwin.tar.gz` | macOS 15 or newer |
| Arch Linux x86_64 | `x86_64-unknown-linux-gnu.tar.gz` | System libraries below; X11 or Wayland and a Vulkan driver for the viewer |
| Windows x86_64 | `x86_64-pc-windows-msvc.zip` | Windows 10 or newer; the C runtime is statically linked |

From the extracted directory, on macOS or Linux:

```sh
./tgw examples/transfer.tgw -o transfer.svg          # render once
./tgw examples/transfer.tgw --view                   # view, refreshing on edits
./tgw examples/transfer.tgw -o transfer.svg --watch  # refresh the file on edits
```

On Windows PowerShell, replace `./tgw` with `.\tgw.exe`. To update both the window and an SVG file, use `--view -o transfer.svg`. `--view` already watches the input; there is no need to add `--watch`. The executable does not need the bundled examples or documentation beside it.

On Arch Linux, install the shared runtime libraries if they are not already present:

```sh
sudo pacman -S --needed glibc gcc-libs fontconfig freetype2 libxkbcommon \
  libxkbcommon-x11 wayland libxcb libx11 openssl vulkan-icd-loader
```

The viewer also needs a Vulkan driver for your GPU, such as `vulkan-intel`, `vulkan-radeon`, or `nvidia-utils`. These are system dependencies, not extra tgw files. The Linux release is built on Ubuntu 22.04 (glibc 2.35 baseline) and is intended to work on current Arch Linux. For an optional Wayland launcher/icon, put `tgw` on `PATH`, install the bundled `tgw.desktop` into `~/.local/share/applications/`, and `tgw.svg` into `~/.local/share/icons/hicolor/scalable/apps/`.

Releases include `SHA256SUMS`. Verify with `sha256sum -c SHA256SUMS --ignore-missing` on Linux, `shasum -a 256 ARCHIVE` on macOS, or `Get-FileHash ARCHIVE -Algorithm SHA256` on Windows (compare the result with `SHA256SUMS`). Downloads are **unsigned**, and the macOS binary is not notarized. macOS may require approval in System Settings → Privacy & Security after the first launch. If a trusted, checksum-verified download remains quarantined, remove quarantine only from that executable with `xattr -d com.apple.quarantine ./tgw`. Windows may show SmartScreen; approve only the verified file, without disabling system-wide protection.

## Quick start

After installing the executable on `PATH`, save this as `handshake.tgw`:

```text
@wvf
@title Request and acknowledge
@tick 0

clk:   P.......
req:   01...0..
ack:   0..1..0.
data:  x.3...x. => "valid data"
```

Render it, then open `handshake.svg` in a browser or image viewer:

```sh
tgw handshake.tgw -o handshake.svg
```

For an editing session, keep the native viewer and the exported SVG up to date together:

```sh
tgw handshake.tgw --view -o handshake.svg
```

Edit and save `handshake.tgw` in your usual editor. The viewer reloads automatically; if a save contains a syntax error, it keeps the last good picture until the next valid save. On a machine without a desktop, use `--watch -o handshake.svg` instead of `--view`.

To build a renderer without GUI or watcher dependencies from source:

```sh
git clone https://github.com/eaSyMaxSU/TheGreatWave.git
cd TheGreatWave
cargo build --locked --release --no-default-features
./target/release/tgw examples/transfer.tgw -o transfer.svg
```

Install [Rust through rustup](https://rustup.rs/) first; the checkout selects the compiler pinned in `rust-toolchain.toml`. See [builds](#builds) for watcher and viewer builds. Commands below assume `tgw` is on `PATH`; use `./tgw`, `.\tgw.exe`, or `./target/release/tgw` as appropriate for your installation.

## Input format

The first non-comment directive chooses the picture. `#` is a comment. `//` is not. A file is one picture: do not mix `@wvf`, `@asm`, `@hls`, and `@gtl`. The text describes the picture. It does not store coordinates, wire routes, or operator counts. A timing diagram that omits `@wvf` still renders, and `--convert` writes `@wvf` at the top.

```text
@wvf
@title Bus transfer
@tick 0

clk:         P.....|...
data [7:0]:  x.345x|=.x => head body tail "read data"
request:     0.1..0|1.0
---
acknowledge: 1.....|01.
```

![Bus transfer](examples/transfer.svg)

## Writing a file

1. Pick the row in the table above. Put `@wvf`, `@asm`, `@hls`, or `@gtl` on the first non-comment line, alone.
2. Copy the matching example, then replace the names. `@title` and `@footer` work in all four.
3. Check it: `tgw FILE` prints SVG, and `tgw FILE --convert` prints the canonical text. A bad file exits with `path:line:col: message` and a byte offset inside the message from the library.
4. Leave layout to the renderer. Indentation matters only inside `@asm` and `@hls`. A gate netlist ignores indent.

Quotes are the same in all four. A token that contains a space, `#`, a quote, or a backslash is written `"like this"`. Escapes are `\n`, `\r`, `\t`, `\\`, `\"`, `\'`, and `\uXXXX`. Unicode can also be written directly.

## Render

```sh
cargo build --release
./target/release/tgw examples/transfer.tgw -o transfer.svg
./target/release/tgw examples/asm.tgw -o asm.svg
./target/release/tgw examples/hls.tgw -o hls.svg
./target/release/tgw examples/gtl.tgw -o gtl.svg
./target/release/tgw < examples/transfer.tgw > transfer.svg
```

`-i` selects the input. `-o` selects the output. Omit the input, or pass `-`, to read stdin. Omit `-o`, or pass `-`, to write stdout. `--indent 2` pretty-prints the SVG. `tgw --help` lists every flag.

A render refuses to replace the diagram with the SVG. The same path, a symlink, and a hard link are all the same file. On Windows, a different case of the name is the same file too.

### Command-line reference

| Argument or option | Meaning |
| --- | --- |
| `INPUT` | One `.tgw` input file; omit it or use `-` for stdin. |
| `-i PATH`, `--input PATH` | Alternative to positional `INPUT`; do not supply both. |
| `-o PATH`, `--output PATH` | Write to a file; omit it or use `-` for stdout in one-off mode. |
| `--convert` | Write canonical `.tgw` text instead of SVG. |
| `-t N`, `--indent N` | SVG indentation in spaces, from 0 to 255; 0 is compact. |
| `-w`, `--watch` | Watch a real input file and refresh the file selected by `-o`. Requires the `watch` feature. |
| `--view` | Open the live viewer; `-o` is optional. Requires the `view` feature. |
| `-h`, `--help` | Print usage and build-specific feature information. |
| `-v`, `--version` | Print the package version. |
| `--` | End option parsing, for example `tgw -- -diagram.tgw`. |

Options with values also accept `--input=diagram.tgw`, `--output=diagram.svg`, and `--indent=2`. Quote paths containing spaces. Output directories must already exist. A successful command exits with status 0; an error exits with status 1 and writes its diagnostic to stderr.

```sh
# Explicit input and formatted SVG output.
tgw --input examples/asm.tgw --output asm.svg --indent 2

# A small diagram supplied through a Unix shell pipeline.
printf '@wvf\nclk: p...\nready: 01..\n' | tgw - -o ready.svg

# Render all bundled .tgw examples into a separate directory (POSIX shell).
mkdir -p rendered
for source in examples/*.tgw; do
  name="${source##*/}"
  tgw "$source" -o "rendered/${name%.tgw}.svg" || exit 1
done
```

## Convert

```sh
./target/release/tgw examples/transfer.tgw --convert
./target/release/tgw examples/asm.tgw --convert
./target/release/tgw examples/hls.tgw --convert
./target/release/tgw examples/gtl.tgw --convert
```

`--convert` reprints a timing diagram starting with `@wvf` and aligned columns, an ASM chart at a 2-space indent, an HLS schedule with every cycle header filled in, and a gate netlist as `@gtl` followed by one gate per line. Supported data is kept. A `#` comment stays with the construct it precedes, including an end-of-line note. `--convert` cannot be combined with `--watch` or `--view`.

To review a formatted version without modifying the original:

```sh
tgw handshake.tgw --convert -o handshake.formatted.tgw
diff -u handshake.tgw handshake.formatted.tgw
tgw handshake.formatted.tgw -o handshake.formatted.svg
```

Conversion is formatting, not translation between diagram types or import from WaveDrom JSON. Do not use `tgw handshake.tgw --convert > handshake.tgw`: the shell truncates the input before tgw can read it. Write to a separate file, review it, and then replace the source if desired.

## Watch and view

```sh
./target/release/tgw examples/transfer.tgw -o transfer.svg --view   # window and file
./target/release/tgw examples/transfer.tgw --view                   # window only
./target/release/tgw examples/transfer.tgw -o transfer.svg --watch  # file only
./target/release/tgw examples/asm.tgw --view                        # chart, same window
./target/release/tgw examples/hls.tgw --view                        # schedule, same window
./target/release/tgw examples/gtl.tgw --view                        # netlist, same window
```

`--view` opens a window. `--watch` does not. Both keep running, and every save of the input refreshes the picture. With `-o`, the file is refreshed too.

Live mode needs a real input file. `--watch` also needs `-o`. A directory is rejected, and the output directory must already exist.

The output is the same bytes as a one-off render with the same `--indent`. An unchanged save does not rewrite it. A new picture is published by renaming a temporary file into place. When a rename is refused, as when Windows is holding the file open, the picture is written in place. Editors that save by renaming a sibling are followed. The file is also polled five times a second, so a save is still seen on a network drive. On Windows the poll compares names without regard to case, and uses the file index so a replaced file is seen even when the size and timestamp do not change.

A syntax error, a missing file, or a write that fails leaves the last good picture in place. A diagram error is reported as `path:line:col: message`. A write error is the operating-system error. The window shows it in the status bar; `--watch` prints it on stderr. A failed write is tried again at the next poll. Ctrl-W or Ctrl-Q closes the window (Command on macOS). `--watch` stops on Ctrl-C.

In the window, signal names stay fixed. The title, tick numbers, and waveforms scroll together. An `@asm` chart, an `@hls` schedule, or a `@gtl` netlist is the same window with no name column: the whole picture scrolls, and both bars appear when it does not fit. When that picture fits, it is scaled by a whole number so each SVG pixel stays a whole device pixel. When it does not fit, the scale stays 1 and both scrollbars appear.

Use the wheel or trackpad, and hold Shift to scroll sideways. Drag a scrollbar thumb, or click the track to move by most of the view. Bars appear only when the diagram overflows. A diagram that fits is enlarged until it fills the window, with the same small padding on every edge. A short diagram opens in a window snug to the drawing. A later resize is kept, and the extra room becomes padding. A waveform wider than the window scrolls horizontally. A taller diagram scrolls vertically. A tick label that would be cut in half is left out until it fits. `@bounds` still decides which cycles are drawn.

The window follows the system appearance. Default light is the palette written into the SVG. Default dark draws the same diagram in a dark palette. Ctrl-Shift-L (Command-Shift-L on macOS) switches between them and keeps the choice. The saved file stays on the light palette.

The saved SVG stays compact: a long clock is one pattern, and an unknown value is a small hatch pattern. The window paints only the visible slice. It repeats a clock or the grid as strokes. An unknown hatch is a set of continuous slashes, carried through a bus transition, so the marks meet from one side to the other. A bus fill overlaps the edge it shares with that transition, covering the page along the seam.

## Timing diagrams

A file whose first directive is `@wvf` is a timing diagram. `@wvf` takes no arguments. One signal per line, `name: wave`. Names may contain spaces and ranges, as in `data [7:0]`. A blank line is ignored. `---` inserts an empty lane. Indentation does not matter. Do not start the file with `@asm`, `@hls`, or `@gtl`.

Labels come after `=>`, separated by spaces. Quote a label that contains a space, `#`, `;`, a quote, or `=>`.

```text
@wvf
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

`@group Name` and `@end` nest, up to 64 deep. `@group` with no name draws an unlabeled bracket. `@end` closes the nearest group. It is not used in an ASM chart, an HLS schedule, or a gate netlist.

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

Register and assign diagrams are not supported.

When generating a timing diagram: start with `@wvf`, then one `name: wave` line per signal, labels only after `=>`, node names only in `node=` or `@edge`, and a `#` comment rather than `//`.

## ASM charts

A file whose first directive is `@asm` is an algorithmic state machine chart. Indentation matters only in this file. `#` comments, `@title`, and `@footer` work as they do on a timing diagram. `@end` is not used. `--convert` reprints the chart at a 2-space indent.

`examples/asm.tgw`:

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
    0 ? hold
      0 wait
      1 idle
    1 idle
```

![Bus handshake ASM chart](examples/asm.svg)

`name:` at column 0 opens a state. The name is a bare token, or a quoted string when it contains a space or `:`. Lines under the state, in order:

1. Moore outputs, one line each, such as `req=0`. These come before any exit. A state box holds at most 32 lines.
2. One exit. `> next` links straight to `next`. `? condition` is a decision and replaces that one exit. A state with no exit is terminal.

A decision has exits `0` and `1`, in either order, at the same indent, one step deeper than the `?` line. `0` continues down. `1` leaves to the right. Each exit is a state name or a nested `?`:

```text
? start
  0 idle
  1 (req=1) wait
```

`1 (req=1) wait` draws a conditional-output box on that exit, then links to `wait`. The parentheses are required around the condition output. A nested decision takes the place of the state name:

```text
0 ? hold
  0 wait
  1 idle
```

Blocks stack in source order. A state is a card with its name in a coloured header and its outputs below. A decision is an amber diamond with `0` and `1` beside its exits. A conditional output is a teal pill. A link into the state directly below is a straight arrow. Any other link runs in a side channel: forward on the right, back on the left. Exits from one state to the same target share one drop column and one channel, and a dot marks where they join. Wires are rounded and end in an arrowhead on the target's entry. The text does not carry coordinates, column numbers, or arrow routes.

Caps: 256 states, 32 lines in one state box, 32 nested decisions. A second exit on a state (`> a` and `> b`, or `>` plus `?`) is an error. A decision that lacks `0` or `1`, repeats one of them, or indents the two exits differently is an error. An unknown target lists the state names. A directive after the first state is an error (`directives belong before the states`).

When generating a chart: start with `@asm`, put every state name at column 0, indent outputs and the exit, indent `0` and `1` one level under `?`, and give every decision both exits. Two spaces per level is the canonical indent.

## HLS schedules

A file whose first directive is `@hls` is a high-level synthesis schedule. Time runs down. A horizontal rule is a clock edge, and the band under that rule is one cycle. Cycle `0` is the top. Operators in one band run in the same cycle. Indentation matters only in this file. `#` comments, `@title`, and `@footer` work as they do on a timing diagram. `@end` and `@asm` are not used.

`examples/hls.tgw`:

```text
@hls
@title Scaled sum of products

0:
  + a b -> s0
  + c d -> s1
  * e f -> p
1:
  * s0 s1 -> m
2:
  + m p -> t
3:
  / t n -> y !3
```

![Scaled sum of products HLS schedule](examples/hls.svg)

Cycle `0` uses two adders and one multiplier. Cycle `1` multiplies `s0` and `s1`; `p` is carried through that cycle because nothing reads it yet. Cycle `2` adds `m` and `p`. Cycle `3` starts a divider that stays busy for three cycles, so cycles 3, 4, and 5 each count one divider, and `y` is ready at the end of cycle 5. No operator reads a value written in the same cycle.

A cycle header is `N:` at column 0. Numbers start at 0, increase, and have no leading zeros. A missing number is still drawn, as an empty band whose usage is `+0 ×0 ÷0`. An operator is indented under the cycle it starts in. Canonical indent is two spaces.

| Line | Unit |
| --- | --- |
| `+ a b -> s0` | Adder. |
| `* e f -> p` | Multiplier. |
| `/ t n -> y` | Divider. |

The two operands are names or integer constants, including negatives (`-2`). The name after `->` is the result. `!N` keeps that unit busy for N cycles, this cycle and the next N−1, and the usage count of each of those cycles includes it. Its card spans those bands, with a dashed line at each clock edge it crosses and `N cycles` in its corner. Omit `!N` when the unit lasts one cycle. `!1` is accepted and `--convert` drops it. `--convert` writes `!N` only when N is not 1, and it prints every cycle header from 0 through the last busy cycle, including gaps and the empty tail of a multi-cycle unit.

A name that is not yet a result is a primary input (`a`, `b`, `n` in the example). A result becomes readable at cycle `start + N`. Using it earlier, or using a result as its own operand, is an error: `result "s0" is not ready until cycle K; results are ...`. Defining the same result twice is `duplicate result "s0"`. A token in the operand position that is `+`, `*`, `/`, `->`, or starts with `!` is an unknown operand, and the message lists the results defined so far.

Each band has its cycle number in the left gutter and a usage column on the right: `+a`, `×m`, and `÷d` count cards of that kind whose lifetime covers the cycle, including a unit that started earlier and is still busy. A nonzero count is a pill in that kind's colour; a zero is greyed out. The usage is computed. Do not write it in the file.

A card is coloured by its unit: blue for an adder, violet for a multiplier, orange for a divider. It shows the operation glyph, the expression (`a + b`, `s0 × s1`, `t ÷ n`), and `→` the result.

A result used later is a wire in the producer's colour from the bottom of its card to an arrowhead on that operand's port: the left operand enters a third of the way across the consumer, the right operand two thirds. In the routing zone under each row of cards, every result gets its own horizontal track. A value read more than one cycle later drops through its own lane in the gap beside the consumer, so no two wires share a run and no wire passes through a card. A small square marks each clock edge a wire crosses: one register per cycle the value is held. A dot marks where one result branches to several readers. A result that never appears again is a live-out: an arrow under its card ends in a pill with its name.

Caps: 64 cycles, 32 operators started in one cycle, 256 operators in the file. A latency that would pass cycle 63 is an error. Cycle numbers that do not increase are an error. A cycle header that is indented is `a cycle header starts at column 0`. An operator before any cycle is `an operator belongs to a cycle`. `@hls` takes no arguments. A directive after the first cycle is `directives belong before the cycles`.

When generating a schedule: one short line per operator, both operands, a fresh result name, and `!N` only for a unit that stays busy. Order the cycle headers from 0 upward. Do not chain two operators in the same cycle. Do not store coordinates or the usage counts.

Quoted names and constants:

```text
@hls
0:
  + "go on" b -> "out put"
1:
  * s -2 -> p
```

An unquoted integer is a constant. A quoted integer is a name, so `"1"` is not the constant `1`.

## Gate netlists

A file whose first directive is `@gtl` is a gate-level netlist. Signals run left to right. Each gate is its distinctive-shape symbol (`AND`, `OR`, `XOR`, a `NOT` triangle, a `MUX` trapezoid) with its function written inside. `NOT`, `NAND`, `NOR`, and `XNOR` end in an inversion bubble. Inputs land on the left, top to bottom in source order; a mux takes its data inputs on the left, marked `0` and `1`, and its select from below. The output leaves on the right. A name that no gate writes is a primary input: a teal pill on a short stub. An integer tie-off is a grey pill. A result that no gate reads is a live-out: a red pill on the right. Every other result is named in small text on its wire. A gate slides down its column to line its input up with the gate that drives it, so a chain is one straight line. A later gate reads an earlier result through a rounded orthogonal wire with its own track in the gap between columns. A wire that skips columns runs straight across when its row is clear, and otherwise steps to the nearest free row. A dot marks where one net branches. Indentation does not change the picture. `#` comments, `@title`, and `@footer` work as they do on a timing diagram. `@end`, `@asm`, `@hls`, and `@wvf` are not used.

`examples/gtl.tgw`:

```text
@gtl
@title Select and invert

xor a b -> d
not d -> nd
mux nd a b -> y
```

![Select and invert gate netlist](examples/gtl.svg)

`xor` and `not` form a chain. `mux` takes the select first (`nd`), then the two data inputs (`a`, `b`). `y` is the live-out. `a` and `b` are primary inputs.

Each gate line is `kind inputs -> output`. The kind decides how many inputs are legal. The parser asks the kind, so the line is not fixed at two operands. The owned model stores `inputs` and `outputs` as lists. Every kind has one output today. A later kind with more ports is a new kind and a new count, not a new placer.

| Kind | Inputs | Output |
| --- | --- | --- |
| `and`, `or`, `xor` | two | one |
| `nand`, `nor`, `xnor` | two, and an inversion bubble on the output | one |
| `not` | one, and an inversion bubble on the output | one |
| `mux` | select, then two data inputs | one |

`not a -> y`. `mux s a b -> y`. `and a b -> y`. Operands are names or integer tie-offs (`0`, `1`, `-1`). A quoted integer stays a name, so `"1"` is not the constant `1`. Quotes and escapes match timing labels.

A result used before it is defined, including a loop or a gate that reads its own output, is an error, and the message lists the results defined so far. Defining the same result twice is `duplicate result "y"`. An unknown kind lists `and, or, not, nand, nor, xor, xnor, mux`. The wrong number of inputs names the count that kind takes.

`--convert` reprints `@gtl`, then the title and footer, then one gate per line in source order. Extra indent is dropped.

Caps: 256 gates, logic depth 64, and 8 inputs or 8 outputs on one gate. Depth is one forward walk: a gate's column is one past the deepest gate that drives it, and primary inputs are not gates. `@gtl` takes no arguments. A directive after the first gate is `directives belong before the gates`.

When generating a netlist: one short line per gate, the kind's inputs in port order, and a fresh result name. Do not store coordinates or wire routes.

## Example cookbook

Each example below is a complete input file. Save a block as `recipe.tgw` and run `tgw recipe.tgw -o recipe.svg`. Use `tgw recipe.tgw --convert` to inspect its canonical form. The [bundled examples](examples/) also include ready-rendered SVGs; [test fixtures](tests/fixtures/) contain more syntax and layout edge cases.

### Annotate a setup interval

Node masks attach named positions to signals. Bracketed names occupy one position, just like a single-letter name. Here `setup` is at cycle 2 on `data`, and `sample` is at cycle 3 on `clk`:

```text
@wvf
@title Data setup before sampling
@tick 0

clk:  0..1.0.. ; node=...[sample]....
data: x.3...x. => payload ; node=..[setup].....

@edge setup~>sample "tSU"
```

The edge is an annotation between the supplied positions, not a measured timing constraint or a setup-time check.

### Group lanes and highlight activity

Groups organize the name column; `over` and `under` mark intervals independently of the waveform. Close every group with `@end`.

```text
@wvf
@title Write transaction
@tick 0

@group Master
  clk:     P.......
  address: x3...x.. => 0x20
  write:   01..0... ; over=.1..0...
@end
@group Slave
  ready:   0..1.0.. ; under=...2.0..
@end
```

### Zoom into part of a trace

`@bounds 2 6` draws cycles 2 through 5, not cycle 6. `@scale` changes cycle width; it does not change the signal data. Fractional numeric ticks can express time units instead of cycle indices.

```text
@wvf
@title Detail of a transfer
@bounds 2 6
@scale 2
@tick 0 0.5
@footer Time in microseconds

clk:  P.......
data: x.3.4.x. => command reply
busy: 01...0..
```

Because the tick step is 0.5 rather than 1, the numeric series retains the explicit start value. See the timing reference for the interaction between ticks and bounds.

### Mix digital and analog lanes

SVG path coordinates use cycles horizontally and normalized low/high rail values vertically. This trace rises, holds, and falls alongside a digital enable:

```text
@wvf
@title Analog response
@tick 0

enable: 01...0..
voltage: path M0,0 L1,0 C1.5,0 1.5,1 2,1 H4 L5,0 H8
```

The path is supplied geometry, not the result of an analog simulation.

### Build a controller with a terminal state

`> state` is an unconditional transition. A state with no exit is terminal. This controller stays in `idle` until `start`, drives a conditional output on entry to `run`, then stops at `done`:

```text
@asm
@title One-shot controller

idle:
  busy=0
  ? start
    0 idle
    1 (load=1) run
run:
  busy=1
  > done
done:
  busy=0
  valid=1
```

### Schedule a multi-cycle result and an idle cycle

The multiplier starts at cycle 0 and is busy in cycles 0 and 1. Its result is readable from cycle 2; the consumer below waits until cycle 3. Omitted cycle headers still appear in the drawing and in canonical output.

```text
@hls
@title Multiply, hold, and offset

0:
  * a b -> product !2
3:
  + product -2 -> adjusted
```

Moving the addition to cycle 1 is an error, not a way to express forwarding. Operator lifetimes and value availability are checked, but scheduling is your responsibility.

### Draw a mux with an enable condition

Mux port order is **select, data 0, data 1**. This netlist combines enable signals and chooses between the two data inputs:

```text
@gtl
@title Enabled data selection

and enable valid -> select
mux select data0 data1 -> chosen
not chosen -> inverted
```

The gates must be in producer-before-consumer order. This is a combinational netlist; feedback and sequential storage are not supported gate kinds.

### Use tie-offs and quoted names

An unquoted integer is a constant; a quoted integer is a signal name. This example uses the named signal `"1"` and the constant `1` in the same gate:

```text
@gtl
@title Names versus constants

and "1" 1 -> "masked value"
xor "masked value" 0 -> result
```

Use quotes for multi-word names rather than trying to align the tokens with spaces. Alignment is cosmetic in netlists, but indentation defines structure in ASM and HLS files.

## Picture

Bus transitions cross at the same point, and a label is centered on the visible part of its value. A gap clears the trace underneath it. Each SVG has a title, a description, and paint ids that belong to that document only. There is no global stylesheet, so several diagrams can sit on one page.

A long clock is one shared pattern. A long hold is one stroke. `@bounds` drops geometry outside the window, including annotations, instead of emitting it and hiding it. `render_into` reuses the caller's buffer. Text is measured against a conservative sans-serif, so a label's exact width can differ slightly by platform.

ASM boxes, HLS cards, and every pill sit on a pixel grid. Text plus padding is snapped outward to 4px. A border is a filled rounded rectangle with its fill inset by 1px, so a 1px edge stays on whole pixels. HLS cycle rules are 1px filled rectangles. A wire in a chart, a schedule, or a netlist is a 2px stroke centred on whole-pixel coordinates, so each straight run covers exactly two pixel rows or columns at any whole-number scale; only its rounded corners, gate outlines, diamonds, bubbles, dots, and text are antialiased. Both palettes have a dark twin for the window. The root transform is `translate(8,8)`.

```sh
cargo run --release --example long_wave
```

That example reports median and p95 render times and SVG sizes for long signals.

## Library

Use the Rust library for in-process SVG rendering, or the [CLI from another language or an AI agent](#use-from-other-languages-and-ai-agents). Both use the same parser, layout, and SVG writers for all four diagram types; callers supply the description, not coordinates or wire routes.

The library provides one-shot rendering, a reusable `Diagram`, an optional `live::Watcher`, and an optional native `view::run` entry point. The CLI uses these same implementations. Parsed models, layout geometry, and drawing primitives stay private so applications do not depend on routing internals. There is no public graph-builder, Python binding, JavaScript binding, or JSON protocol.

```rust
let svg = tgw::render("@wvf\nclk: P...\ndata: x3.4 => ready done")?;
let mut buffer = Vec::new();
tgw::render_into("@wvf\nclk: p...", &mut buffer)?;
let editable = tgw::to_tgw("clk: p...")?;
# Ok::<(), tgw::Error>(())
```

| Function | Result | Use |
| --- | --- | --- |
| `render(source)` | `Result<String, Error>` | One compact, light SVG. |
| `render_into(source, out)` | `Result<(), Error>` | The same SVG in a reusable `Vec<u8>`. |
| `render_opts(source, out, indent)` | `Result<(), Error>` | Light SVG with `u8` indentation; 0 is compact. |
| `render_themed(source, out, indent, scheme)` | `Result<(), Error>` | SVG with a `&'static scheme::Scheme`, including `LIGHT` and `DARK`. |
| `to_tgw(source)` | `Result<String, Error>` | Canonical source, not SVG or a translation to another diagram type. |
| `Diagram::parse(source)` | `Result<Diagram, Error>` | Owned parsed source with lazily cached layout for repeated rendering. |

`render`, `render_into`, and `to_tgw` accept all four pictures. `render_opts` indents the SVG. `render_themed` takes `scheme::LIGHT` or `scheme::DARK`. `render` and `render_opts` always write `scheme::LIGHT`, which is also the palette of a saved file. On error the output buffer is cleared. Invalid paths, excessive nesting, non-finite timing, and out-of-range geometry return `Error` with a byte offset and a message.

A program that only renders can depend on the library with `default-features = false`.

### Add the library to a Rust application

The package is not published to crates.io (`publish = false`). Use a Git dependency, or a path dependency when the repository is checked out beside your application:

```toml
[dependencies]
tgw = { git = "https://github.com/eaSyMaxSU/TheGreatWave", default-features = false }
# Local alternative (use this instead of the Git dependency):
# tgw = { path = "../TheGreatWave", default-features = false }
```

For reproducible builds, commit your application's `Cargo.lock`; a Git dependency can also be pinned with `rev` to a chosen commit. The rendering library uses no third-party dependencies with default features disabled.

Keep `default-features = false` on rendering dependencies: the package defaults include the native viewer and its platform dependencies. The repository's `rust-toolchain.toml` selects a compiler for this checkout, not for downstream applications; use a compatible Rust toolchain in your application too.

This complete program reads a source file, writes an SVG, and writes a separately named canonical source file:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string("handshake.tgw")?;
    let svg = tgw::render(&source)?;
    std::fs::write("handshake.svg", svg)?;

    let canonical = tgw::to_tgw(&source)?;
    std::fs::write("handshake.formatted.tgw", canonical)?;
    Ok(())
}
```

### Render any of the four diagram types

No per-diagram API or manual layout is needed. This complete example reuses one output buffer and writes four standalone SVGs:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let diagrams = [
        ("timing.svg", "@wvf\nclk: P...\ndata: x3.x => payload\n"),
        ("controller.svg", "@asm\nidle:\n  ? start\n    0 idle\n    1 done\ndone:\n  valid=1\n"),
        ("schedule.svg", "@hls\n0:\n  + a b -> sum\n1:\n  * sum c -> result\n"),
        ("gates.svg", "@gtl\nand enable valid -> select\nmux select a b -> y\n"),
    ];
    let mut svg = Vec::new();
    for (path, source) in diagrams {
        tgw::render_into(source, &mut svg)?;
        std::fs::write(path, &svg)?;
    }
    Ok(())
}
```

Display the output with `<img src="timing.svg" alt="Clock and payload timing">`, or open it directly in a browser. Preserve the `viewBox` when resizing SVGs. Text remains text and uses the reader's available fonts, so check the target font environment when exact typography matters.

### Reuse a buffer and choose a palette

All buffer-writing APIs clear the supplied `Vec<u8>` before rendering; they replace its contents rather than append another document. Reuse the allocation when rendering several diagrams:

```rust
fn main() -> Result<(), tgw::Error> {
    let source = "@gtl\nxor a b -> result\n";
    let mut svg = Vec::new();

    tgw::render_into(source, &mut svg)?; // Compact, light SVG.
    tgw::render_opts(source, &mut svg, 2)?; // Indented, light SVG.
    tgw::render_themed(source, &mut svg, 0, &tgw::scheme::DARK)?;
    assert!(svg.starts_with(b"<svg"));
    Ok(())
}
```

The CLI saves light SVGs even when the viewer is dark. Use `render_themed` in an application when you need a dark SVG export.

### Parse once and reuse layout

Retain a `Diagram` when rendering the same source repeatedly or in several palettes. The first render computes layout; subsequent renders reuse it. Each call still writes a fresh SVG into the caller's buffer. The diagram owns its data, can be shared across threads with `Arc<Diagram>`, and does not retain the caller's output buffer.

```rust
fn main() -> Result<(), tgw::Error> {
    let diagram = tgw::Diagram::parse("@gtl\nxor a b -> result\n")?;
    let mut svg = Vec::new();
    diagram.render_into(&mut svg)?;
    diagram.render_themed(&mut svg, 0, &tgw::scheme::DARK)?;
    diagram.render_opts(&mut svg, 2)?; // Reuse layout for an indented light SVG.
    let canonical = diagram.to_tgw();
    assert!(canonical.starts_with("@gtl"));
    Ok(())
}
```

Create a new `Diagram` when source changes. Parsing does not validate every timing geometry constraint; check the render result too. Both parse and render errors use byte offsets into the original source, including a leading UTF-8 BOM. A failed render clears the output buffer.

### Watch from a Rust application

Enable the watcher without GUI dependencies:

```toml
[dependencies]
tgw = { git = "https://github.com/eaSyMaxSU/TheGreatWave", default-features = false, features = ["watch"] }
```

`Job::new(input, output, indent)` validates paths and prevents exporting over the input. Use `None` for the output to receive SVGs without writing a file. `Watcher::next_update(timeout)` returns the initial load immediately, then waits for changes. It follows rename saves, falls back to polling, and retries failed exports. Unchanged source reuses its parsed diagram and SVG; indentation formats that SVG instead of rendering again.

This example exits after 30 seconds without an update. A host can instead loop with a shorter timeout and check its own cancellation flag between calls:

```rust
use std::time::Duration;
use tgw::live::{Job, Loaded, Watcher};

fn main() -> Result<(), String> {
    let job = Job::new("handshake.tgw".into(), Some("handshake.svg".into()), 2)?;
    let mut watcher = Watcher::new(job);
    while let Some(update) = watcher.next_update(Duration::from_secs(30)) {
        match update {
            Loaded::Text { rendered: Ok(snapshot), written, .. } => {
                println!("{} SVG bytes", snapshot.svg.len());
                // Retain this Arc<Snapshot> if your UI needs the last good picture.
                if let Some(Err(error)) = written {
                    eprintln!("{error}");
                }
            }
            Loaded::Text { rendered: Err(error), .. } => eprintln!("{error}"),
            Loaded::Unreadable(error) => eprintln!("{error}"),
        }
    }
    drop(watcher); // Releases the OS watch and joins its notification thread.
    Ok(())
}
```

A successful `Snapshot` contains `source`, compact light `svg`, and the reusable `diagram`. Invalid or missing input does not overwrite the exported file. The application owns the last good snapshot and decides how to display errors. `written` separately reports export success, no change, or an I/O error. The watcher API does not log or terminate the process.

Calls are blocking: the timeout bounds waiting for an update, not reading a file that is still being saved, rendering, or exporting it. Use a worker thread in GUI and async applications. Dropping the watcher stops notifications; no detached watch thread remains alive.

### Open the native viewer from Rust

Enable `features = ["view"]` on the dependency (`view` includes `watch`) and install the [platform dependencies](#platforms). **Also copy the complete `[patch.crates-io]` block from this revision's [Cargo.toml](Cargo.toml) into your application's workspace-root manifest.** Cargo does not inherit dependency patches; these five pinned GPUI Fast patches are required for the viewer. Renderer-only and watcher-only applications do not need them.

```rust
fn main() -> Result<(), String> {
    let job = tgw::live::Job::new("handshake.tgw".into(), None, 0)?;
    tgw::view::run(job)
}
```

Call `view::run` once on the main thread. It owns the native application event loop; it is not a widget for an already-running GUI application. It returns launch errors instead of calling `process::exit`. The window retains the last good diagram through invalid edits and reuses its layout when switching palettes. To export alongside viewing, pass an output path to `Job::new`. For a custom UI, use `Diagram` or `live::Watcher` and display their SVG output yourself.

### Handle diagnostics

Library errors expose `offset` (a byte offset into the input, not a character index) and `message`. This matters for UTF-8 names: do not treat a byte offset as a column number without accounting for the preceding text.

```rust
fn main() {
    let source = "@gtl\nnot a b -> y\n"; // NOT takes only one input.
    match tgw::render(source) {
        Ok(svg) => println!("{svg}"),
        Err(error) => eprintln!("byte {}: {}", error.offset, error.message),
    }
}
```

Use the CLI when you want ready-made `path:line:column` diagnostics. Embed the library when your application needs the SVG string, custom diagnostics, or direct control over the output buffer and palette.

### Use from other languages and AI agents

The portable integration is a subprocess: send UTF-8 `.tgw` text on stdin and read SVG from stdout. Pass arguments as an array rather than interpolating generated text into a shell command. For example, with `tgw` on `PATH`:

```python
import subprocess

source = "@gtl\nand enable valid -> select\nmux select a b -> y\n"
result = subprocess.run(
    ["tgw", "-"],
    input=source,
    text=True,
    encoding="utf-8",
    capture_output=True,
    timeout=10,
    check=False,
)
if result.returncode != 0:
    raise RuntimeError(result.stderr.strip())
svg = result.stdout
```

For agent tool calls:

1. Choose `@wvf`, `@asm`, `@hls`, or `@gtl` and generate only that diagram's source. Use the language sections above for syntax and limits; do not invent layout directives.
2. Render once. Treat exit status 1 as a failed render, and use stderr to correct the source before retrying. Do not treat partial or empty stdout as a successful SVG.
3. Optionally use `--convert` to save canonical source alongside the SVG. Conversion is not a replacement for rendering: some timing geometry is validated during rendering.
4. For an interactive editing session, use `--watch -o PATH` or `--view` with a real source file. These are long-running processes; the host owns their lifetime.

Rendering and formatting APIs do no file I/O and do not start a watcher, window, or background thread. They are synchronous, so move large renders off an application's UI or async executor thread. Services accepting generated or untrusted input should impose input-size, concurrency, and execution limits; the diagram-specific caps are not a general resource budget. Use the built-in palettes unless custom paint values come from trusted application code.

### Performance and maintenance

Use `Diagram` to reuse parsing and layout, retain the output buffer to reuse its allocation, and keep indentation at 0 unless people need to read the SVG markup. The source-taking convenience functions still parse and lay out each call. Retaining a diagram uses memory for its model and layout until it is dropped; cache final SVG bytes instead when no further renders are needed.

Long repeated clocks and held levels have compact output, and `@bounds` limits visible timing geometry. Cropping does not make reading and parsing the input free. Measure both runtime and SVG size with the existing benchmark:

```sh
cargo run --locked --release --no-default-features --example long_wave
```

This benchmark compares one-shot rendering with warmed retained layouts for timing, ASM, HLS, and gate diagrams. It does not measure watcher save-to-output latency or viewer frame times. When changing shared rendering code, run the renderer-only, watcher, and default-feature checks in [Checks](#checks). Preserve the same SVG behavior across CLI and library callers, and use the layout/pixel tests plus browser inspection for readability; performance measurements alone cannot detect crossed wires or clipped labels.

## Builds

| Command | What you get |
| --- | --- |
| `cargo build --release` | Renderer, `--watch`, and `--view` |
| `cargo build --release --no-default-features --features watch` | Renderer and `--watch`, no GUI libraries |
| `cargo build --release --no-default-features` | Renderer only, no dependencies |

`tgw --help` says when a flag is missing from the build. The window uses [GPUI Fast](https://github.com/longbridge/gpui-fast) through GPUI Kit. `Cargo.toml` patches Kit's `gpui-pre-*` dependencies to a pinned fork revision whose compatibility shims match Kit 0.7.0's snapshot version (0.3.7). When upgrading Kit, update all five patches together to a fork revision with matching shim versions, then regenerate `Cargo.lock`. `rust-toolchain.toml` pins the compiler.

The release workflow builds and tests all three native targets with default features and `--locked`. Run the **Release** workflow with a new tag such as `v0.1.0`, or push a `v*` tag; the tag must match the package version in `Cargo.toml`. Only after all targets pass does it publish the archives and checksums to GitHub Releases. A manual run creates its tag at the tested commit when publishing. Desktop-only viewer tests remain opt-in; the automated release tests exercise rendering and file watching, and the packager smoke-tests the copied executable outside the source tree.

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

## Troubleshooting

| Symptom | What to check |
| --- | --- |
| `tgw` is not found | Put the executable on `PATH`, or use its explicit path (`./tgw` on Unix, `.\tgw.exe` in PowerShell). |
| The command waits without producing output | With no input file, tgw reads stdin until EOF. Supply a file or pipe input to it. |
| `--view` or `--watch` is unavailable | Check `tgw --help`; renderer-only builds omit both, and watcher-only builds omit the window. Rebuild with the appropriate features or use the combined release binary. |
| The viewer cannot open on Linux | Check for a working X11 or Wayland session, required shared libraries, and a Vulkan driver. On a headless server, render once or use `--watch`. |
| A save leaves the old picture visible | Read the viewer status bar or watcher stderr. Invalid input, a missing source, or a failed output write keeps the last good picture. Fix the problem and save again. |
| Writing the SVG fails | Create the output directory first and check permissions. A one-off output must not refer to the source diagram itself. |
| A `#` unexpectedly removes part of a label | Quote text containing `#`. Use `#` for comments, not `//`. |
| An ASM decision is rejected | Include exactly one `0` and one `1` exit at the same indent, one level below the decision. |
| An HLS result is not ready | A result from cycle `C` with latency `N` can first be read in cycle `C + N`, never in the cycle that starts its producer. |
| A netlist reports a forward reference | Put each producer before its consumers; loops and self-references are rejected. Check mux input order too. |

When reporting a problem, include `tgw --version`, your OS, the build features if building from source, the exact command, the complete diagnostic, and the smallest `.tgw` file that reproduces it. For a viewer-only problem, note whether a one-off SVG render succeeds.

## Icon

`assets/icon/tgw.svg` is the master: a square pulse that becomes Hokusai's wave. The Dock PNG and the Windows ICO are generated from it.

```sh
cargo run --example icon
```

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

On headless Linux with Xvfb and a Vulkan software driver installed, run the native-window tests with `xvfb-run -a cargo test --locked --test watch -- --ignored --test-threads=1`. Orb setup installs the required virtual-display tools.

The three Clippy lines, and the matching tests, are the default build, the headless watcher, and the renderer alone. The renderer is also tested in release. The default `cargo test --locked` includes the 1:1 screenshot checks for ASM charts, HLS schedules, and gate netlists.

`tests/fixtures` holds diagrams and their SVG snapshots. The original eight timing diagrams must render the same picture under CRLF. Further snapshots cover the wave alphabet, fractional ticks, period and phase, markup, Unicode, a cropped bus, SVG paths, an empty diagram, and nested groups. `tests/fixtures/asm/`, `tests/fixtures/hls/`, and `tests/fixtures/gtl/` hold one edge case per file, plus `tests/fixtures/asm.tgw`, `tests/fixtures/hls.tgw`, and `tests/fixtures/gtl.tgw`. `tests/watch.rs` drives the real binary through in-place saves, a broken save, deletion, and a rename, then through every edge-case picture, including the chart, the schedule, and the netlist, for `--watch` and, when ignored tests run, for `--view`. The window tests also slice and raster those pictures the way the viewer does.

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

Wave rules, lane semantics, and the default bus palette originate in [WaveDrom](https://wavedrom.com/) by Aliaksei Chapyzhenka (Copyright 2011–2026), which is MIT licensed ([source](https://github.com/wavedrom/wavedrom)). SVG arc geometry follows the [SVG 2 arc implementation notes](https://www.w3.org/TR/SVG2/implnote.html#ArcImplementationNotes).
