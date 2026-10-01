//! The Great Wave renders native `.tgw` timing diagrams, ASM charts, HLS schedules, gate netlists, and WaveJSON to SVG.
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

mod asm;
mod asm_emit;
mod asm_layout;
mod emit;
mod format;
mod geom;
mod gtl;
mod gtl_emit;
mod gtl_layout;
mod hls;
mod hls_emit;
mod hls_layout;
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

enum Picture {
    Wave(Box<scan::Doc>),
    Asm(Box<asm::Chart>),
    Hls(Box<hls::Schedule>),
    Gtl(Box<gtl::Netlist>),
}

fn parse(source: &str, format: InputFormat) -> Result<Picture, Error> {
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
        match scan::parse(source) {
            Ok(doc) => Ok(Picture::Wave(Box::new(doc))),
            Err(error)
                if matches!(format, InputFormat::Auto)
                    && (trimmed.starts_with("//") || trimmed.starts_with("/*")) =>
            {
                Err(Error {
                    offset: error.offset,
                    message: format!("{}; a .tgw comment starts with #", error.message),
                })
            }
            Err(error) => Err(error),
        }
    } else if gtl::starts_with_gtl(source) {
        gtl::parse(source).map(|net| Picture::Gtl(Box::new(net)))
    } else if hls::starts_with_hls(source) {
        hls::parse(source).map(|schedule| Picture::Hls(Box::new(schedule)))
    } else if asm::starts_with_asm(source) {
        asm::parse(source).map(|chart| Picture::Asm(Box::new(chart)))
    } else {
        native::parse(source).map(|doc| Picture::Wave(Box::new(doc)))
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
    if let Err(e) = write_picture(&parse(source, format)?, out, indent, &scheme::LIGHT) {
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
    if let Err(e) = write_picture(&parse(source, format)?, out, indent, scheme) {
        out.clear();
        return Err(e);
    }
    Ok(())
}

fn write_picture(
    picture: &Picture,
    out: &mut Vec<u8>,
    indent: u8,
    scheme: &'static scheme::Scheme,
) -> Result<(), Error> {
    match picture {
        Picture::Wave(doc) => emit::write_themed(doc, out, indent, scheme),
        Picture::Asm(chart) => asm_emit::write(chart, out, indent, scheme),
        Picture::Hls(schedule) => hls_emit::write(schedule, out, indent, scheme),
        Picture::Gtl(net) => gtl_emit::write(net, out, indent, scheme),
    }
}

/// Convert JSON5 or native text to canonical, human-readable `.tgw` syntax.
pub fn to_tgw(source: &str) -> Result<String, Error> {
    to_tgw_with_format(source, InputFormat::Auto)
}

/// Convert a source with explicitly selected input syntax to `.tgw`.
pub fn to_tgw_with_format(source: &str, format: InputFormat) -> Result<String, Error> {
    Ok(match parse(source, format)? {
        Picture::Wave(doc) => crate::format::write(&doc),
        Picture::Asm(chart) => crate::asm::write(&chart),
        Picture::Hls(schedule) => crate::hls::write(&schedule),
        Picture::Gtl(net) => crate::gtl::write(&net),
    })
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
