"""
Type definitions for VACT Python SDK
"""

from typing import List, Optional, Tuple, Dict, Any
from dataclasses import dataclass, field
from enum import Enum


class NodeType(str, Enum):
    WINDOW = "WINDOW"
    BUTTON = "BUTTON"
    INPUT_FIELD = "INPUT_FIELD"
    TEXT = "TEXT"
    IMAGE = "IMAGE"
    ICON = "ICON"
    CONTAINER = "CONTAINER"
    LIST = "LIST"
    UNKNOWN = "UNKNOWN"


@dataclass
class Viewport:
    width: int = 1920
    height: int = 1080
    dpi_scale: float = 1.0


@dataclass
class SceneNode:
    id: int
    node_type: NodeType
    bounds: List[int]  # [x, y, w, h]
    label: Optional[str] = None
    value: Optional[str] = None
    interactable: bool = True
    focused: bool = False
    source: Optional[str] = None
    class_name: Optional[str] = None
    semantic_annotation: Optional[str] = None
    confidence: float = 1.0
    color: Optional[str] = None
    color_hex: Optional[str] = None
    children: List["SceneNode"] = field(default_factory=list)

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "SceneNode":
        nt = data.get("type", data.get("node_type", "UNKNOWN"))
        try:
            enum_type = NodeType(nt)
        except ValueError:
            enum_type = NodeType.UNKNOWN

        return cls(
            id=data.get("id", 0),
            node_type=enum_type,
            bounds=data.get("bounds", [0, 0, 0, 0]),
            label=data.get("label"),
            value=data.get("value"),
            interactable=data.get("interactable", True),
            focused=data.get("focused", False),
            source=data.get("source"),
            class_name=data.get("class_name"),
            semantic_annotation=data.get("semantic_annotation"),
            confidence=float(data.get("confidence", 1.0)),
            color=data.get("color"),
            color_hex=data.get("color_hex"),
            children=[cls.from_dict(c) for c in data.get("children", [])],
        )


@dataclass
class SceneGraph:
    seq: int
    timestamp_ms: int
    viewport: Viewport
    root: SceneNode
    active_window: Optional[str] = None

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "SceneGraph":
        vp_raw = data.get("viewport", {})
        vp = Viewport(
            width=vp_raw.get("width", 1920),
            height=vp_raw.get("height", 1080),
            dpi_scale=vp_raw.get("dpi_scale", 1.0),
        )
        root_raw = data.get("root", {"id": 0, "type": "WINDOW", "bounds": [0, 0, 1920, 1080]})
        return cls(
            seq=data.get("seq", 0),
            timestamp_ms=data.get("timestamp_ms", 0),
            viewport=vp,
            root=SceneNode.from_dict(root_raw),
            active_window=data.get("active_window"),
        )


@dataclass
class ActionResult:
    ok: bool
    route: str = "DIRECT"  # "DIRECT" | "GHOST"
    target_id: int = 0
    error: Optional[str] = None
