"""
VACT (Vector Agent Context Transport) Python SDK
60 FPS Sub-Pixel AI Desktop OS Automation
"""

from .types import SceneGraph, SceneNode, Viewport, NodeType, ActionResult
from .client import VactClient

__version__ = "0.1.0"
__all__ = [
    "VactClient",
    "SceneGraph",
    "SceneNode",
    "Viewport",
    "NodeType",
    "ActionResult",
]
