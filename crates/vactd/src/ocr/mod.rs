//! Hardware-accelerated OCR & Text Extraction — V7.
//!
//! Uses Windows Runtime `Windows.Media.Ocr` APIs via `windows-rs` to perform
//! on-device, zero-cloud text extraction from segmented UI bounding box regions.

use std::time::Instant;
use log::{debug, info};
use vact_core::{BoundingBox, OcrWord, TextRegion};

use windows::{
    Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
    Media::Ocr::{OcrEngine as WinRtOcrEngine, OcrResult},
    Storage::Streams::DataWriter,
    Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED},
};


/// Timing metrics for the V7 OCR stage.
#[derive(Copy, Clone, Debug, Default)]
pub struct OcrTimings {
    /// Number of candidate boxes selected for OCR.
    pub candidate_count: usize,
    /// Number of regions with valid recognized text.
    pub recognized_count: usize,
    /// Total time spent in box filtering and sub-image cropping in microseconds.
    pub crop_us: u64,
    /// Total time spent executing WinRT OCR in microseconds.
    pub ocr_us: u64,
    /// Total OCR stage duration in microseconds.
    pub total_us: u64,
}

/// Hardware-accelerated WinRT OCR Engine.
pub struct OcrEngine {
    engine: WinRtOcrEngine,
    language_tag: String,
}

