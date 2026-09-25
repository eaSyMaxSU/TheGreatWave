# The Great Wave (tgw)

tgw renders WaveJSON signal timing diagrams to SVG. A lane is a list of runs — a level and a length — and the picture is patterns, rectangles, and a short edge where the level changes. A 10,000-cycle clock is one pattern and one rectangle.

Wave rules and the default palette come from WaveDrom (Copyright 2011–2026 Aliaksei Chapyzhenka). This crate is MIT licensed.

## Build

```
cargo test
cargo run --release --example long_wave
cargo run --release -- --input diagram.json5
```

Omit `--input` to read WaveJSON from stdin. `--indent N` pretty-prints the SVG. `-h` and `-v` print help and the version.

## Library

```rust
let svg = tgw::render(source)?;
let mut buf = Vec::new();
tgw::render_into(source, &mut buf)?;
```

`render_into` clears and refills a caller-owned buffer. The accepted source is the JSON5 subset used by timing diagrams: objects, arrays, strings, numbers, `true` / `false` / `null`, unquoted keys, single quotes, comments, and trailing commas.

## What it draws

Signal lanes, nested groups, `head` / `foot`, `config.hscale` (capped at 100), `hbounds`, `marks`, nodes, and edges. The wave alphabet is `p n P N h l H L 0 1 x d u z = 2-9`, with `.` repeating, `|` marking a gap, and `<` `>` for sub-cycles. `period`, `phase`, and `data` stay aligned with the usual marker rules. Register and assign diagrams are not part of tgw.
