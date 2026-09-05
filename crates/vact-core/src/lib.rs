//! `vact-core` — Shared types, math primitives, and scene graph structs.
//!
//! This crate is the foundation imported by both `vactd` (the daemon) and
//! `vact-protocol` (the wire format).
//!
//! # V6 additions
//!
//! [`HierarchyNode`] and [`build_hierarchy`] implement the geometric containment
//! tree that promotes the flat list of [`BoundingBox`] structs produced by CCL
//! (V5) into a recursive parent-child structure.
//!
//! # V7 additions
//!
//! [`OcrWord`], [`TextRegion`], and [`SemanticRole`] define structured text
//! extraction results per UI region, with heuristic semantic classification.

use serde::{Deserialize, Serialize};

pub mod color;
pub use color::{analyze_bbox_color, analyze_region_color, ColorInfo};

/// The current version of the VACT core library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 2D Bounding Box representing a segmented UI element or container.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BoundingBox {
    /// Unique identifier within the current frame/scene.
    pub id: u32,
    /// Top-left X coordinate in screen pixels.
    pub x: u32,
    /// Top-left Y coordinate in screen pixels.
    pub y: u32,
    /// Width in screen pixels.
    pub width: u32,
    /// Height in screen pixels.
    pub height: u32,
}

impl BoundingBox {
    /// Create a new `BoundingBox`.
    pub const fn new(id: u32, x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            id,
            x,
            y,
            width,
            height,
        }
    }

    /// Right edge X coordinate ($x + \text{width}$).
    #[inline]
    pub const fn right(&self) -> u32 {
        self.x + self.width
    }

    /// Bottom edge Y coordinate ($y + \text{height}$).
    #[inline]
    pub const fn bottom(&self) -> u32 {
        self.y + self.height
    }

    /// Total area in square pixels.
    #[inline]
    pub const fn area(&self) -> u64 {
        (self.width as u64) * (self.height as u64)
    }

    /// Aspect ratio (width / height as f32).
    #[inline]
    pub fn aspect_ratio(&self) -> f32 {
        if self.height == 0 {
            0.0
        } else {
            self.width as f32 / self.height as f32
        }
    }

    /// Returns `true` if this bounding box completely encloses `other`.
    #[inline]
    pub fn contains(&self, other: &BoundingBox) -> bool {
        self.x <= other.x
            && self.y <= other.y
            && self.right() >= other.right()
            && self.bottom() >= other.bottom()
    }

    /// Returns `true` if this bounding box overlaps with `other`.
    #[inline]
    pub fn intersects(&self, other: &BoundingBox) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }

    /// Compute Intersection-over-Union (IoU) metric between two bounding boxes.
    pub fn iou(&self, other: &BoundingBox) -> f32 {
        let ix1 = self.x.max(other.x);
        let iy1 = self.y.max(other.y);
        let ix2 = self.right().min(other.right());
        let iy2 = self.bottom().min(other.bottom());

        if ix2 <= ix1 || iy2 <= iy1 {
            return 0.0;
        }

        let intersection_area = ((ix2 - ix1) as u64) * ((iy2 - iy1) as u64);
        let union_area = self.area() + other.area() - intersection_area;

        if union_area == 0 {
            0.0
        } else {
            intersection_area as f32 / union_area as f32
        }
    }

    /// Geometric centroid (center point $(x + w/2, y + h/2)$).
    #[inline]
    pub fn center(&self) -> (f32, f32) {
        (self.x as f32 + self.width as f32 * 0.5, self.y as f32 + self.height as f32 * 0.5)
    }

    /// Euclidean distance between the centroids of two bounding boxes.
    pub fn distance_to(&self, other: &BoundingBox) -> f32 {
        let (cx1, cy1) = self.center();
        let (cx2, cy2) = other.center();
        let dx = cx2 - cx1;
        let dy = cy2 - cy1;
        (dx * dx + dy * dy).sqrt()
    }

    /// 2D Vector from this centroid to another centroid $\vec{v} = (\Delta x, \Delta y)$.
    pub fn vector_to(&self, other: &BoundingBox) -> (f32, f32) {
        let (cx1, cy1) = self.center();
        let (cx2, cy2) = other.center();
        (cx2 - cx1, cy2 - cy1)
    }

    /// Gaussian spatial affinity weight $G_{\sigma}(d) = \exp\left(-\frac{d^2}{2\sigma^2}\right)$.
    pub fn gaussian_weight(&self, other: &BoundingBox, sigma_space: f32) -> f32 {
        let d = self.distance_to(other);
        if sigma_space <= 0.0 {
            0.0
        } else {
            (- (d * d) / (2.0 * sigma_space * sigma_space)).exp()
        }
    }

    /// Formatted scientific mathematical description string.
    pub fn scientific_notation(&self) -> String {
        format!(
            "Ω_{} [x:{}, y:{} | {}×{} px | AR:{:.2} | Δ:{}]",
            self.id,
            self.x,
            self.y,
            self.width,
            self.height,
            self.aspect_ratio(),
            self.area()
        )
    }
}

