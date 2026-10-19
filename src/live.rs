//! File watching and live rendering, available with the `watch` feature.
//!
//! Construct a [`Job`], then a [`Watcher`]. Call [`Watcher::next_update`] from
//! a worker thread and drop it to stop watching. This API does not print, exit
//! the process, or start a GUI. The CLI and native viewer use the same jobs.

use std::ffi::OsString;
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll, Wake, Waker};
use std::thread;
use std::time::{Duration, Instant};

use notify::event::EventKind;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher as _};
const DEBOUNCE: Duration = Duration::from_millis(80);
const READ_PAUSE: Duration = Duration::from_millis(15);
pub(crate) const READ_SETTLE: Duration = Duration::from_millis(100);
pub(crate) const READ_DEADLINE: Duration = Duration::from_millis(400);
/// Catches saves the watcher misses, such as on network drives and in WSL.
pub(crate) const SOURCE_POLL: Duration = Duration::from_millis(200);

/// One watched diagram and the file its rendering is written to.
pub struct Job {
    pub(crate) input: PathBuf,
    pub(crate) label: String,
    pub(crate) output: Option<Output>,
    indent: u8,
    cached: Mutex<Option<Cached>>,
}

struct Cached {
    source: Arc<str>,
    rendered: Result<Arc<Snapshot>, tgw::Error>,
}

/// One successful load. Its SVG and prepared diagram always describe the same
/// source, and can be retained while a later edit is invalid or unavailable.
pub struct Snapshot {
    pub source: Arc<str>,
    pub diagram: Arc<tgw::Diagram>,
    /// Compact, light SVG. Output-file indentation does not affect this value.
    pub svg: Arc<str>,
}

impl Job {
    /// Validate paths. `output = None` watches without exporting a file.
    /// Rejects output paths that alias the source, including hard links.
    pub fn new(input: PathBuf, output: Option<PathBuf>, indent: u8) -> Result<Self, String> {
        match fs::metadata(&input) {
            Ok(meta) if meta.is_dir() => {
                return Err(format!("{}: expected a diagram file", input.display()));
            }
            Ok(_) => {}
            Err(error) => return Err(format!("{}: {error}", input.display())),
        }
        if let Some(output) = &output {
            if output.is_dir() {
                return Err(format!(
                    "{}: expected a file, not a directory",
                    output.display()
                ));
            }
            let parent = match output.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent,
                _ => Path::new("."),
            };
            if !parent.is_dir() {
                return Err(format!("{}: directory is missing", parent.display()));
            }
            if crate::same_file(&input, output) {
                return Err(format!(
                    "{}: the output would overwrite the diagram",
                    output.display()
                ));
            }
        }
        Ok(Self {
            label: input.display().to_string(),
            input,
            output: output.map(Output::new),
            indent,
            cached: Mutex::new(None),
        })
    }

    /// Reads the source once its writer has finished, renders it, and
    /// refreshes the output file.
    pub(crate) fn load(&self, epoch: u64) -> Fresh {
        let before = source_stamp(&self.input);
        let loaded = match read_source(&self.input) {
            Ok(source) => self.render(source, epoch),
            Err(error) => Loaded::Unreadable(unreadable(&self.label, error)),
        };
        let stamp = source_stamp(&self.input);
        Fresh {
            loaded,
            stable: before == stamp,
            stamp,
        }
    }

    fn render(&self, source: String, epoch: u64) -> Loaded {
        let (source, rendered) = {
            let mut cached = lock(&self.cached);
            if !cached
                .as_ref()
                .is_some_and(|cached| *cached.source == source)
            {
                let source: Arc<str> = source.into();
                let rendered = tgw::Diagram::parse(&source).and_then(|diagram| {
                    let svg = diagram.render()?.into();
                    Ok(Arc::new(Snapshot {
                        source: Arc::clone(&source),
                        diagram: Arc::new(diagram),
                        svg,
                    }))
                });
                *cached = Some(Cached { source, rendered });
            }
            let cached = cached.as_ref().unwrap();
            (Arc::clone(&cached.source), cached.rendered.clone())
        };
        let written = match (&rendered, &self.output) {
            (Ok(snapshot), Some(output)) => Some(output.write(epoch, &self.file(&snapshot.svg))),
            _ => None,
        };
        Loaded::Text {
            source,
            rendered,
            written,
        }
    }

    /// The bytes `tgw INPUT -o OUTPUT` would write.
    fn file(&self, compact: &str) -> Vec<u8> {
        let mut bytes = if self.indent == 0 {
            compact.as_bytes().to_vec()
        } else {
            crate::w::prettify(compact.as_bytes(), self.indent)
        };
        crate::terminate(&mut bytes);
        bytes
    }
}

