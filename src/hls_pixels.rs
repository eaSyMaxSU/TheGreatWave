//! 1:1 rasters of every HLS fixture. Rules, wires, and box borders are solid
//! ink on whole pixels, and an integer window scale keeps that.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::{size, DevicePixels, SvgRenderer, SvgSize};
use image::{Rgba, RgbaImage};

use crate::layout::{diagram_frame, layout_diagram};
use crate::slice::slice_svg;

#[test]
fn hls_schedules_are_pixel_aligned() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    sources.push(root.join("tests/fixtures/hls.tgw"));
    sources.push(root.join("examples/hls.tgw"));
    let mut edges: Vec<_> = fs::read_dir(root.join("tests/fixtures/hls"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "tgw"))
        .collect();
    edges.sort();
    sources.extend(edges);

    let shots = root.join("target/hls-shots");
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
        let image = raster(&renderer, &svg);
        let (width, height) = svg_size(&svg);
        assert_eq!(image.width(), width);
        assert_eq!(image.height(), height);
        let origin = translate_of(&svg);
        assert_page_and_boxes(&image, &svg, origin, [0, 0, 0], [255, 255, 255], &name);
        save_shots(&shots, &name, &image, &svg, origin);

        let frame = diagram_frame(&svg).unwrap();
        let layout = layout_diagram(&frame, 2000.0, 1600.0, 0.0, 0.0).unwrap();
        assert_eq!(layout.scale.fract(), 0.0, "{name}");
        assert!(layout.scale >= 1.0, "{name}");
        assert!(!layout.show_h && !layout.show_v, "{name}");
        let span = layout.scale.round() as u32;
        let magnified = raster_at(&renderer, &svg, width * span, height * span);
        assert_scaled_border(&image, &magnified, origin, &svg, span, &name);
        let sliced = slice_svg(
            &svg,
            layout.wave.x,
            layout.wave.y,
            layout.wave.w,
            layout.wave.h,
        )
        .unwrap();
        let window = raster(&renderer, &sliced);
        assert_slice_keeps_border(
            &image,
            &window,
            &svg,
            origin,
            layout.wave.x,
            layout.wave.y,
            &name,
        );
    }

    let schedule = fs::read_to_string(root.join("examples/hls.tgw")).unwrap();
    let mut dark = Vec::new();
    tgw::render_themed(
        &schedule,
        &mut dark,
        0,
        tgw::InputFormat::Tgw,
        &tgw::scheme::DARK,
    )
    .unwrap();
    let dark_svg = String::from_utf8(dark).unwrap();
    let dark_image = raster(&renderer, &dark_svg);
    assert_page_and_boxes(
        &dark_image,
        &dark_svg,
        translate_of(&dark_svg),
        hex(tgw::scheme::DARK.ink),
        hex(tgw::scheme::DARK.paper),
        "dark",
    );
    dark_image.save(shots.join("schedule-dark.png")).unwrap();
}

fn assert_scaled_border(
    native: &RgbaImage,
    scaled: &RgbaImage,
    origin: (i64, i64),
    svg: &str,
    span: u32,
    name: &str,
) {
    let Some(rect) = first_box(svg) else {
        return;
    };
    let x = (origin.0 + rect.0) as u32;
    let y = (origin.1 + rect.1) as u32;
    let native_px = native.get_pixel(x, y).0;
    for row in 0..span {
        for col in 0..span {
            let pixel = scaled.get_pixel(x * span + col, y * span + row);
            assert_eq!(
                pixel.0,
                native_px,
                "{name} scaled border at {},{}",
                x * span + col,
                y * span + row
            );
        }
    }
}

fn assert_slice_keeps_border(
    native: &RgbaImage,
    sliced: &RgbaImage,
    svg: &str,
    origin: (i64, i64),
    view_x: f32,
    view_y: f32,
    name: &str,
) {
    let Some(rect) = first_box(svg) else {
        return;
    };
    let x = (origin.0 + rect.0) as u32;
    let y = (origin.1 + rect.1) as u32;
    let col = (x as f32 - view_x).round() as u32;
    let row = (y as f32 - view_y).round() as u32;
    assert_eq!(
        sliced.get_pixel(col, row).0,
        native.get_pixel(x, y).0,
        "{name} sliced border"
    );
}

