//! Where the names end and the waves begin, and how that fits the window.

use crate::slice::{
    attr_f32, label_pill_end, style_after_group, tag_name_is, text_span, TextAnchor, TextStyle,
};

pub(crate) const STATUS_HEIGHT: f32 = 32.0;
pub(crate) const SCROLLBAR: f32 = 12.0;
/// Logical pixels of the scheme background kept around the drawing on every side.
pub(crate) const EDGE: f32 = 8.0;

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Slice {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Frame {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) gutter: f32,
    /// Ink of the drawing, in SVG units. The outer SVG margin is not included.
    pub(crate) x0: f32,
    pub(crate) y0: f32,
    pub(crate) x1: f32,
    pub(crate) y1: f32,
    /// An ASM chart scrolls as one picture. Timing diagrams keep a name column.
    pub(crate) unified: bool,
}

#[cfg(test)]
pub(crate) fn full_frame(width: f32, height: f32, gutter: f32) -> Frame {
    Frame {
        width,
        height,
        gutter,
        x0: 0.0,
        y0: 0.0,
        x1: width,
        y1: height,
        unified: false,
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ViewLayout {
    pub(crate) scale: f32,
    pub(crate) origin_x: f32,
    pub(crate) origin_y: f32,
    pub(crate) label_w: f32,
    pub(crate) wave_w: f32,
    pub(crate) body_h: f32,
    pub(crate) show_h: bool,
    pub(crate) show_v: bool,
    pub(crate) max_x: f32,
    pub(crate) max_y: f32,
    pub(crate) scroll_x: f32,
    pub(crate) scroll_y: f32,
    pub(crate) label: Slice,
    pub(crate) wave: Slice,
    pub(crate) h_thumb: (f32, f32),
    pub(crate) v_thumb: (f32, f32),
    pub(crate) h_bar_x: f32,
    pub(crate) h_bar_y: f32,
    pub(crate) v_bar_x: f32,
    pub(crate) v_bar_y: f32,
}

pub(crate) fn diagram_frame(svg: &str) -> Option<Frame> {
    let (width, height) = svg_size(svg)?;
    let root_end = svg.find('>')?;
    let root = &svg[..root_end];
    let unified = root.contains("class=\"tgw asm\"")
        || root.contains("class=\"tgw hls\"")
        || root.contains("class=\"tgw gtl\"");
    let defs = svg.find("</defs>")?;
    let rest = &svg[defs..];
    let key = "transform=\"translate(";
    let trans = rest.find(key)?;
    let nums = &rest[trans + key.len()..];
    let mut parts = nums
        .split([',', ')', ' '])
        .map(str::trim)
        .filter(|part| !part.is_empty());
    let x: f32 = parts.next()?.parse().ok()?;
    let y: f32 = parts.next()?.parse().ok()?;
    let gutter = (x - 0.5).max(0.0);
    let (mut x0, mut y0, mut x1, mut y1) = content_bounds(svg, x, y, width, height);
    // A chart, schedule, or netlist is scaled by a whole number. The window has to start
    // and end on SVG pixels, or that scale paints a 1px edge across two device
    // pixels.
    if unified {
        x0 = x0.floor().clamp(0.0, width);
        y0 = y0.floor().clamp(0.0, height);
        x1 = x1.ceil().clamp(x0 + 1.0, width.max(x0 + 1.0));
        y1 = y1.ceil().clamp(y0 + 1.0, height.max(y0 + 1.0));
    }
    (gutter < width).then_some(Frame {
        width,
        height,
        gutter,
        x0,
        y0,
        x1,
        y1,
        unified,
    })
}

/// Visible drawing, excluding the SVG's outer margin. Text uses the same ink
/// estimate as label clipping, so a glyph kept in the bounds is kept on screen.
fn content_bounds(svg: &str, ox: f32, oy: f32, width: f32, height: f32) -> (f32, f32, f32, f32) {
    let mut bounds: Option<(f32, f32, f32, f32)> = None;
    let mut include = |left: f32, top: f32, right: f32, bottom: f32| {
        if !(right > left && bottom > top) {
            return;
        }
        bounds = Some(match bounds {
            Some((x0, y0, x1, y1)) => (x0.min(left), y0.min(top), x1.max(right), y1.max(bottom)),
            None => (left, top, right, bottom),
        });
    };
    if let Some((left, top, right, bottom)) = plot_clip_box(svg) {
        include(ox + left, oy + top, ox + right, oy + bottom);
    }
    include_brackets(svg, &mut include);
    include_text(svg, &mut include);
    let Some((left, top, right, bottom)) = bounds else {
        return (0.0, 0.0, width, height);
    };
    let x0 = (left - 1.0).clamp(0.0, width);
    let y0 = (top - 1.0).clamp(0.0, height);
    let x1 = (right + 1.0).clamp(x0 + 1.0, width.max(x0 + 1.0));
    let y1 = (bottom + 1.0).clamp(y0 + 1.0, height.max(y0 + 1.0));
    if x1 - x0 < 8.0 || y1 - y0 < 8.0 {
        (0.0, 0.0, width, height)
    } else {
        (x0, y0, x1, y1)
    }
}

fn plot_clip_box(svg: &str) -> Option<(f32, f32, f32, f32)> {
    let at = svg.find("plot-clip\"")?;
    let rest = &svg[at..];
    let rect = rest.find("<rect ")?;
    let tag_end = rest[rect..].find('>')?;
    let tag = &rest[rect..rect + tag_end + 1];
    let x = attr_f32(tag, "x")?;
    let y = attr_f32(tag, "y")?;
    let w = attr_f32(tag, "width")?;
    let h = attr_f32(tag, "height")?;
    Some((x, y, x + w, y + h))
}

fn include_brackets(svg: &str, include: &mut impl FnMut(f32, f32, f32, f32)) {
    let mut rest = svg;
    let needle = " c -3,0 -5,2 -5,5 l 0,";
    while let Some(index) = rest.find(needle) {
        let head = &rest[..index];
        if let Some(start) = head.rfind("d=\"M") {
            let nums = &rest[start + 4..];
            if let Some((x, y, h)) = bracket_geom(nums) {
                include(x - 5.0, y, x + 1.0, y + h + 10.0);
            }
        }
        rest = &rest[index + needle.len()..];
    }
}

fn bracket_geom(nums: &str) -> Option<(f32, f32, f32)> {
    let (x, rest) = split_num(nums)?;
    let (y, rest) = split_num(rest)?;
    let mark = "l 0,";
    let drop = rest.find(mark)?;
    let (h, _) = split_num(&rest[drop + mark.len()..])?;
    Some((x, y, h))
}

fn split_num(text: &str) -> Option<(f32, &str)> {
    let text = text.trim_start_matches(|c: char| c == ',' || c.is_whitespace());
    let end = text
        .find(|c: char| c == ',' || c.is_whitespace())
        .unwrap_or(text.len());
    let value = text[..end].parse().ok()?;
    Some((value, &text[end..]))
}

fn include_text(svg: &str, include: &mut impl FnMut(f32, f32, f32, f32)) {
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
            i = svg[i..].find('<').map_or(svg.len(), |offset| i + offset);
            continue;
        }
        if svg[i..].starts_with("</g>") {
            if stack.len() > 1 {
                stack.pop();
            }
            i += 4;
            continue;
        }
        let Some(tag_end) = svg[i..].find('>').map(|offset| i + offset + 1) else {
            break;
        };
        let tag = &svg[i..tag_end];
        if tag_name_is(tag, "svg") {
            if let Some(font) = attr_f32(tag, "font-size") {
                stack[0].font = font;
            }
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
                if let Some(text_at) = svg[tag_end..pill_end].find("<text") {
                    let element = &svg[tag_end + text_at..pill_end];
                    if let Some(span) = text_span(element, style) {
                        include(span.0, span.1, span.2, span.3);
                    }
                }
                i = pill_end;
                continue;
            }
            stack.push(style_after_group(parent, tag));
            i = tag_end;
            continue;
        }
        if tag_name_is(tag, "text") {
            let Some(text_end) = svg[tag_end..]
                .find("</text>")
                .map(|offset| tag_end + offset + "</text>".len())
            else {
                break;
            };
            let style = stack.last().copied().unwrap_or(TextStyle {
                x: 0.0,
                y: 0.0,
                vertical: false,
                anchor: TextAnchor::Start,
                font: 12.0,
            });
            if let Some(span) = text_span(&svg[i..text_end], style) {
                include(span.0, span.1, span.2, span.3);
            }
            i = text_end;
            continue;
        }
        i = tag_end;
    }
}