/// Mathematical functions and spatial geometry primitives.
pub mod math {
    /// 2D Gaussian function $G(x, y; \sigma) = \frac{1}{2\pi\sigma^2} \exp\left(-\frac{x^2 + y^2}{2\sigma^2}\right)$.
    #[inline]
    pub fn gaussian_2d(dx: f32, dy: f32, sigma: f32) -> f32 {
        if sigma <= 0.0 {
            return 0.0;
        }
        let two_sigma_sq = 2.0 * sigma * sigma;
        (- (dx * dx + dy * dy) / two_sigma_sq).exp()
    }

    /// Bilateral Filter weighting kernel $W(p, q) = \exp\left(-\frac{\|\mathbf{p}-\mathbf{q}\|^2}{2\sigma_s^2}\right) \cdot \exp\left(-\frac{\|I(\mathbf{p})-I(\mathbf{q})\|^2}{2\sigma_r^2}\right)$.
    #[inline]
    pub fn bilateral_kernel(spatial_dist: f32, intensity_diff: f32, sigma_s: f32, sigma_r: f32) -> f32 {
        let s_weight = (- (spatial_dist * spatial_dist) / (2.0 * sigma_s * sigma_s)).exp();
        let r_weight = (- (intensity_diff * intensity_diff) / (2.0 * sigma_r * sigma_r)).exp();
        s_weight * r_weight
    }

    /// Normalize viewport pixel coordinates $[0..W, 0..H]$ to scientific NDC $[-1.0..1.0]$.
    #[inline]
    pub fn to_ndc(x: f32, y: f32, width: f32, height: f32) -> (f32, f32) {
        if width <= 0.0 || height <= 0.0 {
            (0.0, 0.0)
        } else {
            (
                (x / width) * 2.0 - 1.0,
                1.0 - (y / height) * 2.0,
            )
        }
    }

