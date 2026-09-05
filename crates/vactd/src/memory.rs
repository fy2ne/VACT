//! VACT Spatial-Semantic Agent Memory Engine
//!
//! Persistent SQLite key-value and associative memory cache that stores
//! perceptual hashes (dHash) of visual UI components paired with agent-learned
//! semantic labels, action outcomes, and element types.

use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use vact_core::phash::{compute_dhash, phash_to_hex};
use vact_protocol::{NodeType, SceneGraph, SceneNode};

/// An entry retrieved from the spatial-semantic memory cache.
#[derive(Clone, Debug, PartialEq)]
pub struct MemoryEntry {
    pub phash: String,
    pub app: String,
    pub label: String,
    pub node_type: String,
    pub action_result: Option<String>,
    pub confidence: f64,
    pub seen_count: u64,
}

/// Thread-safe persistent memory store backed by SQLite.
pub struct MemoryStore {
    conn: Mutex<Connection>,
}

impl MemoryStore {
    /// Open or create a memory database at the given filesystem path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open an in-memory database (useful for testing or ephemeral runs).
    pub fn open_in_memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open the default persistent store `vact_memory.db`.
    pub fn open_default() -> Result<Self, rusqlite::Error> {
        Self::open("vact_memory.db")
    }

    fn init_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

            CREATE TABLE IF NOT EXISTS memory (
                phash TEXT NOT NULL,
                app TEXT NOT NULL,
                label TEXT NOT NULL,
                node_type TEXT NOT NULL,
                action_result TEXT,
                confidence REAL NOT NULL DEFAULT 1.0,
                seen_count INTEGER NOT NULL DEFAULT 1,
                last_seen_us INTEGER NOT NULL,
                PRIMARY KEY (phash, app)
            );

            CREATE INDEX IF NOT EXISTS idx_memory_app ON memory(app);
            CREATE INDEX IF NOT EXISTS idx_memory_phash ON memory(phash);
            "#,
        )?;
        Ok(())
    }

    /// Look up a cached semantic label and properties by perceptual hash and app process name.
    pub fn lookup(&self, phash: &str, app: &str) -> Option<MemoryEntry> {
        let conn = self.conn.lock().ok()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT phash, app, label, node_type, action_result, confidence, seen_count
                 FROM memory
                 WHERE phash = ?1 AND (app = ?2 OR app = '*')
                 ORDER BY (app = ?2) DESC, seen_count DESC
                 LIMIT 1",
            )
            .ok()?;

        stmt.query_row(params![phash, app], |row| {
            Ok(MemoryEntry {
                phash: row.get(0)?,
                app: row.get(1)?,
                label: row.get(2)?,
                node_type: row.get(3)?,
                action_result: row.get(4)?,
                confidence: row.get(5)?,
                seen_count: row.get(6)?,
            })
        })
        .ok()
    }

    /// Teach or update a memory entry for an element.
    pub fn learn(
        &self,
        phash: &str,
        app: &str,
        label: &str,
        node_type: &str,
        action_result: Option<&str>,
    ) -> Result<(), rusqlite::Error> {
        let now_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as i64)
            .unwrap_or(0);

        let conn = self.conn.lock().map_err(|_| rusqlite::Error::ExecuteReturnedResults)?;
        conn.execute(
            r#"
            INSERT INTO memory (phash, app, label, node_type, action_result, confidence, seen_count, last_seen_us)
            VALUES (?1, ?2, ?3, ?4, ?5, 1.0, 1, ?6)
            ON CONFLICT(phash, app) DO UPDATE SET
                label = excluded.label,
                node_type = excluded.node_type,
                action_result = excluded.action_result,
                confidence = MIN(memory.confidence + 0.1, 1.0),
                seen_count = memory.seen_count + 1,
                last_seen_us = excluded.last_seen_us
            "#,
            params![phash, app, label, node_type, action_result, now_us],
        )?;

        Ok(())
    }

    /// Record a cache hit (bump seen count and touch timestamp).
    pub fn record_hit(&self, phash: &str, app: &str) {
        let now_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as i64)
            .unwrap_or(0);

        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "UPDATE memory SET seen_count = seen_count + 1, last_seen_us = ?1 WHERE phash = ?2 AND app = ?3",
                params![now_us, phash, app],
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SceneGraph Memory Enrichment
// ─────────────────────────────────────────────────────────────────────────────

