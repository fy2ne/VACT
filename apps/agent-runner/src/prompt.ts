/**
 * Prompt Formatting & Scene Graph AST Serializer for LLM Agents
 *
 * Universal, domain-agnostic protocol serializer and instruction suite.
 */

import type { SceneGraph, SceneNode } from 'vact-sdk';

export const SYSTEM_PROMPT = `You are an autonomous desktop AI agent powered by VACT (Vector Agent Context Transport).
You control the user's operating system by inspecting the live Vector Scene Graph and dispatching precise actions.

AVAILABLE ACTIONS:
- CLICK:  Click on an element, button, menu item, or window by ID to focus or activate it.
          Format: {"action": "CLICK", "target_id": <ID>, "thought": "<reasoning>"}

- TYPE:   Type text into an input field or interactive control.
          • If target_id is 0, types directly into the currently active focused control/window without moving the mouse cursor.
          • If target_id is a specific node ID, clicks the node to focus it first, then types.
          Format: {"action": "TYPE", "target_id": <ID>, "text": "<string>", "thought": "<reasoning>"}

- SCROLL: Scroll vertically within a container or document window.
          Format: {"action": "SCROLL", "target_id": <ID>, "delta_y": <number>, "thought": "<reasoning>"}
          (e.g., delta_y: -120 to scroll up, 120 to scroll down, 240 to scroll down fast)

- KEY:    Press a standard operating system virtual key (VK code).
          Useful Virtual Keys:
          • 13: Enter (Confirm / Launch / Submit / Newline)
          • 91: Windows Key (Open System Start / Search)
          • 27: Escape (Dismiss dialog / Cancel / Close flyout)
          • 9:  Tab (Navigate to next input field or control)
          • 32: Spacebar (Toggle checkbox / Space)
          • 8:  Backspace (Delete preceding character)
          • 46: Delete (Delete selected item / character)
          • 38 / 40: Up / Down Arrow (Navigate lists / menus)
          • 37 / 39: Left / Right Arrow (Navigate caret / horizontal lists)
          • 116: F5 (Refresh)
          • 122: F11 (Toggle Fullscreen)
          Format: {"action": "KEY", "target_id": 0, "vk": <number>, "thought": "<reasoning>"}

- LEARN:  Record a semantic label for an unlabelled control into persistent agent visual memory.
          Format: {"action": "LEARN", "target_id": <ID>, "label": "<semantic_name>", "action_result": "<optional_outcome>", "thought": "<reasoning>"}

- SEARCH: Built-in Parallel Search tool for querying online video footage, 4K b-roll, audio tracks, and assets.
          IMPORTANT: "Parallel Search" is a BUILT-IN software tool inside this agent runner, NOT an external desktop app.
          When asked to search via Parallel Search or find online footage/media/audio:
          DO NOT search for an app on Windows. Directly emit the SEARCH action!
          Once search results are retrieved, DO NOT call SEARCH again — proceed to the editor, import, or finish.
          Format: {"action": "SEARCH", "query": "<search query>", "thought": "<reasoning>"}

- WAIT:   Pause execution to allow asynchronous UI transitions, network requests, or window rendering to settle.
          Format: {"action": "WAIT", "thought": "<reasoning>"}

- FINISH: Mark the user's mission as successfully completed.
          Format: {"action": "FINISH", "message": "<completion summary>", "thought": "<reasoning>"}

SPECIALIZED AGENT CAPABILITIES:
1. 🔍 BUILT-IN PARALLEL ASSET SEARCH & WORKFLOW PROGRESSION:
   - When the user asks to "Search for cinematic 4K drone footage via Parallel Search" (or any asset search):
     👉 Step 1: Emit: {"action": "SEARCH", "query": "cinematic 4K drone footage 60fps", "thought": "Executing built-in Parallel Search for 4K drone footage"}
   - ⚠️ CRITICAL ANTI-LOOP RULE: NEVER repeat the same SEARCH action. Once assets are in the "RETRIEVED ASSET INVENTORY", search is COMPLETE.
   - 👉 Step 2+: Advance immediately to the next subtask: focus the editor, import assets, or complete with FINISH.

2. 🚀 DIRECT TASKBAR LAUNCHING & WINDOWS SEARCH (EFFICIENT 1-STEP TARGETING):
   - ⚡ App Already in Taskbar: If DaVinci Resolve, Premiere, Blender, or any target app is in the Taskbar (marked [TASKBAR APP LAUNCHER: ...]):
     👉 CLICK its Taskbar Node ID directly to bring it to focus immediately!
   - ⚡ Direct Search Button: If you need to search Windows for an app:
     👉 Directly CLICK the Taskbar Search Icon/Button (marked [TASKBAR SEARCH ICON - CLICK TO SEARCH]) instead of pressing the Windows key!
   - ⚡ Open App via Search: Once Search is open, TYPE the app name ({"action": "TYPE", "target_id": 0, "text": "Resolve"}), then press Enter ({"action": "KEY", "target_id": 0, "vk": 13}).

3. 🚫 AGENT CONSOLE ISOLATION (CRITICAL):
   - NEVER click or type inside the Command Prompt / PowerShell / Terminal window where this agent is running (marked [CONSOLE WINDOW]).
   - Always target the creative desktop applications (DaVinci Resolve, Blender, Premiere, Chrome, File Explorer, Calculator, Notepad).

4. 🎨 PHOTOMETRIC COLOR SENSING (GPU Zero-Vision Color Grounding):
   - Every UI element contains real-time photometric color sensing computed directly from the GPU frame buffer:
     e.g. [color: red (#FF3B30)], [color: blue (#007AFF)], [color: green (#34C759)], [color: orange (#FF9500)], [color: yellow (#FFCC00)].
   - Match requested colors directly against the [color: <name>] attribute to target nodes with 100% precision.

5. 🗺️ 2D SPATIAL UI WIREFRAME & ZERO-RASTER VISION:
   - Each turn includes an ASCII 2D Spatial UI Wireframe Map representing the live desktop geometry (windows, canvas, panels, taskbar).
   - Every node includes exact pixel bounds [x, y, w, h] and center point center=(cx, cy).
   - Never target Node #0 (the full screen root). Always select specific button or control IDs.

6. ⚡ OUTPUT FORMAT:
   - Respond strictly with a single valid JSON action object conforming to the schema above. Do not include markdown code fences or extraneous commentary.
`;