fn choose_scale(svg_w: f32, svg_h: f32, avail_w: f32, avail_h: f32) -> f32 {
    if svg_w <= 0.0 || svg_h <= 0.0 {
        return 1.0;
    }
    let fit = (avail_w / svg_w).min(avail_h / svg_h);
    // The waveform fits the window, so every cycle stays on screen.
    if fit >= 1.0 {
        return fit;
    }
    // The time axis is longer than the window. Grow with the window height so
    // a cycle stays readable, and let the extra width scroll.
    if svg_h <= avail_h {
        return avail_h / svg_h;
    }
    1.0
}

fn choose_asm_scale(svg_w: f32, svg_h: f32, avail_w: f32, avail_h: f32) -> f32 {
    if svg_w <= 0.0 || svg_h <= 0.0 {
        return 1.0;
    }
    let fit = (avail_w / svg_w).min(avail_h / svg_h);
    if fit >= 1.0 {
        fit.floor().max(1.0)
    } else {
        1.0
    }
}

/// Names end 10px left of the plot origin, and that origin is half a pixel past
/// the gutter. The split sits 2px to the right of the names, so the tick mark
/// centered on the origin — including its `0` — is entirely in the wave pane.
fn column_seam(gutter: f32) -> f32 {
    (gutter - 7.5).clamp(0.0, gutter)
}

