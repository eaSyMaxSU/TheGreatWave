//! The Great Wave renders WaveJSON signal diagrams to SVG.
//!
//! Wave rules and the default palette come from WaveDrom
//! (Copyright 2011–2026 Aliaksei Chapyzhenka).
#![forbid(unsafe_code)]

mod emit;
mod geom;
mod scan;
mod w;
mod wave;
mod width;

pub(crate) const XS: i64 = 20;
pub(crate) const YS: i64 = 20;
pub(crate) const YO: i64 = 30;
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
    out.clear();
    let doc = scan::parse(source)?;
    if let Err(e) = emit::write(&doc, out, indent) {
        out.clear();
        return Err(e);
    }
    Ok(())
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
