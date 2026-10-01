//! Cut a window out of a diagram and turn pattern fills into ordinary strokes.
//!
//! resvg samples a pattern for every pixel of its rectangle. A clock that is one
//! wide rectangle therefore costs as much as a long filled shape, even outside
//! the window. Clipping those rectangles, then repeating the tile, keeps the
//! picture the same and the paint small.

#[cfg(test)]
pub(crate) fn slice_svg(svg: &str, x: f32, y: f32, w: f32, h: f32) -> Option<String> {
    slice_visible(
        svg,
        x,
        y,
        w,
        h,
        LabelWindow {
            x0: x,
            y0: y,
            x1: x + w,
            y1: y + h,
        },
    )
}

/// `fit` is the whole on-screen picture. A label is drawn when it lies inside
/// that picture and meets this pane, so a tick on the split is not discarded.
pub(crate) fn slice_visible(
    svg: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fit: LabelWindow,
) -> Option<String> {
    let start = svg.find("<svg")?;
    svg[start..].find('>')?;
    let sliced = retain_whole_labels(
        svg,
        SliceRequest {
            clip: LabelWindow {
                x0: x,
                y0: y,
                x1: x + w,
                y1: y + h,
            },
            fit,
            root_w: w,
            root_h: h,
        },
    );
    // resvg paints a pattern by sampling every pixel. A clock that is one
    // wide rectangle therefore costs as much as a long filled shape, even
    // outside the window. Clip those rectangles, then repeat the tile as
    // ordinary strokes across only what is visible.
    Some(expand_patterns(&clip_paint_rects(&sliced)))
}

/// Rectangles that run far past the window are clipped in their own
/// coordinates. Pattern tiles stay put because their origin is user space.
fn clip_paint_rects(svg: &str) -> String {
    let Some((vx, vy, vw, vh)) = root_view_box(svg) else {
        return svg.to_string();
    };
    const SLACK: f64 = 64.0;
    let left = vx - SLACK;
    let top = vy - SLACK;
    let right = vx + vw + SLACK;
    let bottom = vy + vh + SLACK;
    let mut out = String::with_capacity(svg.len());
    let mut stack = vec![(0.0_f64, 0.0_f64, false)];
    // Clip paths, pattern tiles, and masks are in their own coordinates.
    // Rewriting those rects into the window's space crops the picture.
    let mut reserved = 0_i32;
    let mut i = 0;
    while i < svg.len() {
        if !svg[i..].starts_with('<') {
            let next = svg[i..].find('<').map_or(svg.len(), |offset| i + offset);
            out.push_str(&svg[i..next]);
            i = next;
            continue;
        }
        if svg[i..].starts_with("</g>") {
            if stack.len() > 1 {
                stack.pop();
            }
            out.push_str("</g>");
            i += 4;
            continue;
        }
        let Some(end) = svg[i..].find('>').map(|offset| i + offset + 1) else {
            out.push_str(&svg[i..]);
            break;
        };
        let tag = &svg[i..end];
        if tag_name_is(tag, "g") {
            let (tx, ty, complex) = *stack.last().unwrap();
            let (dx, dy) = translate_of(tag).unwrap_or((0.0, 0.0));
            let child_complex = complex || tag.contains("scale(") || tag.contains("rotate(");
            stack.push((tx + f64::from(dx), ty + f64::from(dy), child_complex));
            out.push_str(tag);
        } else if reserved == 0 && tag_name_is(tag, "rect") {
            let (tx, ty, complex) = *stack.last().unwrap();
            if let Some(rewritten) = (!complex)
                .then(|| clip_rect(tag, tx, ty, left, top, right, bottom))
                .flatten()
            {
                out.push_str(&rewritten);
            } else {
                out.push_str(tag);
            }
        } else {
            if closes_reserved(tag) {
                reserved = reserved.saturating_sub(1);
            } else if opens_reserved(tag) {
                reserved += 1;
            }
            out.push_str(tag);
        }
        i = end;
    }
    out
}

fn clip_rect(
    tag: &str,
    tx: f64,
    ty: f64,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
) -> Option<String> {
    let x = f64::from(attr_f32(tag, "x").unwrap_or(0.0));
    let y = f64::from(attr_f32(tag, "y").unwrap_or(0.0));
    let w = f64::from(attr_f32(tag, "width")?);
    let h = f64::from(attr_f32(tag, "height")?);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let x0 = x + tx;
    let y0 = y + ty;
    let x1 = x0 + w;
    let y1 = y0 + h;
    if x0 >= left && x1 <= right && y0 >= top && y1 <= bottom {
        return None;
    }
    let nx0 = x0.max(left);
    let ny0 = y0.max(top);
    let nx1 = x1.min(right);
    let ny1 = y1.min(bottom);
    if nx1 <= nx0 || ny1 <= ny0 {
        return Some(set_attr(&set_attr(tag, "width", "0"), "height", "0"));
    }
    Some(set_attr(
        &set_attr(
            &set_attr(
                &set_attr(tag, "x", &fmt_num((nx0 - tx) as f32)),
                "y",
                &fmt_num((ny0 - ty) as f32),
            ),
            "width",
            &fmt_num((nx1 - nx0) as f32),
        ),
        "height",
        &fmt_num((ny1 - ny0) as f32),
    ))
}

struct PatternTile {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    /// Content viewport. Absent when the tile body is already in tile-local space.
    view_box: Option<(f64, f64, f64, f64)>,
    body: String,
    attrs: String,
}

/// Repeats a pattern tile as strokes. resvg otherwise samples the tile with
/// bicubic filtering across every pixel of the rectangle.
fn expand_patterns(svg: &str) -> String {
    let mut tiles = std::collections::HashMap::<String, PatternTile>::new();
    let mut rest = svg;
    while let Some(start) = rest.find("<pattern ") {
        let Some(tag_end) = rest[start..].find('>').map(|offset| start + offset + 1) else {
            break;
        };
        let Some(close) = rest[tag_end..]
            .find("</pattern>")
            .map(|offset| tag_end + offset)
        else {
            break;
        };
        let tag = &rest[start..tag_end];
        if let Some(id) = attr(tag, "id") {
            let width = f64::from(attr_f32(tag, "width").unwrap_or(0.0));
            if width > 0.0 {
                tiles.insert(
                    id.to_string(),
                    PatternTile {
                        x: f64::from(attr_f32(tag, "x").unwrap_or(0.0)),
                        y: f64::from(attr_f32(tag, "y").unwrap_or(0.0)),
                        width,
                        height: f64::from(attr_f32(tag, "height").unwrap_or(0.0)),
                        view_box: view_box_of(tag),
                        body: rest[tag_end..close].to_string(),
                        attrs: pattern_attrs(tag),
                    },
                );
            }
        }
        rest = &rest[close + "</pattern>".len()..];
    }
    if tiles.is_empty() {
        return svg.to_string();
    }
    let joins = hatch_join_xs(svg, &tiles);
    let mut out = String::with_capacity(svg.len());
    let mut clips = String::new();
    let mut clip_n = 0_u32;
    let mut i = 0;
    while i < svg.len() {
        let rect_at = svg[i..].find("<rect ").map(|offset| i + offset);
        let path_at = svg[i..].find("<path ").map(|offset| i + offset);
        let Some(at) = (match (rect_at, path_at) {
            (Some(rect), Some(path)) => Some(rect.min(path)),
            (Some(rect), None) => Some(rect),
            (None, Some(path)) => Some(path),
            (None, None) => None,
        }) else {
            out.push_str(&svg[i..]);
            break;
        };
        out.push_str(&svg[i..at]);
        let Some(end) = svg[at..].find('>').map(|offset| at + offset + 1) else {
            out.push_str(&svg[at..]);
            break;
        };
        let tag = &svg[at..end];
        if tag.starts_with("<rect ") {
            if let Some(drawn) = tiled_rect(tag, &tiles, &joins) {
                out.push_str(&drawn);
            } else if let Some(drawn) = cover_fill_seam(tag) {
                out.push_str(&drawn);
            } else {
                out.push_str(tag);
            }
        } else if let Some(drawn) = continuous_hatch_path(tag, &tiles, &mut clips, &mut clip_n) {
            out.push_str(&drawn);
        } else {
            out.push_str(tag);
        }
        i = end;
    }
    if !clips.is_empty() {
        if let Some(at) = out.find("</defs>") {
            out.insert_str(at, &clips);
        }
    }
    out
}

