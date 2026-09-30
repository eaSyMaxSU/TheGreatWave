#![deny(unsafe_code)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use tgw::InputFormat;

#[cfg(feature = "view")]
mod layout;
#[cfg(feature = "watch")]
mod live;
#[cfg(feature = "view")]
mod slice;
#[cfg(feature = "view")]
mod view;

const HELP: &str = "\
The Great Wave — ASIC timing diagrams in a text language people and agents can both write, rendered as compact SVG.

Usage: tgw [OPTIONS] [INPUT]

Renders a .tgw or WaveJSON diagram to SVG. With --watch or --view, tgw keeps
running and brings the output file, and the window, up to date every time
INPUT is saved.

Arguments:
  INPUT                Diagram to read (.tgw or WaveJSON); omit or use - for stdin

Options:
  -i, --input PATH     Input file (alternative to positional INPUT)
  -o, --output PATH    Write output to a file; omit or use - for stdout
      --format FORMAT  Input syntax: auto (default), tgw, or json5
      --convert        Convert input to readable, canonical .tgw text
  -t, --indent N       Indent SVG output by N spaces (default: compact)
  -w, --watch          Keep running and rewrite OUTPUT whenever INPUT is saved
      --view           Open a window that redraws whenever INPUT is saved;
                       with -o, OUTPUT is rewritten on every save as well
  -h, --help           Show this help
  -v, --version        Show version

Live mode:
  --watch and --view need an INPUT file; --watch also needs -o PATH. OUTPUT
  holds the same bytes a one-off `tgw INPUT -o OUTPUT` writes, is replaced in
  one step, and is left alone when nothing changed. A syntax error or a missing
  INPUT keeps the last good picture and output and is reported as
  path:line:col until the next good save.

  In the window, signal names stay fixed while the waveforms scroll: use the
  wheel or trackpad, hold Shift to scroll sideways, or drag a scrollbar.
  The window follows the system appearance. Ctrl-Shift-L switches between
  the default light and default dark colors (Command-Shift-L on macOS) and
  keeps that choice. Ctrl-W or Ctrl-Q closes it (Cmd-W or Cmd-Q on macOS).
  --watch runs until Ctrl-C.

Examples:
  tgw diagram.tgw -o diagram.svg
  tgw diagram.tgw -o diagram.svg --view
  tgw diagram.tgw -o diagram.svg --watch
  tgw diagram.tgw --view
  tgw legacy.json5 --convert -o diagram.tgw
  tgw --format tgw < diagram.tgw > diagram.svg

Language:
  One signal per line: name: wave => labels ; node=
  A # comment is kept. // is not a .tgw comment.
  Wave symbols: p n P N h l H L 0 1 x d u z = 2-9
  . repeats, | is a gap, and <...> is half a cycle.
  node=.a.b. names cycles with one letter. . skips a cycle.
  An uppercase letter is a node with no label. [setup] is one cycle with a word.
  @edge setup~>hold \"tSU\" draws an arrow between two nodes.
  A missing node or an unknown connector is an error.
";

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Render,
    Convert,
    Help,
    Version,
}

struct Options {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    format: InputFormat,
    indent: u8,
    action: Action,
    watch: bool,
    view: bool,
}

fn help() -> String {
    let mut text = String::from(HELP);
    if cfg!(not(feature = "watch")) {
        text.push_str("\nThis build renders only; --watch and --view need the default features.\n");
    } else if cfg!(not(feature = "view")) {
        text.push_str("\nThis build has no window; --view needs the `view` feature.\n");
    }
    text
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "tgw: {error}");
            ExitCode::from(1)
        }
    }
}