pub(crate) struct Fresh {
    pub(crate) loaded: Loaded,
    pub(crate) stamp: Option<SourceStamp>,
    pub(crate) stable: bool,
}

/// A file load and its optional export result. An unreadable or invalid source
/// leaves the output file alone; retain the last good [`Snapshot`] yourself.
pub enum Loaded {
    Text {
        source: Arc<str>,
        rendered: Result<Arc<Snapshot>, tgw::Error>,
        /// `Some(Ok(true))` when the output changed, `Some(Ok(false))` when
        /// already current, `Some(Err(_))` on an export failure, and `None`
        /// without an output file or a valid picture.
        written: Option<Result<bool, String>>,
    },
    Unreadable(String),
}

/// A cancellable, blocking file watcher. Drop it to release the OS watch and
/// join its notification thread. No work is performed between calls except
/// collecting OS notifications; polling and rendering happen on the caller.
pub struct Watcher {
    job: Job,
    watch: Option<Watch>,
    observed: Option<SourceStamp>,
    epoch: u64,
    pending: bool,
    retry_at: Option<Instant>,
}

impl Watcher {
    /// Begin watching; the first `next_update` loads the initial picture.
    /// Falls back to file polling if OS notifications are unavailable.
    pub fn new(job: Job) -> Self {
        Self {
            watch: watch(&job.input).ok(),
            job,
            observed: None,
            epoch: 0,
            pending: true,
            retry_at: None,
        }
    }

    /// Wait up to `timeout` for a change, or return `None`. Reading, settling
    /// an active save, rendering, and exporting may take additional time.
    /// Output failures retry at the polling interval, even without a new save.
    pub fn next_update(&mut self, timeout: Duration) -> Option<Loaded> {
        let started = Instant::now();
        loop {
            if self.pending {
                self.epoch += 1;
                let fresh = self.job.load(self.epoch);
                self.pending = !fresh.stable;
                if fresh.stable {
                    self.observed = fresh.stamp;
                }
                self.retry_at = matches!(
                    &fresh.loaded,
                    Loaded::Text {
                        written: Some(Err(_)),
                        ..
                    }
                )
                .then(|| Instant::now() + SOURCE_POLL);
                return Some(fresh.loaded);
            }
            if self.watch.is_none() {
                self.watch = watch(&self.job.input).ok();
            }
            let wait = timeout.saturating_sub(started.elapsed()).min(SOURCE_POLL);
            let signal = match &self.watch {
                Some(watch) => watch.gate.wait(wait),
                None => {
                    thread::sleep(wait);
                    None
                }
            };
            if signal == Some(false) {
                self.watch = None;
            }
            self.pending = if let Some(retry_at) = self.retry_at {
                Instant::now() >= retry_at
            } else {
                signal == Some(true) || source_stamp(&self.job.input) != self.observed
            };
            if !self.pending && started.elapsed() >= timeout {
                return None;
            }
        }
    }
}

pub(crate) struct Output {
    path: PathBuf,
    pub(crate) label: String,
    newest: Mutex<u64>,
}

impl Output {
    fn new(path: PathBuf) -> Self {
        Self {
            label: path.display().to_string(),
            path,
            newest: Mutex::new(0),
        }
    }

    /// Leaves the file alone when it already holds `bytes`, or when a load
    /// that started later has already written it.
    fn write(&self, epoch: u64, bytes: &[u8]) -> Result<bool, String> {
        let mut newest = lock(&self.newest);
        if epoch < *newest {
            return Ok(false);
        }
        *newest = epoch;
        if fs::read(&self.path).is_ok_and(|current| current == bytes) {
            return Ok(false);
        }
        replace(&self.path, bytes).map_err(|error| format!("{}: {error}", self.label))?;
        Ok(true)
    }
}

/// Readers of `path` see the old file or the new one, never a partial write.
/// Falls back to writing in place where a rename is refused, such as a file
/// held open on Windows or a directory without write access.
fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let existing = fs::metadata(&target).ok();
    if existing
        .as_ref()
        .is_some_and(|meta| meta.permissions().readonly())
    {
        return fs::write(&target, bytes);
    }
    let Some(name) = target.file_name() else {
        return fs::write(&target, bytes);
    };
    let mut staged_name = OsString::from(".");
    staged_name.push(name);
    staged_name.push(format!(".{}.tmp", std::process::id()));
    let staged = target.with_file_name(staged_name);
    let moved = fs::write(&staged, bytes).and_then(|()| {
        if let Some(meta) = &existing {
            fs::set_permissions(&staged, meta.permissions())?;
        }
        fs::rename(&staged, &target)
    });
    if moved.is_ok() {
        return Ok(());
    }
    let _ = fs::remove_file(&staged);
    fs::write(&target, bytes)
}

