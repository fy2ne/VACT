//! Integration tests for V7 WinRT OCR pipeline & candidate box filtering.

use vact_core::BoundingBox;
use vactd::ocr::OcrEngine;

#[test]
fn test_ocr_candidate_box_filtering() {
    let engine = match OcrEngine::new() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Skipping OCR test (WinRT engine unavailable in this environment): {e}");
            return;
        }
    };

    let frame_w = 1920;
    let frame_h = 1080;

    let boxes = vec![
        // Valid candidates (buttons, inputs, labels)
        BoundingBox::new(1, 100, 100, 120, 32),   // Button (120x32)
        BoundingBox::new(2, 100, 200, 300, 28),   // Input field (300x28)
        BoundingBox::new(3, 100, 300, 400, 50),   // Heading / banner (400x50)

        // Invalid candidates (to be filtered out)
        BoundingBox::new(4, 0, 0, 1920, 1080),    // Fullscreen root (>25% area)
        BoundingBox::new(5, 50, 50, 4, 4),        // Tiny noise speck (<120 px²)
        BoundingBox::new(6, 50, 50, 200, 200),    // Giant square container (40000 px², height > 120)
    ];

    let candidates = engine.filter_candidate_boxes(&boxes, frame_w, frame_h, 32);
    let candidate_ids: Vec<u32> = candidates.iter().map(|b| b.id).collect();

    assert!(candidate_ids.contains(&1), "Box #1 (Button) should be a candidate");
    assert!(candidate_ids.contains(&2), "Box #2 (Input) should be a candidate");
    assert!(candidate_ids.contains(&3), "Box #3 (Heading) should be a candidate");
    assert!(!candidate_ids.contains(&4), "Box #4 (Fullscreen) must be filtered out");
    assert!(!candidate_ids.contains(&5), "Box #5 (Tiny speck) must be filtered out");
    assert!(!candidate_ids.contains(&6), "Box #6 (Large container) must be filtered out");
}

#[test]
fn test_ocr_crop_bgra_bounds() {
    let frame_w = 100;
    let frame_h = 100;
    let frame_data = vec![128u8; (frame_w * frame_h * 4) as usize];

    let bbox = BoundingBox::new(1, 10, 20, 30, 40);
    let (crop, w, h) = OcrEngine::crop_bgra(&frame_data, frame_w, frame_h, &bbox).unwrap();

    assert_eq!(w, 30);
    assert_eq!(h, 40);
    assert_eq!(crop.len(), (30 * 40 * 4) as usize);
    assert_eq!(crop[0], 128);
}
