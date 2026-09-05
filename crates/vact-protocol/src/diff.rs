//! V9 — Temporal Delta Diffing Engine
//!
//! Compares consecutive [`SceneGraph`] snapshots and emits minimal
//! [`DiffFrame`] messages containing only the mutations that occurred
//! between frames.
//!
//! # ID Stabilization
//!
//! The GPU CCL pipeline assigns node IDs based on scan-order during each
//! frame, so the same UI element may receive a different raw ID on consecutive
//! frames. Before diffing, [`StateTracker`] runs an **IoU-based bipartite
//! match** to link previous and current nodes, then rewrites current node IDs
//! to the stable IDs carried forward from the previous frame.
//!
//! # Wire Format
//!
//! A [`DiffFrame`] serializes to the VACT/1.0 diff schema:
//!
//! ```json
//! {
//!   "type": "DIFF",
//!   "seq": 1043,
//!   "ack_seq": 1042,
//!   "mutations": [
//!     { "op": "UPDATE", "id": 14, "patch": { "value": "Deploy complete." } },
//!     { "op": "INSERT", "parent_id": 1, "node": { "id": 99, "type": "BUTTON", ... } },
//!     { "op": "REMOVE", "id": 37 }
//!   ]
//! }
//! ```
//!
//! Payloads are consistently `< 2 KB` for typical UI interaction.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{NodeType, SceneGraph, SceneNode};

// ─────────────────────────────────────────────────────────────────────────────
// Wire types
// ─────────────────────────────────────────────────────────────────────────────

/// Field-level patch applied by an `UPDATE` mutation.
///
/// Only fields that have *changed* are populated; all others are `None`.
/// When serialized, `None` fields are omitted to keep payloads tiny.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NodePatch {
    /// New bounding rect `[x1, y1, x2, y2]`, if changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<[u32; 4]>,
    /// New semantic type, if changed.
    #[serde(skip_serializing_if = "Option::is_none", rename = "type")]
    pub node_type: Option<NodeType>,
    /// New label text, if changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// New input value, if changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// New focus state, if changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused: Option<bool>,
    /// New interactable flag, if changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interactable: Option<bool>,
    /// New disabled flag, if changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    /// New source provenance, if changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// New window/control class name, if changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    /// New perceptual hash, if changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phash: Option<String>,
}

impl NodePatch {
    /// Returns `true` if every field is `None` (i.e. nothing changed).
    pub fn is_empty(&self) -> bool {
        self.bounds.is_none()
            && self.node_type.is_none()
            && self.label.is_none()
            && self.value.is_none()
            && self.focused.is_none()
            && self.interactable.is_none()
            && self.disabled.is_none()
            && self.source.is_none()
            && self.class_name.is_none()
            && self.phash.is_none()
    }
}

/// A single scene-graph mutation emitted by the diff engine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Mutation {
    /// A new node was added to the scene graph.
    Insert {
        /// Stable ID of the parent node (`0` = virtual root).
        parent_id: u32,
        /// The new node (including its own subtree).
        node: SceneNode,
    },
    /// An existing node (and all its children) was removed.
    Remove {
        /// Stable ID of the removed node.
        id: u32,
    },
    /// An existing node's properties changed (children handled separately).
    Update {
        /// Stable ID of the modified node.
        id: u32,
        /// Field-level patch containing only the changed fields.
        patch: NodePatch,
    },
}

/// A temporal delta frame: the minimal set of mutations between two consecutive
/// scene graph snapshots.
///
/// Serialized as the `"DIFF"` wire message type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiffFrame {
    /// Message discriminant — always `"DIFF"`.
    #[serde(rename = "type")]
    pub msg_type: String,
    /// Monotonically increasing sequence number for this diff.
    pub seq: u64,
    /// Sequence number of the previous snapshot this diff was computed against.
    pub ack_seq: u64,
    /// Ordered list of mutations. May be empty if the scene did not change.
    pub mutations: Vec<Mutation>,
}