fn pattern_attrs(tag: &str) -> String {
    let mut attrs = String::new();
    for name in [
        "fill",
        "stroke",
        "stroke-width",
        "stroke-linecap",
        "stroke-linejoin",
    ] {
        if let Some(value) = attr(tag, name) {
            attrs.push(' ');
            attrs.push_str(name);
            attrs.push_str("=\"");
            attrs.push_str(value);
            attrs.push('"');
        }
    }
    attrs
}

fn opens_reserved(tag: &str) -> bool {
    ["defs", "clipPath", "pattern", "mask", "marker", "symbol"]
        .into_iter()
        .any(|name| tag_name_is(tag, name))
}

fn closes_reserved(tag: &str) -> bool {
    let Some(rest) = tag.strip_prefix("</") else {
        return false;
    };
    ["defs", "clipPath", "pattern", "mask", "marker", "symbol"]
        .into_iter()
        .any(|name| {
            rest.starts_with(name)
                && rest
                    .as_bytes()
                    .get(name.len())
                    .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'>')
        })
}

fn view_box_of(tag: &str) -> Option<(f64, f64, f64, f64)> {
    let value = attr(tag, "viewBox")?;
    let mut parts = value.split_whitespace();
    let x = parts.next()?.parse().ok()?;
    let y = parts.next()?.parse().ok()?;
    let w = parts.next()?.parse().ok()?;
    let h = parts.next()?.parse().ok()?;
    (w > 0.0 && h > 0.0).then_some((x, y, w, h))
}

struct HatchInk<'a> {
    fill: &'a str,
    stroke: &'a str,
    stroke_width: f64,
}

/// The unknown hatch is three stubs of the lines `x + y = 6k`.
fn hatch_ink(tile: &PatternTile) -> Option<HatchInk<'_>> {
    if (tile.width - 6.0).abs() > 0.01 || (tile.height - 6.0).abs() > 0.01 {
        return None;
    }
    let rect_at = tile.body.find("<rect ")?;
    let rect_end = tile.body[rect_at..].find('>')? + rect_at;
    let rect_tag = &tile.body[rect_at..=rect_end];
    let fill = attr(rect_tag, "fill")?;
    let path_at = tile.body.find("<path ")?;
    let path_end = tile.body[path_at..].find('>')? + path_at;
    let path_tag = &tile.body[path_at..=path_end];
    if attr(path_tag, "d")? != "M-1,1 L1,-1 M0,6 L6,0 M5,7 L7,5" {
        return None;
    }
    Some(HatchInk {
        fill,
        stroke: attr(path_tag, "stroke")?,
        stroke_width: f64::from(attr_f32(path_tag, "stroke-width")?),
    })
}

/// Clipping each 6×6 cell cuts the stubs on a pixel boundary, so the slash
/// jogs where two cells meet. One stroke per line, clipped once to the
/// rectangle, stays straight.
fn continuous_hatch(tile: &PatternTile, x: f64, y: f64, width: f64, height: f64) -> Option<String> {
    let HatchInk {
        fill,
        stroke,
        stroke_width,
    } = hatch_ink(tile)?;
    let x1 = x + width;
    let y1 = y + height;
    let step = 6.0;
    let slack = stroke_width * 0.5;
    let k0 = ((x + y - slack) / step).floor() as i64;
    let k1 = ((x1 + y1 + slack) / step).ceil() as i64;
    let mut d = String::new();
    for k in k0..=k1 {
        let Some((ax, ay, bx, by)) = clip_diagonal(x, y, x1, y1, k as f64 * step) else {
            continue;
        };
        if !d.is_empty() {
            d.push(' ');
        }
        d.push('M');
        d.push_str(&fmt_num((ax - x) as f32));
        d.push(' ');
        d.push_str(&fmt_num((ay - y) as f32));
        d.push('L');
        d.push_str(&fmt_num((bx - x) as f32));
        d.push(' ');
        d.push_str(&fmt_num((by - y) as f32));
    }
    if d.is_empty() {
        return None;
    }
    Some(format!(
        "<svg x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" overflow=\"hidden\"><rect width=\"{}\" height=\"{}\" fill=\"{fill}\" stroke=\"none\"/><path d=\"{d}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"{}\"/></svg>",
        fmt_num(x as f32),
        fmt_num(y as f32),
        fmt_num(width as f32),
        fmt_num(height as f32),
        fmt_num(width as f32),
        fmt_num(height as f32),
        fmt_num(stroke_width as f32),
    ))
}

/// The portion of `x + y = c` that lies inside the rectangle.
fn clip_diagonal(x0: f64, y0: f64, x1: f64, y1: f64, c: f64) -> Option<(f64, f64, f64, f64)> {
    let mut pts = [(0.0, 0.0); 4];
    let mut n = 0;
    let mut push = |px: f64, py: f64| {
        if (0..n).any(|i| (pts[i].0 - px).abs() < 1e-4 && (pts[i].1 - py).abs() < 1e-4) {
            return;
        }
        if n < pts.len() {
            pts[n] = (px, py);
            n += 1;
        }
    };
    let y_left = c - x0;
    if (y0..=y1).contains(&y_left) {
        push(x0, y_left);
    }
    let y_right = c - x1;
    if (y0..=y1).contains(&y_right) {
        push(x1, y_right);
    }
    let x_top = c - y0;
    if (x0..=x1).contains(&x_top) {
        push(x_top, y0);
    }
    let x_bottom = c - y1;
    if (x0..=x1).contains(&x_bottom) {
        push(x_bottom, y1);
    }
    if n < 2 {
        return None;
    }
    let (mut a, mut b) = (0, 1);
    let mut best = 0.0;
    for i in 0..n {
        for j in (i + 1)..n {
            let dist = (pts[i].0 - pts[j].0).hypot(pts[i].1 - pts[j].1);
            if dist > best {
                best = dist;
                a = i;
                b = j;
            }
        }
    }
    (best > 0.05).then_some((pts[a].0, pts[a].1, pts[b].0, pts[b].1))
}