/// Enrich SceneGraph nodes with pHash and cached semantic labels from the agent memory.
pub fn enrich_scene_graph_memory(
    graph: &mut SceneGraph,
    frame_data: &[u8],
    frame_w: u32,
    frame_h: u32,
    app_name: &str,
    memory: &MemoryStore,
) {
    enrich_node_memory_recursive(&mut graph.root, frame_data, frame_w, frame_h, app_name, memory);
}

fn enrich_node_memory_recursive(
    node: &mut SceneNode,
    frame_data: &[u8],
    frame_w: u32,
    frame_h: u32,
    app_name: &str,
    memory: &MemoryStore,
) {
    let [x1, y1, x2, y2] = node.bounds;

    // 1. Always compute and attach perceptual hash if not already computed
    if node.phash.is_none() && frame_w > 0 && frame_h > 0 && !frame_data.is_empty() {
        let hash = compute_dhash(frame_data, frame_w, frame_h, x1, y1, x2, y2);
        node.phash = Some(phash_to_hex(hash));
    }

    // 2. Query spatial memory cache if label is missing or came from memory
    if let Some(ref phash) = node.phash {
        if node.label.is_none() || node.source.as_deref() == Some("memory") {
            if let Some(entry) = memory.lookup(phash, app_name) {
                node.label = Some(entry.label);
                node.source = Some("memory".to_string());
                
                // Parse and map NodeType if entry specified a distinct role
                match entry.node_type.to_uppercase().as_str() {
                    "BUTTON" => {
                        node.node_type = NodeType::Button;
                        node.interactable = Some(true);
                    }
                    "INPUT_FIELD" => {
                        node.node_type = NodeType::InputField;
                        node.interactable = Some(true);
                    }
                    "LIST" => {
                        node.node_type = NodeType::List;
                    }
                    "TEXT" => {
                        node.node_type = NodeType::Text;
                    }
                    _ => {}
                }

                memory.record_hit(phash, app_name);
            }
        }
    }

    for child in &mut node.children {
        enrich_node_memory_recursive(child, frame_data, frame_w, frame_h, app_name, memory);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_store_lifecycle() {
        let store = MemoryStore::open_in_memory().expect("failed to open memory db");

        let phash = "a1b2c3d4e5f60718";
        let app = "discord.exe";

        // Initial lookup should be empty
        assert!(store.lookup(phash, app).is_none());

        // Learn a label
        store
            .learn(phash, app, "User Settings", "BUTTON", Some("opened_modal"))
            .expect("learn failed");

        // Lookup should hit
        let entry = store.lookup(phash, app).expect("lookup failed");
        assert_eq!(entry.label, "User Settings");
        assert_eq!(entry.node_type, "BUTTON");
        assert_eq!(entry.action_result.as_deref(), Some("opened_modal"));
        assert_eq!(entry.seen_count, 1);

        // Update / learn again -> should bump seen_count
        store
            .learn(phash, app, "User Settings", "BUTTON", Some("opened_modal_v2"))
            .expect("re-learn failed");

        let entry2 = store.lookup(phash, app).expect("lookup 2 failed");
        assert_eq!(entry2.seen_count, 2);
        assert_eq!(entry2.action_result.as_deref(), Some("opened_modal_v2"));
    }

    #[test]
    fn test_memory_enrich_scene_graph() {
        let store = MemoryStore::open_in_memory().expect("db open");
        let phash = "ffff0000ffff0000";
        store
            .learn(phash, "code.exe", "Explorer Toggle", "BUTTON", None)
            .expect("learn");

        let node = SceneNode {
            id: 1,
            node_type: NodeType::Icon,
            bounds: [10, 10, 30, 30],
            label: None,
            value: None,
            focused: None,
            interactable: None,
            disabled: None,
            children: Vec::new(),
            source: None,
            class_name: None,
            phash: Some(phash.to_string()),
            color: None,
            color_hex: None,
        };

        let mut graph = SceneGraph {
            protocol: "VACT/1.0".to_string(),
            seq: 1,
            timestamp_us: 1000,
            viewport: vact_protocol::Viewport {
                width: 1920,
                height: 1080,
                scale_factor: 1.0,
            },
            active_window: Some("code.exe".to_string()),
            root: node.clone(),
        };

        enrich_scene_graph_memory(&mut graph, &[], 0, 0, "code.exe", &store);

        assert_eq!(graph.root.label.as_deref(), Some("Explorer Toggle"));
        assert_eq!(graph.root.source.as_deref(), Some("memory"));
        assert_eq!(graph.root.node_type, NodeType::Button);
        assert_eq!(graph.root.interactable, Some(true));
    }
}
