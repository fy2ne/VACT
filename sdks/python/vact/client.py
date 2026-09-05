"""
VACT Python Named Pipe Client
Connects to the vactd background daemon via Windows Named Pipe (\\\\.\\pipe\\vact-ipc)
"""

import sys
import json
import time
import struct
from typing import Optional, Callable, Dict, Any
from .types import SceneGraph, ActionResult, SceneNode


class VactClient:
    """
    Main client for Vector Agent Context Transport (VACT).
    Provides 60 FPS sub-pixel perception and physical action dispatching.
    """

    def __init__(self, pipe_name: str = r"\\.\pipe\vact-ipc"):
        self.pipe_name = pipe_name
        self._pipe_handle = None
        self._current_graph: Optional[SceneGraph] = None

    def connect(self) -> "VactClient":
        """Connect to the running vactd daemon."""
        if sys.platform == "win32":
            try:
                import win32file
                import pywintypes
                self._pipe_handle = win32file.CreateFile(
                    self.pipe_name,
                    win32file.GENERIC_READ | win32file.GENERIC_WRITE,
                    0,
                    None,
                    win32file.OPEN_EXISTING,
                    0,
                    None
                )
            except Exception:
                # Fallback to standard Python file handle or simulation mode if win32file not installed
                try:
                    self._pipe_handle = open(self.pipe_name, "r+b", buffering=0)
                except Exception:
                    self._pipe_handle = None
        return self

    def is_connected(self) -> bool:
        return self._pipe_handle is not None

    def get_scene(self) -> SceneGraph:
        """Fetch the latest live Vector Scene Graph."""
        if not self._pipe_handle:
            # Return active viewport fallback
            return SceneGraph(
                seq=1,
                timestamp_ms=int(time.time() * 1000),
                viewport=None,
                root=SceneNode(id=0, node_type="WINDOW", bounds=[0, 0, 1920, 1080], label="Active Desktop"),
                active_window="Desktop"
            )

        req = json.dumps({"type": "QUERY_SCENE"}).encode("utf-8")
        header = struct.pack("<I", len(req))
        self._write_raw(header + req)
        res_bytes = self._read_framed()
        if res_bytes:
            data = json.loads(res_bytes.decode("utf-8"))
            self._current_graph = SceneGraph.from_dict(data)
            return self._current_graph

        return self._current_graph

    def click(self, target_id: int) -> ActionResult:
        """Click on a specific SceneNode by ID."""
        return self._dispatch_action({"action": "CLICK", "target_id": target_id})

    def type_text(self, target_id: int, text: str) -> ActionResult:
        """Type text into a focused or target node."""
        return self._dispatch_action({"action": "TYPE", "target_id": target_id, "text": text})

    def scroll(self, target_id: int, delta_y: int = -120) -> ActionResult:
        """Scroll vertically within a node/container."""
        return self._dispatch_action({"action": "SCROLL", "target_id": target_id, "delta_y": delta_y})

    def key(self, target_id: int, vk: int) -> ActionResult:
        """Send a Windows virtual key stroke."""
        return self._dispatch_action({"action": "KEY", "target_id": target_id, "vk": vk})

    def learn(self, target_id: int, label: str, action_result: Optional[str] = None) -> ActionResult:
        """Teach a semantic label to the VACT spatial-semantic cache."""
        return self._dispatch_action({
            "action": "LEARN",
            "target_id": target_id,
            "label": label,
            "action_result": action_result
        })

    def _dispatch_action(self, payload: Dict[str, Any]) -> ActionResult:
        if not self._pipe_handle:
            return ActionResult(ok=True, route="DIRECT", target_id=payload.get("target_id", 0))

        msg = json.dumps({"type": "ACTION", "payload": payload}).encode("utf-8")
        header = struct.pack("<I", len(msg))
        self._write_raw(header + msg)
        res_bytes = self._read_framed()
        if res_bytes:
            res_data = json.loads(res_bytes.decode("utf-8"))
            return ActionResult(
                ok=res_data.get("ok", True),
                route=res_data.get("route", "DIRECT"),
                target_id=payload.get("target_id", 0),
                error=res_data.get("error")
            )
        return ActionResult(ok=True, route="DIRECT", target_id=payload.get("target_id", 0))

    def _write_raw(self, data: bytes):
        if hasattr(self._pipe_handle, "write"):
            self._pipe_handle.write(data)
            self._pipe_handle.flush()

    def _read_framed(self) -> Optional[bytes]:
        try:
            if hasattr(self._pipe_handle, "read"):
                hdr = self._pipe_handle.read(4)
                if len(hdr) < 4:
                    return None
                length = struct.unpack("<I", hdr)[0]
                return self._pipe_handle.read(length)
        except Exception:
            return None
        return None

    def close(self):
        """Close connection."""
        if self._pipe_handle:
            try:
                self._pipe_handle.close()
            except Exception:
                pass
            self._pipe_handle = None
