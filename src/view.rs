//! Live window for one `.tgw` or WaveJSON file.

use std::future::{poll_fn, Future};
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};
use std::thread;

use gpui_kit::{
    div, img, px, rgb, size, App, AppContext, Bounds, Context, DevicePixels, FocusHandle,
    ImageSource, InteractiveElement, IntoElement, KeyBinding, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, QuitMode, Render, RenderImage, ScrollWheelEvent,
    Styled, SvgSize, TitlebarOptions, Window, WindowBounds, WindowOptions,
};

use crate::live::{self, lock, Fresh, Gate, Job, Loaded, SourceStamp, SOURCE_POLL};

gpui_kit::actions!(tgw_view, [Quit]);

const MAX_DEVICE_PX: f32 = 8192.0;
const STATUS_HEIGHT: f32 = 32.0;
const SCROLLBAR: f32 = 12.0;
/// Logical pixels of white kept around the drawing on every side.
const EDGE: f32 = 8.0;
/// Wayland and X11 match windows to `tgw.desktop` by this id.
const APP_ID: &str = "tgw";

/// Opens the window and returns when it closes.
pub(crate) fn run(job: Job) -> Result<(), String> {
    display_available()?;
    gpui_kit::application().run(move |cx| {
        if let Err(error) = launch(job, cx) {
            eprintln!("tgw: {error}");
            std::process::exit(1);
        }
    });
    Ok(())
}

/// Without a display server GPUI falls back to a headless platform whose
/// window is never shown.
fn display_available() -> Result<(), String> {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        let set = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
        if !set("WAYLAND_DISPLAY") && !set("DISPLAY") {
            return Err(
                "--view needs a display, but neither WAYLAND_DISPLAY nor DISPLAY is set; \
                        use --watch -o PATH to keep the output current without a window"
                    .into(),
            );
        }
    }
    Ok(())
}

fn launch(job: Job, cx: &mut App) -> Result<(), String> {
    gpui_kit::init(cx);
    cx.set_quit_mode(QuitMode::LastWindowClosed);
    cx.bind_keys([
        KeyBinding::new("secondary-q", Quit, None),
        KeyBinding::new("secondary-w", Quit, None),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    #[cfg(target_os = "macos")]
    dock_icon();
    let name = |path: &std::path::Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
    };
    let mut title = name(&job.input).unwrap_or_else(|| "tgw".into());
    if let Some(output) = &job.output {
        title = format!("{title} → {}", output.label);
    }
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            ..TitlebarOptions::default()
        }),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1040.0), px(720.0)),
            cx,
        ))),
        window_min_size: Some(size(px(320.0), px(200.0))),
        app_id: Some(APP_ID.into()),
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        icon: window_icon(cx),
        ..WindowOptions::default()
    };
    let job = Arc::new(job);
    gpui_kit::open_window(options, cx, move |window, cx| {
        let viewer = cx.new(|cx| {
            let mut viewer = Viewer::new(job, cx.focus_handle());
            viewer.start(cx);
            viewer
        });
        viewer.update(cx, |viewer, cx| viewer.focus_handle.focus(window, cx));
        viewer
    })
    .map_err(|error| error.to_string())?;
    cx.activate(true);
    Ok(())
}

/// A binary started from a terminal has no bundle icon, so the Dock would
/// show a generic executable.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn dock_icon() {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(include_bytes!("../assets/icon/tgw.png"));
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    // SAFETY: on the main thread, with an image rather than nil.
    unsafe { NSApplication::sharedApplication(main_thread).setApplicationIconImage(Some(&image)) };
}

/// X11 draws this in title bars and task switchers. Wayland takes the icon
/// from the desktop entry named by `APP_ID` instead.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn window_icon(cx: &App) -> Option<Arc<image::RgbaImage>> {
    const SIDE: u32 = 128;
    let renderer = cx.svg_renderer();
    let tree = renderer
        .parse_svg(include_bytes!("../assets/icon/tgw.svg"))
        .ok()?;
    let side = DevicePixels(SIDE as i32);
    let image = renderer
        .render_parsed(&tree, SvgSize::ExactSize(size(side, side)))
        .ok()?;
    let mut pixels = image.as_bytes(0)?.to_vec();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    image::RgbaImage::from_raw(SIDE, SIDE, pixels).map(Arc::new)
}

struct Viewer {
    job: Arc<Job>,
    state: DiagramState,
    /// The output file could not be written; retried on every poll.
    output_fault: Option<String>,
    epoch: u64,
    loading: bool,
    watching: bool,
    observed: Option<SourceStamp>,
    focus_handle: FocusHandle,
    scroll_x: f32,
    scroll_y: f32,
    drag: Option<ScrollDrag>,
    layout: Option<ViewLayout>,
    labels: Option<PaneImage>,
    waves: Option<PaneImage>,
    paint_key: Option<PaintKey>,
    pixels_per_point: f32,
    raster_epoch: u64,
    raster_waiting: bool,
    /// The user dragged the window, so later frames keep that size.
    user_sized: bool,
    seen_viewport: Option<(f32, f32)>,
    hug_request: Option<(f32, f32)>,
    cached_frame: Option<(u64, Frame)>,
}

