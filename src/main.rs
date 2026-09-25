use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use tgw::InputFormat;

const HELP: &str = "The Great Wave — compact timing diagrams\n\
\n\
Usage: tgw [OPTIONS] [INPUT]\n\
\n\
  INPUT                  Read .tgw or WaveJSON; omit or use - for stdin\n\
  -i, --input PATH        Input file (alternative to positional INPUT)\n\
  -o, --output PATH       Write output to a file; omit or use - for stdout\n\
      --format FORMAT    Input syntax: auto (default), tgw, or json5\n\
      --convert          Convert input to readable, canonical .tgw text\n\
  -t, --indent N          Indent SVG output by N spaces (default: compact)\n\
  -h, --help              Show this help\n\
  -v, --version           Show version\n\
\n\
Examples:\n\
  tgw diagram.tgw -o diagram.svg\n\
  tgw legacy.json5 --convert -o diagram.tgw\n\
  tgw --format tgw < diagram.tgw > diagram.svg\n";

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
    Ok(config)
}

fn run() -> Result<(), String> {
    let options = options(env::args_os().skip(1))?;
    match options.action {
        Action::Help => return stdout(HELP.as_bytes()),
        Action::Version => return stdout(format!("{}\n", tgw::VERSION).as_bytes()),
        _ => {}
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
    let mut output = Vec::new();
    let result = if options.action == Action::Convert {
        tgw::to_tgw_with_format(&source, options.format)
            .map(|text| output.extend_from_slice(text.as_bytes()))
    } else {
        tgw::render_with_format(&source, &mut output, options.indent, options.format)
    };
    result.map_err(|e| diagnostic(&label, &source, &e))?;
    if !output.ends_with(b"\n") {
        output.push(b'\n');
    }
    if let Some(path) = options.output.as_ref().filter(|p| p.as_os_str() != "-") {
        fs::write(path, &output).map_err(|e| format!("{}: {e}", path.display()))
    } else {
        stdout(&output)
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
