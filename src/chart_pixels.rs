//! 1:1 rasters of every ASM chart, HLS schedule, and gate netlist fixture.
//!
//! Card and pill borders are solid on whole pixels, the paper inside them is
//! solid, and every straight run of a wire covers exactly two whole pixel
//! rows or columns. An integer window scale and a viewer slice keep that.
//! The rasters are saved under `target/asm-shots`, `target/hls-shots`, and
//! `target/gtl-shots`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::{size, DevicePixels, SvgRenderer, SvgSize};
use image::RgbaImage;

use crate::layout::{diagram_frame, layout_diagram};
use crate::slice::slice_svg;

#[test]
fn asm_charts_are_pixel_aligned() {
    check("asm");
}

#[test]
fn hls_schedules_are_pixel_aligned() {
    check("hls");
}

#[test]
fn gtl_netlists_are_pixel_aligned() {
    check("gtl");
}

fn check(kind: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut sources = vec![
        root.join(format!("tests/fixtures/{kind}.tgw")),
        root.join(format!("examples/{kind}.tgw")),
    ];
    let mut edges: Vec<_> = fs::read_dir(root.join(format!("tests/fixtures/{kind}")))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "tgw"))
        .collect();
    edges.sort();
    sources.extend(edges);

    let shots = root.join(format!("target/{kind}-shots"));
    let _ = fs::remove_dir_all(&shots);
    fs::create_dir_all(&shots).unwrap();
    let renderer = SvgRenderer::new(Arc::new(()));

    for path in &sources {
        let stem = path.file_stem().unwrap().to_string_lossy();
        let name = if path.components().any(|part| part.as_os_str() == "examples") {
            format!("example-{stem}")
        } else {
            stem.into_owned()
        };
        let source = fs::read_to_string(path).unwrap();
        let svg = tgw::render(&source).unwrap_or_else(|error| panic!("{name}: {error}"));
        check_svg(&renderer, &svg, &tgw::scheme::LIGHT, &name, &shots);

        let image = raster(&renderer, &svg);
        let (width, height) = svg_size(&svg);
        let origin = translate_of(&svg);
        let frame = diagram_frame(&svg).unwrap();
        let layout = layout_diagram(&frame, 2000.0, 1600.0, 0.0, 0.0).unwrap();
        assert_eq!(layout.scale.fract(), 0.0, "{name}");
        assert!(layout.scale >= 1.0, "{name}");
        assert!(!layout.show_h && !layout.show_v, "{name}");
        let Some(card) = framed(&svg).into_iter().next() else {
            continue;
        };
        let (x, y) = (
            (origin.0 + card.x + (card.rx + 1).max(card.w / 4)) as u32,
            (origin.1 + card.y) as u32,
        );
        let span = layout.scale.round() as u32;
        let magnified = raster_at(&renderer, &svg, width * span, height * span);
        for row in 0..span {
            for col in 0..span {
                assert_eq!(
                    magnified.get_pixel(x * span + col, y * span + row).0,
                    image.get_pixel(x, y).0,
                    "{name} scaled border"
                );
            }
        }
        let sliced = slice_svg(
            &svg,
            layout.wave.x,
            layout.wave.y,
            layout.wave.w,
            layout.wave.h,
        )
        .unwrap();
        let window = raster(&renderer, &sliced);
        let col = (x as f32 - layout.wave.x).round() as u32;
        let row = (y as f32 - layout.wave.y).round() as u32;
        assert_eq!(
            window.get_pixel(col, row).0,
            image.get_pixel(x, y).0,
            "{name} sliced border"
        );
    }

    let example = fs::read_to_string(root.join(format!("examples/{kind}.tgw"))).unwrap();
    let mut dark = Vec::new();
    tgw::render_themed(&example, &mut dark, 0, &tgw::scheme::DARK).unwrap();
    let dark = String::from_utf8(dark).unwrap();
    check_svg(&renderer, &dark, &tgw::scheme::DARK, "example-dark", &shots);
}

