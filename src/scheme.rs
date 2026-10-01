//! One palette for the diagram and the window around it.
//!
//! [`LIGHT`] is the picture a file render writes. [`DARK`] is the same diagram
//! with those roles repainted. The emitter writes the colors directly, so a
//! saved SVG stays on the light palette and the window does not scan for hex.

/// Named colors for one appearance. Paint fields are the exact tokens written
/// into attributes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Scheme {
    pub name: &'static str,
    pub background: u32,
    pub status_background: u32,
    pub status_text: u32,
    pub scroll_track: u32,
    pub scroll_thumb: u32,
    /// Page, gap knockout, label pills, and gap backdrops.
    pub paper: &'static str,
    /// Strokes written as `#000`.
    pub ink: &'static str,
    /// Strokes written as `#000000`. A different token from [`Self::ink`].
    pub ink_full: &'static str,
    /// Default text on the root `<svg>`.
    pub text: &'static str,
    /// Signal names.
    pub label: &'static str,
    /// Brackets, edges, and arrowheads.
    pub bracket: &'static str,
    /// Group captions.
    pub muted: &'static str,
    /// Tick labels.
    pub tick: &'static str,
    pub hatch_fill: &'static str,
    pub hatch_line: &'static str,
    pub grid: &'static str,
    /// An edge shape the renderer does not recognize.
    pub fault: &'static str,
    pub bus_plain: &'static str,
    pub bus3: &'static str,
    pub bus4: &'static str,
    pub bus5: &'static str,
    pub bus6: &'static str,
    pub bus7: &'static str,
    pub bus8: &'static str,
    pub bus9: &'static str,
    pub mark2: &'static str,
    pub mark3: &'static str,
    pub mark4: &'static str,
    pub mark5: &'static str,
    pub mark6: &'static str,
    pub mark7: &'static str,
    pub mark8: &'static str,
}

/// The palette the file writer uses.
pub static LIGHT: Scheme = Scheme {
    name: "default light",
    background: 0xffffff,
    status_background: 0xfef2f2,
    status_text: 0x991b1b,
    scroll_track: 0xe2e8f0,
    scroll_thumb: 0x94a3b8,
    paper: "#fff",
    ink: "#000",
    ink_full: "#000000",
    text: "#0f172a",
    label: "#334155",
    bracket: "#0041c4",
    muted: "#475569",
    tick: "#64748b",
    hatch_fill: "#f1f5f9",
    hatch_line: "#94a3b8",
    grid: "#cbd5e1",
    fault: "#F00",
    bus_plain: "#ffffff",
    bus3: "#ffffb4",
    bus4: "#ffe0b9",
    bus5: "#b9e0ff",
    bus6: "#ccfdfe",
    bus7: "#cdfdc5",
    bus8: "#f0c1fb",
    bus9: "#f5c2c0",
    mark2: "#e90000",
    mark3: "#3edd00",
    mark4: "#0074cd",
    mark5: "#ff15db",
    mark6: "#af9800",
    mark7: "#00864f",
    mark8: "#a076ff",
};

/// The same roles as [`LIGHT`], for a dark window.
pub static DARK: Scheme = Scheme {
    name: "default dark",
    background: 0x10151f,
    status_background: 0x3f1d1d,
    status_text: 0xfecaca,
    scroll_track: 0x1e293b,
    scroll_thumb: 0x64748b,
    paper: "#10151f",
    ink: "#e7edf4",
    ink_full: "#e7edf4",
    text: "#f8fafc",
    label: "#d5dee8",
    bracket: "#7eb6ff",
    muted: "#c5d0dc",
    tick: "#a8b6c7",
    hatch_fill: "#1e293b",
    hatch_line: "#7d8da3",
    grid: "#3d4d63",
    fault: "#ff6b6b",
    bus_plain: "#4b5d73",
    bus3: "#7a7034",
    bus4: "#7a5438",
    bus5: "#2f5d80",
    bus6: "#2a686a",
    bus7: "#2f6840",
    bus8: "#68407a",
    bus9: "#7a403e",
    mark2: "#ff6b6b",
    mark3: "#7aef45",
    mark4: "#5eb0ff",
    mark5: "#ff6be6",
    mark6: "#f0d060",
    mark7: "#3dce8c",
    mark8: "#c4b5fd",
};

