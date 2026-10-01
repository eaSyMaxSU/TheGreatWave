//! Live window for one `.tgw` file.

use std::future::{poll_fn, Future};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};
use std::thread;

use gpui_kit::{
    div, img, px, rgb, size, App, AppContext, Bounds, Context, DevicePixels, FocusHandle,
    ImageSource, InteractiveElement, IntoElement, KeyBinding, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, QuitMode, Render, RenderImage, ScrollWheelEvent,
    Styled, SvgSize, TitlebarOptions, Window, WindowBounds, WindowOptions,
};

use crate::live::{self, lock, Fresh, Gate, Job, Loaded, SourceStamp, StampPoller, SOURCE_POLL};
use tgw::scheme::{self, Scheme};

use crate::layout::{
    diagram_frame, hugged_height, layout_diagram, Frame, Slice, ViewLayout, SCROLLBAR,
    STATUS_HEIGHT,
};
use crate::slice::{slice_visible, LabelWindow};

gpui_kit::actions!(tgw_view, [Quit, CycleScheme]);

const MAX_DEVICE_PX: f32 = 8192.0;
/// Wayland and X11 match windows to `tgw.desktop` by this id.
const APP_ID: &str = "tgw";

/// Opens the window and returns when it closes.
pub(crate) fn run(job: Job) -> Result<(), String> {
    display_available()?;
    gpui_kit::application().run(move |cx| {
        if let Err(error) = launch(job, cx) {
            let _ = writeln!(io::stderr().lock(), "tgw: {error}");
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
        KeyBinding::new("secondary-shift-l", CycleScheme, None),
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
        viewer.update(cx, |viewer, cx| {
            viewer.watch_appearance(window, cx);
            viewer.focus_handle.focus(window, cx);
        });
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
    for pixel in pixels.as_chunks_mut::<4>().0 {
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
    /// Atlas entries for pictures this window no longer shows.
    retired: Vec<Arc<RenderImage>>,
    scheme: &'static Scheme,
    /// The user picked a scheme, so later system appearance changes wait.
    scheme_pinned: bool,
    /// Dark picture for the current diagram generation. Light uses the file SVG.
    themed: Option<ThemedSvg>,
    appearance_watch: Option<gpui_kit::Subscription>,
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
            retired: Vec::new(),
            scheme: &scheme::LIGHT,
            scheme_pinned: false,
            themed: None,
            appearance_watch: None,
        }
    }

    fn watch_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_appearance(window);
        self.appearance_watch = Some(cx.observe_window_appearance(window, |view, window, cx| {
            if view.apply_appearance(window) {
                cx.notify();
            }
        }));
    }

    fn apply_appearance(&mut self, window: &Window) -> bool {
        if self.scheme_pinned {
            return false;
        }
        let next = scheme_for(window.appearance());
        if next.name == self.scheme.name {
            return false;
        }
        self.scheme = next;
        true
    }

    fn cycle_scheme(&mut self, cx: &mut Context<Self>) {
        self.scheme_pinned = true;
        self.scheme = self.scheme.next();
        cx.notify();
    }

    fn retire(&mut self, previous: Option<PaneImage>) {
        if let Some(previous) = previous {
            self.retired.push(previous.image);
        }
    }

    /// GPUI keeps a sprite-atlas entry until `drop_image`. Replaced pictures
    /// would otherwise accumulate for the life of the window.
    fn release_images(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.retired.is_empty() {
            return;
        }
        for image in std::mem::take(&mut self.retired) {
            cx.drop_image(image, Some(window));
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
        let poller = StampPoller::start(self.job.input.clone()).ok();
        cx.spawn(async move |this, cx| {
            let executor = cx.background_executor().clone();
            loop {
                executor.timer(SOURCE_POLL).await;
                let stamp = poller.as_ref().and_then(StampPoller::latest);
                let gone = this
                    .update(cx, |view, cx| {
                        if !view.watching {
                            let _ = view.attach_watch(cx);
                        }
                        let changed = stamp.is_some_and(|stamp| stamp != view.observed);
                        if (changed || view.output_fault.is_some()) && !view.loading {
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
            self.scheme,
        ) {
            return;
        }
        let key = paint_key(
            &layout,
            self.state.generation,
            self.pixels_per_point,
            self.scheme,
        );
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
                            let previous = view.labels.take();
                            view.retire(previous);
                        } else if let Some(labels) = painted.0 {
                            let previous = view.labels.replace(labels);
                            view.retire(previous);
                        }
                        if let Some(waves) = painted.1 {
                            let previous = view.waves.replace(waves);
                            view.retire(previous);
                        }
                        if let Some(svg) = painted.2 {
                            view.themed = Some(ThemedSvg {
                                generation: view.state.generation,
                                scheme: view.scheme.name,
                                svg,
                            });
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
        let light = Arc::clone(self.state.svg.as_ref()?);
        if panes_ready(
            self.labels.as_ref(),
            self.waves.as_ref(),
            &layout,
            self.state.generation,
            self.pixels_per_point,
            self.scheme,
        ) {
            return None;
        }
        let (label, wave) = (layout.label, layout.wave);
        self.paint_key = Some(paint_key(
            &layout,
            self.state.generation,
            self.pixels_per_point,
            self.scheme,
        ));
        let cached = self.themed.as_ref().filter(|themed| {
            themed.generation == self.state.generation && themed.scheme == self.scheme.name
        });
        let (svg, theme_source) = if self.scheme.is_light() {
            (light, None)
        } else if let Some(themed) = cached {
            (Arc::clone(&themed.svg), None)
        } else {
            let source = self.state.source.as_deref().map(Arc::<str>::from);
            (light, source)
        };
        Some(PaintJob {
            epoch: self.raster_epoch,
            generation: self.state.generation,
            scale: layout.scale,
            dpr: self.pixels_per_point,
            label,
            wave,
            svg,
            theme_source,
            renderer: cx.svg_renderer(),
            scheme: self.scheme,
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
        self.release_images(window, cx);
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
            .on_action(cx.listener(|view, _: &CycleScheme, _, cx| view.cycle_scheme(cx)))
            .on_scroll_wheel(cx.listener(Self::on_wheel))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .bg(rgb(self.scheme.background));
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
                        .child(scrollbar(
                            layout.v_thumb,
                            SCROLLBAR,
                            layout.body_h,
                            false,
                            self.scheme,
                        )),
                );
            }
            if layout.show_h {
                column = column.child(
                    div()
                        .absolute()
                        .left(px(layout.h_bar_x))
                        .top(px(layout.h_bar_y))
                        .child(scrollbar(
                            layout.h_thumb,
                            layout.wave_w,
                            SCROLLBAR,
                            true,
                            self.scheme,
                        )),
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
                    .bg(rgb(self.scheme.status_background))
                    .text_color(rgb(self.scheme.status_text))
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
struct PaintKey {
    generation: u64,
    scale_bits: u32,
    dpr_bits: u32,
    label: Slice,
    wave: Slice,
    scheme: &'static Scheme,
}

struct PaintJob {
    epoch: u64,
    generation: u64,
    scale: f32,
    dpr: f32,
    label: Slice,
    wave: Slice,
    /// Picture to raster. Light is the file SVG; dark is the cached themed one.
    svg: Arc<str>,
    /// Source to render when this scheme is not cached yet. The file is not written.
    theme_source: Option<Arc<str>>,
    renderer: gpui_kit::SvgRenderer,
    scheme: &'static Scheme,
}

struct ThemedSvg {
    generation: u64,
    scheme: &'static str,
    svg: Arc<str>,
}

struct PaneImage {
    image: std::sync::Arc<RenderImage>,
    slice: Slice,
    logical_w: f32,
    logical_h: f32,
    scale: f32,
    dpr: f32,
    generation: u64,
    scheme: &'static Scheme,
}

fn paint_key(layout: &ViewLayout, generation: u64, dpr: f32, scheme: &'static Scheme) -> PaintKey {
    PaintKey {
        generation,
        scale_bits: layout.scale.to_bits(),
        dpr_bits: dpr.to_bits(),
        label: layout.label,
        wave: layout.wave,
        scheme,
    }
}

fn panes_ready(
    labels: Option<&PaneImage>,
    waves: Option<&PaneImage>,
    layout: &ViewLayout,
    generation: u64,
    dpr: f32,
    scheme: &'static Scheme,
) -> bool {
    let waves_ok = waves
        .is_some_and(|image| covers(image, layout.wave, layout.scale, dpr, generation, scheme));
    let labels_ok = layout.label.w < 1.0
        || labels.is_some_and(|image| {
            covers(image, layout.label, layout.scale, dpr, generation, scheme)
        });
    waves_ok && labels_ok
}

fn covers(
    image: &PaneImage,
    visible: Slice,
    scale: f32,
    dpr: f32,
    generation: u64,
    scheme: &'static Scheme,
) -> bool {
    image.generation == generation
        && image.scheme.name == scheme.name
        && (image.scale - scale).abs() <= 0.01
        && (image.dpr - dpr).abs() <= 0.01
        && (image.slice.x - visible.x).abs() <= 0.5
        && (image.slice.y - visible.y).abs() <= 0.5
        && (image.slice.w - visible.w).abs() <= 0.5
        && (image.slice.h - visible.h).abs() <= 0.5
}

fn paint_panes(job: PaintJob) -> (Option<PaneImage>, Option<PaneImage>, Option<Arc<str>>) {
    // Names and waves are separate pictures. A tick centered on the split is
    // fully on screen only when each picture keeps the part it covers.
    let mut fit = LabelWindow {
        x0: job.wave.x,
        y0: job.wave.y,
        x1: job.wave.x + job.wave.w,
        y1: job.wave.y + job.wave.h,
    };
    if job.label.w >= 1.0 {
        fit.x0 = fit.x0.min(job.label.x);
        fit.y0 = fit.y0.min(job.label.y);
        fit.x1 = fit.x1.max(job.label.x + job.label.w);
        fit.y1 = fit.y1.max(job.label.y + job.label.h);
    }
    let PaintJob {
        renderer,
        svg,
        theme_source,
        label,
        wave,
        scale,
        dpr,
        generation,
        scheme,
        ..
    } = job;
    let themed = theme_source.and_then(|source| {
        let mut buf = Vec::new();
        tgw::render_themed(&source, &mut buf, 0, scheme).ok()?;
        String::from_utf8(buf).ok().map(Arc::<str>::from)
    });
    let svg = themed.clone().unwrap_or(svg);
    std::thread::scope(|scope| {
        let names = (label.w >= 1.0).then(|| {
            let renderer = renderer.clone();
            let svg = Arc::clone(&svg);
            scope.spawn(move || {
                raster_slice(&renderer, &svg, label, fit, scale, dpr, generation, scheme)
            })
        });
        let waves = raster_slice(&renderer, &svg, wave, fit, scale, dpr, generation, scheme);
        let names = names.and_then(|job| job.join().ok()).flatten();
        (names, waves, themed)
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "the scheme is stamped on the picture so a theme change repaints"
)]
fn raster_slice(
    renderer: &gpui_kit::SvgRenderer,
    svg: &str,
    slice: Slice,
    fit: LabelWindow,
    scale: f32,
    dpr: f32,
    generation: u64,
    scheme: &'static Scheme,
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
    let sliced = slice_visible(svg, slice.x, slice.y, slice.w, slice.h, fit)?;
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
        scheme,
    })
}

fn pointer(window: &Window, position: gpui_kit::Point<gpui_kit::Pixels>) -> (f32, f32) {
    let origin = window.visual_viewport_bounds().origin;
    (
        (position.x - origin.x).as_f32(),
        (position.y - origin.y).as_f32(),
    )
}

fn scrollbar(
    thumb: (f32, f32),
    width: f32,
    height: f32,
    horizontal: bool,
    scheme: &Scheme,
) -> gpui_kit::Div {
    let (origin, length) = thumb;
    let thumb = if horizontal {
        div()
            .absolute()
            .left(px(origin))
            .top(px(2.0))
            .w(px(length))
            .h(px((height - 4.0).max(1.0)))
            .bg(rgb(scheme.scroll_thumb))
    } else {
        div()
            .absolute()
            .left(px(2.0))
            .top(px(origin))
            .w(px((width - 4.0).max(1.0)))
            .h(px(length))
            .bg(rgb(scheme.scroll_thumb))
    };
    div()
        .w(px(width))
        .h(px(height))
        .flex_none()
        .relative()
        .bg(rgb(scheme.scroll_track))
        .child(thumb)
}

fn scheme_for(appearance: gpui_kit::WindowAppearance) -> &'static Scheme {
    match appearance {
        gpui_kit::WindowAppearance::Dark | gpui_kit::WindowAppearance::VibrantDark => &scheme::DARK,
        gpui_kit::WindowAppearance::Light | gpui_kit::WindowAppearance::VibrantLight => {
            &scheme::LIGHT
        }
    }
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
    use gpui_kit::WindowAppearance;

    #[test]
    fn system_appearance_selects_the_matching_default_scheme() {
        assert_eq!(scheme_for(WindowAppearance::Light).name, "default light");
        assert_eq!(
            scheme_for(WindowAppearance::VibrantLight).name,
            "default light"
        );
        assert_eq!(scheme_for(WindowAppearance::Dark).name, "default dark");
        assert_eq!(
            scheme_for(WindowAppearance::VibrantDark).name,
            "default dark"
        );
    }

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
        assert_eq!(
            state.status(),
            Some(
                "a.tgw:1:7: unknown wave symbol '?'; expected p n P N h l H L 0 1 x d u z = 2-9 . | < >"
            )
        );
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
        assert_eq!(
            state.status(),
            Some(
                "a.tgw:1:7: unknown wave symbol '?'; expected p n P N h l H L 0 1 x d u z = 2-9 . | < >"
            )
        );
        assert_eq!(state.svg, svg);
        assert_eq!(state.generation, generation);
    }
}