/// Height of a window that leaves `EDGE` above and below a diagram which does
/// not fill the viewport. `None` when the height is already tight, or when the
/// diagram is tall enough to scroll.
pub(crate) fn hugged_height(layout: &ViewLayout, viewport_h: f32, status: bool) -> Option<f32> {
    if layout.show_v || layout.origin_y <= EDGE + 1.0 {
        return None;
    }
    let chrome = if layout.show_h { SCROLLBAR } else { 0.0 };
    let status_h = if status { STATUS_HEIGHT } else { 0.0 };
    let height = (layout.body_h + EDGE * 2.0 + chrome + status_h).max(200.0);
    (viewport_h > height + 2.0).then_some(height)
}

pub(crate) fn layout_diagram(
    frame: &Frame,
    view_w: f32,
    view_h: f32,
    scroll_x: f32,
    scroll_y: f32,
) -> Option<ViewLayout> {
    if !(frame.width > 0.0 && frame.height > 0.0 && view_w >= 1.0 && view_h >= 1.0) {
        return None;
    }
    let x0 = frame.x0.clamp(0.0, frame.width);
    let y0 = frame.y0.clamp(0.0, frame.height);
    let x1 = frame.x1.clamp(x0 + 1.0, frame.width.max(x0 + 1.0));
    let y1 = frame.y1.clamp(y0 + 1.0, frame.height.max(y0 + 1.0));
    let seam = if frame.unified {
        x0
    } else {
        column_seam(frame.gutter).clamp(x0, x1)
    };
    let label_span = (seam - x0).max(0.0);
    let wave_span = (x1 - seam).max(1.0);
    let content_w = (label_span + wave_span).max(1.0);
    let content_h = (y1 - y0).max(1.0);
    let edge_x = EDGE.min(view_w / 8.0);
    let edge_y = EDGE.min(view_h / 8.0);
    let mut show_h = false;
    let mut show_v = false;
    let mut scale = 1.0;
    let mut label_w = 0.0;
    let mut wave_w = 0.0;
    let mut body_h = 0.0;
    for _ in 0..4 {
        let inner_w = (view_w - edge_x * 2.0 - if show_v { SCROLLBAR } else { 0.0 }).max(1.0);
        let inner_h = (view_h - edge_y * 2.0 - if show_h { SCROLLBAR } else { 0.0 }).max(1.0);
        scale = if frame.unified {
            choose_asm_scale(content_w, content_h, inner_w, inner_h)
        } else {
            choose_scale(content_w, content_h, inner_w, inner_h)
        };
        let natural = label_span * scale;
        label_w = if natural + 64.0 <= inner_w {
            natural
        } else {
            (inner_w - 64.0).max(0.0)
        };
        let wave_px = wave_span * scale;
        let height_px = content_h * scale;
        let mut next_h = wave_px > (inner_w - label_w) + 1.0;
        let mut next_v = height_px > inner_h + 1.0;
        if frame.unified && (next_h || next_v) {
            next_h = true;
            next_v = true;
        }
        wave_w = if next_h {
            (inner_w - label_w).max(1.0)
        } else {
            wave_px.max(1.0)
        };
        body_h = if next_v { inner_h } else { height_px.max(1.0) };
        if next_h == show_h && next_v == show_v {
            break;
        }
        show_h = next_h;
        show_v = next_v;
    }
    let bar_v = if show_v { SCROLLBAR } else { 0.0 };
    let bar_h = if show_h { SCROLLBAR } else { 0.0 };
    let inner_w = (view_w - edge_x * 2.0 - bar_v).max(1.0);
    let inner_h = (view_h - edge_y * 2.0 - bar_h).max(1.0);
    let origin_x = edge_x + (inner_w - (label_w + wave_w)).max(0.0) / 2.0;
    let origin_y = edge_y + (inner_h - body_h).max(0.0) / 2.0;
    let view_svg_w = wave_w / scale;
    let view_svg_h = body_h / scale;
    let max_x = (wave_span - view_svg_w).max(0.0);
    let max_y = (content_h - view_svg_h).max(0.0);
    let scroll_x = scroll_x.clamp(0.0, max_x);
    let scroll_y = scroll_y.clamp(0.0, max_y);
    let label_svg_w = (label_w / scale).min(label_span).max(0.0);
    let label_x = (seam - label_svg_w).max(x0);
    let wave_w_svg = view_svg_w.min((x1 - seam - scroll_x).max(0.0)).max(0.0);
    let body_svg_h = view_svg_h.min((content_h - scroll_y).max(0.0)).max(0.0);
    let y = y0 + scroll_y;
    Some(ViewLayout {
        scale,
        origin_x,
        origin_y,
        label_w,
        wave_w,
        body_h,
        show_h,
        show_v,
        max_x,
        max_y,
        scroll_x,
        scroll_y,
        label: Slice {
            x: label_x,
            y,
            w: label_svg_w,
            h: body_svg_h,
        },
        wave: Slice {
            x: seam + scroll_x,
            y,
            w: wave_w_svg,
            h: body_svg_h,
        },
        h_thumb: thumb(wave_w, wave_w, wave_span * scale, scroll_x * scale),
        v_thumb: thumb(body_h, body_h, content_h * scale, scroll_y * scale),
        h_bar_x: origin_x + label_w,
        h_bar_y: view_h - SCROLLBAR,
        v_bar_x: view_w - SCROLLBAR,
        v_bar_y: origin_y,
    })
}

