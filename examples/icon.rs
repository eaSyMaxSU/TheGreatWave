//! Regenerates the raster icons from `assets/icon/tgw.svg`.
//!
//!     cargo run --example icon

use std::error::Error;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use gpui_kit::{size, DevicePixels, SvgRenderer, SvgSize};
use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};

/// The macOS Dock draws an 824-unit tile on a 1024-unit canvas.
const DOCK_CANVAS: u32 = 512;
const DOCK_TILE: u32 = 412;
const ICO_SIDES: [u32; 9] = [16, 20, 24, 32, 40, 48, 64, 128, 256];

fn main() -> Result<(), Box<dyn Error>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icon");
    let svg = fs::read(dir.join("tgw.svg"))?;
    let renderer = SvgRenderer::new(Arc::new(()));
    let tree = renderer.parse_svg(&svg)?;
    let rgba = |side: u32| -> Result<Vec<u8>, Box<dyn Error>> {
        let image = renderer.render_parsed(
            &tree,
            SvgSize::ExactSize(size(DevicePixels(side as i32), DevicePixels(side as i32))),
        )?;
        let mut pixels = image.as_bytes(0).ok_or("empty raster")?.to_vec();
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        Ok(pixels)
    };

    let tile = rgba(DOCK_TILE)?;
    let inset = ((DOCK_CANVAS - DOCK_TILE) / 2) as usize;
    let (canvas_row, tile_row) = (DOCK_CANVAS as usize * 4, DOCK_TILE as usize * 4);
    let mut dock = vec![0; canvas_row * DOCK_CANVAS as usize];
    for (y, row) in tile.chunks_exact(tile_row).enumerate() {
        let start = (inset + y) * canvas_row + inset * 4;
        dock[start..start + tile_row].copy_from_slice(row);
    }
    let mut png = Vec::new();
    PngEncoder::new(&mut png).write_image(
        &dock,
        DOCK_CANVAS,
        DOCK_CANVAS,
        ExtendedColorType::Rgba8,
    )?;
    fs::write(dir.join("tgw.png"), png)?;

    let mut frames = Vec::new();
    for side in ICO_SIDES {
        frames.push(IcoFrame::as_png(
            &rgba(side)?,
            side,
            side,
            ExtendedColorType::Rgba8,
        )?);
    }
    let mut ico = Vec::new();
    IcoEncoder::new(&mut ico).encode_images(&frames)?;
    fs::write(dir.join("tgw.ico"), ico)?;
    Ok(())
}