fn check_svg(
    renderer: &SvgRenderer,
    svg: &str,
    scheme: &tgw::scheme::Scheme,
    name: &str,
    shots: &Path,
) {
    let image = raster(renderer, svg);
    let (width, height) = svg_size(svg);
    assert_eq!((image.width(), image.height()), (width, height), "{name}");
    let paper = hex(scheme.paper);
    assert_eq!(
        image.get_pixel(0, 0).0,
        [paper[0], paper[1], paper[2], 255],
        "{name} page"
    );
    let origin = translate_of(svg);
    let cards = framed(svg);
    for card in &cards {
        let (x, y) = (origin.0 + card.x, origin.1 + card.y);
        // Off centre, where no wire lands on a border, and clear of the corners.
        let at_x = x + (card.rx + 1).max(card.w / 4);
        let mid_y = y + card.h / 2;
        expect(&image, at_x, y, card.ink, name, "top border");
        expect(
            &image,
            at_x,
            y + card.h - 1,
            card.ink,
            name,
            "bottom border",
        );
        expect(
            &image,
            at_x,
            y + card.h - 2,
            card.fill,
            name,
            "paper over the bottom border",
        );
        if card.rx * 2 < card.h {
            expect(&image, x, mid_y, card.ink, name, "left border");
            expect(
                &image,
                x + card.w - 1,
                mid_y,
                card.ink,
                name,
                "right border",
            );
            expect(
                &image,
                x + 1,
                mid_y,
                card.fill,
                name,
                "paper inside the left border",
            );
        }
    }
    let wires = wire_runs(svg);
    let lone = wires_only(svg);
    let bare = raster(renderer, &lone);
    let inks: Vec<[u8; 3]> = wires.iter().map(|run| run.ink).collect();
    for run in &wires {
        // Each straight run covers the two pixels on either side of its centre line.
        let (mx, my) = (
            origin.0 + (run.x0 + run.x1) / 2,
            origin.1 + (run.y0 + run.y1) / 2,
        );
        let covered = if run.y0 == run.y1 {
            [(mx, my - 1), (mx, my)]
        } else {
            [(mx - 1, my), (mx, my)]
        };
        for (px, py) in covered {
            let pixel = bare.get_pixel(px as u32, py as u32).0;
            assert!(
                pixel[3] == 255 && inks.iter().any(|ink| pixel[..3] == ink[..]),
                "{name} wire {run:?} at {px},{py} is {pixel:?}"
            );
        }
    }
    if !["<desc>0 cycles", "<desc>0 gates", "<desc>0 states"]
        .iter()
        .any(|empty| svg.contains(empty))
    {
        assert!(!cards.is_empty(), "{name} drew no card or pill");
    }
    image.save(shots.join(format!("{name}.png"))).unwrap();
}

fn expect(image: &RgbaImage, x: i64, y: i64, rgb: [u8; 3], name: &str, what: &str) {
    let pixel = image.get_pixel(x as u32, y as u32).0;
    assert_eq!(
        pixel,
        [rgb[0], rgb[1], rgb[2], 255],
        "{name} {what} at {x},{y} is {:02x}{:02x}{:02x}",
        pixel[0],
        pixel[1],
        pixel[2]
    );
}

#[derive(Debug)]
struct Card {
    x: i64,
    y: i64,
    w: i64,
    h: i64,
    rx: i64,
    ink: [u8; 3],
    fill: [u8; 3],
}

/// An ink rectangle followed by its fill inset by one pixel.
fn framed(svg: &str) -> Vec<Card> {
    let body = svg.split("<g transform=\"translate").nth(1).unwrap_or("");
    let rects: Vec<&str> = body
        .split("<rect ")
        .skip(1)
        .map(|tag| tag.split('>').next().unwrap())
        .filter(|tag| !tag.contains("fill-opacity"))
        .collect();
    let num = |tag: &str, key: &str| {
        attr(tag, key)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0)
    };
    let mut cards = Vec::new();
    for pair in rects.windows(2) {
        let (outer, inner) = (pair[0], pair[1]);
        let (x, y, w, h) = (
            num(outer, "x"),
            num(outer, "y"),
            num(outer, "width"),
            num(outer, "height"),
        );
        if w < 6 || h < 6 {
            continue;
        }
        if num(inner, "x") == x + 1
            && num(inner, "y") == y + 1
            && num(inner, "width") == w - 2
            && num(inner, "height") == h - 2
        {
            cards.push(Card {
                x,
                y,
                w,
                h,
                rx: num(outer, "rx"),
                ink: hex(attr(outer, "fill").unwrap()),
                fill: hex(attr(inner, "fill").unwrap()),
            });
        }
    }
    cards
}