fn options(args: impl IntoIterator<Item = OsString>) -> Result<Options, String> {
    let mut config = Options {
        input: None,
        output: None,
        format: InputFormat::Auto,
        indent: 0,
        action: Action::Render,
        watch: false,
        view: false,
    };
    let mut args = args.into_iter();
    let mut positional_only = false;
    while let Some(arg) = args.next() {
        if !positional_only && arg == "--" {
            positional_only = true;
            continue;
        }
        let text = arg.to_str();
        if !positional_only && text.is_some_and(|s| s.starts_with('-') && s != "-") {
            let text = text.unwrap();
            let (flag, inline) = text
                .split_once('=')
                .map_or((text, None), |(a, b)| (a, Some(b)));
            match flag {
                "-h" | "--help" if inline.is_none() => {
                    config.action = Action::Help;
                    return Ok(config);
                }
                "-v" | "--version" if inline.is_none() => {
                    config.action = Action::Version;
                    return Ok(config);
                }
                "--convert" if inline.is_none() => config.action = Action::Convert,
                "-w" | "--watch" if inline.is_none() => config.watch = true,
                "--view" if inline.is_none() => config.view = true,
                "-i" | "--input" | "-o" | "--output" | "--format" | "-t" | "--indent" => {
                    let value = match inline {
                        Some(value) => OsString::from(value),
                        None => args.next().ok_or_else(|| format!("{flag} needs a value"))?,
                    };
                    if value.is_empty() {
                        return Err(format!("{flag} needs a value"));
                    }
                    match flag {
                        "-i" | "--input" => {
                            if config.input.is_some() {
                                return Err("provide only one input file".into());
                            }
                            config.input = Some(value.into());
                        }
                        "-o" | "--output" => {
                            if config.output.is_some() {
                                return Err("provide only one output file".into());
                            }
                            config.output = Some(value.into());
                        }
                        "--format" => {
                            config.format = match value.to_str() {
                                Some("auto") => InputFormat::Auto,
                                Some("tgw") => InputFormat::Tgw,
                                Some("json5") => InputFormat::Json5,
                                _ => return Err("--format must be auto, tgw, or json5".into()),
                            }
                        }
                        "-t" | "--indent" => {
                            config.indent = value
                                .to_str()
                                .and_then(|s| s.parse().ok())
                                .ok_or("--indent must be an integer from 0 to 255")?;
                        }
                        _ => unreachable!(),
                    }
                }
                _ => return Err(format!("unknown option {text}; use --help for usage")),
            }
        } else {
            if config.input.is_some() {
                return Err("provide only one input file".into());
            }
            config.input = Some(arg.into());
        }
    }
    if config.watch || config.view {
        let is_file = |path: &Option<PathBuf>| path.as_ref().is_some_and(|p| p.as_os_str() != "-");
        if config.action == Action::Convert {
            return Err("--convert cannot be combined with --watch or --view".into());
        }
        if !is_file(&config.input) {
            return Err("--watch and --view need an input file, not stdin".into());
        }
        if config.output.is_some() && !is_file(&config.output) {
            return Err("--watch and --view write to a file, not stdout".into());
        }
        if !config.view && config.output.is_none() {
            return Err("--watch needs -o PATH".into());
        }
    }
    Ok(config)
}

fn run() -> Result<(), String> {
    let options = options(env::args_os().skip(1))?;
    match options.action {
        Action::Help => return stdout(help().as_bytes()),
        Action::Version => return stdout(format!("{}\n", tgw::VERSION).as_bytes()),
        _ => {}
    }
    if options.watch || options.view {
        return live(options);
    }
    if options.action == Action::Render {
        if let (Some(input), Some(output)) = (
            options
                .input
                .as_ref()
                .filter(|path| path.as_os_str() != "-"),
            options
                .output
                .as_ref()
                .filter(|path| path.as_os_str() != "-"),
        ) {
            if same_file(input, output) {
                return Err(format!(
                    "{}: the output would overwrite the diagram",
                    output.display()
                ));
            }
        }
    }
    let label = options
        .input
        .as_ref()
        .filter(|p| p.as_os_str() != "-")
        .map_or_else(|| "<stdin>".into(), |p| p.display().to_string());
    let source = if let Some(path) = options.input.as_ref().filter(|p| p.as_os_str() != "-") {
        fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?
    } else {
        let mut source = String::new();
        io::stdin()
            .read_to_string(&mut source)
            .map_err(|e| format!("stdin: {e}"))?;
        source
    };
    let output = if options.action == Action::Convert {
        tgw::to_tgw_with_format(&source, options.format).map(|text| {
            let mut bytes = text.into_bytes();
            terminate(&mut bytes);
            bytes
        })
    } else {
        render_file(&source, options.format, options.indent)
    }
    .map_err(|e| diagnostic(&label, &source, &e))?;
    if let Some(path) = options.output.as_ref().filter(|p| p.as_os_str() != "-") {
        fs::write(path, &output).map_err(|e| format!("{}: {e}", path.display()))
    } else {
        stdout(&output)
    }
}