pub static ALL: &[&Scheme] = &[&LIGHT, &DARK];

impl Scheme {
    pub fn next(&self) -> &'static Scheme {
        let index = ALL
            .iter()
            .position(|scheme| scheme.name == self.name)
            .unwrap_or(0);
        ALL[(index + 1) % ALL.len()]
    }

    /// The file palette. The window can borrow that SVG instead of painting again.
    pub fn is_light(&self) -> bool {
        std::ptr::eq(self, &LIGHT)
    }

    pub fn bus(&self, n: u8) -> &'static str {
        match n {
            3 => self.bus3,
            4 => self.bus4,
            5 => self.bus5,
            6 => self.bus6,
            7 => self.bus7,
            8 => self.bus8,
            9 => self.bus9,
            _ => self.bus_plain,
        }
    }

    /// Color of an over/under mark. Anything other than `2`–`8` is the full ink.
    pub fn mark(&self, symbol: u8) -> &'static str {
        match symbol {
            b'2' => self.mark2,
            b'3' => self.mark3,
            b'4' => self.mark4,
            b'5' => self.mark5,
            b'6' => self.mark6,
            b'7' => self.mark7,
            b'8' => self.mark8,
            _ => self.ink_full,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
@title Palette\n@footer Foot\n@tick 0\n@grid on\n\
@group Inner\n  clk: p.....\n  data: =23456789\n  unknown: xx..\n\
 bars: 0101 ; over=123456789 under=1.2.3.4.5.6.7.8.9\n\
 analog: path M0,0 L2,1\n  gap: 01|10\n@end\n\
 a: 01.. ; node=.a.\nb: 0.1. ; node=..b\n@edge a~>b delay\n@gaps | [\n";

    #[test]
    fn schemes_cycle() {
        assert_eq!(LIGHT.next().name, DARK.name);
        assert_eq!(DARK.next().name, LIGHT.name);
        assert!(LIGHT.is_light());
        assert!(!DARK.is_light());
        assert_eq!(hex_rgb(DARK.paper), DARK.background);
        assert_eq!(hex_rgb(LIGHT.paper), LIGHT.background);
    }

    #[test]
    fn light_render_is_the_file_and_dark_uses_its_own_tokens() {
        let file = crate::render(SAMPLE).unwrap();
        let mut buf = Vec::new();
        crate::render_themed(SAMPLE, &mut buf, 0, &LIGHT).unwrap();
        assert_eq!(buf, file.as_bytes());

        buf.clear();
        crate::render_themed(SAMPLE, &mut buf, 0, &DARK).unwrap();
        let dark = String::from_utf8(buf).unwrap();
        assert!(dark.contains("fill=\"#10151f\""));
        assert!(dark.contains("fill=\"#4b5d73\""));
        assert!(dark.contains("stroke=\"#ff6b6b\""));
        assert!(!dark.contains("#fff"));
        assert!(!dark.contains("#0041c4"));
        assert!(!dark.contains("#F00"));
        assert!(!dark.contains("#0f172a"));
        let again = {
            let mut out = Vec::new();
            crate::render_themed(SAMPLE, &mut out, 0, &DARK).unwrap();
            String::from_utf8(out).unwrap()
        };
        assert_eq!(again, dark);
    }

    fn hex_rgb(token: &str) -> u32 {
        let hex = token.trim_start_matches('#');
        let expanded;
        let digits = if hex.len() == 3 {
            expanded = hex.chars().flat_map(|ch| [ch, ch]).collect::<String>();
            expanded.as_str()
        } else {
            hex
        };
        u32::from_str_radix(digits, 16).unwrap()
    }
}