#[derive(Debug)]
struct Run {
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    ink: [u8; 3],
}

fn is_wire(tag: &str) -> bool {
    tag.contains("fill=\"none\"") && tag.contains("stroke-width=\"2\"")
}

/// Straight runs at least 6px long in every wire, without the corners.
fn wire_runs(svg: &str) -> Vec<Run> {
    let mut runs = Vec::new();
    for tag in svg.split("<path ").skip(1) {
        let tag = tag.split('>').next().unwrap();
        if !is_wire(tag) {
            continue;
        }
        let ink = hex(attr(tag, "stroke").unwrap());
        let d = attr(tag, "d").unwrap();
        let mut at = (0i64, 0i64);
        let mut rest = d;
        while let Some(command) = rest.chars().next() {
            let end = rest[1..]
                .find(['M', 'L', 'Q'])
                .map_or(rest.len(), |i| i + 1);
            let nums: Vec<i64> = rest[1..end]
                .split([' ', ','])
                .filter(|part| !part.is_empty())
                .map(|part| part.parse().unwrap())
                .collect();
            let to = (nums[nums.len() - 2], nums[nums.len() - 1]);
            if command == 'L'
                && (at.0 == to.0 || at.1 == to.1)
                && (at.0 - to.0).abs() + (at.1 - to.1).abs() >= 6
            {
                runs.push(Run {
                    x0: at.0,
                    y0: at.1,
                    x1: to.0,
                    y1: to.1,
                    ink,
                });
            }
            at = to;
            rest = &rest[end..];
        }
    }
    runs
}

/// The same picture with only its wires, so a crossing card cannot hide a run.
fn wires_only(svg: &str) -> String {
    let start = svg.find("<g transform=\"translate").unwrap();
    let open = start + svg[start..].find('>').unwrap() + 1;
    let mut out = svg[..open].to_string();
    for tag in svg[open..].split("<path ").skip(1) {
        let tag = tag.split("/>").next().unwrap();
        if is_wire(tag) {
            out.push_str("<path ");
            out.push_str(tag);
            out.push_str("/>");
        }
    }
    out.push_str("</g></svg>");
    out
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let rest = tag
        .split(&key)
        .nth(1)
        .or_else(|| tag.strip_prefix(&key[1..]))?;
    rest.split('"').next()
}

fn translate_of(svg: &str) -> (i64, i64) {
    let key = "transform=\"translate(";
    let rest = &svg[svg.find(key).unwrap() + key.len()..];
    let body = &rest[..rest.find(')').unwrap()];
    let mut parts = body.split(',');
    let x = parts.next().unwrap().trim().parse().unwrap();
    let y = parts.next().unwrap().trim().parse().unwrap();
    (x, y)
}

fn svg_size(svg: &str) -> (u32, u32) {
    let tag = &svg[..=svg.find('>').unwrap()];
    let width = attr(tag, "width").unwrap().parse::<f32>().unwrap().round() as u32;
    let height = attr(tag, "height").unwrap().parse::<f32>().unwrap().round() as u32;
    (width, height)
}

fn hex(token: &str) -> [u8; 3] {
    let hex = token.trim_start_matches('#');
    let hex = if hex.len() == 3 {
        hex.chars().flat_map(|ch| [ch, ch]).collect::<String>()
    } else {
        hex.to_string()
    };
    let value = u32::from_str_radix(&hex, 16).unwrap();
    [(value >> 16) as u8, (value >> 8) as u8, value as u8]
}

fn raster(renderer: &SvgRenderer, svg: &str) -> RgbaImage {
    let (width, height) = svg_size(svg);
    raster_at(renderer, svg, width, height)
}

fn raster_at(renderer: &SvgRenderer, svg: &str, width: u32, height: u32) -> RgbaImage {
    let parsed = renderer.parse_svg(svg.as_bytes()).unwrap();
    let image = renderer
        .render_parsed(
            &parsed,
            SvgSize::ExactSize(size(
                DevicePixels(width as i32),
                DevicePixels(height as i32),
            )),
        )
        .unwrap();
    let mut pixels = image.as_bytes(0).unwrap().to_vec();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    RgbaImage::from_raw(width, height, pixels).unwrap()
}