    /// Compute Minecraft F3 style chunk coordinates `(cx, cy)` from screen pixel coordinates.
    #[inline]
    pub fn to_chunk(x: u32, y: u32, chunk_size: u32) -> (u32, u32) {
        let cs = if chunk_size == 0 { 160 } else { chunk_size };
        (x / cs, y / cs)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Perceptual Hashing (dHash) Engine for Spatial-Semantic Memory
// ─────────────────────────────────────────────────────────────────────────────

pub mod phash {
    use super::BoundingBox;

    /// Computes a 64-bit gradient/difference perceptual hash (dHash) for an arbitrary
    /// sub-rectangle `[x1, y1, x2, y2]` within a BGRA image frame buffer.
    ///
    /// # Algorithm
    /// 1. Samples a 9×8 downscaled grid across the region using bilinear sampling.
    /// 2. Converts each sample to standard Rec.601 luminance: $Y = 0.299R + 0.587G + 0.114B$.
    /// 3. Computes 64 horizontal gradient comparisons: `grid[y][x] > grid[y][x+1]`.
    /// 4. Returns a 64-bit integer invariant to minor contrast, noise, and scaling.
    pub fn compute_dhash(
        bgra_data: &[u8],
        img_w: u32,
        img_h: u32,
        x1: u32,
        y1: u32,
        x2: u32,
        y2: u32,
    ) -> u64 {
        if img_w == 0 || img_h == 0 || bgra_data.is_empty() {
            return 0;
        }

        let x1 = x1.min(img_w.saturating_sub(1));
        let y1 = y1.min(img_h.saturating_sub(1));
        let x2 = x2.clamp(x1 + 1, img_w);
        let y2 = y2.clamp(y1 + 1, img_h);

        let region_w = (x2 - x1) as f32;
        let region_h = (y2 - y1) as f32;

        let mut grid = [[0.0f32; 9]; 8];

        for gy in 0..8 {
            let sample_y = (y1 as f32 + (gy as f32 + 0.5) * (region_h / 8.0)).min((img_h - 1) as f32) as usize;
            let row_stride = sample_y * (img_w as usize) * 4;

            for gx in 0..9 {
                let sample_x = (x1 as f32 + (gx as f32 + 0.5) * (region_w / 9.0)).min((img_w - 1) as f32) as usize;
                let px_offset = row_stride + sample_x * 4;

                if px_offset + 3 < bgra_data.len() {
                    let b = bgra_data[px_offset] as f32;
                    let g = bgra_data[px_offset + 1] as f32;
                    let r = bgra_data[px_offset + 2] as f32;
                    // Rec. 601 luma formula
                    grid[gy][gx] = 0.299 * r + 0.587 * g + 0.114 * b;
                }
            }
        }

        let mut hash: u64 = 0;
        for gy in 0..8 {
            for gx in 0..8 {
                let bit = if grid[gy][gx] > grid[gy][gx + 1] { 1u64 } else { 0u64 };
                hash = (hash << 1) | bit;
            }
        }

        hash
    }

    /// Compute dHash directly from a [`BoundingBox`].
    #[inline]
    pub fn compute_dhash_box(
        bgra_data: &[u8],
        img_w: u32,
        img_h: u32,
        bbox: &BoundingBox,
    ) -> u64 {
        compute_dhash(bgra_data, img_w, img_h, bbox.x, bbox.y, bbox.right(), bbox.bottom())
    }

    /// Convert 64-bit perceptual hash into a 16-character lowercase hex string.
    #[inline]
    pub fn phash_to_hex(hash: u64) -> String {
        format!("{:016x}", hash)
    }

    /// Calculate Hamming distance (number of differing bits) between two 64-bit hashes.
    #[inline]
    pub fn hamming_distance(h1: u64, h2: u64) -> u32 {
        (h1 ^ h2).count_ones()
    }
}



// ─────────────────────────────────────────────────────────────────────────────
// V6 — Geometric Containment Hierarchy
// ─────────────────────────────────────────────────────────────────────────────

/// A node in the containment hierarchy tree.
///
/// Each node wraps one [`BoundingBox`] and holds its direct children — those
/// boxes that are geometrically contained inside this one with no smaller
/// intermediate box in between.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HierarchyNode {
    /// Identifier matching the source [`BoundingBox::id`].
    pub id: u32,
    /// The bounding box for this node.
    pub bounds: BoundingBox,
    /// Nesting depth from the root (root nodes have depth 0).
    pub depth: u32,
    /// Direct children — boxes contained inside `bounds` with no smaller
    /// intermediate ancestor.
    pub children: Vec<HierarchyNode>,
}

impl HierarchyNode {
    fn new(bounds: BoundingBox, depth: u32) -> Self {
        Self {
            id: bounds.id,
            bounds,
            depth,
            children: Vec::new(),
        }
    }
}

/// Build a containment hierarchy from a flat slice of [`BoundingBox`] values.
///
/// **Algorithm (O(N²)):**
///
/// 1. Sort boxes by area, descending (largest first = outermost first).
/// 2. For each box **j**, find its *direct* parent: the **smallest** box **i**
///    that fully contains **j** (i.e. `i.contains(j)` is true and `i ≠ j`).  
///    This guarantees the intermediate-skip property — if A ⊃ B ⊃ C then
///    B is assigned as C's parent, not A.
/// 3. Assemble the flat parent-index map into a recursive tree.
///
/// Returns the root nodes (boxes with no parent). A typical desktop scene
/// produces one or a few top-level roots.
///
/// # Complexity
/// O(N²) comparisons, O(N) memory. For the expected N < 1 000 regions per
/// frame this runs in well under 500 µs.
pub fn build_hierarchy(boxes: &[BoundingBox]) -> Vec<HierarchyNode> {
    let n = boxes.len();
    if n == 0 {
        return Vec::new();
    }

    // Step 1: sort indices by area descending (largest box first).
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_unstable_by(|&a, &b| boxes[b].area().cmp(&boxes[a].area()));

    // Step 2: for every box in sorted order, find its direct parent.
    // `parent[k]` = index into `order` of the direct parent of order[k],
    //               or `usize::MAX` when there is no parent (root).
    let mut parent: Vec<usize> = vec![usize::MAX; n];

    for i in 0..n {
        let bi = &boxes[order[i]];
        // The candidate parent is the *smallest* already-found ancestor, so
        // we scan all earlier boxes (which are all larger by the sort) and
        // keep the one with the minimum area that still contains bi.
        let mut best_area: u64 = u64::MAX;
        let mut best_k: usize = usize::MAX;

        for k in 0..i {
            let bk = &boxes[order[k]];
            if bk.id != bi.id && bk.contains(bi) && bk.area() < best_area {
                best_area = bk.area();
                best_k = k;
            }
        }
        parent[i] = best_k;
    }

    // Step 3: recursively build the tree.
    // We work depth-first: build children lists in sorted order.
    // Each position in `order` maps to one node; we collect roots separately.
    fn build_node(
        pos: usize,
        order: &[usize],
        parent: &[usize],
        boxes: &[BoundingBox],
        depth: u32,
    ) -> HierarchyNode {
        let bbox = boxes[order[pos]];
        let mut node = HierarchyNode::new(bbox, depth);

        // Find all direct children of `pos` (those whose parent == pos).
        for k in 0..order.len() {
            if parent[k] == pos {
                node.children.push(build_node(k, order, parent, boxes, depth + 1));
            }
        }
        node
    }

    // Roots are entries where parent == usize::MAX.
    let mut roots = Vec::new();
    for i in 0..n {
        if parent[i] == usize::MAX {
            roots.push(build_node(i, &order, &parent, boxes, 0));
        }
    }
    roots
}

/// Print the containment hierarchy to `stdout` using indented tree notation.
///
/// Each line shows the node depth (via `│  ` / `└─ ` prefixes), the box ID,
/// and its bounds.
///
/// # Example output
/// ```text
/// [0] id=1  bounds=[0, 0, 1920, 1080]
/// ├─ [1] id=3  bounds=[100, 50, 800, 600]
/// │  └─ [2] id=7  bounds=[120, 80, 200, 100]
/// └─ [1] id=5  bounds=[900, 50, 400, 300]
/// ```
pub fn print_tree(nodes: &[HierarchyNode]) {
    fn print_node(node: &HierarchyNode, prefix: &str, is_last: bool) {
        let connector = if is_last { "└─ " } else { "├─ " };
        let header = if node.depth == 0 { String::new() } else { format!("{}{}", prefix, connector) };

        println!(
            "{}[{}] id={:<4} bounds=[{:>5}, {:>5}, {:>5}, {:>5}]  ({}×{} px)",
            header,
            node.depth,
            node.id,
            node.bounds.x,
            node.bounds.y,
            node.bounds.right(),
            node.bounds.bottom(),
            node.bounds.width,
            node.bounds.height,
        );

        let child_prefix = if node.depth == 0 {
            String::new()
        } else if is_last {
            format!("{}   ", prefix)
        } else {
            format!("{}│  ", prefix)
        };

        let child_count = node.children.len();
        for (i, child) in node.children.iter().enumerate() {
            print_node(child, &child_prefix, i + 1 == child_count);
        }
    }

    let count = nodes.len();
    for (i, root) in nodes.iter().enumerate() {
        print_node(root, "", i + 1 == count);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// V7 — OCR & Text Extraction Primitives
// ─────────────────────────────────────────────────────────────────────────────

/// A single recognized word with its absolute screen-space bounding box.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrWord {
    /// Recognized text content of the word.
    pub text: String,
    /// Absolute bounding box in desktop/frame screen pixels.
    pub bounds: BoundingBox,
}

impl OcrWord {
    /// Create a new recognized word with absolute coordinates.
    pub fn new(text: impl Into<String>, bounds: BoundingBox) -> Self {
        Self {
            text: text.into(),
            bounds,
        }
    }
}

/// Coarse semantic role inferred from extracted text content and geometric heuristics.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticRole {
    /// Short action or navigation label (e.g. "Save", "Cancel", "Submit", "Settings").
    ButtonLabel,
    /// Header / title text in a prominent UI region.
    Heading,
    /// Input field value, search placeholder, or editable text box.
    InputValue,
    /// List, table, or menu item text.
    ListItem,
    /// Multi-word or multi-line descriptive text block.
    BodyText,
    /// General UI text that does not match specific heuristics.
    GeneralText,
}

/// OCR extraction result for a discrete UI bounding box region.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextRegion {
    /// Identifier matching the parent [`BoundingBox::id`].
    pub id: u32,
    /// Source bounding box of the UI element in screen pixels.
    pub source_box: BoundingBox,
    /// Concatenated full text string across all detected words/lines.
    pub text: String,
    /// Word-level bounding boxes in screen pixel coordinates.
    pub words: Vec<OcrWord>,
    /// Inferred semantic role of this UI component.
    pub role: SemanticRole,
}

