use std::env;
use std::fs;
use std::io::{self, Read};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tgw: {e}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut input: Option<String> = None;
    let mut indent: u8 = 0;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!(
                    "Usage: tgw --input <path> [--indent <n>]\n       tgw [--indent <n>] < file.json5"
                );
                return Ok(());
            }
            "-v" | "--version" => {
                println!("{}", tgw::VERSION);
                return Ok(());
            }
            "-i" | "--input" => {
                input = Some(args.next().ok_or("--input needs a path")?);
            }
            "-t" | "--indent" => {
                let n = args.next().ok_or("--indent needs a number")?;
                indent = n.parse::<u8>().map_err(|_| "indent must be a number".to_string())?;
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let source = if let Some(path) = input {
        fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?
    } else {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf).map_err(|e| e.to_string())?;
        buf
    };
    let mut out = Vec::new();
    tgw::render_opts(&source, &mut out, indent).map_err(|e| e.to_string())?;
    let text = String::from_utf8(out).map_err(|_| "svg was not utf-8".to_string())?;
    println!("{text}");
    Ok(())
}