#[cfg(feature = "watch")]
fn live(options: Options) -> Result<(), String> {
    let input = options
        .input
        .ok_or("--watch and --view need an input file")?;
    let job = live::Job::new(input, options.output, options.format, options.indent)?;
    if options.view {
        #[cfg(feature = "view")]
        return view::run(job);
        #[cfg(not(feature = "view"))]
        return Err("--view needs a build with the `view` feature, which is on by default".into());
    }
    live::follow(job)
}

#[cfg(not(feature = "watch"))]
fn live(_: Options) -> Result<(), String> {
    Err("--watch and --view need a build with the default features".into())
}

/// The bytes `tgw INPUT -o OUTPUT` writes for a rendered diagram.
fn render_file(source: &str, format: InputFormat, indent: u8) -> Result<Vec<u8>, tgw::Error> {
    let mut output = Vec::new();
    tgw::render_with_format(source, &mut output, indent, format)?;
    terminate(&mut output);
    Ok(output)
}

/// True when writing `output` would truncate the diagram, including through a
/// different path, a symlink, a hard link, or another case on Windows.
pub(crate) fn same_file(input: &Path, output: &Path) -> bool {
    if let (Some(left), Some(right)) = (file_identity(input), file_identity(output)) {
        if left == right {
            return true;
        }
    }
    fn resolve(path: &Path) -> Option<PathBuf> {
        if let Ok(path) = path.canonicalize() {
            return Some(path);
        }
        let parent = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
        Some(parent.canonicalize().ok()?.join(path.file_name()?))
    }
    match (resolve(input), resolve(output)) {
        (Some(input), Some(output)) => paths_equal(&input, &output),
        _ => paths_equal(input, output),
    }
}

/// Device and file id, so a replaced file is distinct even when its size and
/// timestamp are unchanged. `None` when the path cannot be opened.
pub(crate) fn file_identity(path: &Path) -> Option<u128> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = fs::metadata(path).ok()?;
        Some(((meta.dev() as u128) << 64) | meta.ino() as u128)
    }
    #[cfg(windows)]
    {
        windows_file_id(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        None
    }
}