impl TextRegion {
    /// Create a new `TextRegion` and automatically infer its [`SemanticRole`].
    pub fn new(source_box: BoundingBox, text: impl Into<String>, words: Vec<OcrWord>) -> Self {
        let text_str = text.into();
        let role = Self::infer_role(&source_box, &text_str, words.len());
        Self {
            id: source_box.id,
            source_box,
            text: text_str,
            words,
            role,
        }
    }

    /// Infer semantic role using text content, length, word count, and box geometry.
    pub fn infer_role(bbox: &BoundingBox, text: &str, word_count: usize) -> SemanticRole {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return SemanticRole::GeneralText;
        }

        let char_len = trimmed.chars().count();
        let aspect = bbox.aspect_ratio();
        let lower = trimmed.to_lowercase();

        // Common action button keywords
        let is_action_verb = matches!(
            lower.as_str(),
            "ok" | "cancel" | "apply" | "close" | "save" | "submit" | "yes" | "no"
            | "next" | "back" | "finish" | "search" | "login" | "sign in" | "sign up"
            | "continue" | "delete" | "edit" | "copy" | "paste" | "cut" | "add" | "new"
            | "download" | "upload" | "send" | "run" | "retry" | "stop" | "play"
            | "settings" | "help" | "menu" | "options" | "done"
        );