/// A bus transition is a polygon filled with the same hatch. Leaving it as a
/// 6×6 pattern puts a jog where that polygon meets the continuous rectangle.
fn continuous_hatch_path(
    tag: &str,
    tiles: &std::collections::HashMap<String, PatternTile>,
    clips: &mut String,
    clip_n: &mut u32,
) -> Option<String> {
    let fill_ref = attr(tag, "fill")?;
    let id = fill_ref.strip_prefix("url(#")?.strip_suffix(')')?;
    let ink = hatch_ink(tiles.get(id)?)?;
    let d = attr(tag, "d")?;
    let polygons = polygons_of(d)?;
    if polygons.is_empty() || polygons.iter().any(|poly| !convex(poly)) {
        return None;
    }
    let mut drawn = String::new();
    for poly in &polygons {
        let (min_x, min_y, max_x, max_y) = polygon_bounds(poly);
        let step = 6.0;
        let slack = ink.stroke_width;
        let k0 = ((min_x + min_y - slack) / step).floor() as i64;
        let k1 = ((max_x + max_y + slack) / step).ceil() as i64;
        for k in k0..=k1 {
            let sum = k as f64 * step;
            // The clip cuts the stroke, so the slash runs through the edge and
            // meets the flat hatch on the side they share.
            let Some((ax, ay, bx, by)) = clip_diagonal(
                min_x - slack,
                min_y - slack,
                max_x + slack,
                max_y + slack,
                sum,
            ) else {
                continue;
            };
            if clip_line_polygon((ax, ay), (bx, by), poly).is_none() {
                continue;
            }
            if !drawn.is_empty() {
                drawn.push(' ');
            }
            drawn.push('M');
            drawn.push_str(&fmt_num(ax as f32));
            drawn.push(' ');
            drawn.push_str(&fmt_num(ay as f32));
            drawn.push('L');
            drawn.push_str(&fmt_num(bx as f32));
            drawn.push(' ');
            drawn.push_str(&fmt_num(by as f32));
        }
    }
    let closed = if d.trim_end().ends_with(['Z', 'z']) {
        d.to_string()
    } else {
        format!("{d}Z")
    };
    if drawn.is_empty() {
        return Some(format!(
            "<path d=\"{closed}\" fill=\"{}\" stroke=\"none\"/>",
            ink.fill
        ));
    }
    let clip_id = format!("tgw-hatch-{clip_n}");
    *clip_n += 1;
    clips.push_str("<clipPath id=\"");
    clips.push_str(&clip_id);
    clips.push_str("\" clipPathUnits=\"userSpaceOnUse\"><path d=\"");
    clips.push_str(&closed);
    clips.push_str("\"/></clipPath>");
    Some(format!(
        "<g clip-path=\"url(#{clip_id})\"><path d=\"{closed}\" fill=\"{}\" stroke=\"none\"/><path d=\"{drawn}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"/></g>",
        ink.fill,
        ink.stroke,
        fmt_num(ink.stroke_width as f32),
    ))
}

/// The flat bus and the transition wedge share an edge. Each shape covers
/// that edge only up to an antialiased pixel, and the page shows through as a
/// hairline. The flat extends a half unit into the wedge, which is the same
/// paint, and the outline stroke stays on top.
fn cover_fill_seam(tag: &str) -> Option<String> {
    let fill = attr(tag, "fill")?;
    if fill.starts_with("url(") || attr(tag, "stroke") != Some("none") {
        return None;
    }
    let y = f64::from(attr_f32(tag, "y").unwrap_or(0.0));
    let height = f64::from(attr_f32(tag, "height")?);
    if y.abs() > 0.01 || (height - 20.0).abs() > 0.01 {
        return None;
    }
    let x = f64::from(attr_f32(tag, "x").unwrap_or(0.0));
    let width = f64::from(attr_f32(tag, "width")?);
    if width <= 0.0 {
        return None;
    }
    const OVERLAP: f64 = 0.5;
    Some(set_attr(
        &set_attr(tag, "x", &fmt_num((x - OVERLAP) as f32)),
        "width",
        &fmt_num((width + OVERLAP * 2.0) as f32),
    ))
}

fn polygons_of(d: &str) -> Option<Vec<Vec<(f64, f64)>>> {
    let mut rest = d;
    let mut polygons = Vec::new();
    let mut poly = Vec::new();
    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut cmd = 'M';
    let mut have_point = false;
    let finish = |poly: &mut Vec<(f64, f64)>, polygons: &mut Vec<Vec<(f64, f64)>>| {
        if poly.len() >= 3 {
            polygons.push(std::mem::take(poly));
        } else {
            poly.clear();
        }
    };
    loop {
        rest = rest.trim_start_matches([' ', ',', '\n', '\t', '\r']);
        if rest.is_empty() {
            break;
        }
        let next = rest.as_bytes()[0] as char;
        if next.is_ascii_alphabetic() {
            cmd = next;
            rest = &rest[next.len_utf8()..];
            if cmd == 'Z' || cmd == 'z' {
                finish(&mut poly, &mut polygons);
                cx = sx;
                cy = sy;
                continue;
            }
        }
        let relative = cmd.is_ascii_lowercase();
        match cmd.to_ascii_uppercase() {
            'M' => {
                let (x, next) = take_num(rest)?;
                let (y, next) = take_num(next)?;
                finish(&mut poly, &mut polygons);
                // The first moveto of a path is absolute, even when written `m`.
                if relative && have_point {
                    cx += x;
                    cy += y;
                } else {
                    cx = x;
                    cy = y;
                }
                have_point = true;
                sx = cx;
                sy = cy;
                poly.push((cx, cy));
                cmd = if relative { 'l' } else { 'L' };
                rest = next;
            }
            'L' => {
                let (x, next) = take_num(rest)?;
                let (y, next) = take_num(next)?;
                cx = if relative { cx + x } else { x };
                cy = if relative { cy + y } else { y };
                have_point = true;
                poly.push((cx, cy));
                rest = next;
            }
            'H' => {
                let (x, next) = take_num(rest)?;
                cx = if relative { cx + x } else { x };
                have_point = true;
                poly.push((cx, cy));
                rest = next;
            }
            'V' => {
                let (y, next) = take_num(rest)?;
                cy = if relative { cy + y } else { y };
                have_point = true;
                poly.push((cx, cy));
                rest = next;
            }
            _ => return None,
        }
    }
    finish(&mut poly, &mut polygons);
    Some(polygons)
}

fn take_num(text: &str) -> Option<(f64, &str)> {
    let text = text.trim_start_matches([' ', ',', '\n', '\t', '\r']);
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let mut end = usize::from(bytes[0] == b'+' || bytes[0] == b'-');
    let mut dot = false;
    let mut exp = false;
    while end < bytes.len() {
        let c = bytes[end];
        if c.is_ascii_digit() {
            end += 1;
        } else if c == b'.' && !dot && !exp {
            dot = true;
            end += 1;
        } else if (c == b'e' || c == b'E') && !exp && end > 0 {
            exp = true;
            end += 1;
            if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
                end += 1;
            }
        } else {
            break;
        }
    }
    if end == 0 || !bytes[..end].iter().any(u8::is_ascii_digit) {
        return None;
    }
    let value = text[..end].parse().ok()?;
    Some((value, &text[end..]))
}

fn polygon_bounds(poly: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (x, y) in poly {
        min_x = min_x.min(*x);
        min_y = min_y.min(*y);
        max_x = max_x.max(*x);
        max_y = max_y.max(*y);
    }
    (min_x, min_y, max_x, max_y)
}