/// Path equality, ignoring ASCII-and-Unicode case on Windows.
pub(crate) fn paths_equal(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        windows_paths_equal(left, right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

/// `GetFileInformationByHandle` is the stable way to read the file index.
/// `MetadataExt::file_index` is still nightly-only (`windows_by_handle`).
#[cfg(windows)]
#[allow(unsafe_code)]
fn windows_file_id(path: &Path) -> Option<u128> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;

    const FILE_READ_ATTRIBUTES: u32 = 0x80;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    let file = fs::OpenOptions::new()
        .read(true)
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .ok()?;

    #[allow(dead_code)]
    #[repr(C)]
    struct ByHandleFileInformation {
        file_attributes: u32,
        creation_low: u32,
        creation_high: u32,
        access_low: u32,
        access_high: u32,
        write_low: u32,
        write_high: u32,
        volume_serial_number: u32,
        file_size_high: u32,
        file_size_low: u32,
        number_of_links: u32,
        file_index_high: u32,
        file_index_low: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandle(
            file: *mut std::ffi::c_void,
            information: *mut ByHandleFileInformation,
        ) -> i32;
    }

    let mut info = ByHandleFileInformation {
        file_attributes: 0,
        creation_low: 0,
        creation_high: 0,
        access_low: 0,
        access_high: 0,
        write_low: 0,
        write_high: 0,
        volume_serial_number: 0,
        file_size_high: 0,
        file_size_low: 0,
        number_of_links: 0,
        file_index_high: 0,
        file_index_low: 0,
    };
    // SAFETY: `file` owns the handle for this call, and `info` is a valid
    // out-buffer of the documented struct.
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
    if ok == 0 {
        return None;
    }
    let index = ((info.file_index_high as u64) << 32) | u64::from(info.file_index_low);
    Some((u128::from(info.volume_serial_number) << 64) | u128::from(index))
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn windows_paths_equal(left: &Path, right: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;

    let left: Vec<u16> = left.as_os_str().encode_wide().collect();
    let right: Vec<u16> = right.as_os_str().encode_wide().collect();
    let (Ok(left_len), Ok(right_len)) = (i32::try_from(left.len()), i32::try_from(right.len()))
    else {
        return false;
    };
    const CSTR_EQUAL: i32 = 2;
    #[link(name = "kernel32")]
    extern "system" {
        fn CompareStringOrdinal(
            left: *const u16,
            left_len: i32,
            right: *const u16,
            right_len: i32,
            ignore_case: i32,
        ) -> i32;
    }
    // SAFETY: both buffers live for the call and the lengths match them.
    let order =
        unsafe { CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) };
    order == CSTR_EQUAL
}

fn terminate(output: &mut Vec<u8>) {
    if !output.ends_with(b"\n") {
        output.push(b'\n');
    }
}

fn stdout(bytes: &[u8]) -> Result<(), String> {
    match io::stdout().lock().write_all(bytes) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(format!("stdout: {error}")),
    }
}