        // 1. Explicit action keywords are always button labels
        if is_action_verb {
            return SemanticRole::ButtonLabel;
        }

        // 2. Long multi-word/multi-line blocks are body text
        if word_count > 6 || char_len > 40 {
            return SemanticRole::BodyText;
        }

        // 3. Tall/prominent text with moderate length is a heading
        if bbox.height >= 34 && aspect >= 2.0 && word_count <= 6 {
            return SemanticRole::Heading;
        }

        // 4. Compact short label is a button
        if word_count <= 2 && char_len <= 16 && bbox.height <= 36 && aspect >= 1.0 && aspect <= 5.0 {
            return SemanticRole::ButtonLabel;
        }

        // 5. Elongated single-line slot is an input value
        if word_count <= 4 && bbox.height <= 32 && aspect >= 3.5 {
            return SemanticRole::InputValue;
        }

        // 6. Shallow horizontal row item
        if word_count <= 8 && bbox.height <= 30 && aspect >= 1.8 {
            return SemanticRole::ListItem;
        }

        SemanticRole::GeneralText
    }
}


// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;


    // ── BoundingBox geometry (V1–V5 regression) ──────────────────────────────

    #[test]
    fn test_bounding_box_geometry() {
        let b1 = BoundingBox::new(1, 10, 20, 100, 50);
        assert_eq!(b1.right(), 110);
        assert_eq!(b1.bottom(), 70);
        assert_eq!(b1.area(), 5000);
        assert!((b1.aspect_ratio() - 2.0).abs() < 1e-4);

        let b2 = BoundingBox::new(2, 20, 30, 40, 20);
        assert!(b1.contains(&b2));
        assert!(!b2.contains(&b1));
        assert!(b1.intersects(&b2));

        let b3 = BoundingBox::new(3, 200, 200, 50, 50);
        assert!(!b1.intersects(&b3));
        assert_eq!(b1.iou(&b3), 0.0);
    }

    // ── V6 Hierarchy tests ────────────────────────────────────────────────────

    /// Helper: collect all node IDs in DFS order.
    #[allow(dead_code)]
    fn collect_ids(nodes: &[HierarchyNode]) -> Vec<u32> {
        let mut out = Vec::new();
        fn walk(nodes: &[HierarchyNode], out: &mut Vec<u32>) {
            for n in nodes {
                out.push(n.id);
                walk(&n.children, out);
            }
        }
        walk(nodes, &mut out);
        out
    }

    /// Find a node by ID anywhere in the tree.
    fn find_node(nodes: &[HierarchyNode], id: u32) -> Option<&HierarchyNode> {
        for n in nodes {
            if n.id == id { return Some(n); }
            if let Some(found) = find_node(&n.children, id) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn test_empty_input() {
        let roots = build_hierarchy(&[]);
        assert!(roots.is_empty());
    }

    #[test]
    fn test_single_box() {
        let b = BoundingBox::new(1, 0, 0, 100, 100);
        let roots = build_hierarchy(&[b]);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].id, 1);
        assert_eq!(roots[0].depth, 0);
        assert!(roots[0].children.is_empty());
    }

    /// Flat list — 3 non-overlapping boxes → 3 root nodes, 0 children each.
    #[test]
    fn test_flat_no_containment() {
        let boxes = vec![
            BoundingBox::new(1, 0,   0, 100, 100),
            BoundingBox::new(2, 200, 0, 100, 100),
            BoundingBox::new(3, 400, 0, 100, 100),
        ];
        let roots = build_hierarchy(&boxes);
        assert_eq!(roots.len(), 3, "all three should be roots");
        for r in &roots {
            assert_eq!(r.depth, 0);
            assert!(r.children.is_empty());
        }
    }

    /// Simple nesting — outer contains inner → 1 root with 1 child.
    #[test]
    fn test_simple_nesting() {
        let outer = BoundingBox::new(1, 0,  0, 200, 200);
        let inner = BoundingBox::new(2, 50, 50, 100, 100);
        let roots = build_hierarchy(&[outer, inner]);

        assert_eq!(roots.len(), 1, "outer should be the single root");
        let root = &roots[0];
        assert_eq!(root.id, 1);
        assert_eq!(root.depth, 0);

        assert_eq!(root.children.len(), 1);
        let child = &root.children[0];
        assert_eq!(child.id, 2);
        assert_eq!(child.depth, 1);
        assert!(child.children.is_empty());
    }

    /// Deep chain — A ⊃ B ⊃ C → depths 0 / 1 / 2.
    #[test]
    fn test_deep_chain() {
        // A is the largest, B is inside A, C is inside B.
        let a = BoundingBox::new(1, 0,   0, 300, 300);
        let b = BoundingBox::new(2, 50,  50, 200, 200);
        let c = BoundingBox::new(3, 100, 100, 100, 100);
        let roots = build_hierarchy(&[a, b, c]);

        assert_eq!(roots.len(), 1);
        let root = &roots[0];
        assert_eq!(root.id, 1);
        assert_eq!(root.depth, 0);
        assert_eq!(root.children.len(), 1);

        let mid = &root.children[0];
        assert_eq!(mid.id, 2);
        assert_eq!(mid.depth, 1);
        assert_eq!(mid.children.len(), 1);

        let leaf = &mid.children[0];
        assert_eq!(leaf.id, 3);
        assert_eq!(leaf.depth, 2);
        assert!(leaf.children.is_empty());
    }

    /// Siblings — 1 parent, 2 children side-by-side.
    #[test]
    fn test_siblings() {
        let parent   = BoundingBox::new(1, 0,  0, 500, 200);
        let child_a  = BoundingBox::new(2, 10, 10, 200, 150);
        let child_b  = BoundingBox::new(3, 280, 10, 200, 150);
        let roots = build_hierarchy(&[parent, child_a, child_b]);

        assert_eq!(roots.len(), 1);
        let root = &roots[0];
        assert_eq!(root.id, 1);
        assert_eq!(root.children.len(), 2, "parent should have exactly 2 children");

        let ids: Vec<u32> = root.children.iter().map(|c| c.id).collect();
        assert!(ids.contains(&2));
        assert!(ids.contains(&3));
        for child in &root.children {
            assert_eq!(child.depth, 1);
            assert!(child.children.is_empty());
        }
    }

    /// Skip-intermediate — A ⊃ B ⊃ C: B must be C's parent, not A.
    #[test]
    fn test_skip_intermediate_ancestor() {
        let a = BoundingBox::new(10, 0,   0, 400, 400);
        let b = BoundingBox::new(20, 50,  50, 300, 300);
        let c = BoundingBox::new(30, 100, 100, 100, 100);
        let roots = build_hierarchy(&[a, b, c]);

        // A is root
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].id, 10);

        // B is a direct child of A
        let node_b = find_node(&roots, 20).expect("B must exist");
        assert_eq!(node_b.depth, 1);

        // C is a direct child of B, NOT of A
        let node_c = find_node(&roots, 30).expect("C must exist");
        assert_eq!(node_c.depth, 2);
        assert_eq!(
            node_b.children.iter().find(|n| n.id == 30).is_some(),
            true,
            "C should be a direct child of B"
        );
        assert_eq!(
            roots[0].children.iter().find(|n| n.id == 30).is_some(),
            false,
            "C should NOT be a direct child of A"
        );
    }

    /// Verify depth field is set correctly across a mixed tree.
    #[test]
    fn test_depth_field() {
        let root  = BoundingBox::new(1, 0,  0, 500, 500);
        let child = BoundingBox::new(2, 50, 50, 300, 300);
        let grand = BoundingBox::new(3, 100, 100, 100, 100);

        let roots = build_hierarchy(&[root, child, grand]);
        assert_eq!(find_node(&roots, 1).unwrap().depth, 0);
        assert_eq!(find_node(&roots, 2).unwrap().depth, 1);
        assert_eq!(find_node(&roots, 3).unwrap().depth, 2);
    }

    // ── V7 OCR & Semantic Role tests ──────────────────────────────────────────

    #[test]
    fn test_semantic_role_heuristics() {
        // Button cases
        let btn_box = BoundingBox::new(1, 100, 100, 80, 32);
        assert_eq!(TextRegion::infer_role(&btn_box, "Submit", 1), SemanticRole::ButtonLabel);
        assert_eq!(TextRegion::infer_role(&btn_box, "Cancel", 1), SemanticRole::ButtonLabel);
        assert_eq!(TextRegion::infer_role(&btn_box, "Sign In", 2), SemanticRole::ButtonLabel);

        // Heading cases
        let heading_box = BoundingBox::new(2, 50, 50, 300, 40);
        assert_eq!(TextRegion::infer_role(&heading_box, "Account Overview", 2), SemanticRole::Heading);

        // Input value cases
        let input_box = BoundingBox::new(3, 50, 120, 240, 28);
        assert_eq!(TextRegion::infer_role(&input_box, "user@example.com", 1), SemanticRole::InputValue);

        // Body text cases
        let body_box = BoundingBox::new(4, 50, 200, 500, 300);
        assert_eq!(
            TextRegion::infer_role(
                &body_box,
                "This is a long paragraph of descriptive content that explains the system details in full.",
                15
            ),
            SemanticRole::BodyText
        );
    }

    #[test]
    fn test_text_region_word_bounds() {
        let parent_box = BoundingBox::new(10, 100, 200, 300, 50);
        let word1 = OcrWord::new("Hello", BoundingBox::new(1, 105, 210, 50, 20));
        let word2 = OcrWord::new("World", BoundingBox::new(2, 160, 210, 55, 20));

        let tr = TextRegion::new(parent_box, "Hello World", vec![word1, word2]);
        assert_eq!(tr.id, 10);
        assert_eq!(tr.words.len(), 2);
        assert_eq!(tr.words[0].text, "Hello");
        assert_eq!(tr.words[1].text, "World");
        assert_eq!(tr.text, "Hello World");
    }

    #[test]
    fn test_bounding_box_vector_math() {
        let b1 = BoundingBox::new(1, 100, 100, 100, 100); // center: (150, 150)
        let b2 = BoundingBox::new(2, 400, 500, 100, 100); // center: (450, 550)

        assert_eq!(b1.center(), (150.0, 150.0));
        assert_eq!(b2.center(), (450.0, 550.0));

        let dist = b1.distance_to(&b2);
        assert_eq!(dist, 500.0); // (300^2 + 400^2)^0.5 = 500

        let vec = b1.vector_to(&b2);
        assert_eq!(vec, (300.0, 400.0));

        let g_weight = b1.gaussian_weight(&b2, 500.0);
        assert!((g_weight - (-0.5f32).exp()).abs() < 1e-5);

        let notation = b1.scientific_notation();
        assert!(notation.contains("Ω_1"));
        assert!(notation.contains("100×100 px"));
    }

    #[test]
    fn test_math_functions() {
        let g = math::gaussian_2d(0.0, 0.0, 1.0);
        assert_eq!(g, 1.0);

        let bk = math::bilateral_kernel(0.0, 0.0, 2.5, 0.15);
        assert_eq!(bk, 1.0);

        let (ndc_x, ndc_y) = math::to_ndc(960.0, 540.0, 1920.0, 1080.0);
        assert_eq!(ndc_x, 0.0);
        assert_eq!(ndc_y, 0.0);

        let (cx, cy) = math::to_chunk(320, 480, 160);
        assert_eq!(cx, 2);
        assert_eq!(cy, 3);
    }

    #[test]
    fn test_phash_computation() {
        // Create 20x20 test image with horizontal gradient (left=black, right=white)
        let w = 20u32;
        let h = 20u32;
        let mut img = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let offset = ((y * w + x) * 4) as usize;
                let val = (x * 255 / w) as u8;
                img[offset] = val;     // B
                img[offset + 1] = val; // G
                img[offset + 2] = val; // R
                img[offset + 3] = 255; // A
            }
        }

        let hash1 = phash::compute_dhash(&img, w, h, 0, 0, 20, 20);
        let hex1 = phash::phash_to_hex(hash1);
        assert_eq!(hex1.len(), 16);

        // Gradient from left to right means left < right, so grid[x] > grid[x+1] is false (0 bits)
        assert_eq!(hash1, 0);

        // Now reverse gradient: left=white, right=black -> grid[x] > grid[x+1] is true (all 1 bits)
        let mut img_rev = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let offset = ((y * w + x) * 4) as usize;
                let val = 255 - (x * 255 / w) as u8;
                img_rev[offset] = val;
                img_rev[offset + 1] = val;
                img_rev[offset + 2] = val;
                img_rev[offset + 3] = 255;
            }
        }
        let hash2 = phash::compute_dhash(&img_rev, w, h, 0, 0, 20, 20);
        assert_eq!(hash2, u64::MAX);

        assert_eq!(phash::hamming_distance(hash1, hash2), 64);
    }
}