fn convex(poly: &[(f64, f64)]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut sign = 0.0;
    for i in 0..n {
        let (ax, ay) = poly[i];
        let (bx, by) = poly[(i + 1) % n];
        let (cx, cy) = poly[(i + 2) % n];
        let cross = (bx - ax) * (cy - by) - (by - ay) * (cx - bx);
        if cross.abs() < 1e-8 {
            continue;
        }
        let next = cross.signum();
        if sign == 0.0 {
            sign = next;
        } else if next != sign {
            return false;
        }
    }
    sign != 0.0
}

/// Portion of the segment that lies inside a convex polygon.
fn clip_line_polygon(
    a: (f64, f64),
    b: (f64, f64),
    poly: &[(f64, f64)],
) -> Option<((f64, f64), (f64, f64))> {
    let n = poly.len();
    let mut area = 0.0;
    for i in 0..n {
        let p = poly[i];
        let q = poly[(i + 1) % n];
        area += p.0 * q.1 - q.0 * p.1;
    }
    if area.abs() < 1e-8 {
        return None;
    }
    let mut t0 = 0.0;
    let mut t1 = 1.0;
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    for i in 0..n {
        let p = poly[i];
        let q = poly[(i + 1) % n];
        let ex = q.0 - p.0;
        let ey = q.1 - p.1;
        let (nx, ny) = if area > 0.0 { (-ey, ex) } else { (ey, -ex) };
        let denom = nx * dx + ny * dy;
        let start = nx * (a.0 - p.0) + ny * (a.1 - p.1);
        if denom.abs() < 1e-12 {
            if start < -1e-8 {
                return None;
            }
            continue;
        }
        let hit = -start / denom;
        if denom > 0.0 {
            if hit > t1 {
                return None;
            }
            if hit > t0 {
                t0 = hit;
            }
        } else {
            if hit < t0 {
                return None;
            }
            if hit < t1 {
                t1 = hit;
            }
        }
    }
    if t1 - t0 < 1e-3 {
        return None;
    }
    Some((
        (a.0 + t0 * dx, a.1 + t0 * dy),
        (a.0 + t1 * dx, a.1 + t1 * dy),
    ))
}

