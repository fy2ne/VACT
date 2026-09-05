<div align="center">

# 🐍 VACT Python SDK

[![PyPI version](https://img.shields.io/badge/pypi-v0.1.0-blue.svg)](https://pypi.org/project/vact/)
[![Python 3.10+](https://img.shields.io/badge/python-3.10+-brightgreen.svg)](https://python.org)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Built with Google Cloud AI](https://img.shields.io/badge/Google%20Cloud-Vertex%20AI%20%26%20Gemini-4285F4?logo=google-cloud&logoColor=white)](https://cloud.google.com/vertex-ai)

**Sub-Pixel Vector Scene Graphs & Direct3D11 Hardware Action Dispatch for Autonomous AI Agents**

*Built for the Google Cloud Agentic AI Hackathon 2026 by fy2ne ([@fy2ne](https://github.com/fy2ne)).*

</div>

---

## ⚡ Installation

```bash
pip install vact
```

---

## 🚀 Quickstart: Google Gemini 2.5 Flash Agent

```python
import os
import json
from vact import VactClient
from google import genai

# 1. Connect to VACT 60 FPS Sub-Pixel Vector Engine
client = VactClient().connect()
scene = client.get_scene()

print(f"Active Foreground Window: {scene.active_window}")
print(f"Root Viewport: {scene.viewport.width}x{scene.viewport.height}")

# 2. Query Google Gemini 2.5 Flash with the Compact Vector AST
gemini = genai.Client(api_key=os.environ["GEMINI_API_KEY"])

prompt = f"""
You are an autonomous GUI agent.
Active Window: {scene.active_window}
User Mission: Open Notepad and write 'Automated via Google Gemini 2.5 Flash + VACT'

Return JSON: {{"action": "CLICK" | "TYPE" | "KEY", "target_id": <int>, "text": "<string>"}}
"""

response = gemini.models.generate_content(
    model="gemini-2.5-flash",
    contents=prompt
)
action = json.loads(response.text)

# 3. Dispatch Physical Hardware Action (Zero-Vision Overhead)
if action["action"] == "CLICK":
    client.click(action["target_id"])
elif action["action"] == "TYPE":
    client.type_text(action["target_id"], action.get("text", ""))

print("✅ Action dispatched via VACT 60 FPS Bus!")
```

---

## 🏛️ Architecture

```
                                  ┌───────────────────────────┐
                                  │   Google Cloud Vertex AI  │
                                  │   or Google AI Studio     │
                                  │   (Gemini 2.5 Flash/Pro)  │
                                  └─────────────▲─────────────┘
                                                │ Compact Vector AST
                                                │ (<300 tokens / frame)
                                  ┌─────────────▼─────────────┐
                                  │      VACT Python SDK      │
                                  │       `vact-python`       │
                                  └─────────────▲─────────────┘
                                                │ \\.\pipe\vact-ipc
                                                │ (Named Pipe / 60 FPS)
┌───────────────────────────────────────────────▼───────────────────────────────────────────────┐
│                                       vactd Rust Daemon                                       │
│  ┌──────────────────────┐   ┌─────────────────────────────┐   ┌────────────────────────────┐  │
│  │ D3D11 GPU Bilateral  │ ➜ │  CCL Watershed Vectorizer   │ ➜ │ Hardware Action Dispatcher │  │
│  │ (0.8ms Shader Pass)  │   │  (Incremental Delta DAG)    │   │ (SendInput Sub-Pixel Synced│  │
│  └──────────────────────┘   └─────────────────────────────┘   └────────────────────────────┘  │
└───────────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 📜 License

Licensed under the **Apache License 2.0**.
Copyright © 2026 **fy2ne ([@fy2ne](https://github.com/fy2ne))**.