fn thumb(track: f32, visible: f32, content: f32, scroll_px: f32) -> (f32, f32) {
    if track <= 1.0 {
        return (0.0, track.max(0.0));
    }
    let thumb = if content <= visible + 0.5 {
        track
    } else {
        (track * visible / content).clamp(24.0_f32.min(track), track)
    };
    let travel = (track - thumb).max(0.0);
    let max_scroll = (content - visible).max(0.0);
    let origin = if max_scroll <= 0.0 {
        0.0
    } else {
        travel * (scroll_px / max_scroll).clamp(0.0, 1.0)
    };
    (origin, thumb)
}

pub(crate) fn svg_size(svg: &str) -> Option<(f32, f32)> {
    let tag = &svg[..=svg.find('>')?];
    let width = attr_f32(tag, "width")?;
    let height = attr_f32(tag, "height")?;
    (width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite())
        .then_some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slice::slice_svg;
    use tgw::scheme;

    #[test]
    fn frame_finds_the_label_gutter() {
        let svg = tgw::render(include_str!("../examples/transfer.tgw")).unwrap();
        let frame = diagram_frame(&svg).unwrap();
        assert!(frame.gutter > 40.0);
        assert!(frame.gutter < frame.width);
    }

    #[test]
    fn wide_diagram_scrolls_horizontally_at_a_readable_scale() {
        let frame = full_frame(4000.0, 220.0, 120.0);
        let layout = layout_diagram(&frame, 1000.0, 600.0, 0.0, 0.0).unwrap();
        assert!(layout.scale >= 1.0);
        assert!(layout.show_h);
        assert!(!layout.show_v);
        assert!(layout.label_w > 100.0);
        assert!(layout.max_x > 1000.0);
    }

    #[test]
    fn short_diagram_stays_whole_without_a_scrollbar() {
        let frame = full_frame(540.0, 296.0, 120.0);
        let layout = layout_diagram(&frame, 1040.0, 680.0, 0.0, 0.0).unwrap();
        assert!(layout.scale > 1.0);
        assert!(!layout.show_h);
        assert!(!layout.show_v);
        assert!(layout.max_x < 0.05);
        assert!(layout.max_y < 0.05);
        let right = 1040.0 - (layout.origin_x + layout.label_w + layout.wave_w);
        let bottom = 680.0 - (layout.origin_y + layout.body_h);
        assert!((layout.origin_x - right).abs() < 1.0);
        assert!((layout.origin_y - bottom).abs() < 1.0);
        assert!((layout.origin_x - EDGE).abs() < 1.0);
        let hugged = hugged_height(&layout, 680.0, false).unwrap();
        assert!(hugged < 680.0);
        let snug = layout_diagram(&frame, 1040.0, hugged, 0.0, 0.0).unwrap();
        assert!(!snug.show_h && !snug.show_v);
        let snug_bottom = hugged - (snug.origin_y + snug.body_h);
        assert!((snug.origin_y - EDGE).abs() < 1.5, "top {}", snug.origin_y);
        assert!((snug_bottom - EDGE).abs() < 1.5, "bottom {snug_bottom}");
        assert!(hugged_height(&snug, hugged, false).is_none());
    }

    #[test]
    fn tall_diagram_scrolls_vertically_without_shrinking() {
        let frame = full_frame(400.0, 900.0, 80.0);
        let layout = layout_diagram(&frame, 800.0, 500.0, 0.0, 0.0).unwrap();
        assert!((layout.scale - 1.0).abs() < 0.01);
        assert!(layout.show_v);
        assert!(!layout.show_h);
        assert!(layout.max_y > 100.0);
    }

    #[test]
    fn tick_zero_is_whole_in_the_wave_pane_and_leaves_when_scrolled() {
        let svg = tgw::render(&format!(
            "@title Clock\n@tick 0\nclk: p{}\n",
            ".".repeat(80)
        ))
        .unwrap();
        let frame = diagram_frame(&svg).unwrap();
        let fitted = layout_diagram(&frame, 420.0, 360.0, 0.0, 0.0).unwrap();
        assert!(fitted.max_x > 40.0, "the fixture should scroll");
        let labels = slice_svg(
            &svg,
            fitted.label.x,
            fitted.label.y,
            fitted.label.w,
            fitted.label.h,
        )
        .unwrap();
        let waves = slice_svg(
            &svg,
            fitted.wave.x,
            fitted.wave.y,
            fitted.wave.w,
            fitted.wave.h,
        )
        .unwrap();
        assert!(labels.contains(">clk</text>"));
        assert!(!labels.contains(">0</text>"));
        assert!(waves.contains(">0</text>"));
        assert!(!waves.contains(">clk</text>"));

        let origin = frame.gutter + 0.5;
        let through_zero = slice_svg(&svg, origin - 2.0, 0.0, 80.0, frame.height).unwrap();
        assert!(
            !through_zero.contains(">0</text>"),
            "a window that cuts the zero label must omit it"
        );

        let scrolled = layout_diagram(&frame, 420.0, 360.0, fitted.max_x, 0.0).unwrap();
        let away = slice_svg(
            &svg,
            scrolled.wave.x,
            scrolled.wave.y,
            scrolled.wave.w,
            scrolled.wave.h,
        )
        .unwrap();
        assert!(!away.contains(">0</text>"));
        assert!(!away.contains(">clk</text>"));

        let short = tgw::render(include_str!("../examples/transfer.tgw")).unwrap();
        let short_frame = diagram_frame(&short).unwrap();
        let full = layout_diagram(&short_frame, 1040.0, 680.0, 0.0, 0.0).unwrap();
        let full_waves =
            slice_svg(&short, full.wave.x, full.wave.y, full.wave.w, full.wave.h).unwrap();
        let full_labels = slice_svg(
            &short,
            full.label.x,
            full.label.y,
            full.label.w,
            full.label.h,
        )
        .unwrap();
        assert!(full_waves.contains("Bus transfer"));
        assert!(full_waves.contains(">0</text>"));
        assert!(full_labels.contains(">clk</text>"));
        assert!(!full_labels.contains(">0</text>"));
    }

    #[test]
    fn drawing_keeps_the_same_padding_on_every_edge() {
        let short = tgw::render(include_str!("../examples/transfer.tgw")).unwrap();
        let frame = diagram_frame(&short).unwrap();
        assert!(frame.x0 > 12.0, "left gutter blank should be cropped");
        assert!(frame.y0 > 8.0, "top margin should be cropped");
        assert!(
            frame.y1 < frame.height - 8.0,
            "bottom margin should be cropped"
        );
        let fitted = layout_diagram(&frame, 1040.0, 680.0, 0.0, 0.0).unwrap();
        assert!(
            !fitted.show_h && !fitted.show_v,
            "a short clock is fully visible"
        );
        let right = 1040.0 - (fitted.origin_x + fitted.label_w + fitted.wave_w);
        let bottom = 680.0 - (fitted.origin_y + fitted.body_h);
        assert!(
            (fitted.origin_x - right).abs() < 1.0,
            "left {}",
            fitted.origin_x
        );
        assert!(
            (fitted.origin_y - bottom).abs() < 1.0,
            "top {}",
            fitted.origin_y
        );
        assert!((fitted.origin_x.min(fitted.origin_y) - EDGE).abs() < 1.0);

        let long = tgw::render(&format!(
            "@title Clock\n@tick 0\nclk: p{}\n",
            ".".repeat(80)
        ))
        .unwrap();
        let frame = diagram_frame(&long).unwrap();
        let scrolled = layout_diagram(&frame, 1040.0, 680.0, 0.0, 0.0).unwrap();
        assert!(scrolled.show_h);
        let right = 1040.0 - (scrolled.origin_x + scrolled.label_w + scrolled.wave_w);
        let bottom = scrolled.h_bar_y - (scrolled.origin_y + scrolled.body_h);
        assert!((scrolled.origin_x - EDGE).abs() < 1.0);
        assert!((scrolled.origin_y - EDGE).abs() < 1.0);
        assert!((right - EDGE).abs() < 1.0, "right pad {right}");
        assert!((bottom - EDGE).abs() < 1.0, "bottom pad {bottom}");
    }

    #[test]
    fn dark_picture_keeps_the_light_frame() {
        let source = include_str!("../examples/transfer.tgw");
        let light = tgw::render(source).unwrap();
        let mut buf = Vec::new();
        tgw::render_themed(source, &mut buf, 0, &scheme::DARK).unwrap();
        let dark = String::from_utf8(buf).unwrap();
        assert_eq!(diagram_frame(&light), diagram_frame(&dark));
        assert!(dark.contains("fill=\"#10151f\""));
        assert!(!dark.contains("#fff"));
        assert!(!dark.contains("#0041c4"));
    }

    #[test]
    fn asm_chart_scrolls_as_one_picture() {
        let svg = tgw::render(include_str!("../examples/asm.tgw")).unwrap();
        let frame = diagram_frame(&svg).unwrap();
        assert!(frame.unified);
        let fitted = layout_diagram(&frame, 2000.0, 1600.0, 0.0, 0.0).unwrap();
        assert_eq!(fitted.label_w, 0.0);
        assert!(fitted.scale >= 1.0);
        assert_eq!(fitted.scale.fract(), 0.0);
        assert!(!fitted.show_h && !fitted.show_v);
        let whole = slice_svg(
            &svg,
            fitted.wave.x,
            fitted.wave.y,
            fitted.wave.w,
            fitted.wave.h,
        )
        .unwrap();
        assert!(whole.contains(">idle</text>"));
        assert!(whole.contains("<polygon"));
        let tight = layout_diagram(&frame, 120.0, 80.0, 0.0, 0.0).unwrap();
        assert!((tight.scale - 1.0).abs() < 0.01);
        assert!(tight.show_h && tight.show_v);
    }

    #[test]
    fn hls_schedule_scrolls_as_one_picture() {
        let svg = tgw::render(include_str!("../examples/hls.tgw")).unwrap();
        let frame = diagram_frame(&svg).unwrap();
        assert!(frame.unified);
        let fitted = layout_diagram(&frame, 2000.0, 1600.0, 0.0, 0.0).unwrap();
        assert_eq!(fitted.label_w, 0.0);
        assert!(fitted.scale >= 1.0);
        assert_eq!(fitted.scale.fract(), 0.0);
        assert!(!fitted.show_h && !fitted.show_v);
        let whole = slice_svg(
            &svg,
            fitted.wave.x,
            fitted.wave.y,
            fitted.wave.w,
            fitted.wave.h,
        )
        .unwrap();
        assert!(whole.contains("s0"));
        assert!(whole.contains("/1"));
        let tight = layout_diagram(&frame, 120.0, 80.0, 0.0, 0.0).unwrap();
        assert!((tight.scale - 1.0).abs() < 0.01);
        assert!(tight.show_h && tight.show_v);
    }

    #[test]
    fn gtl_netlist_scrolls_as_one_picture() {
        let svg = tgw::render(include_str!("../examples/gtl.tgw")).unwrap();
        let frame = diagram_frame(&svg).unwrap();
        assert!(frame.unified);
        let fitted = layout_diagram(&frame, 2000.0, 1600.0, 0.0, 0.0).unwrap();
        assert_eq!(fitted.label_w, 0.0);
        assert!(fitted.scale >= 1.0);
        assert_eq!(fitted.scale.fract(), 0.0);
        assert!(!fitted.show_h && !fitted.show_v);
        let whole = slice_svg(
            &svg,
            fitted.wave.x,
            fitted.wave.y,
            fitted.wave.w,
            fitted.wave.h,
        )
        .unwrap();
        assert!(whole.contains(">XOR</text>"));
        assert!(whole.contains(">MUX</text>"));
        let tight = layout_diagram(&frame, 120.0, 80.0, 0.0, 0.0).unwrap();
        assert!((tight.scale - 1.0).abs() < 0.01);
        assert!(tight.show_h && tight.show_v);
    }
}