impl DiffFrame {
    pub fn new(seq: u64, ack_seq: u64, mutations: Vec<Mutation>) -> Self {
        Self {
            msg_type: "DIFF".to_string(),
            seq,
            ack_seq,
            mutations,
        }
    }

    /// Returns `true` if no mutations occurred (scene is identical).
    pub fn is_empty(&self) -> bool {
        self.mutations.is_empty()
    }

    /// Estimate the JSON byte size of this diff without full serialisation.
    ///
    /// Used as a fast heuristic for the `< 2 KB` payload metric.
    pub fn approx_json_bytes(&self) -> usize {
        // Each mutation is roughly 80–200 bytes in JSON; header ~60 bytes.
        60 + self.mutations.len() * 160
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal flat representation for diffing
// ─────────────────────────────────────────────────────────────────────────────

/// A flat, parent-aware record of a single scene node — used internally during
/// the diff computation to avoid recursive traversal in hot paths.
#[derive(Clone, Debug)]
struct FlatNode {
    /// Stable ID assigned by the tracker (may differ from raw CCL ID).
    id: u32,
    /// Stable ID of this node's parent (`0` = no parent / root).
    parent_id: u32,
    /// Clone of the node's properties (children excluded for flat storage).
    node: SceneNode,
}

/// Flatten a `SceneNode` DAG into a list of `FlatNode` records.
fn flatten(node: &SceneNode, parent_id: u32, out: &mut Vec<FlatNode>) {
    out.push(FlatNode {
        id: node.id,
        parent_id,
        node: SceneNode { children: Vec::new(), ..node.clone() },
    });
    for child in &node.children {
        flatten(child, node.id, out);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// IoU-based ID stabilization
// ─────────────────────────────────────────────────────────────────────────────

/// Compute Intersection-over-Union between two bounding boxes `[x1,y1,x2,y2]`.
fn iou(a: [u32; 4], b: [u32; 4]) -> f32 {
    let ix1 = a[0].max(b[0]);
    let iy1 = a[1].max(b[1]);
    let ix2 = a[2].min(b[2]);
    let iy2 = a[3].min(b[3]);

    if ix2 <= ix1 || iy2 <= iy1 {
        return 0.0;
    }

    let intersection = ((ix2 - ix1) as f32) * ((iy2 - iy1) as f32);
    let area_a = ((a[2] - a[0]) as f32) * ((a[3] - a[1]) as f32);
    let area_b = ((b[2] - b[0]) as f32) * ((b[3] - b[1]) as f32);
    let union = area_a + area_b - intersection;

    if union <= 0.0 {
        0.0
    } else {
        intersection / union
    }
}

/// Greedy bipartite match: for each current node, find the best previous node
/// with matching `NodeType` and `IoU > threshold`. Returns a map from
/// `current_raw_id` → `prev_stable_id`.
///
/// Greedy matching is O(N²) which is fine for the small node counts typical in
/// a UI scene graph (generally < 256 nodes).
fn match_nodes(
    prev: &[FlatNode],
    curr: &[FlatNode],
    iou_threshold: f32,
) -> HashMap<u32, u32> {
    // Build a mutable "available" set from prev nodes
    let mut available: Vec<Option<&FlatNode>> = prev.iter().map(Some).collect();
    let mut result = HashMap::new();

    for c in curr {
        let mut best_iou = iou_threshold;
        let mut best_idx: Option<usize> = None;

        for (i, slot) in available.iter().enumerate() {
            let Some(p) = slot else { continue };
            if p.node.node_type != c.node.node_type {
                continue;
            }
            let score = iou(p.node.bounds, c.node.bounds);
            if score > best_iou {
                best_iou = score;
                best_idx = Some(i);
            }
        }

        if let Some(idx) = best_idx {
            let prev_id = available[idx].unwrap().id;
            result.insert(c.id, prev_id);
            available[idx] = None; // consume from available pool
        }
    }

    result
}

/// Rewrite node IDs in a `SceneNode` subtree using the given remapping table.
/// Nodes without an entry in `remap` keep their original (raw) ID.
fn remap_ids(node: &mut SceneNode, remap: &HashMap<u32, u32>) {
    if let Some(&stable_id) = remap.get(&node.id) {
        node.id = stable_id;
    }
    for child in &mut node.children {
        remap_ids(child, remap);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// StateTracker — stateful diff engine
// ─────────────────────────────────────────────────────────────────────────────

/// Stateful engine that maintains the previous frame's scene graph and emits
/// minimal [`DiffFrame`] messages when fed successive [`SceneGraph`] snapshots.
///
/// # Usage
///
/// ```rust,ignore
/// let mut tracker = StateTracker::new();
/// loop {
///     let graph = build_scene_graph(...);
///     match tracker.process_frame(graph) {
///         FrameOutput::Snapshot(g) => { /* first frame: send full snapshot */ }
///         FrameOutput::Diff(d)     => { /* subsequent frames: send diff */ }
///     }
/// }
/// ```
pub struct StateTracker {
    /// The previous frame's scene graph (after ID stabilization).
    prev: Option<SceneGraph>,
    /// Counter for minting new stable IDs for unseen nodes.
    next_stable_id: u32,
    /// IoU threshold for node matching (default: 0.5).
    iou_threshold: f32,
}

impl StateTracker {
    /// Create a new `StateTracker` with default IoU threshold of `0.5`.
    pub fn new() -> Self {
        Self {
            prev: None,
            next_stable_id: 1,
            iou_threshold: 0.5,
        }
    }

    /// Process a new raw frame from the perception pipeline.
    ///
    /// - On the **first call**, stores the graph as-is and returns
    ///   [`FrameOutput::Snapshot`].
    /// - On **subsequent calls**, runs ID stabilization + diff and returns
    ///   [`FrameOutput::Diff`] (which may have zero mutations if unchanged).
    pub fn process_frame(&mut self, mut current: SceneGraph) -> FrameOutput {
        let Some(prev) = &self.prev else {
            // First frame: assign stable IDs, store, emit snapshot.
            self.assign_stable_ids(&mut current.root, 0);
            self.prev = Some(current.clone());
            return FrameOutput::Snapshot(current);
        };

        let prev_seq = prev.seq;

        // ── Step 1: Flatten both graphs ────────────────────────────────────
        let mut prev_flat: Vec<FlatNode> = Vec::new();
        flatten(&prev.root, 0, &mut prev_flat);

        let mut curr_flat: Vec<FlatNode> = Vec::new();
        flatten(&current.root, 0, &mut curr_flat);

        // ── Step 2: IoU bipartite match curr_raw_id → prev_stable_id ──────
        let remap = match_nodes(&prev_flat, &curr_flat, self.iou_threshold);

        // ── Step 3: Remap current IDs to stable IDs ────────────────────────
        remap_ids(&mut current.root, &remap);

        // ── Step 4: Assign fresh stable IDs to unmatched (new) nodes ───────
        let pre_next = self.next_stable_id;
        self.assign_stable_ids_unmatched(&mut current.root, &remap, pre_next);

        // ── Step 5: Re-flatten with stable IDs ────────────────────────────
        let mut curr_stable: Vec<FlatNode> = Vec::new();
        flatten(&current.root, 0, &mut curr_stable);

        // ── Step 6: Compute diff ───────────────────────────────────────────
        let curr_seq = current.seq;
        let mutations = compute_diff(&prev_flat, &curr_stable);

        // ── Step 7: Store stabilized current graph as next prev ────────────
        self.prev = Some(current);

        FrameOutput::Diff(DiffFrame::new(curr_seq, prev_seq, mutations))
    }

    /// Reset the tracker (e.g. on window/desktop switch). Next frame will be
    /// emitted as a full snapshot.
    pub fn reset(&mut self) {
        self.prev = None;
    }

    /// Access the latest stabilized SceneGraph stored in the tracker.
    pub fn current_scene(&self) -> Option<&SceneGraph> {
        self.prev.as_ref()
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Recursively assign stable IDs to all nodes in a subtree, starting from
    /// the current `next_stable_id` counter. Used for the first frame.
    fn assign_stable_ids(&mut self, node: &mut SceneNode, _parent: u32) {
        node.id = self.next_stable_id;
        self.next_stable_id += 1;
        for child in &mut node.children {
            self.assign_stable_ids(child, node.id);
        }
    }

    fn assign_stable_ids_unmatched(
        &mut self,
        node: &mut SceneNode,
        remap: &HashMap<u32, u32>,
        pre_next: u32,
    ) {
        if node.id >= pre_next {
            node.id = self.next_stable_id;
            self.next_stable_id += 1;
        }
        for child in &mut node.children {
            self.assign_stable_ids_unmatched(child, remap, pre_next);
        }
    }
}

impl Default for StateTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Diff computation
// ─────────────────────────────────────────────────────────────────────────────

/// Compare two flat node lists and produce the minimal mutation set.
fn compute_diff(prev: &[FlatNode], curr: &[FlatNode]) -> Vec<Mutation> {
    let prev_map: HashMap<u32, &FlatNode> = prev.iter().map(|n| (n.id, n)).collect();
    let curr_map: HashMap<u32, &FlatNode> = curr.iter().map(|n| (n.id, n)).collect();

    let mut mutations = Vec::new();

    // ── REMOVE: nodes in prev but not in curr ─────────────────────────────
    for p in prev {
        if !curr_map.contains_key(&p.id) {
            mutations.push(Mutation::Remove { id: p.id });
        }
    }

    // ── INSERT / UPDATE: nodes in curr ────────────────────────────────────
    for c in curr {
        match prev_map.get(&c.id) {
            None => {
                // New node — INSERT
                mutations.push(Mutation::Insert {
                    parent_id: c.parent_id,
                    node: c.node.clone(),
                });
            }
            Some(p) => {
                // Existing node — diff fields
                let patch = diff_fields(&p.node, &c.node);
                if !patch.is_empty() {
                    mutations.push(Mutation::Update { id: c.id, patch });
                }
            }
        }
    }

    mutations
}

/// Produce a [`NodePatch`] containing only the fields that changed between
/// `prev` and `curr`.
fn diff_fields(prev: &SceneNode, curr: &SceneNode) -> NodePatch {
    NodePatch {
        bounds: if prev.bounds != curr.bounds { Some(curr.bounds) } else { None },
        node_type: if prev.node_type != curr.node_type {
            Some(curr.node_type.clone())
        } else {
            None
        },
        label: if prev.label != curr.label { curr.label.clone() } else { None },
        value: if prev.value != curr.value { curr.value.clone() } else { None },
        focused: if prev.focused != curr.focused { curr.focused } else { None },
        interactable: if prev.interactable != curr.interactable {
            curr.interactable
        } else {
            None
        },
        disabled: if prev.disabled != curr.disabled { curr.disabled } else { None },
        source: if prev.source != curr.source { curr.source.clone() } else { None },
        class_name: if prev.class_name != curr.class_name { curr.class_name.clone() } else { None },
        phash: if prev.phash != curr.phash { curr.phash.clone() } else { None },
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Output type
// ─────────────────────────────────────────────────────────────────────────────

/// Result of processing a single frame through the [`StateTracker`].
#[derive(Debug)]
pub enum FrameOutput {
    /// First frame (or after a reset): full scene graph snapshot.
    Snapshot(SceneGraph),
    /// Subsequent frames: temporal delta diff (may have zero mutations).
    Diff(DiffFrame),
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeType, SceneGraph, SceneNode, Viewport, PROTOCOL_VERSION};

    fn viewport() -> Viewport {
        Viewport { width: 1920, height: 1080, scale_factor: 1.0 }
    }

    fn make_node(id: u32, node_type: NodeType, bounds: [u32; 4]) -> SceneNode {
        SceneNode {
            id,
            node_type,
            bounds,
            label: None,
            value: None,
            focused: None,
            interactable: None,
            disabled: None,
            children: Vec::new(),
            source: None,
            class_name: None,
            phash: None,
            color: None,
            color_hex: None,
        }
    }

    fn make_graph(seq: u64, root: SceneNode) -> SceneGraph {
        SceneGraph {
            protocol: PROTOCOL_VERSION.to_string(),
            seq,
            timestamp_us: seq * 16_000,
            viewport: viewport(),
            active_window: None,
            root,
        }
    }

    // ── IoU ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_iou_identical() {
        let b = [100, 100, 300, 300];
        let result = iou(b, b);
        assert!((result - 1.0).abs() < 1e-5, "IoU of identical boxes must be 1.0, got {result}");
    }

    #[test]
    fn test_iou_no_overlap() {
        let a = [0, 0, 100, 100];
        let b = [200, 200, 300, 300];
        assert_eq!(iou(a, b), 0.0);
    }

    #[test]
    fn test_iou_partial() {
        // a = [0,0,100,100] (10000 px²), b = [50,50,150,150] (10000 px²)
        // intersection = [50,50,100,100] = 2500 px²
        // union = 10000 + 10000 - 2500 = 17500
        let a = [0, 0, 100, 100];
        let b = [50, 50, 150, 150];
        let expected = 2500.0 / 17500.0;
        let got = iou(a, b);
        assert!((got - expected).abs() < 1e-5, "expected {expected}, got {got}");
    }

    // ── First frame is a Snapshot ────────────────────────────────────────────

    #[test]
    fn test_first_frame_is_snapshot() {
        let mut tracker = StateTracker::new();
        let root = make_node(1, NodeType::Container, [0, 0, 1920, 1080]);
        let g = make_graph(1, root);

        match tracker.process_frame(g) {
            FrameOutput::Snapshot(_) => {}
            FrameOutput::Diff(_) => panic!("first frame must be a Snapshot"),
        }
    }

    // ── Identical frames → empty diff ────────────────────────────────────────

    #[test]
    fn test_identical_frames_empty_diff() {
        let mut tracker = StateTracker::new();

        let mut root1 = make_node(1, NodeType::Container, [0, 0, 1920, 1080]);
        root1.children.push(make_node(2, NodeType::Button, [100, 100, 200, 130]));
        let g1 = make_graph(1, root1.clone());

        // First frame
        tracker.process_frame(g1);

        // Second frame identical geometry
        let root2 = root1;
        let g2 = make_graph(2, root2);
        match tracker.process_frame(g2) {
            FrameOutput::Diff(d) => {
                assert!(
                    d.is_empty(),
                    "identical frames must produce empty diff, got: {d:?}"
                );
            }
            FrameOutput::Snapshot(_) => panic!("second frame must be a Diff"),
        }
    }

    // ── Field UPDATE mutation ────────────────────────────────────────────────

    #[test]
    fn test_update_field_change() {
        let mut tracker = StateTracker::new();

        let mut btn = make_node(10, NodeType::Button, [100, 100, 200, 130]);
        btn.label = Some("OK".to_string());
        let mut root1 = make_node(1, NodeType::Container, [0, 0, 1920, 1080]);
        root1.children.push(btn.clone());
        tracker.process_frame(make_graph(1, root1));

        // Change button label
        let mut btn2 = btn.clone();
        btn2.label = Some("Cancel".to_string());
        let mut root2 = make_node(1, NodeType::Container, [0, 0, 1920, 1080]);
        root2.children.push(btn2);
        let output = tracker.process_frame(make_graph(2, root2));

        match output {
            FrameOutput::Diff(d) => {
                assert_eq!(d.ack_seq, 1);
                assert_eq!(d.seq, 2);
                let updates: Vec<_> = d
                    .mutations
                    .iter()
                    .filter(|m| matches!(m, Mutation::Update { .. }))
                    .collect();
                assert!(
                    !updates.is_empty(),
                    "expected UPDATE mutation for label change"
                );
                for m in &updates {
                    if let Mutation::Update { patch, .. } = m {
                        assert_eq!(patch.label, Some("Cancel".to_string()));
                        assert!(patch.bounds.is_none(), "bounds unchanged");
                    }
                }
            }
            FrameOutput::Snapshot(_) => panic!("expected Diff"),
        }
    }

    // ── INSERT mutation ──────────────────────────────────────────────────────

    #[test]
    fn test_insert_new_node() {
        let mut tracker = StateTracker::new();

        let root1 = make_node(1, NodeType::Container, [0, 0, 1920, 1080]);
        tracker.process_frame(make_graph(1, root1));

        // Add a child button in frame 2
        let mut root2 = make_node(1, NodeType::Container, [0, 0, 1920, 1080]);
        // Note: we use id >= pre_next (pre_next is likely 2 here) to force un-matched status
        root2.children.push(make_node(99, NodeType::Button, [500, 500, 600, 530]));
        let output = tracker.process_frame(make_graph(2, root2));

        match output {
            FrameOutput::Diff(d) => {
                let inserts: Vec<_> = d
                    .mutations
                    .iter()
                    .filter(|m| matches!(m, Mutation::Insert { .. }))
                    .collect();
                assert_eq!(inserts.len(), 1, "expected 1 INSERT mutation");
                if let Mutation::Insert { node, .. } = &inserts[0] {
                    assert_eq!(node.node_type, NodeType::Button);
                }
            }
            FrameOutput::Snapshot(_) => panic!("expected Diff"),
        }
    }

    // ── REMOVE mutation ──────────────────────────────────────────────────────

    #[test]
    fn test_remove_node() {
        let mut tracker = StateTracker::new();

        let mut root1 = make_node(1, NodeType::Container, [0, 0, 1920, 1080]);
        root1.children.push(make_node(2, NodeType::Button, [100, 100, 200, 130]));
        tracker.process_frame(make_graph(1, root1));

        // Remove the button in frame 2
        let root2 = make_node(1, NodeType::Container, [0, 0, 1920, 1080]);
        let output = tracker.process_frame(make_graph(2, root2));

        match output {
            FrameOutput::Diff(d) => {
                let removes: Vec<_> = d
                    .mutations
                    .iter()
                    .filter(|m| matches!(m, Mutation::Remove { .. }))
                    .collect();
                assert_eq!(removes.len(), 1, "expected 1 REMOVE mutation");
            }
            FrameOutput::Snapshot(_) => panic!("expected Diff"),
        }
    }

    // ── JSON serialization ───────────────────────────────────────────────────

    #[test]
    fn test_diff_frame_json_serialization() {
        let diff = DiffFrame::new(
            2,
            1,
            vec![Mutation::Update {
                id: 14,
                patch: NodePatch {
                    value: Some("Deploy complete.".to_string()),
                    ..Default::default()
                },
            }],
        );

        let json = serde_json::to_string_pretty(&diff).expect("serialize DiffFrame");
        assert!(json.contains("\"type\": \"DIFF\""));
        assert!(json.contains("\"op\": \"UPDATE\""));
        assert!(json.contains("\"Deploy complete.\""));
        assert!(!json.contains("\"bounds\""), "empty patch fields must be omitted");

        // Verify < 2KB
        assert!(
            json.len() < 2048,
            "DiffFrame JSON must be < 2KB, got {} bytes",
            json.len()
        );
    }

    // ── Payload size metric ──────────────────────────────────────────────────

    #[test]
    fn test_payload_size_metric() {
        // Simulate 10 simultaneous field updates (busy frame)
        let mutations: Vec<Mutation> = (0..10)
            .map(|i| Mutation::Update {
                id: i,
                patch: NodePatch {
                    label: Some(format!("Label {i}")),
                    value: Some(format!("Value {i}")),
                    ..Default::default()
                },
            })
            .collect();

        let diff = DiffFrame::new(100, 99, mutations);
        let json = serde_json::to_string(&diff).expect("serialize");
        assert!(
            json.len() < 2048,
            "10-mutation diff must be < 2 KB, got {} bytes:\n{json}",
            json.len()
        );
    }
}
