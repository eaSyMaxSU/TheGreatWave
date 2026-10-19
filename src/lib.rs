//! The Great Wave renders native `.tgw` timing diagrams, ASM charts, HLS schedules, and gate netlists to SVG.
//!
//! Copyright (c) 2026 eaSyMaxSU. Licensed under the MIT License; see `LICENSE`.
//!
//! Wave rules and the default palette come from WaveDrom
//! (Copyright 2011–2026 Aliaksei Chapyzhenka, MIT):
//! <https://wavedrom.com/>, <https://github.com/wavedrom/wavedrom>.
//! SVG arc geometry: <https://www.w3.org/TR/SVG2/implnote.html#ArcImplementationNotes>.
#![deny(unsafe_code)]

// The CLI and optional native adapters use the same library, including in tests.
#[cfg(feature = "view")]
extern crate gpui_kit as gpui;
extern crate self as tgw;

mod asm;
mod asm_emit;
mod asm_layout;
mod cli;
mod draw;
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

#[cfg(all(test, feature = "view"))]
mod chart_pixels;
#[cfg(feature = "view")]
mod layout;
#[cfg(feature = "watch")]
pub mod live;
#[cfg(feature = "view")]
mod slice;
#[cfg(feature = "view")]
pub mod view;

#[cfg(all(test, feature = "watch"))]
pub(crate) use cli::render_file;
pub use cli::run_cli;
#[cfg(feature = "watch")]
pub(crate) use cli::{diagnostic, file_identity, paths_equal, same_file, terminate};
pub use width::text_width;

use std::sync::OnceLock;

pub(crate) const XS: i64 = 20;
pub(crate) const YS: i64 = 20;
pub(crate) const YO: i64 = 36;
pub(crate) const YM: i64 = 15;
pub(crate) const Y0: i64 = 5;
pub(crate) const XLABEL: i64 = 6;
pub(crate) const TGO: i64 = -10;

#[derive(Clone, Debug)]
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
    Diagram::parse(source)?.render()
}

/// Clear `out` and write an SVG document into it.
pub fn render_into(source: &str, out: &mut Vec<u8>) -> Result<(), Error> {
    render_opts(source, out, 0)
}

pub fn render_opts(source: &str, out: &mut Vec<u8>, indent: u8) -> Result<(), Error> {
    render_themed(source, out, indent, &scheme::LIGHT)
}

enum Picture {
    Wave(Box<scan::Doc>, OnceLock<Result<Box<emit::Layout>, Error>>),
    Asm(Box<asm::Chart>, OnceLock<Box<asm_layout::Scene>>),
    Hls(Box<hls::Schedule>, OnceLock<Box<hls_layout::Scene>>),
    Gtl(Box<gtl::Netlist>, OnceLock<Box<gtl_layout::Scene>>),
}

fn parse(source: &str) -> Result<Picture, Error> {
    if gtl::starts_with_gtl(source) {
        gtl::parse(source).map(|net| Picture::Gtl(Box::new(net), OnceLock::new()))
    } else if hls::starts_with_hls(source) {
        hls::parse(source).map(|schedule| Picture::Hls(Box::new(schedule), OnceLock::new()))
    } else if asm::starts_with_asm(source) {
        asm::parse(source).map(|chart| Picture::Asm(Box::new(chart), OnceLock::new()))
    } else {
        native::parse(source).map(|doc| Picture::Wave(Box::new(doc), OnceLock::new()))
    }
}

/// An owned parsed diagram. Layout is computed on the first render and reused
/// for later renders, including other palettes. Safe to share across threads.
/// Internal models and geometry are intentionally not part of the public API.
///
/// ```
/// let diagram = tgw::Diagram::parse("@gtl\nnot a -> y\n")?;
/// let light = diagram.render()?;
/// let mut dark = Vec::new();
/// diagram.render_themed(&mut dark, 0, &tgw::scheme::DARK)?;
/// # Ok::<(), tgw::Error>(())
/// ```
pub struct Diagram {
    picture: Picture,
    /// Parser and geometry offsets refer to the BOM-free source internally.
    offset_base: usize,
}

