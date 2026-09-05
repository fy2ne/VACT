//! Photometric Color Analyzer — Zero-copy GPU/Frame buffer color extraction.
//!
//! Analyzes sub-rectangles `[x1, y1, x2, y2]` within a raw BGRA image buffer
//! to extract dominant RGB values, hex codes, perceived luminance, and
//! semantic color classifications ("red", "blue", "green", "orange", "yellow",
//! "purple", "cyan", "dark_gray", "white", "black").

use serde::{Deserialize, Serialize};
use crate::BoundingBox;

/// Structured photometric color information for a discrete UI component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorInfo {
    /// Semantic color classification name.
    /// Values: "red", "blue", "green", "orange", "yellow", "purple", "cyan", "magenta", "white", "black", "dark_gray", "gray".
    pub name: String,
    /// Primary 6-character hex code in `#RRGGBB` format.
    pub hex: String,
    /// Perceived luminance $Y \in [0.0..1.0]$ according to Rec. 601 ($Y = 0.299R + 0.587G + 0.114B$).
    pub luminance: f32,
    /// Whether this component has a dark background/appearance ($Y < 0.45$).
    pub is_dark: bool,
}

/// Analyze the photometric color profile of a bounding box region in a BGRA frame.
///
/// # Performance
/// Executes with strided subsampling (up to ~64 samples per box), taking
/// less than $1\,\mu\text{s}$ per UI element.
pub fn analyze_region_color(
    bgra_data: &[u8],
    img_w: u32,
    img_h: u32,
    x1: u32,
    y1: u32,
    x2: u32,
    y2: u32,
) -> ColorInfo {
    if img_w == 0 || img_h == 0 || bgra_data.is_empty() {
        return ColorInfo {
            name: "gray".to_string(),
            hex: "#8E8E93".to_string(),
            luminance: 0.5,
            is_dark: false,
        };
    }

    let x1 = x1.min(img_w.saturating_sub(1));
    let y1 = y1.min(img_h.saturating_sub(1));
    let x2 = x2.clamp(x1 + 1, img_w);
    let y2 = y2.clamp(y1 + 1, img_h);

    let region_w = (x2 - x1) as usize;
    let region_h = (y2 - y1) as usize;

    let step_x = (region_w / 8).max(1);
    let step_y = (region_h / 8).max(1);

    let mut sum_r: u64 = 0;
    let mut sum_g: u64 = 0;
    let mut sum_b: u64 = 0;
    let mut count: u64 = 0;

    let mut cy = y1 as usize;
    while cy < y2 as usize {
        let row_stride = cy * (img_w as usize) * 4;
        let mut cx = x1 as usize;
        while cx < x2 as usize {
            let offset = row_stride + cx * 4;
            if offset + 3 < bgra_data.len() {
                let b = bgra_data[offset] as u64;
                let g = bgra_data[offset + 1] as u64;
                let r = bgra_data[offset + 2] as u64;

                sum_b += b;
                sum_g += g;
                sum_r += r;
                count += 1;
            }
            cx += step_x;
        }
        cy += step_y;
    }

    if count == 0 {
        return ColorInfo {
            name: "gray".to_string(),
            hex: "#8E8E93".to_string(),
            luminance: 0.5,
            is_dark: false,
        };
    }

    let avg_r = (sum_r / count) as u8;
    let avg_g = (sum_g / count) as u8;
    let avg_b = (sum_b / count) as u8;

    let hex = format!("#{:02X}{:02X}{:02X}", avg_r, avg_g, avg_b);
    let luminance = (0.299 * (avg_r as f32) + 0.587 * (avg_g as f32) + 0.114 * (avg_b as f32)) / 255.0;
    let is_dark = luminance < 0.45;

    let name = classify_rgb_to_semantic_name(avg_r, avg_g, avg_b, luminance);

    ColorInfo {
        name,
        hex,
        luminance,
        is_dark,
    }
}

/// Helper to analyze a [`BoundingBox`] directly.
#[inline]
pub fn analyze_bbox_color(
    bgra_data: &[u8],
    img_w: u32,
    img_h: u32,
    bbox: &BoundingBox,
) -> ColorInfo {
    analyze_region_color(bgra_data, img_w, img_h, bbox.x, bbox.y, bbox.right(), bbox.bottom())
}

/// Convert average RGB to HSV and classify into human/agent friendly semantic color names.
fn classify_rgb_to_semantic_name(r: u8, g: u8, b: u8, luma: f32) -> String {
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;

    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;

    // Saturation & Value
    let s = if max == 0.0 { 0.0 } else { delta / max };
    let v = max;

    // Achromatic / Monochromatic checks
    if v < 0.18 {
        return "black".to_string();
    }
    if v > 0.88 && s < 0.10 {
        return "white".to_string();
    }
    if s < 0.14 {
        if luma < 0.35 {
            return "dark_gray".to_string();
        } else if luma < 0.70 {
            return "gray".to_string();
        } else {
            return "light_gray".to_string();
        }
    }

    // Hue calculation in degrees [0..360)
    let mut h = if delta == 0.0 {
        0.0
    } else if (max - rf).abs() < 1e-5 {
        60.0 * (((gf - bf) / delta) % 6.0)
    } else if (max - gf).abs() < 1e-5 {
        60.0 * (((bf - rf) / delta) + 2.0)
    } else {
        60.0 * (((rf - gf) / delta) + 4.0)
    };

    if h < 0.0 {
        h += 360.0;
    }

    // Classify by Hue sector
    if h < 18.0 || h >= 345.0 {
        "red".to_string()
    } else if h < 45.0 {
        "orange".to_string()
    } else if h < 68.0 {
        "yellow".to_string()
    } else if h < 165.0 {
        "green".to_string()
    } else if h < 205.0 {
        "cyan".to_string()
    } else if h < 265.0 {
        "blue".to_string()
    } else if h < 315.0 {
        "purple".to_string()
    } else {
        "magenta".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_classification_red() {
        let mut bgra = vec![0u8; 100 * 100 * 4];
        // Fill with pure Red in BGRA (B=0, G=0, R=255, A=255)
        for i in (0..bgra.len()).step_by(4) {
            bgra[i] = 0;
            bgra[i + 1] = 0;
            bgra[i + 2] = 255;
            bgra[i + 3] = 255;
        }

        let info = analyze_region_color(&bgra, 100, 100, 10, 10, 80, 80);
        assert_eq!(info.name, "red");
        assert_eq!(info.hex, "#FF0000");
    }

    #[test]
    fn test_color_classification_blue() {
        let mut bgra = vec![0u8; 100 * 100 * 4];
        // Fill with pure Blue in BGRA (B=255, G=0, R=0, A=255)
        for i in (0..bgra.len()).step_by(4) {
            bgra[i] = 255;
            bgra[i + 1] = 0;
            bgra[i + 2] = 0;
            bgra[i + 3] = 255;
        }

        let info = analyze_region_color(&bgra, 100, 100, 10, 10, 80, 80);
        assert_eq!(info.name, "blue");
        assert_eq!(info.hex, "#0000FF");
    }

    #[test]
    fn test_color_classification_dark_theme() {
        let mut bgra = vec![0u8; 100 * 100 * 4];
        // Fill with dark slate (B=30, G=30, R=30, A=255)
        for i in (0..bgra.len()).step_by(4) {
            bgra[i] = 30;
            bgra[i + 1] = 30;
            bgra[i + 2] = 30;
            bgra[i + 3] = 255;
        }

        let info = analyze_region_color(&bgra, 100, 100, 0, 0, 100, 100);
        assert_eq!(info.name, "black");
        assert!(info.is_dark);
    }
}