fn diagnostic(label: &str, source: &str, error: &tgw::Error) -> String {
    let mut offset = error.offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &source[..offset];
    let line = before.bytes().filter(|&b| b == b'\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    let end = source[offset..]
        .find('\n')
        .map_or(source.len(), |i| offset + i);
    let column = source[start..offset].chars().count();
    let left = column.saturating_sub(60);
    let mut snippet = if left == 0 {
        String::new()
    } else {
        "…".into()
    };
    let mut caret = usize::from(left != 0);
    let mut shown = caret;
    for (i, c) in source[start..end]
        .trim_end_matches('\r')
        .chars()
        .enumerate()
        .skip(left)
        .take(160)
    {
        let width = if c == '\t' { 4 - shown % 4 } else { 1 };
        if c == '\t' {
            snippet.extend(std::iter::repeat_n(' ', width));
        } else if c.is_control() {
            snippet.push('�');
        } else {
            snippet.push(c);
        }
        if i < column {
            caret += width;
        }
        shown += width;
    }
    if source[start..end].chars().count() > left + 160 {
        snippet.push('…');
    }
    let gutter = " ".repeat(line.to_string().len());
    format!(
        "{label}:{line}:{}: {}\n {line} | {snippet}\n {gutter} | {}^",
        column + 1,
        error.message,
        " ".repeat(caret)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        options(args.iter().map(OsString::from))
    }

    #[test]
    fn positional_input_output_conversion_and_explicit_format() {
        let parsed = parse(&[
            "diagram.json5",
            "--convert",
            "-o",
            "diagram.tgw",
            "--format=json5",
        ])
        .unwrap();
        assert_eq!(parsed.input, Some(PathBuf::from("diagram.json5")));
        assert_eq!(parsed.output, Some(PathBuf::from("diagram.tgw")));
        assert_eq!(parsed.action, Action::Convert);
        assert!(matches!(parsed.format, InputFormat::Json5));
        assert_eq!(
            parse(&["--", "-named.tgw"]).unwrap().input,
            Some(PathBuf::from("-named.tgw"))
        );
        assert_eq!(
            parse(&["--input=-", "--output=-", "--indent=2"])
                .unwrap()
                .indent,
            2
        );
    }

    #[test]
    fn malformed_options_fail_without_silently_ignoring_paths() {
        for args in [
            vec!["a", "b"],
            vec!["a", "-i", "b"],
            vec!["-o", "a", "-o", "b"],
            vec!["--input"],
            vec!["--format", "xml"],
            vec!["--indent", "-1"],
            vec!["--indent=256"],
            vec!["--unknown"],
            vec!["--convert=yes"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn live_mode_needs_a_file_to_watch_and_a_file_to_write() {
        let watch = parse(&["a.tgw", "-o", "a.svg", "--watch"]).unwrap();
        assert!(watch.watch && !watch.view);
        let view = parse(&["--view", "a.tgw"]).unwrap();
        assert!(view.view && view.output.is_none());
        assert!(parse(&["-w", "--view", "-i", "a.tgw", "-o", "a.svg", "-t", "2"]).is_ok());
        for args in [
            vec!["a.tgw", "--watch"],
            vec!["--watch", "-o", "a.svg"],
            vec!["-", "--view"],
            vec!["a.tgw", "-o", "-", "--watch"],
            vec!["a.tgw", "--view", "-o", "-"],
            vec!["a.json5", "--convert", "-o", "a.tgw", "--watch"],
            vec!["a.tgw", "--view=yes"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn help_documents_every_option() {
        let text = help();
        for flag in [
            "--input",
            "--output",
            "--format",
            "--convert",
            "--indent",
            "--watch",
            "--view",
            "--help",
            "--version",
        ] {
            assert!(text.contains(flag), "{flag}");
        }
        assert!(parse(&["--help", "--watch"]).unwrap().action == Action::Help);
        assert!(text.contains("people and agents"));
        assert!(text.contains("name: wave => labels ; node="));
        assert!(text.contains("p n P N h l H L 0 1 x d u z = 2-9"));
        assert!(text.contains("[setup]"));
        assert!(text.contains("@edge setup~>hold \"tSU\""));
        assert!(text.contains("A # comment"));
    }

    #[test]
    fn render_refuses_to_overwrite_the_diagram() {
        let dir = std::env::temp_dir().join(format!("tgw-self-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("a.tgw");
        fs::write(&input, "clk: p\n").unwrap();
        assert!(same_file(&input, &dir.join(".").join("a.tgw")));
        assert!(!same_file(&input, &dir.join("a.svg")));
        #[cfg(unix)]
        {
            let link = dir.join("link.tgw");
            std::os::unix::fs::symlink(&input, &link).unwrap();
            assert!(same_file(&link, &input));
            let hard = dir.join("hard.tgw");
            fs::hard_link(&input, &hard).unwrap();
            assert!(same_file(&input, &hard));
        }
        #[cfg(windows)]
        {
            let hard = dir.join("hard.tgw");
            fs::hard_link(&input, &hard).unwrap();
            assert!(same_file(&input, &hard));
            assert!(same_file(&input, &dir.join("A.TGW")));
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn diagnostics_show_unicode_line_column_and_a_bounded_excerpt() {
        let source = "@title test\n时钟: p?\n";
        let error = tgw::Error {
            offset: source.find('?').unwrap(),
            message: "unknown symbol".into(),
        };
        let result = diagnostic("clock.tgw", source, &error);
        assert!(result.starts_with("clock.tgw:2:6: unknown symbol\n"));
        assert!(result.contains("2 | 时钟: p?"));
        assert!(result.ends_with("     ^"));
        let source = "x".repeat(10_000);
        let error = tgw::Error {
            offset: 9000,
            message: "bad value".into(),
        };
        assert!(diagnostic("test", &source, &error).len() < 300);
    }
}