impl Viewer {
    fn new(job: Arc<Job>, focus_handle: FocusHandle) -> Self {
        Self {
            job,
            state: DiagramState::default(),
            output_fault: None,
            epoch: 0,
            loading: false,
            watching: false,
            observed: None,
            focus_handle,
            scroll_x: 0.0,
            scroll_y: 0.0,
            drag: None,
            layout: None,
            labels: None,
            waves: None,
            paint_key: None,
            pixels_per_point: 1.0,
            raster_epoch: 0,
            raster_waiting: false,
            user_sized: false,
            seen_viewport: None,
            hug_request: None,
            cached_frame: None,
        }
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        let _ = self.attach_watch(cx);
        self.observe_source(cx);
        self.request_reload(cx);
    }

    fn attach_watch(&mut self, cx: &mut Context<Self>) -> bool {
        if self.watching {
            return true;
        }
        let Ok(gate) = live::watch(&self.job.input) else {
            return false;
        };
        self.watching = true;
        self.follow(gate, cx);
        true
    }

    fn observe_source(&mut self, cx: &mut Context<Self>) {
        let job = Arc::clone(&self.job);
        cx.spawn(async move |this, cx| {
            let executor = cx.background_executor().clone();
            loop {
                executor.timer(SOURCE_POLL).await;
                let stamp = live::source_stamp(&job.input);
                let gone = this
                    .update(cx, |view, cx| {
                        if !view.watching {
                            let _ = view.attach_watch(cx);
                        }
                        let stale = view.observed != stamp || view.output_fault.is_some();
                        if stale && !view.loading {
                            view.request_reload(cx);
                        }
                        false
                    })
                    .unwrap_or(true);
                if gone {
                    break;
                }
            }
        })
        .detach();
    }