/// A closed stderr pipe must not panic the process that is still rewriting the file.
fn say(message: impl std::fmt::Display) {
    let _ = writeln!(io::stderr().lock(), "{message}");
}

/// Rewrites the output after every save and reports each change on stderr.
/// Runs until the process is stopped.
pub(crate) fn follow(job: Job) -> Result<(), String> {
    let output = job
        .output
        .as_ref()
        .ok_or("--watch needs -o PATH")?
        .label
        .clone();
    let label = job.label.clone();
    say(format!("tgw: watching {} (Ctrl-C to stop)", job.label));
    let mut watcher = Watcher::new(job);
    let mut reported: Option<String> = None;
    let mut first = true;
    loop {
        if let Some(loaded) = watcher.next_update(SOURCE_POLL) {
            let problem = match loaded {
                Loaded::Unreadable(message) => Some(message),
                Loaded::Text {
                    source,
                    rendered: Err(error),
                    ..
                } => Some(crate::diagnostic(&label, &source, &error)),
                Loaded::Text {
                    written: Some(Err(message)),
                    ..
                } => Some(message),
                Loaded::Text { written, .. } => {
                    if written == Some(Ok(true)) {
                        say(format!("tgw: wrote {output}"));
                    } else if first || reported.is_some() {
                        say(format!("tgw: {output} is up to date"));
                    }
                    None
                }
            };
            if let Some(problem) = &problem {
                if reported.as_ref() != Some(problem) {
                    say(format!("tgw: {problem}"));
                }
            }
            reported = problem;
            first = false;
        }
    }
}

/// The watch is owned by its consumer, not its notification thread. Dropping
/// it closes the event channel, wakes pending consumers, and joins the worker.
pub(crate) struct Watch {
    pub(crate) gate: Arc<Gate>,
    watcher: Option<RecommendedWatcher>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.watcher.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Watches the directory rather than the file, so editors that save by
/// replacing the file are still seen.
pub(crate) fn watch(path: &Path) -> Result<Watch, String> {
    let parents = watch_dirs(path)?;
    let names = watch_names(path)?;
    let parents_for_events = parents.clone();
    let (raw_tx, raw_rx) = mpsc::channel();
    let gate = Gate::new();
    let thread_gate = Arc::clone(&gate);
    let mut watcher = notify::recommended_watcher(move |result: Result<Event, notify::Error>| {
        let relevant = match result {
            Ok(event) => event_targets(&event, &parents_for_events, &names),
            Err(_) => true,
        };
        if relevant {
            let _ = raw_tx.send(());
        }
    })
    .map_err(|error| error.to_string())?;
    for parent in &parents {
        watcher
            .watch(parent, RecursiveMode::NonRecursive)
            .map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let worker = thread::Builder::new()
        .name("tgw-watch".into())
        .spawn(move || debounce(raw_rx, thread_gate))
        .map_err(|error| error.to_string())?;
    Ok(Watch {
        gate,
        watcher: Some(watcher),
        worker: Some(worker),
    })
}

fn watch_dirs(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut dirs = vec![existing_dir(path)?];
    if let Some(parent) = path.canonicalize().ok().and_then(|canonical| {
        canonical
            .parent()
            .map(Path::to_path_buf)
            .filter(|parent| parent.is_dir())
    }) {
        let parent = parent.canonicalize().unwrap_or(parent);
        if !dirs.iter().any(|dir| crate::paths_equal(dir, &parent)) {
            dirs.push(parent);
        }
    }
    Ok(dirs)
}

fn existing_dir(path: &Path) -> Result<PathBuf, String> {
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    if !parent.is_dir() {
        return Err(format!("{}: directory is missing", parent.display()));
    }
    parent
        .canonicalize()
        .map_err(|error| format!("{}: {error}", parent.display()))
}

fn watch_names(path: &Path) -> Result<Vec<OsString>, String> {
    let mut names = vec![path
        .file_name()
        .ok_or_else(|| format!("{}: expected a file name", path.display()))?
        .to_os_string()];
    if let Some(name) = path.canonicalize().ok().and_then(|canonical| {
        canonical
            .file_name()
            .map(|name| name.to_os_string())
            .filter(|name| {
                !names
                    .iter()
                    .any(|existing| crate::paths_equal(Path::new(existing), Path::new(name)))
            })
    }) {
        names.push(name);
    }
    Ok(names)
}

fn event_targets(event: &Event, parents: &[PathBuf], names: &[OsString]) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    event.paths.is_empty()
        || event.paths.iter().any(|path| {
            names.iter().any(|name| {
                path.file_name()
                    .is_some_and(|file| crate::paths_equal(Path::new(file), Path::new(name)))
            }) || parents
                .iter()
                .any(|parent| crate::paths_equal(parent, path))
        })
}

fn debounce(incoming: Receiver<()>, gate: Arc<Gate>) {
    while incoming.recv().is_ok() {
        let mut deadline = Instant::now() + DEBOUNCE;
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            match incoming.recv_timeout(remaining) {
                Ok(()) => deadline = Instant::now() + DEBOUNCE,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    gate.close();
                    return;
                }
            }
        }
        gate.signal();
    }
    gate.close();
}

