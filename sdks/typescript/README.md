# ⚡ `vact-sdk`
### Enterprise TypeScript / Node.js SDK for the VACT Protocol

[![npm version](https://img.shields.io/npm/v/vact-sdk.svg?style=flat-square)](https://www.npmjs.com/package/vact-sdk)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg?style=flat-square)](https://opensource.org/licenses/Apache-2.0)
[![TypeScript Strict](https://img.shields.io/badge/TypeScript-Strict-blue?style=flat-square)](https://www.typescriptlang.org/)
[![Website](https://img.shields.io/badge/Website-vact.fy2ne.me-purple?style=flat-square)](https://vact.fy2ne.me)

**VACT (Vector Agent Context Transport)** is a hardware-accelerated, real-time vector scene graph streaming and I/O transport protocol for autonomous AI agents.

`vact-sdk` connects your Node.js/TypeScript agents (LangChain.js, AutoGen, custom LLM loops) directly to the high-speed `vactd` daemon over Windows Named Pipe IPC.

---

## 🎯 The Paradigm Shift

| Feature | Anthropic Computer Use | Microsoft OmniParser | OS Accessibility (UIA) | **VACT (`vact-sdk`)** |
| :--- | :--- | :--- | :--- | :--- |
| **Perception Method** | Full-frame Screenshot | YOLO + Florence-2 VLM | OS COM Tree Traversal | **GPU Compute Shaders + Micro-Tensors** |
| **End-to-End Latency** | 4,000ms – 8,000ms | 150ms – 500ms | 500ms – 2,000ms | **10ms – 25ms** |
| **Payload Size** | ~2,000KB (PNG) | ~100KB (JSON) | ~500KB (XML) | **< 2KB (Temporal Vector Delta)** |
| **Vision Token Cost** | ~$0.05 / step | ~$0.005 / step | $0.00 | **$0.00 (Zero Vision Tokens)** |
| **Figma / Canvas / WebGL** | Slow, partial accuracy | Medium accuracy | **100% Blind (0% coverage)** | **100% Full Geometric Perception** |
| **Architecture** | Stateless Polling | Stateless API | Polling / Events | **Continuous 60 FPS State Machine** |

---

## 📦 Installation

```bash
npm install vact-sdk
# or
pnpm add vact-sdk
# or
yarn add vact-sdk
```

---

## 🚀 Quick Start

### 1. Connect and Stream Vector Scene Graph

```typescript
import { VactClient } from 'vact-sdk';

const client = new VactClient();
await client.connect();

// Receive initial full Vector Scene Graph snapshot
client.on('snapshot', (graph) => {
  console.log(`Initial UI Loaded: ${graph.active_window} (${graph.viewport.width}x${graph.viewport.height})`);
});

// Stream real-time mutations (< 2KB per frame)
client.on('diff', (diff) => {
  console.log(`Frame #${diff.seq}: ${diff.mutations.length} mutations detected.`);
});
```

---

### 2. Querying Elements via Fluent DSL (`SceneQuery`)

No manual DAG traversals or coordinate calculations required:

```typescript
// Find interactive button by label regex
const submitBtn = client.query()
  .ofType('BUTTON')
  .withLabel(/submit|send|save/i)
  .interactable()
  .notDisabled()
  .first();

if (submitBtn) {
  console.log(`Found node #${submitBtn.id} at [${submitBtn.bounds.join(', ')}]`);
  
  // Dispatch synthetic hardware click
  await client.agent().click(submitBtn.id);
}
```

---

### 3. Dispatching Actions (`AgentContext`)

VACT features a progressive **Hybrid I/O Bus** that auto-routes actions between hardware-level synthetic input (`SendInput`) and background message queues (`PostMessageW`):

```typescript
const agent = client.agent();

// Click an element
await agent.click(targetNodeId);

// Type text into an input field
await agent.type(targetNodeId, 'Autonomous agent typing');

// Scroll an element (deltaY: negative = up, positive = down)
await agent.scroll(targetNodeId, -120);

// Send virtual key code (e.g. Enter = 0x0D)
await agent.key(targetNodeId, 0x0D);

// Focus an element
await agent.focus(targetNodeId);
```

---

### 4. Async Iterator (LLM Agent Loop Pattern)

```typescript
// Seamlessly iterate over scene changes in an agent control loop
for await (const frame of client.frames()) {
  if (frame.type === 'SNAPSHOT') {
    // Initial scene snapshot
    const state = client.getState();
    await agentBrain.plan(state);
  } else if (frame.type === 'DIFF' && frame.diff.mutations.length > 0) {
    // UI mutated — evaluate progress
    const state = client.getState();
    await agentBrain.step(state, frame.diff);
  }
}
```

---

## 🛠️ LangChain / Agent Frameworks Integration

```typescript
import { DynamicStructuredTool } from '@langchain/core/tools';
import { z } from 'zod';
import { VactClient } from 'vact-sdk';

const client = new VactClient();
await client.connect();

export const clickTool = new DynamicStructuredTool({
  name: 'click_element',
  description: 'Click on an interactive UI element by its stable node ID from the VACT scene graph.',
  schema: z.object({
    node_id: z.number().describe('The ID of the UI element to click'),
  }),
  func: async ({ node_id }) => {
    const res = await client.agent().click(node_id);
    return `Action dispatched via ${res.route} mode (ok: ${res.ok})`;
  },
});
```

---

## 🏛️ Architecture & Wire Protocol

```
┌────────────────────────────────────────────────────────┐
│              TypeScript SDK (`vact-sdk`)               │
│  ┌──────────────┐  ┌─────────────┐  ┌───────────────┐  │
│  │  VactClient  │  │ SceneQuery  │  │ AgentContext  │  │
│  └──────┬───────┘  └──────┬──────┘  └───────┬───────┘  │
│         │                 │                 │          │
│  ┌──────▼─────────────────▼─────────────────▼───────┐  │
│  │       StateManager (Hydrated In-Memory DAG)       │  │
│  └────────────────────────┬─────────────────────────┘  │
│                           │                            │
│  ┌────────────────────────▼─────────────────────────┐  │
│  │   Framing Codec (4-byte LE length prefix + JSON) │  │
│  └────────────────────────┬─────────────────────────┘  │
└───────────────────────────┼────────────────────────────┘
                            │ Named Pipe (\\.\pipe\VACT)
┌───────────────────────────▼────────────────────────────┐
│                    `vactd` Rust Daemon                 │
│  (GPU Shaders ➔ Watershed CCL ➔ DirectML OCR ➔ I/O)    │
└────────────────────────────────────────────────────────┘
```

---

## 🧪 Testing & Verification

The SDK includes a comprehensive test suite with 100% mock transport coverage:

```bash
npm test
npm run test:coverage
npm run build
```

---

## 👥 Authors

- **fy2ne**: Core Architecture, DirectX GPU Compute Shaders & Rust Daemon (<hello@fy2ne.me>)

## 📄 License & Attribution

- **Repository:** [https://github.com/fy2ne/VACT](https://github.com/fy2ne/VACT)
- **Website:** [https://fy2ne.me](https://fy2ne.me)
- **License:** Apache License 2.0