    fn follow(&mut self, gate: Arc<Gate>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            if !poll_fn(|task| gate.poll_wait(task)).await {
                break;
            }
            if this.update(cx, |view, cx| view.request_reload(cx)).is_err() {
                break;
            }
        })
        .detach();
    }

    fn request_reload(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.epoch = self.epoch.wrapping_add(1);
        let epoch = self.epoch;
        let job = Arc::clone(&self.job);
        cx.spawn(async move |this, cx| {
            // Reading waits for the writer to finish, so it stays off the
            // shared worker pool that also rasterizes the picture.
            let fresh = io_task(move || job.load(epoch)).await;
            this.update(cx, |view, cx| view.finish_load(epoch, fresh, cx))
                .ok();
        })
        .detach();
    }

    fn finish_load(&mut self, epoch: u64, fresh: Fresh, cx: &mut Context<Self>) {
        if epoch != self.epoch {
            return;
        }
        self.loading = false;
        self.output_fault = None;
        match fresh.loaded {
            Loaded::Unreadable(message) => {
                self.state.mark_unavailable(message);
            }
            Loaded::Text {
                source,
                svg,
                written,
            } => {
                self.state.commit(&self.job.label, &source, svg);
                self.output_fault = written.and_then(Result::err);
            }
        };
        if fresh.stable {
            self.observed = fresh.stamp;
        } else {
            self.request_reload(cx);
        }
        cx.notify();
    }

    fn status(&self) -> Option<&str> {
        self.state.status().or(self.output_fault.as_deref())
    }

    fn note_viewport(&mut self, window: &Window) {
        let viewport = window.viewport_size();
        let next = (viewport.width.as_f32(), viewport.height.as_f32());
        if let Some(seen) = self.seen_viewport {
            let changed = (seen.0 - next.0).abs() > 1.0 || (seen.1 - next.1).abs() > 1.0;
            let ours = self.hug_request.is_some_and(|requested| {
                (requested.0 - next.0).abs() < 2.0 && (requested.1 - next.1).abs() < 2.0
            });
            if changed && !ours {
                self.user_sized = true;
            }
        }
        self.seen_viewport = Some(next);
    }

    /// A short diagram opens snug to the drawing. A size the user chose is kept,
    /// and the extra room becomes padding.
    fn hug_window(&mut self, window: &mut Window, layout: &ViewLayout) {
        if self.user_sized {
            return;
        }
        let viewport = window.viewport_size();
        let Some(height) = hugged_height(layout, viewport.height.as_f32(), self.status().is_some())
        else {
            return;
        };
        let width = viewport.width.as_f32();
        self.hug_request = Some((width, height));
        window.resize(size(px(width), px(height)));
    }

    fn sync_layout(&mut self, window: &Window) -> Option<ViewLayout> {
        self.pixels_per_point = window.scale_factor();
        let viewport = window.viewport_size();
        let mut height = viewport.height.as_f32();
        if self.status().is_some() {
            height -= STATUS_HEIGHT;
        }
        let frame = self.frame()?;
        let layout = layout_diagram(
            &frame,
            viewport.width.as_f32(),
            height,
            self.scroll_x,
            self.scroll_y,
        )?;
        self.scroll_x = layout.scroll_x;
        self.scroll_y = layout.scroll_y;
        Some(layout)
    }

    /// The SVG walk that finds the drawing bounds is reused until the picture changes.
    fn frame(&mut self) -> Option<Frame> {
        if let Some((generation, frame)) = self.cached_frame {
            if generation == self.state.generation {
                return Some(frame);
            }
        }
        let frame = self.state.svg.as_deref().and_then(diagram_frame)?;
        self.cached_frame = Some((self.state.generation, frame));
        Some(frame)
    }

    fn request_paint(&mut self, cx: &mut Context<Self>) {
        let Some(layout) = self.layout else {
            return;
        };
        if panes_ready(
            self.labels.as_ref(),
            self.waves.as_ref(),
            &layout,
            self.state.generation,
            self.pixels_per_point,
        ) {
            return;
        }
        let key = paint_key(&layout, self.state.generation, self.pixels_per_point);
        if self.paint_key == Some(key) {
            return;
        }
        self.paint_key = Some(key);
        self.raster_epoch = self.raster_epoch.wrapping_add(1);
        if self.raster_waiting {
            return;
        }
        self.raster_waiting = true;
        cx.spawn(async move |this, cx| {
            let executor = cx.background_executor().clone();
            loop {
                let job = this
                    .update(cx, |view, cx| view.take_paint_job(cx))
                    .ok()
                    .flatten();
                let Some(job) = job else {
                    this.update(cx, |view, _cx| view.raster_waiting = false)
                        .ok();
                    break;
                };
                let epoch = job.epoch;
                let painted = executor.spawn(async move { paint_panes(job) }).await;
                let settled = this
                    .update(cx, |view, cx| {
                        if view.raster_epoch != epoch {
                            return false;
                        }
                        let hide_labels = view
                            .layout
                            .as_ref()
                            .is_some_and(|layout| layout.label.w < 1.0);
                        if hide_labels {
                            view.labels = None;
                        } else if let Some(labels) = painted.0 {
                            view.labels = Some(labels);
                        }
                        if let Some(waves) = painted.1 {
                            view.waves = Some(waves);
                        }
                        view.raster_waiting = false;
                        cx.notify();
                        true
                    })
                    .unwrap_or(true);
                if settled {
                    break;
                }
            }
        })
        .detach();
    }

    fn take_paint_job(&mut self, cx: &mut Context<Self>) -> Option<PaintJob> {
        let layout = self.layout?;
        let svg = Arc::clone(self.state.svg.as_ref()?);
        if panes_ready(
            self.labels.as_ref(),
            self.waves.as_ref(),
            &layout,
            self.state.generation,
            self.pixels_per_point,
        ) {
            return None;
        }
        let (label, wave) = (layout.label, layout.wave);
        self.paint_key = Some(paint_key(
            &layout,
            self.state.generation,
            self.pixels_per_point,
        ));
        Some(PaintJob {
            epoch: self.raster_epoch,
            generation: self.state.generation,
            scale: layout.scale,
            dpr: self.pixels_per_point,
            label,
            wave,
            svg,
            renderer: cx.svg_renderer(),
        })
    }

    fn on_wheel(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(layout) = self.layout else {
            return;
        };
        let delta = event.delta.pixel_delta(window.line_height());
        let mut dx = delta.x.as_f32();
        let mut dy = delta.y.as_f32();
        if event.shift {
            dx += dy;
            dy = 0.0;
        }
        let scale = layout.scale.max(0.01);
        self.scroll_x = (self.scroll_x - dx / scale).clamp(0.0, layout.max_x);
        self.scroll_y = (self.scroll_y - dy / scale).clamp(0.0, layout.max_y);
        cx.stop_propagation();
        cx.notify();
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(layout) = self.layout else {
            return;
        };
        let (x, y) = pointer(window, event.position);
        if layout.show_h && y >= layout.h_bar_y && y < layout.h_bar_y + SCROLLBAR {
            let local = x - layout.h_bar_x;
            if local >= 0.0 && local < layout.wave_w {
                self.scroll_bar(ScrollAxis::Horizontal, local, x, &layout, cx);
            }
        } else if layout.show_v && x >= layout.v_bar_x && x < layout.v_bar_x + SCROLLBAR {
            let local = y - layout.v_bar_y;
            if local >= 0.0 && local < layout.body_h {
                self.scroll_bar(ScrollAxis::Vertical, local, y, &layout, cx);
            }
        }
    }

    fn scroll_bar(
        &mut self,
        axis: ScrollAxis,
        local: f32,
        pointer_at: f32,
        layout: &ViewLayout,
        cx: &mut Context<Self>,
    ) {
        let (origin, thumb, track, page, max_scroll) = match axis {
            ScrollAxis::Horizontal => (
                layout.h_thumb.0,
                layout.h_thumb.1,
                layout.wave_w,
                layout.wave.w * 0.85,
                layout.max_x,
            ),
            ScrollAxis::Vertical => (
                layout.v_thumb.0,
                layout.v_thumb.1,
                layout.body_h,
                layout.wave.h * 0.85,
                layout.max_y,
            ),
        };
        if max_scroll <= 0.0 {
            return;
        }
        if local >= origin && local < origin + thumb {
            let scroll = match axis {
                ScrollAxis::Horizontal => self.scroll_x,
                ScrollAxis::Vertical => self.scroll_y,
            };
            self.drag = Some(ScrollDrag {
                axis,
                origin_mouse: pointer_at,
                origin_scroll: scroll,
                travel: (track - thumb).max(1.0),
                max_scroll,
            });
        } else {
            let scroll = match axis {
                ScrollAxis::Horizontal => &mut self.scroll_x,
                ScrollAxis::Vertical => &mut self.scroll_y,
            };
            let delta = if local < origin { -page } else { page };
            *scroll = (*scroll + delta).clamp(0.0, max_scroll);
            cx.notify();
        }
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.drag else {
            return;
        };
        if !event.dragging() {
            self.drag = None;
            return;
        }
        let (x, y) = pointer(window, event.position);
        let mouse = match drag.axis {
            ScrollAxis::Horizontal => x,
            ScrollAxis::Vertical => y,
        };
        let next = (drag.origin_scroll
            + (mouse - drag.origin_mouse) / drag.travel * drag.max_scroll)
            .clamp(0.0, drag.max_scroll);
        match drag.axis {
            ScrollAxis::Horizontal => self.scroll_x = next,
            ScrollAxis::Vertical => self.scroll_y = next,
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.drag = None;
    }

    fn pane(&self, image: Option<&PaneImage>, width: f32, height: f32) -> gpui_kit::Div {
        let mut pane = div()
            .w(px(width))
            .h(px(height))
            .flex_none()
            .relative()
            .overflow_hidden();
        if let Some(image) = image {
            // A finished bitmap stays put until the next slice replaces it.
            // Sliding it under the pane edge would cut a tick label in half.
            pane = pane.child(
                div()
                    .absolute()
                    .left(px(0.0))
                    .top(px(0.0))
                    .w(px(image.logical_w))
                    .h(px(image.logical_h))
                    .child(
                        img(ImageSource::Render(image.image.clone()))
                            .w(px(image.logical_w))
                            .h(px(image.logical_h)),
                    ),
            );
        }
        pane
    }
}