/// A parked watch thread wakes its reader without occupying a worker-pool
/// thread. The window polls it as a future; `--watch` blocks on it.
pub(crate) struct Gate {
    ready: AtomicBool,
    closed: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl Gate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            ready: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            waker: Mutex::new(None),
        })
    }

    fn signal(&self) {
        self.ready.store(true, Ordering::Release);
        self.wake();
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.wake();
    }

    fn wake(&self) {
        if let Some(waker) = lock(&self.waker).take() {
            waker.wake();
        }
    }

    /// `Ready(true)` after a change, `Ready(false)` once the watcher stops.
    pub(crate) fn poll_wait(&self, cx: &mut TaskContext<'_>) -> Poll<bool> {
        if self.ready.swap(false, Ordering::AcqRel) {
            return Poll::Ready(true);
        }
        if self.closed.load(Ordering::Acquire) {
            return Poll::Ready(false);
        }
        *lock(&self.waker) = Some(cx.waker().clone());
        if self.ready.swap(false, Ordering::AcqRel) {
            return Poll::Ready(true);
        }
        if self.closed.load(Ordering::Acquire) {
            return Poll::Ready(false);
        }
        Poll::Pending
    }

    /// Blocks until `poll_wait` is ready; `None` when `timeout` passes first.
    fn wait(&self, timeout: Duration) -> Option<bool> {
        let waker = Waker::from(Arc::new(Unpark(thread::current())));
        let mut cx = TaskContext::from_waker(&waker);
        let deadline = Instant::now() + timeout;
        loop {
            if let Poll::Ready(open) = self.poll_wait(&mut cx) {
                return Some(open);
            }
            thread::park_timeout(deadline.checked_duration_since(Instant::now())?);
        }
    }
}