/// Vertical sides of hatch wedges. The flat rectangle shares those x positions.
fn hatch_join_xs(svg: &str, tiles: &std::collections::HashMap<String, PatternTile>) -> Vec<f64> {
    let mut joins = Vec::new();
    let mut rest = svg;
    while let Some(at) = rest.find("<path ") {
        let Some(end) = rest[at..].find('>') else {
            break;
        };
        let tag = &rest[at..=at + end];
        let hatch = attr(tag, "fill")
            .and_then(|fill| fill.strip_prefix("url(#"))
            .and_then(|fill| fill.strip_suffix(')'))
            .and_then(|id| tiles.get(id))
            .is_some_and(|tile| hatch_ink(tile).is_some());
        if !hatch {
            rest = &rest[at + 5..];
            continue;
        }
        if let Some(polygons) = attr(tag, "d").and_then(polygons_of) {
            for poly in polygons {
                let n = poly.len();
                for i in 0..n {
                    let a = poly[i];
                    let b = poly[(i + 1) % n];
                    if (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() > 1.0 {
                        joins.push(a.0);
                    }
                }
            }
        }
        rest = &rest[at + 5..];
    }
    joins
}

fn tiled_rect(
    tag: &str,
    tiles: &std::collections::HashMap<String, PatternTile>,
    joins: &[f64],
) -> Option<String> {
    let fill = attr(tag, "fill")?;
    let id = fill.strip_prefix("url(#")?.strip_suffix(')')?;
    let tile = tiles.get(id)?;
    let x = f64::from(attr_f32(tag, "x").unwrap_or(0.0));
    let y = f64::from(attr_f32(tag, "y").unwrap_or(0.0));
    let width = f64::from(attr_f32(tag, "width")?);
    let height = f64::from(attr_f32(tag, "height")?);
    if width <= 0.0 || height <= 0.0 || tile.width <= 0.0 {
        return None;
    }
    if let Some(mut drawn) = continuous_hatch(tile, x, y, width, height) {
        // The flat hatch and the transition wedge each stop on the shared
        // edge, and the antialiased pixel between them is the page. A strip
        // on that edge is the same paint. It stays clear of the outline,
        // which is a one-unit stroke along the top and bottom.
        // Wide enough that the shared edge sits in the middle of the strip.
        // A strip that merely ends on the edge is antialiased there too, and
        // the page still shows through that one pixel.
        const OVERLAP: f64 = 2.0;
        const RAIL: f64 = 0.5;
        let touches = |edge: f64| joins.iter().any(|join| (join - edge).abs() < 0.05);
        let mut strip = |sx: f64, sw: f64| {
            if sw <= 0.0 || height <= RAIL * 2.0 {
                return;
            }
            if let Some(extra) = continuous_hatch(tile, sx, y + RAIL, sw, height - RAIL * 2.0) {
                drawn.push_str(&extra);
            }
        };
        if touches(x) {
            strip(x - OVERLAP, OVERLAP * 2.0);
        }
        if touches(x + width) {
            strip(x + width - OVERLAP, OVERLAP * 2.0);
        }
        return Some(drawn);
    }
    let tile_h = if tile.height > 0.0 {
        tile.height
    } else {
        height
    };
    let first_x = ((x - tile.x) / tile.width).floor() as i64;
    let last_x = ((x + width - tile.x) / tile.width).ceil() as i64;
    let first_y = ((y - tile.y) / tile_h).floor() as i64;
    let last_y = ((y + height - tile.y) / tile_h).ceil() as i64;
    let cols = last_x.saturating_sub(first_x);
    let rows = last_y.saturating_sub(first_y);
    let count = cols.saturating_mul(rows);
    if !(1..=512_i64).contains(&count) {
        return None;
    }
    // The tile grid stays in user space. A rectangle that starts at x=4 must
    // not slide the clock edge with it. Each tile is clipped, because a hatch
    // stroke deliberately crosses the 6×6 boundary and the pattern would
    // otherwise paint that overflow twice. `stroke="none"` keeps the tile from
    // inheriting the waveform stroke; inside a pattern that stroke is not in
    // effect, and inheriting it draws a box around every hatch cell.
    let mut out = format!(
        "<svg x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" overflow=\"hidden\"><g fill=\"none\" stroke=\"none\"><g{}>",
        fmt_num(x as f32),
        fmt_num(y as f32),
        fmt_num(width as f32),
        fmt_num(height as f32),
        tile.attrs
    );
    let (vb_x, vb_y, sx, sy) = if let Some((vx, vy, vw, vh)) = tile.view_box {
        (vx, vy, tile.width / vw, tile_h / vh)
    } else {
        (0.0, 0.0, 1.0, 1.0)
    };
    for row in first_y..last_y {
        let top = tile.y + row as f64 * tile_h;
        if top + tile_h < y || top > y + height {
            continue;
        }
        for col in first_x..last_x {
            let left = tile.x + col as f64 * tile.width;
            if left + tile.width < x || left > x + width {
                continue;
            }
            out.push_str("<svg x=\"");
            out.push_str(&fmt_num((left - x) as f32));
            out.push_str("\" y=\"");
            out.push_str(&fmt_num((top - y) as f32));
            out.push_str("\" width=\"");
            out.push_str(&fmt_num(tile.width as f32));
            out.push_str("\" height=\"");
            out.push_str(&fmt_num(tile_h as f32));
            out.push_str("\" overflow=\"hidden\">");
            let ox = -vb_x * sx;
            let oy = -vb_y * sy;
            let scaled = (sx - 1.0).abs() > 0.001 || (sy - 1.0).abs() > 0.001;
            let moved = ox.abs() > 0.001 || oy.abs() > 0.001;
            if scaled || moved {
                out.push_str("<g transform=\"");
                if moved {
                    out.push_str("translate(");
                    out.push_str(&fmt_num(ox as f32));
                    out.push(' ');
                    out.push_str(&fmt_num(oy as f32));
                    out.push(')');
                }
                if scaled {
                    if moved {
                        out.push(' ');
                    }
                    out.push_str("scale(");
                    out.push_str(&fmt_num(sx as f32));
                    out.push(' ');
                    out.push_str(&fmt_num(sy as f32));
                    out.push(')');
                }
                out.push_str("\">");
            }
            out.push_str(&tile.body);
            if scaled || moved {
                out.push_str("</g>");
            }
            out.push_str("</svg>");
        }
    }
    out.push_str("</g></g></svg>");
    Some(out)
}

fn root_view_box(svg: &str) -> Option<(f64, f64, f64, f64)> {
    let tag_end = svg.find('>')?;
    let tag = &svg[..=tag_end];
    let value = attr(tag, "viewBox")?;
    let mut parts = value.split_whitespace();
    let x = parts.next()?.parse().ok()?;
    let y = parts.next()?.parse().ok()?;
    let w = parts.next()?.parse().ok()?;
    let h = parts.next()?.parse().ok()?;
    Some((x, y, w, h))
}

fn root_tag(tag: &str, x: f32, y: f32, w: f32, h: f32) -> String {
    let width = fmt_num(w);
    let height = fmt_num(h);
    let view = format!("{} {} {} {}", fmt_num(x), fmt_num(y), width, height);
    set_attr(
        &set_attr(&set_attr(tag, "width", &width), "height", &height),
        "viewBox",
        &view,
    )
}

fn set_attr(tag: &str, name: &str, value: &str) -> String {
    let key = format!("{name}=\"");
    if let Some(start) = tag.find(&key) {
        let value_at = start + key.len();
        let value_end = tag[value_at..]
            .find('"')
            .map_or(tag.len(), |index| value_at + index);
        let mut out = String::with_capacity(tag.len() + value.len());
        out.push_str(&tag[..value_at]);
        out.push_str(value);
        out.push_str(&tag[value_end..]);
        out
    } else {
        let self_closing = tag.ends_with("/>");
        let mut out = if self_closing {
            tag.trim_end_matches('>')
                .trim_end_matches('/')
                .trim_end()
                .to_string()
        } else {
            tag.trim_end_matches('>').to_string()
        };
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        out.push_str(value);
        out.push_str(if self_closing { "\"/>" } else { "\">" });
        out
    }
}

fn fmt_num(value: f32) -> String {
    format!("{value:.3}")
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum TextAnchor {
    Start,
    Middle,
    End,
}

#[derive(Clone, Copy)]
pub(crate) struct TextStyle {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vertical: bool,
    pub(crate) anchor: TextAnchor,
    pub(crate) font: f32,
}

#[derive(Clone, Copy)]
pub(crate) struct LabelWindow {
    pub(crate) x0: f32,
    pub(crate) y0: f32,
    pub(crate) x1: f32,
    pub(crate) y1: f32,
}

struct SliceRequest {
    clip: LabelWindow,
    fit: LabelWindow,
    root_w: f32,
    root_h: f32,
}

fn retain_whole_labels(svg: &str, request: SliceRequest) -> String {
    let window = request.clip;
    let fit = request.fit;
    let root_w = request.root_w;
    let root_h = request.root_h;
    let mut out = String::with_capacity(svg.len());
    let mut stack = vec![TextStyle {
        x: 0.0,
        y: 0.0,
        vertical: false,
        anchor: TextAnchor::Start,
        font: 12.0,
    }];
    let mut i = 0;
    while i < svg.len() {
        if !svg[i..].starts_with('<') {
            let next = svg[i..].find('<').map_or(svg.len(), |offset| i + offset);
            out.push_str(&svg[i..next]);
            i = next;
            continue;
        }
        if svg[i..].starts_with("</g>") {
            if stack.len() > 1 {
                stack.pop();
            }
            out.push_str("</g>");
            i += 4;
            continue;
        }
        let Some(tag_end) = svg[i..].find('>').map(|offset| i + offset + 1) else {
            out.push_str(&svg[i..]);
            break;
        };
        let tag = &svg[i..tag_end];
        if tag_name_is(tag, "svg") {
            if let Some(font) = attr_f32(tag, "font-size") {
                stack[0].font = font;
            }
            out.push_str(&root_tag(tag, window.x0, window.y0, root_w, root_h));
            i = tag_end;
            continue;
        }
        if tag_name_is(tag, "g") {
            let parent = stack.last().copied().unwrap_or(TextStyle {
                x: 0.0,
                y: 0.0,
                vertical: false,
                anchor: TextAnchor::Start,
                font: 12.0,
            });
            if let Some(pill_end) = label_pill_end(svg, tag_end) {
                let style = style_after_group(parent, tag);
                if pill_fits(svg, tag_end, pill_end, style, window, fit) {
                    out.push_str(&svg[i..pill_end]);
                }
                i = pill_end;
                continue;
            }
            stack.push(style_after_group(parent, tag));
            out.push_str(tag);
            i = tag_end;
            continue;
        }
        if tag_name_is(tag, "text") {
            let Some(text_end) = svg[tag_end..]
                .find("</text>")
                .map(|offset| tag_end + offset + "</text>".len())
            else {
                out.push_str(&svg[i..]);
                break;
            };
            let style = stack.last().copied().unwrap_or(TextStyle {
                x: 0.0,
                y: 0.0,
                vertical: false,
                anchor: TextAnchor::Start,
                font: 12.0,
            });
            if text_fits(&svg[i..text_end], style, window, fit) {
                out.push_str(&svg[i..text_end]);
            }
            i = text_end;
            continue;
        }
        out.push_str(tag);
        i = tag_end;
    }
    out
}

pub(crate) fn tag_name_is(tag: &str, name: &str) -> bool {
    // A closing tag shares the name but must be copied through unchanged.
    // Rewriting `</svg>` produced `</svg width=...>` and the picture failed to parse.
    let Some(rest) = tag.strip_prefix('<') else {
        return false;
    };
    if rest.starts_with('/') {
        return false;
    }
    rest.starts_with(name)
        && rest
            .as_bytes()
            .get(name.len())
            .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(*byte, b'>' | b'/'))
}

pub(crate) fn label_pill_end(svg: &str, after_group: usize) -> Option<usize> {
    let rest = svg.get(after_group..)?.trim_start();
    if !tag_name_is(rest, "rect") {
        return None;
    }
    let rect_end = rest.find('>')? + 1;
    let after_rect = rest.get(rect_end..)?.trim_start();
    if !tag_name_is(after_rect, "text") {
        return None;
    }
    let close = after_rect.find("</text>")?;
    let after_text = after_rect.get(close + "</text>".len()..)?.trim_start();
    if !after_text.starts_with("</g>") {
        return None;
    }
    Some(svg.len() - after_text.len() + "</g>".len())
}

fn pill_fits(
    svg: &str,
    after_group: usize,
    pill_end: usize,
    style: TextStyle,
    window: LabelWindow,
    fit: LabelWindow,
) -> bool {
    let body = &svg[after_group..pill_end];
    let Some(text_at) = body.find("<text") else {
        return true;
    };
    let Some(rel_end) = body[text_at..].find("</text>") else {
        return true;
    };
    let text_end = text_at + rel_end + "</text>".len();
    text_fits(&body[text_at..text_end], style, window, fit)
}

pub(crate) fn style_after_group(mut style: TextStyle, tag: &str) -> TextStyle {
    if let Some((x, y)) = translate_of(tag) {
        style.x += x;
        style.y += y;
    }
    if tag.contains("rotate(270)") {
        style.vertical = true;
    }
    if let Some(font) = attr_f32(tag, "font-size") {
        style.font = font;
    }
    if let Some(anchor) = attr(tag, "text-anchor") {
        style.anchor = parse_anchor(anchor);
    }
    style
}

fn text_fits(element: &str, style: TextStyle, window: LabelWindow, fit: LabelWindow) -> bool {
    let Some((left, top, right, bottom)) = text_span(element, style) else {
        return true;
    };
    let inside = left >= fit.x0 - 0.5
        && right <= fit.x1 + 0.5
        && top >= fit.y0 - 0.5
        && bottom <= fit.y1 + 0.5;
    let hits = right > window.x0 + 0.5
        && left < window.x1 - 0.5
        && bottom > window.y0 + 0.5
        && top < window.y1 - 0.5;
    inside && hits
}

pub(crate) fn text_span(element: &str, mut style: TextStyle) -> Option<(f32, f32, f32, f32)> {
    let tag_end = element.find('>')?;
    let tag = &element[..tag_end];
    if let Some(font) = attr_f32(tag, "font-size") {
        style.font = font;
    }
    if let Some(anchor) = attr(tag, "text-anchor") {
        style.anchor = parse_anchor(anchor);
    }
    let local_x = attr_f32(tag, "x").unwrap_or(0.0);
    let local_y = attr_f32(tag, "y").unwrap_or(0.0);
    let content_at = tag_end + 1;
    let content_end = element.rfind("</text>").unwrap_or(element.len());
    if content_end < content_at {
        return None;
    }
    let content = &element[content_at..content_end];
    let ink = label_ink(
        content,
        style.font,
        attr_f32(tag, "textLength"),
        style.anchor,
    );
    Some(if style.vertical {
        let half = style.font * 0.8;
        (
            style.x - half,
            style.y - ink / 2.0,
            style.x + half,
            style.y + ink / 2.0,
        )
    } else {
        let x = style.x + local_x;
        let y = style.y + local_y;
        let (left, right) = match style.anchor {
            TextAnchor::Start => (x, x + ink),
            TextAnchor::Middle => (x - ink / 2.0, x + ink / 2.0),
            TextAnchor::End => (x - ink, x),
        };
        (left, y - style.font * 0.8, right, y + style.font * 0.3)
    })
}

fn label_ink(text: &str, font: f32, text_length: Option<f32>, anchor: TextAnchor) -> f32 {
    if let Some(length) = text_length.filter(|length| *length > 0.0) {
        return length;
    }
    let measured = tgw::text_width(text, f64::from(font.max(1.0))) as f32;
    // Names are end-aligned, so a loose width estimate becomes empty space on
    // the left only. Keep that estimate close to the drawn glyph. Tick labels
    // stay a little wide so a slice never keeps a number it would cut in half.
    match anchor {
        TextAnchor::End => (measured * 0.90).max(font * 0.35),
        _ => (measured * 1.12 + 1.0).max(font * 0.4),
    }
}

fn parse_anchor(value: &str) -> TextAnchor {
    match value {
        "middle" => TextAnchor::Middle,
        "end" => TextAnchor::End,
        _ => TextAnchor::Start,
    }
}

fn translate_of(tag: &str) -> Option<(f32, f32)> {
    let key = "translate(";
    let rest = &tag[tag.find(key)? + key.len()..];
    let body = &rest[..rest.find(')')?];
    let mut parts = body.split(',');
    let x = parts.next()?.trim().parse().ok()?;
    let y = parts
        .next()
        .map(|part| part.trim().parse().unwrap_or(0.0))
        .unwrap_or(0.0);
    Some((x, y))
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let bytes = tag.as_bytes();
    let name = name.as_bytes();
    if name.is_empty() {
        return None;
    }
    let mut i = 0;
    while i + name.len() + 2 <= bytes.len() {
        let boundary = i == 0 || bytes[i - 1].is_ascii_whitespace();
        if boundary
            && bytes[i..].starts_with(name)
            && bytes[i + name.len()] == b'='
            && bytes[i + name.len() + 1] == b'"'
        {
            let start = i + name.len() + 2;
            let end = tag[start..].find('"')? + start;
            return Some(&tag[start..end]);
        }
        i += 1;
    }
    None
}

pub(crate) fn attr_f32(tag: &str, name: &str) -> Option<f32> {
    let value = attr(tag, name)?.trim().trim_end_matches("px");
    value.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use gpui_kit::{size, DevicePixels, SvgRenderer, SvgSize};

    use crate::layout::{diagram_frame, full_frame, layout_diagram, svg_size};

    #[test]
    fn slice_svg_rewrites_only_the_root_window() {
        let svg = r#"<svg width="100" height="40" viewBox="0 0 100 40"><rect width="100" height="40"/></svg>"#;
        let sliced = slice_svg(svg, 10.0, 2.0, 30.0, 20.0).unwrap();
        let head = sliced.split_once('>').unwrap().0;
        assert!(head.contains("width=\"30.000\""));
        assert!(head.contains("height=\"20.000\""));
        assert!(head.contains("viewBox=\"10.000 2.000 30.000 20.000\""));
        assert!(sliced.contains("<rect width=\"100\" height=\"40\""));
        assert!(sliced.ends_with("</svg>"));
        assert!(!sliced.contains("</svg "));
        assert!(layout_diagram(&full_frame(10.0, 10.0, 2.0), 0.0, 100.0, 0.0, 0.0).is_none());
    }

    #[test]
    fn transfer_svg_raster_contains_ink() {
        let svg = tgw::render(include_str!("../examples/transfer.tgw")).unwrap();
        let (width, height) = svg_size(&svg).unwrap();
        assert!(width > height);
        let renderer = SvgRenderer::new(Arc::new(()));
        let parsed = renderer.parse_svg(svg.as_bytes()).unwrap();
        let image = renderer
            .render_parsed(
                &parsed,
                SvgSize::Size(size(DevicePixels(480), DevicePixels(1))),
            )
            .unwrap();
        let rendered = image.size(0);
        assert!(rendered.width.0 > 0 && rendered.width.0 <= 8192);
        let bytes = image.as_bytes(0).unwrap();
        let ink = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[0] < 250 || pixel[1] < 250 || pixel[2] < 250)
            .count();
        assert!(ink > 50, "ink pixels {ink}");

        let frame = diagram_frame(&svg).unwrap();
        let layout = layout_diagram(&frame, 1040.0, 680.0, 0.0, 0.0).unwrap();
        for slice in [layout.label, layout.wave] {
            let sliced = slice_svg(&svg, slice.x, slice.y, slice.w, slice.h).unwrap();
            assert!(
                sliced.ends_with("</svg>"),
                "closing tag must stay a closing tag"
            );
            let parsed = renderer.parse_svg(sliced.as_bytes()).unwrap();
            let image = renderer
                .render_parsed(
                    &parsed,
                    SvgSize::Size(size(DevicePixels(480), DevicePixels(1))),
                )
                .unwrap();
            let ink = image
                .as_bytes(0)
                .unwrap()
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[0] < 250 || pixel[1] < 250 || pixel[2] < 250)
                .count();
            assert!(ink > 20, "slice ink {ink}");
        }
    }

    #[test]
    fn expanded_tiles_match_pattern_paint() {
        let svg = tgw::render(include_str!("../examples/transfer.tgw")).unwrap();
        let frame = diagram_frame(&svg).unwrap();
        let layout = layout_diagram(&frame, 1040.0, 680.0, 0.0, 0.0).unwrap();
        let slice = layout.wave;
        let request = SliceRequest {
            clip: LabelWindow {
                x0: slice.x,
                y0: slice.y,
                x1: slice.x + slice.w,
                y1: slice.y + slice.h,
            },
            fit: LabelWindow {
                x0: slice.x,
                y0: slice.y,
                x1: slice.x + slice.w,
                y1: slice.y + slice.h,
            },
            root_w: slice.w,
            root_h: slice.h,
        };
        let windowed = retain_whole_labels(&svg, request);
        let clipped = clip_paint_rects(&windowed);
        let expanded = expand_patterns(&clipped);
        let renderer = SvgRenderer::new(Arc::new(()));
        let pixels = |source: &str| {
            let parsed = renderer.parse_svg(source.as_bytes()).unwrap();
            let image = renderer
                .render_parsed(
                    &parsed,
                    SvgSize::ExactSize(size(
                        DevicePixels(slice.w.round() as i32),
                        DevicePixels(slice.h.round() as i32),
                    )),
                )
                .unwrap();
            image.as_bytes(0).unwrap().to_vec()
        };
        let pattern = pixels(&clipped);
        let tiled = pixels(&expanded);
        assert_eq!(pattern.len(), tiled.len());
        // Pattern sampling and a plain stroke disagree by a few levels of gray.
        // A moved edge or a missing hatch is a much larger jump.
        let mut moved = 0;
        for (a, b) in pattern
            .as_chunks::<4>()
            .0
            .iter()
            .zip(tiled.as_chunks::<4>().0)
        {
            let delta = a
                .iter()
                .zip(b)
                .map(|(left, right)| left.abs_diff(*right))
                .max()
                .unwrap_or(0);
            if delta > 80 {
                moved += 1;
            }
        }
        assert_eq!(moved, 0, "expanded tiles moved {moved} pixels of ink");
    }

    #[test]
    fn transfer_waves_start_on_tick_zero() {
        let svg = tgw::render(include_str!("../examples/transfer.tgw")).unwrap();
        let frame = diagram_frame(&svg).unwrap();
        let layout = layout_diagram(&frame, 1040.0, 680.0, 0.0, 0.0).unwrap();
        let origin = frame.gutter + 0.5;
        assert!(
            layout.wave.x <= origin && origin < layout.wave.x + layout.wave.w,
            "tick 0 at {origin} is outside the wave slice {}..{}",
            layout.wave.x,
            layout.wave.x + layout.wave.w
        );
        let sliced = slice_svg(
            &svg,
            layout.wave.x,
            layout.wave.y,
            layout.wave.w,
            layout.wave.h,
        )
        .unwrap();
        assert!(
            sliced.contains(">0</text>"),
            "the zero tick is in the wave pane"
        );
        assert!(
            sliced.contains("M0 20"),
            "the first clock edge is in the wave pane"
        );
        let at = sliced.find("lane-clip").expect("lane clip");
        let rest = &sliced[at..];
        let rect_at = rest.find("<rect ").expect("clip rect");
        let tag_end = rest[rect_at..].find('>').unwrap();
        let tag = &rest[rect_at..rect_at + tag_end + 1];
        let clip_x = attr_f32(tag, "x").unwrap();
        assert!(
            clip_x <= 0.0,
            "lane clip x={clip_x} drops the start of the trace"
        );
    }

    #[test]
    fn unknown_hatch_fills_the_bus_height() {
        let svg = tgw::render("data: x3\n").unwrap();
        let frame = diagram_frame(&svg).unwrap();
        let layout = layout_diagram(&frame, 1040.0, 680.0, 0.0, 0.0).unwrap();
        let sliced = slice_svg(
            &svg,
            layout.wave.x,
            layout.wave.y,
            layout.wave.w,
            layout.wave.h,
        )
        .unwrap();
        assert_eq!(
            sliced.matches("width=\"6.000\"").count(),
            0,
            "hatch cells would jog the slash at every boundary"
        );
        let path_at = sliced
            .find("stroke-width=\"0.700\"")
            .expect("continuous hatch stroke");
        let d_at = sliced[..path_at].rfind("d=\"").expect("hatch path");
        let d = &sliced[d_at + 3..];
        let d = &d[..d.find('"').unwrap()];
        let mut sums = Vec::new();
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        for segment in d.split('M').filter(|part| !part.is_empty()) {
            let mut nums = segment
                .split(|c: char| c == 'L' || c.is_whitespace())
                .filter(|part| !part.is_empty())
                .map(|part| part.parse::<f32>().unwrap());
            let x0 = nums.next().unwrap();
            let y0 = nums.next().unwrap();
            let x1 = nums.next().unwrap();
            let y1 = nums.next().unwrap();
            let sum = x0 + y0;
            assert!(
                (sum - (x1 + y1)).abs() < 0.02,
                "slash {segment} is not a straight diagonal"
            );
            sums.push(sum);
            min_y = min_y.min(y0).min(y1);
            max_y = max_y.max(y0).max(y1);
        }
        sums.sort_by(f32::total_cmp);
        assert!(min_y < 1.0 && max_y > 19.0, "hatch y span {min_y}..{max_y}");
        for pair in sums.windows(2) {
            assert!(
                (pair[1] - pair[0] - 6.0).abs() < 0.02,
                "slash spacing {} then {}",
                pair[0],
                pair[1]
            );
        }
        assert!(
            sliced.contains("stroke=\"none\""),
            "the hatch background must not inherit the waveform stroke"
        );
    }

    #[test]
    fn transfer_hatch_crosses_the_transition_and_the_bus_has_no_seam() {
        let svg = tgw::render(include_str!("../examples/transfer.tgw")).unwrap();
        let frame = diagram_frame(&svg).unwrap();
        let layout = layout_diagram(&frame, 1040.0, 680.0, 0.0, 0.0).unwrap();
        let sliced = slice_svg(
            &svg,
            layout.wave.x,
            layout.wave.y,
            layout.wave.w,
            layout.wave.h,
        )
        .unwrap();
        let body = sliced.split("</defs>").nth(1).expect("defs");
        assert!(
            !body.contains("-xh)"),
            "a transition wedge still samples the hatch tile"
        );
        assert!(
            body.contains("x=\"299.500\"") && body.contains("width=\"61.000\""),
            "the read-data block still meets its wedges on a bare edge"
        );
        assert!(
            body.contains("x=\"218.000\"") && body.contains("x=\"378.000\""),
            "the hatch stops on the shared edge instead of overlapping it"
        );
        let wedge = body
            .find("M220 0L209 0L206 10L209 20L220 20Z")
            .expect("hatch wedge");
        let stroke = body[wedge..].find("stroke-width=\"0.700\"").unwrap();
        let d_at = body[..wedge + stroke].rfind("d=\"").unwrap();
        let d = &body[d_at + 3..];
        let d = &d[..d.find('"').unwrap()];
        let mut sums = Vec::new();
        for segment in d.split('M').filter(|part| !part.is_empty()) {
            let mut nums = segment
                .split(|c: char| c == 'L' || c.is_whitespace())
                .filter(|part| !part.is_empty())
                .map(|part| part.parse::<f32>().unwrap());
            let x0 = nums.next().unwrap();
            let y0 = nums.next().unwrap();
            let x1 = nums.next().unwrap();
            let y1 = nums.next().unwrap();
            assert!((x0 + y0 - (x1 + y1)).abs() < 0.02);
            sums.push(x0 + y0);
        }
        assert!(
            sums.iter()
                .any(|sum| (sum - 222.0).abs() < 0.02 || (sum - 228.0).abs() < 0.02),
            "wedge slashes {sums:?} miss the flat hatch at x=220"
        );
    }

    #[test]
    fn a_long_clock_slice_repeats_tiles_instead_of_one_huge_rectangle() {
        let long = tgw::render(&format!("clk: p{}", ".".repeat(4000))).unwrap();
        let frame = diagram_frame(&long).unwrap();
        let layout = layout_diagram(&frame, 1040.0, 680.0, 0.0, 0.0).unwrap();
        let sliced = slice_svg(
            &long,
            layout.wave.x,
            layout.wave.y,
            layout.wave.w,
            layout.wave.h,
        )
        .unwrap();
        let mut rest = sliced.as_str();
        let mut widest = 0.0_f32;
        let mut reserved = 0_i32;
        while let Some(index) = rest.find('<') {
            rest = &rest[index..];
            let Some(end) = rest.find('>') else { break };
            let tag = &rest[..=end];
            if closes_reserved(tag) {
                reserved = reserved.saturating_sub(1);
            } else if opens_reserved(tag) {
                reserved += 1;
            } else if reserved == 0 && tag_name_is(tag, "rect") {
                if let Some(width) = attr_f32(tag, "width") {
                    widest = widest.max(width);
                }
            }
            rest = &rest[end + 1..];
        }
        assert!(
            widest < layout.wave.w + 200.0,
            "widest paint rect {widest} is larger than the window"
        );
        assert!(sliced.contains("M-4 20"), "the clock tile is still drawn");
        let renderer = SvgRenderer::new(Arc::new(()));
        renderer.parse_svg(sliced.as_bytes()).unwrap();
    }

    #[test]
    fn ticks_on_the_split_are_kept_for_the_picture() {
        let svg = tgw::render(include_str!("../tests/fixtures/ticks.tgw")).unwrap();
        let frame = diagram_frame(&svg).unwrap();
        let layout = layout_diagram(&frame, 1040.0, 680.0, 0.0, 0.0).unwrap();
        let fit = LabelWindow {
            x0: layout.label.x.min(layout.wave.x),
            y0: layout.label.y.min(layout.wave.y),
            x1: (layout.label.x + layout.label.w).max(layout.wave.x + layout.wave.w),
            y1: (layout.label.y + layout.label.h).max(layout.wave.y + layout.wave.h),
        };
        let labels = slice_visible(
            &svg,
            layout.label.x,
            layout.label.y,
            layout.label.w,
            layout.label.h,
            fit,
        )
        .unwrap();
        let waves = slice_visible(
            &svg,
            layout.wave.x,
            layout.wave.y,
            layout.wave.w,
            layout.wave.h,
            fit,
        )
        .unwrap();
        let shown = format!("{labels}{waves}");
        assert!(
            shown.contains(">0.0<"),
            "the first fractional tick must stay visible"
        );
        assert!(
            shown.contains(">idle<"),
            "the first footer label must stay visible"
        );
        assert!(
            !slice_svg(&svg, frame.gutter, 0.0, 8.0, frame.height)
                .unwrap()
                .contains(">0.0<"),
            "a slice that cuts through the label still omits it"
        );
    }

    /// The pictures the watcher writes are the ones the window slices and draws.
    #[test]
    fn edge_case_diagrams_frame_slice_and_raster() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let renderer = SvgRenderer::new(Arc::new(()));
        for name in [
            "alphabet", "ticks", "spans", "markup", "unicode", "crop", "paths", "empty", "groups",
            "asm",
        ] {
            let source = std::fs::read_to_string(root.join(format!("{name}.tgw"))).unwrap();
            let svg = tgw::render(&source).unwrap();
            let parsed = renderer.parse_svg(svg.as_bytes()).unwrap();
            let image = renderer
                .render_parsed(
                    &parsed,
                    SvgSize::Size(size(DevicePixels(640), DevicePixels(1))),
                )
                .unwrap();
            let ink = image
                .as_bytes(0)
                .unwrap()
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[0] < 250 || pixel[1] < 250 || pixel[2] < 250)
                .count();
            assert!(ink > 10, "{name} has no ink ({ink})");
            let frame = diagram_frame(&svg).unwrap_or_else(|| panic!("{name} has no frame"));
            let layout = layout_diagram(&frame, 1040.0, 680.0, 0.0, 0.0).unwrap();
            if name == "asm" {
                assert!(frame.unified, "a chart scrolls as one picture");
                assert_eq!(layout.label_w, 0.0);
                let waves = slice_svg(
                    &svg,
                    layout.wave.x,
                    layout.wave.y,
                    layout.wave.w,
                    layout.wave.h,
                )
                .unwrap();
                assert!(waves.contains(">idle</text>"), "{waves}");
                assert!(waves.contains("<polygon"), "{waves}");
                let scrolled = layout_diagram(&frame, 240.0, 180.0, 0.0, 10_000.0).unwrap();
                assert!((scrolled.scale - 1.0).abs() < 0.01);
                assert!(scrolled.show_h && scrolled.show_v);
                let tail = slice_svg(
                    &svg,
                    scrolled.wave.x,
                    scrolled.wave.y,
                    scrolled.wave.w,
                    scrolled.wave.h,
                )
                .unwrap();
                assert!(
                    tail.contains(">wait</text>") || tail.contains(">hold</text>"),
                    "{tail}"
                );
                renderer
                    .parse_svg(tail.as_bytes())
                    .unwrap_or_else(|error| panic!("scrolled chart: {error}"));
            }
            for (part, slice) in [("labels", layout.label), ("waves", layout.wave)] {
                if slice.w < 1.0 || slice.h < 1.0 {
                    continue;
                }
                let sliced = slice_svg(&svg, slice.x, slice.y, slice.w, slice.h).unwrap();
                assert!(
                    sliced.ends_with("</svg>"),
                    "{name} {part} closing tag was rewritten"
                );
                renderer
                    .parse_svg(sliced.as_bytes())
                    .unwrap_or_else(|error| panic!("{name} {part} slice: {error}"));
            }
        }
    }
}