fn assert_page_and_boxes(
    image: &RgbaImage,
    svg: &str,
    origin: (i64, i64),
    ink: [u8; 3],
    paper: [u8; 3],
    name: &str,
) {
    assert_eq!(
        image.get_pixel(0, 0).0,
        [paper[0], paper[1], paper[2], 255],
        "{name} page"
    );
    let rects = rects_in_group(svg);
    let mut boxes = 0;
    let mut bars = 0;
    for rect in &rects {
        let x = origin.0 + rect.x;
        let y = origin.1 + rect.y;
        if rect.w == 1 || rect.h == 1 {
            bars += 1;
            fill_is(image, x, y, rect.w, rect.h, ink, name);
            continue;
        }
        let inner = rects.iter().find(|other| {
            other.x == rect.x + 1
                && other.y == rect.y + 1
                && other.w == rect.w - 2
                && other.h == rect.h - 2
                && other.fill == paper
        });
        if rect.fill == ink && inner.is_some() {
            boxes += 1;
            ring_is(image, x, y, rect.w, rect.h, ink, name);
            assert_eq!(
                image.get_pixel((x + 1) as u32, (y + 1) as u32).0,
                [paper[0], paper[1], paper[2], 255],
                "{name} paper corner"
            );
        }
    }
    if !svg.contains("<desc>0 cycles") && name != "dark" {
        assert!(boxes > 0, "{name} drew no operator");
        assert!(bars > 0, "{name} drew no rule");
    }
}

fn fill_is(image: &RgbaImage, x: i64, y: i64, w: i64, h: i64, rgb: [u8; 3], name: &str) {
    for row in y..y + h {
        for col in x..x + w {
            let pixel = image.get_pixel(col as u32, row as u32);
            assert_eq!(
                pixel.0,
                [rgb[0], rgb[1], rgb[2], 255],
                "{name} {col},{row} is {:02x}{:02x}{:02x}",
                pixel[0],
                pixel[1],
                pixel[2]
            );
        }
    }
}

fn ring_is(image: &RgbaImage, x: i64, y: i64, w: i64, h: i64, rgb: [u8; 3], name: &str) {
    for col in x..x + w {
        expect(image, col, y, rgb, name);
        expect(image, col, y + h - 1, rgb, name);
    }
    for row in y..y + h {
        expect(image, x, row, rgb, name);
        expect(image, x + w - 1, row, rgb, name);
    }
}

fn expect(image: &RgbaImage, x: i64, y: i64, rgb: [u8; 3], name: &str) {
    let pixel = image.get_pixel(x as u32, y as u32);
    assert_eq!(
        pixel.0,
        [rgb[0], rgb[1], rgb[2], 255],
        "{name} border {x},{y} is {:02x}{:02x}{:02x}",
        pixel[0],
        pixel[1],
        pixel[2]
    );
}

fn save_shots(dir: &std::path::Path, name: &str, image: &RgbaImage, svg: &str, origin: (i64, i64)) {
    image.save(dir.join(format!("{name}.png"))).unwrap();
    let Some(rect) = first_box(svg) else {
        return;
    };
    let x = (origin.0 + rect.0 - 4).max(0) as u32;
    let y = (origin.1 + rect.1 - 4).max(0) as u32;
    let side = 28;
    let mut zoom = RgbaImage::new(side * 4, side * 4);
    for row in 0..side {
        for col in 0..side {
            let px = image.get_pixel(x + col, y + row).0;
            for dy in 0..4 {
                for dx in 0..4 {
                    zoom.put_pixel(col * 4 + dx, row * 4 + dy, Rgba(px));
                }
            }
        }
    }
    zoom.save(dir.join(format!("{name}-corner.png"))).unwrap();
}

struct Rect {
    x: i64,
    y: i64,
    w: i64,
    h: i64,
    fill: [u8; 3],
}

fn first_box(svg: &str) -> Option<(i64, i64, i64, i64)> {
    rects_in_group(svg).into_iter().find_map(|rect| {
        (rect.w > 2 && rect.h > 2 && rect.fill == [0, 0, 0])
            .then_some((rect.x, rect.y, rect.w, rect.h))
    })
}

fn rects_in_group(svg: &str) -> Vec<Rect> {
    let Some(body) = svg.split("<g transform=\"translate").nth(1) else {
        return Vec::new();
    };
    let mut rects = Vec::new();
    for tag in body.split("<rect ").skip(1) {
        let tag = tag.split('>').next().unwrap();
        let Some(fill) = attr(tag, "fill") else {
            continue;
        };
        rects.push(Rect {
            x: attr(tag, "x").unwrap_or("0").parse().unwrap_or(0),
            y: attr(tag, "y").unwrap_or("0").parse().unwrap_or(0),
            w: attr(tag, "width").unwrap_or("0").parse().unwrap_or(0),
            h: attr(tag, "height").unwrap_or("0").parse().unwrap_or(0),
            fill: hex(fill),
        });
    }
    rects
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let rest = tag.split(&key).nth(1)?;
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
        format!(
            "{a}{a}{b}{b}{c}{c}",
            a = &hex[0..1],
            b = &hex[1..2],
            c = &hex[2..3]
        )
    } else {
        hex.to_string()
    };
    let value = u32::from_str_radix(&hex, 16).unwrap();
    [
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    ]
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
