#![deny(unsafe_code)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use tgw::InputFormat;

#[cfg(feature = "watch")]
mod live;
#[cfg(feature = "view")]
mod view;

const HELP: &str = "\
The Great Wave — compact timing diagrams

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
  Ctrl-W or Ctrl-Q closes it (Cmd-W or Cmd-Q on macOS). --watch runs until
  Ctrl-C.

Examples:
  tgw diagram.tgw -o diagram.svg
  tgw diagram.tgw -o diagram.svg --view
  tgw diagram.tgw -o diagram.svg --watch
  tgw diagram.tgw --view
  tgw legacy.json5 --convert -o diagram.tgw
  tgw --format tgw < diagram.tgw > diagram.svg
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
/// different path, a symlink, or a hard link.
fn same_file(input: &Path, output: &Path) -> bool {
    if let (Ok(left), Ok(right)) = (fs::metadata(input), fs::metadata(output)) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if left.dev() == right.dev() && left.ino() == right.ino() {
                return true;
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if left.file_index().is_some()
                && left.file_index() == right.file_index()
                && left.volume_serial_number() == right.volume_serial_number()
            {
                return true;
            }
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
        (Some(input), Some(output)) => input == output,
        _ => input == output,
    }
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