impl OcrEngine {
    /// Initialize the WinRT OCR engine using user profile languages.
    ///
    /// Initializes the COM/WinRT multithreaded runtime if not already active.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Initialize WinRT apartment (ignore error if already initialized by another component)
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }

        let engine = WinRtOcrEngine::TryCreateFromUserProfileLanguages().map_err(|e| {
            format!("Failed to initialize Windows.Media.Ocr engine: {e}")
        })?;

        let language_tag = engine
            .RecognizerLanguage()
            .and_then(|lang| lang.LanguageTag())
            .map(|tag| tag.to_string_lossy())
            .unwrap_or_else(|_| "default".to_string());

        info!("WinRT OCR engine initialized (language: {language_tag})");

        Ok(Self {
            engine,
            language_tag,
        })
    }

    /// Language tag of the active OCR recognizer.
    pub fn language_tag(&self) -> &str {
        &self.language_tag
    }

    /// Filters bounding boxes to select candidate UI regions that are likely to contain text.
    ///
    /// Excludes full-screen background frames, massive structural containers,
    /// and micro-noise specs to stay strictly within the 8–15ms per-frame latency budget.
    pub fn filter_candidate_boxes<'a>(
        &self,
        boxes: &'a [BoundingBox],
        frame_w: u32,
        frame_h: u32,
        max_candidates: usize,
    ) -> Vec<&'a BoundingBox> {
        let max_screen_area = (frame_w as u64 * frame_h as u64) / 4; // < 25% screen area
        let max_w = (frame_w * 95) / 100;

        let mut candidates: Vec<&'a BoundingBox> = boxes
            .iter()
            .filter(|b| {
                // Height: between 10px and 120px (typical single/multi-line UI text)
                b.height >= 10 && b.height <= 120
                    // Width: between 12px and 95% of screen width
                    && b.width >= 12 && b.width <= max_w
                    // Area: at least 120 px², less than 25% of total screen
                    && b.area() >= 120 && b.area() <= max_screen_area
            })
            .collect();

        // Limit candidate count to avoid exceeding latency budget on dense scenes
        if candidates.len() > max_candidates {
            candidates.truncate(max_candidates);
        }

        candidates
    }

    /// Extract a BGRA8 sub-image crop corresponding to a bounding box.
    pub fn crop_bgra(
        frame_bgra: &[u8],
        frame_w: u32,
        frame_h: u32,
        bbox: &BoundingBox,
    ) -> Option<(Vec<u8>, u32, u32)> {
        let x0 = bbox.x.min(frame_w);
        let y0 = bbox.y.min(frame_h);
        let x1 = bbox.right().min(frame_w);
        let y1 = bbox.bottom().min(frame_h);

        if x1 <= x0 || y1 <= y0 {
            return None;
        }

        let crop_w = x1 - x0;
        let crop_h = y1 - y0;
        let row_bytes = (crop_w * 4) as usize;
        let mut crop_data = vec![0u8; (crop_w * crop_h * 4) as usize];

        for row in 0..crop_h {
            let src_y = y0 + row;
            let src_start = ((src_y * frame_w + x0) * 4) as usize;
            let src_end = src_start + row_bytes;
            let dst_start = (row * crop_w * 4) as usize;
            let dst_end = dst_start + row_bytes;

            if src_end <= frame_bgra.len() && dst_end <= crop_data.len() {
                crop_data[dst_start..dst_end].copy_from_slice(&frame_bgra[src_start..src_end]);
            }
        }

        Some((crop_data, crop_w, crop_h))
    }

    /// Recognize text within a single cropped BGRA8 sub-texture.
    pub fn recognize_crop(
        &self,
        crop_bgra: &[u8],
        crop_w: u32,
        crop_h: u32,
        source_box: &BoundingBox,
    ) -> Result<Option<TextRegion>, Box<dyn std::error::Error>> {
        if crop_w == 0 || crop_h == 0 || crop_bgra.is_empty() {
            return Ok(None);
        }

        // 1. Create an IBuffer from raw crop bytes using DataWriter
        let writer = DataWriter::new()?;
        writer.WriteBytes(crop_bgra)?;
        let buffer = writer.DetachBuffer()?;

        // 2. Create SoftwareBitmap from buffer
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
            &buffer,
            BitmapPixelFormat::Bgra8,
            crop_w as i32,
            crop_h as i32,
        )?;


        // 3. Execute WinRT OCR recognition asynchronously and await completion synchronously
        let async_op = self.engine.RecognizeAsync(&bitmap)?;
        let ocr_result: OcrResult = async_op.get()?;

        // 4. Parse detected lines and word bounding boxes
        let lines = match ocr_result.Lines() {
            Ok(l) => l,
            Err(_) => return Ok(None),
        };

        let line_count = match lines.Size() {
            Ok(s) => s,
            Err(_) => return Ok(None),
        };

        if line_count == 0 {
            return Ok(None);
        }

        let mut words = Vec::new();
        let mut text_parts = Vec::new();
        let mut next_word_id = 1u32;

        for line_idx in 0..line_count {
            if let Ok(line) = lines.GetAt(line_idx) {
                if let Ok(line_text) = line.Text() {
                    let line_str = line_text.to_string_lossy();
                    let trimmed = line_str.trim();
                    if !trimmed.is_empty() {
                        text_parts.push(trimmed.to_string());
                    }
                }

                if let Ok(line_words) = line.Words() {
                    if let Ok(word_count) = line_words.Size() {
                        for word_idx in 0..word_count {
                            if let Ok(w) = line_words.GetAt(word_idx) {
                                if let (Ok(w_text), Ok(rect)) = (w.Text(), w.BoundingRect()) {
                                    let word_str = w_text.to_string_lossy();
                                    if !word_str.trim().is_empty() {
                                        // Offset local crop coordinates by source_box top-left
                                        let wx = source_box.x + (rect.X.max(0.0) as u32);
                                        let wy = source_box.y + (rect.Y.max(0.0) as u32);
                                        let ww = (rect.Width.max(1.0) as u32).min(source_box.width);
                                        let wh = (rect.Height.max(1.0) as u32).min(source_box.height);

                                        let word_box = BoundingBox::new(next_word_id, wx, wy, ww, wh);
                                        next_word_id += 1;

                                        words.push(OcrWord::new(word_str.trim(), word_box));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let full_text = text_parts.join(" ");
        if full_text.trim().is_empty() {
            return Ok(None);
        }

        Ok(Some(TextRegion::new(*source_box, full_text, words)))
    }

    /// High-speed single-pass full-screen OCR (V7 High-Performance Mode).
    ///
    /// Instead of cropping up to 64 sub-images and executing 64 synchronous WinRT calls
    /// (which costs 300–800ms), this converts the entire frame to a single `SoftwareBitmap`
    /// and invokes `RecognizeAsync` once (~8–12ms total). It then performs spatial intersection
    /// in microseconds to associate recognized text lines and words with candidate bounding boxes.
    pub fn recognize_full_frame(
        &self,
        frame_bgra: &[u8],
        frame_w: u32,
        frame_h: u32,
        boxes: &[BoundingBox],
    ) -> Result<(Vec<TextRegion>, OcrTimings), Box<dyn std::error::Error>> {
        let t_start = Instant::now();
        let mut timings = OcrTimings::default();

        if frame_w == 0 || frame_h == 0 || frame_bgra.is_empty() {
            return Ok((Vec::new(), timings));
        }

        let t_prep_start = Instant::now();
        let writer = DataWriter::new()?;
        writer.WriteBytes(frame_bgra)?;
        let buffer = writer.DetachBuffer()?;

        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
            &buffer,
            BitmapPixelFormat::Bgra8,
            frame_w as i32,
            frame_h as i32,
        )?;
        let prep_us = t_prep_start.elapsed().as_micros() as u64;

        let t_ocr_start = Instant::now();
        let async_op = self.engine.RecognizeAsync(&bitmap)?;
        let ocr_result: OcrResult = async_op.get()?;
        let ocr_us = t_ocr_start.elapsed().as_micros() as u64;

        let lines = match ocr_result.Lines() {
            Ok(l) => l,
            Err(_) => return Ok((Vec::new(), timings)),
        };

        let line_count = lines.Size().unwrap_or(0);
        if line_count == 0 {
            timings.total_us = t_start.elapsed().as_micros() as u64;
            return Ok((Vec::new(), timings));
        }

        // Extracted full-screen line data: (line_text, line_box, words)
        struct LineData {
            text: String,
            bounds: BoundingBox,
            words: Vec<OcrWord>,
        }

        let mut extracted_lines = Vec::with_capacity(line_count as usize);
        let mut next_word_id = 1u32;
        let mut next_line_id = 10000u32;

        for line_idx in 0..line_count {
            if let Ok(line) = lines.GetAt(line_idx) {
                let line_text = match line.Text() {
                    Ok(t) => t.to_string_lossy(),
                    Err(_) => continue,
                };
                let trimmed_text = line_text.trim().to_string();
                if trimmed_text.is_empty() {
                    continue;
                }

                let mut line_words = Vec::new();
                let mut min_x = u32::MAX;
                let mut min_y = u32::MAX;
                let mut max_r = 0u32;
                let mut max_b = 0u32;

                if let Ok(words) = line.Words() {
                    let word_count = words.Size().unwrap_or(0);
                    for w_idx in 0..word_count {
                        if let Ok(w) = words.GetAt(w_idx) {
                            if let (Ok(w_text), Ok(rect)) = (w.Text(), w.BoundingRect()) {
                                let w_str = w_text.to_string_lossy();
                                let w_trimmed = w_str.trim();
                                if !w_trimmed.is_empty() {
                                    let wx = rect.X.max(0.0) as u32;
                                    let wy = rect.Y.max(0.0) as u32;
                                    let ww = (rect.Width.max(1.0) as u32).min(frame_w.saturating_sub(wx));
                                    let wh = (rect.Height.max(1.0) as u32).min(frame_h.saturating_sub(wy));

                                    min_x = min_x.min(wx);
                                    min_y = min_y.min(wy);
                                    max_r = max_r.max(wx + ww);
                                    max_b = max_b.max(wy + wh);

                                    let word_box = BoundingBox::new(next_word_id, wx, wy, ww, wh);
                                    next_word_id += 1;
                                    line_words.push(OcrWord::new(w_trimmed, word_box));
                                }
                            }
                        }
                    }
                }

                if min_x == u32::MAX {
                    continue;
                }

                let line_w = max_r.saturating_sub(min_x).max(1);
                let line_h = max_b.saturating_sub(min_y).max(1);
                let line_box = BoundingBox::new(next_line_id, min_x, min_y, line_w, line_h);
                next_line_id += 1;

                extracted_lines.push(LineData {
                    text: trimmed_text,
                    bounds: line_box,
                    words: line_words,
                });
            }
        }

        // Spatial association: associate lines/words with candidate CCL boxes
        let mut text_regions = Vec::new();
        let mut matched_line_indices = std::collections::HashSet::new();

        for bbox in boxes {
            let mut matched_words = Vec::new();
            let mut matched_texts = Vec::new();

            for (idx, line) in extracted_lines.iter().enumerate() {
                let line_cx = line.bounds.x + line.bounds.width / 2;
                let line_cy = line.bounds.y + line.bounds.height / 2;

                let inside = line_cx >= bbox.x
                    && line_cx <= bbox.right()
                    && line_cy >= bbox.y
                    && line_cy <= bbox.bottom();

                if inside {
                    matched_texts.push(line.text.clone());
                    matched_words.extend(line.words.clone());
                    matched_line_indices.insert(idx);
                }
            }

            if !matched_texts.is_empty() {
                let full_text = matched_texts.join(" ");
                text_regions.push(TextRegion::new(*bbox, full_text, matched_words));
            }
        }

        // Also add any prominent unassociated text lines as standalone TextRegions
        for (idx, line) in extracted_lines.into_iter().enumerate() {
            if !matched_line_indices.contains(&idx) {
                text_regions.push(TextRegion::new(line.bounds, line.text, line.words));
            }
        }

        timings.candidate_count = boxes.len();
        timings.recognized_count = text_regions.len();
        timings.crop_us = prep_us;
        timings.ocr_us = ocr_us;
        timings.total_us = t_start.elapsed().as_micros() as u64;

        Ok((text_regions, timings))
    }

    /// Extract text across all candidate bounding boxes in the current desktop frame.
    pub fn extract_all(
        &self,
        frame_bgra: &[u8],
        frame_w: u32,
        frame_h: u32,
        boxes: &[BoundingBox],
        max_candidates: usize,
    ) -> (Vec<TextRegion>, OcrTimings) {
        // Fast path: use single-pass full frame recognition
        if let Ok((regions, timings)) = self.recognize_full_frame(frame_bgra, frame_w, frame_h, boxes) {
            return (regions, timings);
        }

        let t_start = Instant::now();
        let mut timings = OcrTimings::default();

        let candidates = self.filter_candidate_boxes(boxes, frame_w, frame_h, max_candidates);
        timings.candidate_count = candidates.len();

        let mut text_regions = Vec::new();
        let mut total_crop_us = 0u64;
        let mut total_ocr_us = 0u64;

        for bbox in candidates {
            let t_crop_start = Instant::now();
            let crop_opt = Self::crop_bgra(frame_bgra, frame_w, frame_h, bbox);
            total_crop_us += t_crop_start.elapsed().as_micros() as u64;

            if let Some((crop_data, crop_w, crop_h)) = crop_opt {
                let t_ocr_start = Instant::now();
                match self.recognize_crop(&crop_data, crop_w, crop_h, bbox) {
                    Ok(Some(region)) => {
                        text_regions.push(region);
                    }
                    Ok(None) => {}
                    Err(e) => {
                        debug!("OCR crop error on box #{}: {e}", bbox.id);
                    }
                }
                total_ocr_us += t_ocr_start.elapsed().as_micros() as u64;
            }
        }

        timings.recognized_count = text_regions.len();
        timings.crop_us = total_crop_us;
        timings.ocr_us = total_ocr_us;
        timings.total_us = t_start.elapsed().as_micros() as u64;

        (text_regions, timings)
    }
}