impl Render for Viewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.note_viewport(window);
        self.layout = self.sync_layout(window);
        if let Some(layout) = self.layout {
            self.hug_window(window, &layout);
        }
        self.request_paint(cx);
        let mut column = div()
            .track_focus(&self.focus_handle)
            .on_action(|_: &Quit, window, _| {
                window.remove_window();
            })
            .on_scroll_wheel(cx.listener(Self::on_wheel))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .bg(rgb(0x00ffffff));
        if let Some(layout) = self.layout {
            let picture = div()
                .absolute()
                .left(px(layout.origin_x))
                .top(px(layout.origin_y))
                .w(px(layout.label_w + layout.wave_w))
                .h(px(layout.body_h))
                .flex()
                .flex_row()
                .flex_none()
                .child(self.pane(self.labels.as_ref(), layout.label_w, layout.body_h))
                .child(self.pane(self.waves.as_ref(), layout.wave_w, layout.body_h));
            column = column.child(picture);
            if layout.show_v {
                column = column.child(
                    div()
                        .absolute()
                        .left(px(layout.v_bar_x))
                        .top(px(layout.v_bar_y))
                        .child(scrollbar(layout.v_thumb, SCROLLBAR, layout.body_h, false)),
                );
            }
            if layout.show_h {
                column = column.child(
                    div()
                        .absolute()
                        .left(px(layout.h_bar_x))
                        .top(px(layout.h_bar_y))
                        .child(scrollbar(layout.h_thumb, layout.wave_w, SCROLLBAR, true)),
                );
            }
        }
        column = column.child(div().flex_1());
        if let Some(status) = self.status() {
            column = column.child(
                div()
                    .w_full()
                    .h(px(STATUS_HEIGHT))
                    .flex_none()
                    .px(px(8.0))
                    .py(px(6.0))
                    .text_size(px(13.0))
                    .bg(rgb(0x00fef2f2))
                    .text_color(rgb(0x00991b1b))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(status.to_string()),
            );
        }
        column
    }
}