impl Diagram {
    /// Parse any of the four diagram types without borrowing `source`.
    /// Rendering may still reject out-of-range timing geometry.
    pub fn parse(source: &str) -> Result<Self, Error> {
        let normalized = source.strip_prefix('\u{feff}').unwrap_or(source);
        let offset_base = source.len() - normalized.len();
        let picture = parse(normalized).map_err(|mut error| {
            error.offset += offset_base;
            error
        })?;
        Ok(Self {
            picture,
            offset_base,
        })
    }

    /// Render a compact light SVG, reusing this diagram's layout.
    pub fn render(&self) -> Result<String, Error> {
        let mut out = Vec::new();
        self.render_into(&mut out)?;
        Ok(String::from_utf8(out).expect("SVG writer produces UTF-8"))
    }

    /// Replace `out` with a compact light SVG; clear it on error.
    pub fn render_into(&self, out: &mut Vec<u8>) -> Result<(), Error> {
        self.render_themed(out, 0, &scheme::LIGHT)
    }

    /// Replace `out` with a light SVG, indented by `indent` spaces.
    pub fn render_opts(&self, out: &mut Vec<u8>, indent: u8) -> Result<(), Error> {
        self.render_themed(out, indent, &scheme::LIGHT)
    }

    /// Replace `out` with a themed SVG without parsing or laying out again.
    /// On error, `out` is empty and the offset refers to the original source,
    /// including a leading UTF-8 BOM when present.
    pub fn render_themed(
        &self,
        out: &mut Vec<u8>,
        indent: u8,
        scheme: &'static scheme::Scheme,
    ) -> Result<(), Error> {
        out.clear();
        write_picture(&self.picture, out, indent, scheme).map_err(|mut error| {
            out.clear();
            error.offset += self.offset_base;
            error
        })
    }

    /// Canonical source, preserving comments without parsing again.
    pub fn to_tgw(&self) -> String {
        match &self.picture {
            Picture::Wave(doc, _) => crate::format::write(doc),
            Picture::Asm(chart, _) => crate::asm::write(chart),
            Picture::Hls(schedule, _) => crate::hls::write(schedule),
            Picture::Gtl(net, _) => crate::gtl::write(net),
        }
    }
}

/// Render `source` with `scheme`. File output uses [`render_opts`], which is the
/// light palette. A dark window calls this for the picture only.
pub fn render_themed(
    source: &str,
    out: &mut Vec<u8>,
    indent: u8,
    scheme: &'static scheme::Scheme,
) -> Result<(), Error> {
    out.clear();
    Diagram::parse(source)?.render_themed(out, indent, scheme)
}

fn write_picture(
    picture: &Picture,
    out: &mut Vec<u8>,
    indent: u8,
    scheme: &'static scheme::Scheme,
) -> Result<(), Error> {
    match picture {
        Picture::Wave(doc, layout) => {
            let layout = layout.get_or_init(|| emit::layout(doc).map(Box::new));
            emit::write_themed(
                doc,
                layout.as_ref().map_err(Clone::clone)?,
                out,
                indent,
                scheme,
            )
        }
        Picture::Asm(chart, layout) => asm_emit::write(
            chart,
            layout.get_or_init(|| Box::new(asm_layout::layout(chart))),
            out,
            indent,
            scheme,
        ),
        Picture::Hls(schedule, layout) => hls_emit::write(
            schedule,
            layout.get_or_init(|| Box::new(hls_layout::layout(schedule))),
            out,
            indent,
            scheme,
        ),
        Picture::Gtl(net, layout) => gtl_emit::write(
            net,
            layout.get_or_init(|| Box::new(gtl_layout::layout(net))),
            out,
            indent,
            scheme,
        ),
    }
}

/// Reprint `source` as canonical, human-readable `.tgw` text.
pub fn to_tgw(source: &str) -> Result<String, Error> {
    Ok(Diagram::parse(source)?.to_tgw())
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