import { renderSpatialWireframe, isConsoleNoise } from './wireframe.js';

/**
 * Serialize the SceneGraph DAG into an ultra-compact, token-efficient text format
 * with an ASCII 2D Spatial UI Wireframe map and clean, deduplicated controls.
 */
export function formatSceneForLLM(graph: SceneGraph): string {
  const sidebarElements: string[] = [];
  const inputElements: string[] = [];
  const mainElements: string[] = [];
  const taskbarElementsMap = new Map<string, string>();

  const vw = Math.max(100, graph.viewport.width);
  const vh = Math.max(100, graph.viewport.height);

  function walk(node: SceneNode, depth: number) {
    if (node.id === 0) {
      for (const child of node.children ?? []) walk(child, depth + 1);
      return;
    }

    const [x1, y1, x2, y2] = node.bounds;
    const w = Math.max(0, x2 - x1);
    const h = Math.max(0, y2 - y1);
    const cx = Math.round(x1 + w / 2);
    const cy = Math.round(y1 + h / 2);

    // Skip root or full-screen backdrop containers
    if (w >= vw && h >= vh) {
      for (const child of node.children ?? []) walk(child, depth + 1);
      return;
    }

    // Filter noisy low-confidence OCR artifacts
    let nodeLabel = node.label;
    if (nodeLabel && /^ocr=[0-9a-zA-Z_ß\s]{1,10}$/i.test(nodeLabel)) {
      nodeLabel = undefined;
    }

    // Suppress console / runner OCR noise
    if (isConsoleNoise(nodeLabel)) {
      for (const child of node.children ?? []) walk(child, depth + 1);
      return;
    }

    const isTaskbar = y1 >= vh - 60 || y2 >= vh - 60;
    const isSidebar = x1 < 85 && x2 <= 95 && !isTaskbar;

    let annotation = '';
    if (isTaskbar) {
      if (nodeLabel && /links/i.test(nodeLabel)) {
        annotation = ' [TASKBAR TOOLBAR]';
      } else if (node.type === 'ICON' && (nodeLabel === 'Search' || (x1 >= 50 && x2 <= 95))) {
        annotation = ' [TASKBAR SEARCH ICON - CLICK TO SEARCH]';
      } else if (node.type === 'ICON' && (nodeLabel === 'Start' || (x1 < 50 && w < 80))) {
        annotation = ' [TASKBAR START BUTTON]';
      } else if (nodeLabel) {
        annotation = ` [TASKBAR APP LAUNCHER: ${nodeLabel} - CLICK TO OPEN/FOCUS]`;
      } else {
        annotation = ' [TASKBAR ITEM]';
      }
    } else if (node.type === 'CONTAINER' || node.type === 'IMAGE') {
      if (w >= 200 && h >= 100) {
        annotation = ' [WORKSPACE CANVAS - CLICK TO FOCUS]';
      }
    }

    const isAnonymousContainer =
      node.type === 'CONTAINER' &&
      !nodeLabel &&
      node.value === undefined &&
      !node.interactable &&
      !node.focused &&
      !(w >= 200 && h >= 100);

    if (!isAnonymousContainer) {
      const labelStr = nodeLabel ? ` "${nodeLabel}"` : '';
      const val = node.value !== undefined ? ` (value: "${node.value}")` : '';
      const bounds = `[x:${x1}, y:${y1}, w:${w}, h:${h}] center=(${cx},${cy})`;
      const focus = node.focused ? ' [FOCUSED]' : '';
      const interact = node.interactable ? ' [CLICKABLE]' : '';
      const col = (node as any).color && (node as any).color !== 'gray' && (node as any).color !== 'light_gray'
        ? ` [color: ${(node as any).color} (${(node as any).color_hex ?? ''})]`
        : '';
      const src = (node as any).source ? ` [src: ${(node as any).source}]` : '';
      const cls = (node as any).class_name ? ` [class: ${(node as any).class_name}]` : '';
      const indent = '  '.repeat(Math.min(depth, 2));

      const line = `${indent}• [ID: #${node.id}] ${node.type}${labelStr}${val}${col}${interact}${focus}${src}${cls}${annotation} bounds=${bounds}`;

      if (isTaskbar) {
        // Deduplicate taskbar entries by cleaned label
        const taskbarKey = (nodeLabel ?? node.type).replace(/\s*-\s*\d+\s*running\s*windows?/i, '').trim();
        if (node.type === 'BUTTON' || !taskbarElementsMap.has(taskbarKey) || (node.interactable && w >= 15)) {
          taskbarElementsMap.set(taskbarKey, line);
        }
      } else if (node.type === 'INPUT_FIELD' || (node.label && /search|type|edit|input|text/i.test(node.label))) {
        if (inputElements.length < 10) inputElements.push(line);
      } else if (isSidebar) {
        if (sidebarElements.length < 10) sidebarElements.push(line);
      } else if (
        node.label ||
        node.value !== undefined ||
        node.interactable ||
        node.type === 'BUTTON' ||
        node.type === 'ICON' ||
        node.type === 'LIST' ||
        (w >= 200 && h >= 100)
      ) {
        if (mainElements.length < 30) mainElements.push(line);
      }
    }

    for (const child of node.children ?? []) {
      walk(child, depth + 1);
    }
  }

  walk(graph.root, 0);

  const wireframe = renderSpatialWireframe(graph, { cols: 66, rows: 12 });

  const sections: string[] = [
    `=== DESKTOP STATE (Frame #${graph.seq}) ===`,
    `Active Foreground Window: "${graph.active_window ?? 'Desktop'}"`,
    `Viewport: ${graph.viewport.width}x${graph.viewport.height}`,
    ``,
    `🗺️ 2D SPATIAL UI WIREFRAME (ASCII Zero-Raster Visual Map):`,
    wireframe,
    ``,
  ];

  if (mainElements.length > 0) {
    sections.push(`🖥️ WORKSPACE & INTERACTIVE CONTROLS:`);
    sections.push(mainElements.join('\n'));
    sections.push(``);
  }

  if (inputElements.length > 0) {
    sections.push(`⌨️ TEXT INPUTS & EDITORS:`);
    sections.push(inputElements.join('\n'));
    sections.push(``);
  }

  if (sidebarElements.length > 0) {
    sections.push(`🗂️ NAVIGATION & SIDEBAR:`);
    sections.push(sidebarElements.join('\n'));
    sections.push(``);
  }

  const taskbarList = Array.from(taskbarElementsMap.values());
  if (taskbarList.length > 0) {
    sections.push(`📱 TASKBAR & RUNNING APPLICATIONS:`);
    sections.push(taskbarList.join('\n'));
    sections.push(``);
  }

  return sections.join('\n');
}

/**
 * Format the user prompt combining the user's mission, current UI state, and action history.
 */
export function buildUserPrompt(
  mission: string,
  sceneGraph: SceneGraph,
  history: string[],
  retrievedAssets?: string[]
): string {
  const sections: string[] = [`MISSION: "${mission}"`];

  if (retrievedAssets && retrievedAssets.length > 0) {
    sections.push(``);
    sections.push(`📦 RETRIEVED ASSET INVENTORY (Assets Downloaded / Staged):`);
    for (const asset of retrievedAssets) {
      sections.push(`  • ${asset}`);
    }
    sections.push(`(Notice: Online search is complete. Proceed to editor / timeline / finish. DO NOT SEARCH AGAIN.)`);
  }

  sections.push(``);
  sections.push(formatSceneForLLM(sceneGraph));
  sections.push(``);
  sections.push(`=== RECENT ACTION HISTORY ===`);
  sections.push(history.length > 0 ? history.slice(-6).join('\n') : '(None yet - this is Step 1)');
  sections.push(``);
  sections.push(`What is your next action to progress the mission? Return JSON.`);

  return sections.join('\n');
}