fn io_task<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> impl Future<Output = T> {
    let slot = Arc::new(IoSlot {
        value: Mutex::new(None),
        waker: Mutex::new(None),
    });
    let worker = Arc::clone(&slot);
    thread::spawn(move || {
        let value = work();
        *lock(&worker.value) = Some(value);
        if let Some(waker) = lock(&worker.waker).take() {
            waker.wake();
        }
    });
    poll_fn(move |cx| {
        if let Some(value) = lock(&slot.value).take() {
            return Poll::Ready(value);
        }
        *lock(&slot.waker) = Some(cx.waker().clone());
        if let Some(value) = lock(&slot.value).take() {
            Poll::Ready(value)
        } else {
            Poll::Pending
        }
    })
}

struct IoSlot<T> {
    value: Mutex<Option<T>>,
    waker: Mutex<Option<Waker>>,
}

#[derive(Clone, Copy)]
enum ScrollAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy)]
struct ScrollDrag {
    axis: ScrollAxis,
    origin_mouse: f32,
    origin_scroll: f32,
    travel: f32,
    max_scroll: f32,
}

#[derive(Clone, Copy, PartialEq)]
struct Slice {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[derive(Clone, Copy, PartialEq)]
struct PaintKey {
    generation: u64,
    scale_bits: u32,
    dpr_bits: u32,
    label: Slice,
    wave: Slice,
}

struct PaintJob {
    epoch: u64,
    generation: u64,
    scale: f32,
    dpr: f32,
    label: Slice,
    wave: Slice,
    svg: Arc<str>,
    renderer: gpui_kit::SvgRenderer,
}

struct PaneImage {
    image: std::sync::Arc<RenderImage>,
    slice: Slice,
    logical_w: f32,
    logical_h: f32,
    scale: f32,
    dpr: f32,
    generation: u64,
}

#[derive(Clone, Copy)]
struct Frame {
    width: f32,
    height: f32,
    gutter: f32,
    /// Ink of the drawing, in SVG units. The outer SVG margin is not included.
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

#[cfg(test)]
fn full_frame(width: f32, height: f32, gutter: f32) -> Frame {
    Frame {
        width,
        height,
        gutter,
        x0: 0.0,
        y0: 0.0,
        x1: width,
        y1: height,
    }
}

#[derive(Clone, Copy)]
struct ViewLayout {
    scale: f32,
    origin_x: f32,
    origin_y: f32,
    label_w: f32,
    wave_w: f32,
    body_h: f32,
    show_h: bool,
    show_v: bool,
    max_x: f32,
    max_y: f32,
    scroll_x: f32,
    scroll_y: f32,
    label: Slice,
    wave: Slice,
    h_thumb: (f32, f32),
    v_thumb: (f32, f32),
    h_bar_x: f32,
    h_bar_y: f32,
    v_bar_x: f32,
    v_bar_y: f32,
}

fn diagram_frame(svg: &str) -> Option<Frame> {
    let (width, height) = svg_size(svg)?;
    let marker = svg.find("fill=\"#fff\"")?;
    let rest = &svg[marker..];
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
    let (x0, y0, x1, y1) = content_bounds(svg, x, y, width, height);
    (gutter < width).then_some(Frame {
        width,
        height,
        gutter,
        x0,
        y0,
        x1,
        y1,
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
    while let Some(index) = rest.find("stroke=\"#0041c4\"") {
        let after = &rest[index..];
        if let Some(path) = after.find("d=\"M") {
            let nums = &after[path + 4..];
            if let Some((x, y, h)) = bracket_geom(nums) {
                include(x - 5.0, y, x + 1.0, y + h + 10.0);
            }
        }
        rest = &rest[index + 16..];
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

/// Names end 10px left of the plot origin, and that origin is half a pixel past
/// the gutter. The split sits 2px to the right of the names, so the tick mark
/// centered on the origin — including its `0` — is entirely in the wave pane.
fn column_seam(gutter: f32) -> f32 {
    (gutter - 7.5).clamp(0.0, gutter)
}

/// Height of a window that leaves `EDGE` above and below a diagram which does
/// not fill the viewport. `None` when the height is already tight, or when the
/// diagram is tall enough to scroll.
fn hugged_height(layout: &ViewLayout, viewport_h: f32, status: bool) -> Option<f32> {
    if layout.show_v || layout.origin_y <= EDGE + 1.0 {
        return None;
    }
    let chrome = if layout.show_h { SCROLLBAR } else { 0.0 };
    let status_h = if status { STATUS_HEIGHT } else { 0.0 };
    let height = (layout.body_h + EDGE * 2.0 + chrome + status_h).max(200.0);
    (viewport_h > height + 2.0).then_some(height)
}

fn layout_diagram(
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
    let seam = column_seam(frame.gutter).clamp(x0, x1);
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
        scale = choose_scale(content_w, content_h, inner_w, inner_h);
        let natural = label_span * scale;
        label_w = if natural + 64.0 <= inner_w {
            natural
        } else {
            (inner_w - 64.0).max(0.0)
        };
        let wave_px = wave_span * scale;
        let height_px = content_h * scale;
        let next_h = wave_px > (inner_w - label_w) + 1.0;
        let next_v = height_px > inner_h + 1.0;
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

fn paint_key(layout: &ViewLayout, generation: u64, dpr: f32) -> PaintKey {
    PaintKey {
        generation,
        scale_bits: layout.scale.to_bits(),
        dpr_bits: dpr.to_bits(),
        label: layout.label,
        wave: layout.wave,
    }
}

fn panes_ready(
    labels: Option<&PaneImage>,
    waves: Option<&PaneImage>,
    layout: &ViewLayout,
    generation: u64,
    dpr: f32,
) -> bool {
    let waves_ok =
        waves.is_some_and(|image| covers(image, layout.wave, layout.scale, dpr, generation));
    let labels_ok = layout.label.w < 1.0
        || labels.is_some_and(|image| covers(image, layout.label, layout.scale, dpr, generation));
    waves_ok && labels_ok
}

fn covers(image: &PaneImage, visible: Slice, scale: f32, dpr: f32, generation: u64) -> bool {
    image.generation == generation
        && (image.scale - scale).abs() <= 0.01
        && (image.dpr - dpr).abs() <= 0.01
        && (image.slice.x - visible.x).abs() <= 0.5
        && (image.slice.y - visible.y).abs() <= 0.5
        && (image.slice.w - visible.w).abs() <= 0.5
        && (image.slice.h - visible.h).abs() <= 0.5
}

fn paint_panes(job: PaintJob) -> (Option<PaneImage>, Option<PaneImage>) {
    let labels = (job.label.w >= 1.0).then(|| {
        raster_slice(
            &job.renderer,
            &job.svg,
            job.label,
            job.scale,
            job.dpr,
            job.generation,
        )
    });
    let waves = raster_slice(
        &job.renderer,
        &job.svg,
        job.wave,
        job.scale,
        job.dpr,
        job.generation,
    );
    (labels.flatten(), waves)
}

fn raster_slice(
    renderer: &gpui_kit::SvgRenderer,
    svg: &str,
    slice: Slice,
    scale: f32,
    dpr: f32,
    generation: u64,
) -> Option<PaneImage> {
    if slice.w < 1.0 || slice.h < 1.0 || scale <= 0.0 || dpr <= 0.0 {
        return None;
    }
    let logical_w = slice.w * scale;
    let logical_h = slice.h * scale;
    if logical_w < 1.0 || logical_h < 1.0 {
        return None;
    }
    let mut device_w = (logical_w * dpr).round().max(1.0);
    let mut device_h = (logical_h * dpr).round().max(1.0);
    let cap = (MAX_DEVICE_PX / device_w)
        .min(MAX_DEVICE_PX / device_h)
        .min(1.0);
    device_w = (device_w * cap).round().clamp(1.0, MAX_DEVICE_PX);
    device_h = (device_h * cap).round().clamp(1.0, MAX_DEVICE_PX);
    let sliced = slice_svg(svg, slice.x, slice.y, slice.w, slice.h)?;
    let parsed = renderer.parse_svg(sliced.as_bytes()).ok()?;
    let image = renderer
        .render_parsed(
            &parsed,
            SvgSize::ExactSize(size(
                DevicePixels(device_w as i32),
                DevicePixels(device_h as i32),
            )),
        )
        .ok()?;
    Some(PaneImage {
        image,
        slice,
        logical_w,
        logical_h,
        scale,
        dpr,
        generation,
    })
}

fn slice_svg(svg: &str, x: f32, y: f32, w: f32, h: f32) -> Option<String> {
    let start = svg.find("<svg")?;
    svg[start..].find('>')?;
    Some(retain_whole_labels(svg, x, y, x + w, y + h, w, h))
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
        let mut out = tag.trim_end_matches('>').to_string();
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        out.push_str(value);
        out.push_str("\">");
        out
    }
}

fn fmt_num(value: f32) -> String {
    format!("{value:.3}")
}

#[derive(Clone, Copy, PartialEq)]
enum TextAnchor {
    Start,
    Middle,
    End,
}

#[derive(Clone, Copy)]
struct TextStyle {
    x: f32,
    y: f32,
    vertical: bool,
    anchor: TextAnchor,
    font: f32,
}

#[derive(Clone, Copy)]
struct LabelWindow {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

fn retain_whole_labels(
    svg: &str,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    root_w: f32,
    root_h: f32,
) -> String {
    let window = LabelWindow { x0, y0, x1, y1 };
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
                if pill_fits(svg, tag_end, pill_end, style, window) {
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
            if text_fits(&svg[i..text_end], style, window) {
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

fn tag_name_is(tag: &str, name: &str) -> bool {
    let rest = tag.strip_prefix('<').unwrap_or(tag);
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    rest.starts_with(name)
        && rest
            .as_bytes()
            .get(name.len())
            .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(*byte, b'>' | b'/'))
}

fn label_pill_end(svg: &str, after_group: usize) -> Option<usize> {
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
) -> bool {
    let body = &svg[after_group..pill_end];
    let Some(text_at) = body.find("<text") else {
        return true;
    };
    let Some(rel_end) = body[text_at..].find("</text>") else {
        return true;
    };
    let text_end = text_at + rel_end + "</text>".len();
    text_fits(&body[text_at..text_end], style, window)
}

fn style_after_group(mut style: TextStyle, tag: &str) -> TextStyle {
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

fn text_fits(element: &str, style: TextStyle, window: LabelWindow) -> bool {
    let Some((left, top, right, bottom)) = text_span(element, style) else {
        return true;
    };
    left >= window.x0 - 0.5
        && right <= window.x1 + 0.5
        && top >= window.y0 - 0.5
        && bottom <= window.y1 + 0.5
}

fn text_span(element: &str, mut style: TextStyle) -> Option<(f32, f32, f32, f32)> {
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

fn attr_f32(tag: &str, name: &str) -> Option<f32> {
    let value = attr(tag, name)?.trim().trim_end_matches("px");
    value.parse().ok()
}

fn pointer(window: &Window, position: gpui_kit::Point<gpui_kit::Pixels>) -> (f32, f32) {
    let origin = window.visual_viewport_bounds().origin;
    (
        (position.x - origin.x).as_f32(),
        (position.y - origin.y).as_f32(),
    )
}

fn scrollbar(thumb: (f32, f32), width: f32, height: f32, horizontal: bool) -> gpui_kit::Div {
    let (origin, length) = thumb;
    let thumb = if horizontal {
        div()
            .absolute()
            .left(px(origin))
            .top(px(2.0))
            .w(px(length))
            .h(px((height - 4.0).max(1.0)))
            .bg(rgb(0x0094a3b8))
    } else {
        div()
            .absolute()
            .left(px(2.0))
            .top(px(origin))
            .w(px((width - 4.0).max(1.0)))
            .h(px(length))
            .bg(rgb(0x0094a3b8))
    };
    div()
        .w(px(width))
        .h(px(height))
        .flex_none()
        .relative()
        .bg(rgb(0x00e2e8f0))
        .child(thumb)
}

fn svg_size(svg: &str) -> Option<(f32, f32)> {
    let tag = &svg[..=svg.find('>')?];
    let width = attr_f32(tag, "width")?;
    let height = attr_f32(tag, "height")?;
    (width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite())
        .then_some((width, height))
}

#[derive(Default)]
struct DiagramState {
    source: Option<String>,
    svg: Option<Arc<str>>,
    fault: Option<Fault>,
    generation: u64,
}

enum Fault {
    Parse(String),
    Unavailable(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Unchanged,
    Rendered,
    StatusOnly,
}

impl DiagramState {
    fn status(&self) -> Option<&str> {
        match &self.fault {
            Some(Fault::Parse(text) | Fault::Unavailable(text)) => Some(text),
            None => None,
        }
    }

    #[cfg(test)]
    fn apply_text(&mut self, label: &str, source: &str) -> Step {
        let rendered = tgw::render(source);
        self.commit(label, source, rendered)
    }

    fn commit(&mut self, label: &str, source: &str, rendered: Result<String, tgw::Error>) -> Step {
        let same_source = self.source.as_deref() == Some(source);
        if same_source && !matches!(self.fault, Some(Fault::Unavailable(_))) {
            return Step::Unchanged;
        }
        match rendered {
            Ok(svg) => {
                let same_svg = self.svg.as_deref() == Some(svg.as_str());
                self.source = Some(source.to_string());
                self.fault = None;
                if same_svg {
                    return Step::StatusOnly;
                }
                self.svg = Some(Arc::from(svg));
                self.generation += 1;
                Step::Rendered
            }
            Err(error) => {
                self.source = Some(source.to_string());
                self.fault = Some(Fault::Parse(diagnostic(label, source, &error)));
                Step::StatusOnly
            }
        }
    }

    fn mark_unavailable(&mut self, message: String) -> Step {
        if matches!(&self.fault, Some(Fault::Unavailable(current)) if current == &message) {
            return Step::Unchanged;
        }
        self.fault = Some(Fault::Unavailable(message));
        Step::StatusOnly
    }
}

fn diagnostic(label: &str, source: &str, error: &tgw::Error) -> String {
    let mut offset = error.offset.min(source.len());
    while offset > 0 && !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &source[..offset];
    let line = before.bytes().filter(|&byte| byte == b'\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |index| index + 1);
    let column = source[start..offset].chars().count() + 1;
    format!("{label}:{line}:{column}: {}", error.message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::SvgRenderer;

    #[test]
    fn valid_source_replaces_svg_and_identical_bytes_keep_generation() {
        let mut state = DiagramState::default();
        let source = "clk: p...\n";
        assert_eq!(state.apply_text("a.tgw", source), Step::Rendered);
        let generation = state.generation;
        let svg = state.svg.clone();
        assert!(svg.as_deref().unwrap().contains("<svg"));
        assert!(state.status().is_none());
        assert_eq!(state.apply_text("a.tgw", source), Step::Unchanged);
        assert_eq!(state.generation, generation);
        assert_eq!(state.svg, svg);

        assert_eq!(state.apply_text("a.tgw", "clk: n...\n"), Step::Rendered);
        assert!(state.generation > generation);
        assert_ne!(state.svg, svg);
    }

    #[test]
    fn invalid_source_keeps_the_picture_and_records_the_diagnostic() {
        let mut state = DiagramState::default();
        state.apply_text("a.tgw", "clk: p...\n");
        let svg = state.svg.clone();
        let generation = state.generation;
        let bad = "clk: p?\n";
        assert_eq!(state.apply_text("a.tgw", bad), Step::StatusOnly);
        assert_eq!(state.svg, svg);
        assert_eq!(state.generation, generation);
        assert_eq!(state.status(), Some("a.tgw:1:1: unknown wave symbol '?'"));
        assert_eq!(state.apply_text("a.tgw", bad), Step::Unchanged);
        assert_eq!(state.generation, generation);
        assert_eq!(state.svg, svg);
    }

    #[test]
    fn missing_file_keeps_svg_until_the_same_source_returns() {
        let mut state = DiagramState::default();
        state.apply_text("a.tgw", "clk: p...\n");
        let svg = state.svg.clone();
        let generation = state.generation;
        assert_eq!(
            state.mark_unavailable("a.tgw: file is missing".into()),
            Step::StatusOnly
        );
        assert_eq!(state.svg, svg);
        assert_eq!(state.generation, generation);
        assert_eq!(state.status(), Some("a.tgw: file is missing"));
        assert_eq!(
            state.mark_unavailable("a.tgw: file is missing".into()),
            Step::Unchanged
        );
        assert_eq!(state.generation, generation);
        assert_eq!(state.apply_text("a.tgw", "clk: p...\n"), Step::StatusOnly);
        assert!(state.status().is_none());
        assert_eq!(state.generation, generation);
        assert_eq!(state.svg, svg);

        state.apply_text("a.tgw", "clk: p?\n");
        let svg = state.svg.clone();
        let generation = state.generation;
        state.mark_unavailable("a.tgw: file is missing".into());
        assert_eq!(state.apply_text("a.tgw", "clk: p?\n"), Step::StatusOnly);
        assert_eq!(state.status(), Some("a.tgw:1:1: unknown wave symbol '?'"));
        assert_eq!(state.svg, svg);
        assert_eq!(state.generation, generation);
    }

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
    fn slice_svg_rewrites_only_the_root_window() {
        let svg = r#"<svg width="100" height="40" viewBox="0 0 100 40"><rect width="100" height="40"/></svg>"#;
        let sliced = slice_svg(svg, 10.0, 2.0, 30.0, 20.0).unwrap();
        let head = sliced.split_once('>').unwrap().0;
        assert!(head.contains("width=\"30.000\""));
        assert!(head.contains("height=\"20.000\""));
        assert!(head.contains("viewBox=\"10.000 2.000 30.000 20.000\""));
        assert!(sliced.contains("<rect width=\"100\" height=\"40\""));
        assert!(layout_diagram(&full_frame(10.0, 10.0, 2.0), 0.0, 100.0, 0.0, 0.0).is_none());
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
            .chunks_exact(4)
            .filter(|pixel| pixel[0] < 250 || pixel[1] < 250 || pixel[2] < 250)
            .count();
        assert!(ink > 50, "ink pixels {ink}");
    }
}