struct Unpark(thread::Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

#[derive(Debug)]
enum SourceError {
    Missing,
    Utf8,
    Io(io::Error),
}

fn unreadable(label: &str, error: SourceError) -> String {
    match error {
        SourceError::Missing => format!("{label}: file is missing"),
        SourceError::Utf8 => format!("{label}: file is not utf-8"),
        SourceError::Io(error) => format!("{label}: {error}"),
    }
}

fn read_source(path: &Path) -> Result<String, SourceError> {
    if let Some(settled) = read_settled(path) {
        return settled;
    }
    let deadline = Instant::now() + READ_DEADLINE;
    let mut last_text = None;
    let mut stable_since = None;
    let mut missing_reads = 0u8;
    let mut last_error = SourceError::Missing;
    loop {
        match fs::read(path) {
            Ok(bytes) => {
                missing_reads = 0;
                match String::from_utf8(bytes) {
                    Ok(text) => {
                        if last_text.as_ref() == Some(&text) {
                            let since = *stable_since.get_or_insert_with(Instant::now);
                            if since.elapsed() >= READ_SETTLE {
                                return Ok(text);
                            }
                        } else {
                            last_text = Some(text);
                            stable_since = Some(Instant::now());
                        }
                    }
                    Err(_) => return Err(SourceError::Utf8),
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                missing_reads += 1;
                stable_since = None;
                last_error = SourceError::Missing;
                if missing_reads >= 2 && last_text.is_none() {
                    return Err(SourceError::Missing);
                }
            }
            Err(error) if transient(&error) => {
                stable_since = None;
                last_error = SourceError::Io(error);
            }
            Err(error) => return Err(SourceError::Io(error)),
        }
        if Instant::now() >= deadline {
            return last_text.ok_or(last_error);
        }
        thread::sleep(READ_PAUSE);
    }
}

/// A file left alone since the last save can be read immediately. Two identical
/// reads reject a torn in-place write; a fresh modification falls through to
/// the settle loop.
fn read_settled(path: &Path) -> Option<Result<String, SourceError>> {
    if !file_is_settled(path) {
        return None;
    }
    let first = fs::read(path).ok()?;
    if !file_is_settled(path) {
        return None;
    }
    let second = fs::read(path).ok()?;
    if first != second || !file_is_settled(path) {
        return None;
    }
    Some(String::from_utf8(first).map_err(|_| SourceError::Utf8))
}

fn file_is_settled(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age >= READ_SETTLE)
}

/// Stats the source every [`SOURCE_POLL`] on its own thread, so a stalled
/// drive delays the answer instead of blocking the caller. The thread ends
/// once the poller is dropped.
#[cfg(feature = "view")]
pub(crate) struct StampPoller(Arc<Mutex<Option<Option<SourceStamp>>>>);

#[cfg(feature = "view")]
impl StampPoller {
    pub(crate) fn start(path: PathBuf) -> io::Result<Self> {
        let latest = Arc::new(Mutex::new(None));
        let shared = Arc::downgrade(&latest);
        thread::Builder::new()
            .name("tgw-poll".into())
            .spawn(move || loop {
                let stamp = source_stamp(&path);
                let Some(latest) = shared.upgrade() else {
                    break;
                };
                *lock(&latest) = Some(stamp);
                drop(latest);
                thread::sleep(SOURCE_POLL);
            })?;
        Ok(Self(latest))
    }

    /// The most recent stamp, or `None` before the first poll completes.
    pub(crate) fn latest(&self) -> Option<Option<SourceStamp>> {
        *lock(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourceStamp {
    len: u64,
    modified: u128,
    identity: u128,
}

pub(crate) fn source_stamp(path: &Path) -> Option<SourceStamp> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|time| time.as_nanos())
        .unwrap_or(0);
    Some(SourceStamp {
        len: meta.len(),
        modified,
        identity: crate::file_identity(path).unwrap_or(0),
    })
}

/// Errors an editor causes while it is still saving. Windows reports a file
/// another process holds open as a sharing (32) or lock (33) violation.
fn transient(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::Interrupted | ErrorKind::WouldBlock | ErrorKind::PermissionDenied
    ) || (cfg!(windows) && matches!(error.raw_os_error(), Some(32 | 33)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, EventAttributes, ModifyKind};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tgw-live-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn age(path: &Path) {
        let file = fs::File::options().write(true).open(path).unwrap();
        file.set_times(
            fs::FileTimes::new()
                .set_modified(std::time::SystemTime::now() - Duration::from_secs(2)),
        )
        .unwrap();
    }

    #[test]
    fn watcher_accepts_the_diagram_name_and_ignores_reads() {
        let parent = PathBuf::from("/diagrams");
        let names = vec![OsString::from("demo.tgw"), OsString::from("target.tgw")];
        let parents = [parent.clone()];
        let event = |kind, paths| Event {
            kind,
            paths,
            attrs: EventAttributes::default(),
        };
        let modify = EventKind::Modify(ModifyKind::Any);
        assert!(event_targets(
            &event(modify, vec![parent.join("demo.tgw")]),
            &parents,
            &names
        ));
        assert!(event_targets(
            &event(
                modify,
                vec![parent.join("demo.tgw.tmp"), parent.join("demo.tgw")]
            ),
            &parents,
            &names
        ));
        assert!(event_targets(
            &event(modify, vec![parent.clone()]),
            &parents,
            &names
        ));
        assert!(event_targets(
            &event(modify, vec![parent.join("target.tgw")]),
            &parents,
            &names
        ));
        assert!(!event_targets(
            &event(
                EventKind::Access(AccessKind::Read),
                vec![parent.join("demo.tgw")]
            ),
            &parents,
            &names
        ));
        assert!(!event_targets(
            &event(modify, vec![parent.join("other.tgw")]),
            &parents,
            &names
        ));
    }

    #[test]
    fn read_source_waits_until_bytes_settle() {
        let dir = scratch("settle");
        let path = dir.join("a.tgw");
        fs::write(&path, "clk: p\n").unwrap();
        let started = Instant::now();
        assert_eq!(read_source(&path).unwrap(), "clk: p\n");
        assert!(started.elapsed() >= READ_SETTLE);

        let missing = dir.join("missing.tgw");
        let started = Instant::now();
        assert!(matches!(read_source(&missing), Err(SourceError::Missing)));
        assert!(started.elapsed() < READ_DEADLINE);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_source_returns_a_settled_file_without_waiting() {
        let dir = scratch("old");
        let path = dir.join("a.tgw");
        fs::write(&path, "clk: p\n").unwrap();
        age(&path);
        let started = Instant::now();
        assert_eq!(read_source(&path).unwrap(), "clk: p\n");
        assert!(started.elapsed() < READ_SETTLE);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_writes_the_cli_output_and_keeps_it_through_errors() {
        let dir = scratch("load");
        let input = dir.join("a.tgw");
        let output = dir.join("a.svg");
        fs::write(&input, "clk: p...\n").unwrap();
        age(&input);
        let job = Job::new(input.clone(), Some(output.clone()), 2).unwrap();
        let fresh = job.load(1);
        assert!(fresh.stable);
        assert!(matches!(
            fresh.loaded,
            Loaded::Text {
                rendered: Ok(_),
                written: Some(Ok(true)),
                ..
            }
        ));
        let expected = crate::render_file("clk: p...\n", 2).unwrap();
        assert_eq!(fs::read(&output).unwrap(), expected);
        assert!(matches!(
            job.load(2).loaded,
            Loaded::Text {
                written: Some(Ok(false)),
                ..
            }
        ));

        fs::write(&input, "clk: p?\n").unwrap();
        age(&input);
        let Loaded::Text {
            rendered, written, ..
        } = job.load(3).loaded
        else {
            panic!("the source is readable");
        };
        assert!(rendered.is_err());
        assert!(written.is_none());
        assert_eq!(fs::read(&output).unwrap(), expected);

        fs::remove_file(&input).unwrap();
        assert!(matches!(job.load(4).loaded, Loaded::Unreadable(_)));
        assert_eq!(fs::read(&output).unwrap(), expected);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stale_load_never_replaces_a_newer_output() {
        let dir = scratch("stale");
        let output = Output::new(dir.join("a.svg"));
        assert_eq!(output.write(2, b"new"), Ok(true));
        assert_eq!(output.write(1, b"old"), Ok(false));
        assert_eq!(fs::read(dir.join("a.svg")).unwrap(), b"new");
        assert_eq!(output.write(3, b"new"), Ok(false));
        assert_eq!(output.write(4, b"newer"), Ok(true));
        assert_eq!(fs::read(dir.join("a.svg")).unwrap(), b"newer");
        let leftovers = fs::read_dir(&dir).unwrap().count();
        assert_eq!(leftovers, 1, "the staging file is renamed into place");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn output_must_not_be_the_diagram() {
        let dir = scratch("same");
        let input = dir.join("a.tgw");
        fs::write(&input, "clk: p\n").unwrap();
        let aliased = dir.join(".").join("a.tgw");
        assert!(Job::new(input.clone(), Some(aliased), 0).is_err());
        assert!(Job::new(dir.clone(), None, 0).is_err());
        assert!(Job::new(input.clone(), Some(dir.clone()), 0).is_err());
        assert!(Job::new(dir.join("typo.tgw"), None, 0).is_err());
        assert!(Job::new(input.clone(), Some(dir.join("missing").join("a.svg")), 0).is_err());
        assert!(Job::new(input, Some(dir.join("a.svg")), 0).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "view")]
    #[test]
    fn stamp_poller_reports_changes_and_stops_with_its_owner() {
        let dir = scratch("poller");
        let path = dir.join("a.tgw");
        fs::write(&path, "clk: p\n").unwrap();
        let poller = StampPoller::start(path.clone()).unwrap();
        let settle = |expected: Option<SourceStamp>| {
            let started = Instant::now();
            while poller.latest() != Some(expected) {
                assert!(started.elapsed() < Duration::from_secs(10), "{expected:?}");
                thread::sleep(Duration::from_millis(10));
            }
        };
        settle(source_stamp(&path));
        fs::write(&path, "clk: p...\n").unwrap();
        settle(source_stamp(&path));
        fs::remove_file(&path).unwrap();
        settle(None);
        let shared = Arc::downgrade(&poller.0);
        drop(poller);
        let started = Instant::now();
        while shared.strong_count() > 0 {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "poller kept alive"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
