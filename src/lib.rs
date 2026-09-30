//! The Great Wave renders native `.tgw` and WaveJSON timing diagrams to SVG.
//!
//! Copyright (c) 2026 eaSyMaxSU. Licensed under the MIT License; see `LICENSE`.
//!
//! Wave rules and the default palette come from WaveDrom
//! (Copyright 2011–2026 Aliaksei Chapyzhenka, MIT):
//! <https://wavedrom.com/>, <https://github.com/wavedrom/wavedrom>.
//! WaveJSON: <https://github.com/wavedrom/schema>.
//! JSON5 subset: <https://spec.json5.org/>.
//! SVG arc geometry: <https://www.w3.org/TR/SVG2/implnote.html#ArcImplementationNotes>.
#![forbid(unsafe_code)]

mod emit;
mod format;
mod geom;
mod native;
mod path;
mod scan;
pub mod scheme;
mod w;
mod wave;
mod width;

pub use width::text_width;

pub(crate) const XS: i64 = 20;
pub(crate) const YS: i64 = 20;
pub(crate) const YO: i64 = 36;
pub(crate) const YM: i64 = 15;
pub(crate) const Y0: i64 = 5;
pub(crate) const XLABEL: i64 = 6;
pub(crate) const TGO: i64 = -10;

#[derive(Debug)]
pub struct Error {
    pub offset: usize,
    pub message: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for Error {}

/// Render `source` into a new SVG document.
pub fn render(source: &str) -> Result<String, Error> {
    let mut buf = Vec::new();
    render_into(source, &mut buf)?;
    String::from_utf8(buf).map_err(|_| Error {
        offset: 0,
        message: "svg was not utf-8".to_string(),
    })
}

/// Clear `out` and write an SVG document into it.
pub fn render_into(source: &str, out: &mut Vec<u8>) -> Result<(), Error> {
    render_opts(source, out, 0)
}

pub fn render_opts(source: &str, out: &mut Vec<u8>, indent: u8) -> Result<(), Error> {
    render_with_format(source, out, indent, InputFormat::Auto)
}

/// Input syntax. Auto recognizes legacy JSON5 objects and native `.tgw` text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputFormat {
    #[default]
    Auto,
    Tgw,
    Json5,
}

fn parse(source: &str, format: InputFormat) -> Result<scan::Doc, Error> {
    let trimmed = source.trim_start_matches('\u{feff}').trim_start();
    let json = match format {
        InputFormat::Auto => {
            trimmed.starts_with(['{', '['])
                || trimmed.starts_with("//")
                || trimmed.starts_with("/*")
        }
        InputFormat::Json5 => true,
        InputFormat::Tgw => false,
    };
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    if json {
        scan::parse(source)
    } else {
        native::parse(source)
    }
}

/// Clear and refill an output buffer, choosing input syntax explicitly.
pub fn render_with_format(
    source: &str,
    out: &mut Vec<u8>,
    indent: u8,
    format: InputFormat,
) -> Result<(), Error> {
    out.clear();
    let doc = parse(source, format)?;
    if let Err(e) = emit::write(&doc, out, indent) {
        out.clear();
        return Err(e);
    }
    Ok(())
}

/// Render `source` with `scheme`. File output uses [`render_with_format`], which
/// is the light palette. A dark window calls this for the picture only.
pub fn render_themed(
    source: &str,
    out: &mut Vec<u8>,
    indent: u8,
    format: InputFormat,
    scheme: &'static scheme::Scheme,
) -> Result<(), Error> {
    out.clear();
    let doc = parse(source, format)?;
    if let Err(e) = emit::write_themed(&doc, out, indent, scheme) {
        out.clear();
        return Err(e);
    }
    Ok(())
}

/// Convert JSON5 or native text to canonical, human-readable `.tgw` syntax.
pub fn to_tgw(source: &str) -> Result<String, Error> {
    to_tgw_with_format(source, InputFormat::Auto)
}

/// Convert a source with explicitly selected input syntax to `.tgw`.
pub fn to_tgw_with_format(source: &str, format: InputFormat) -> Result<String, Error> {
    Ok(crate::format::write(&parse(source, format)?))
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
