/**
 * 2D Spatial UI Wireframe Renderer for Zero-Raster Vision
 *
 * Renders an ASCII / Unicode 2D spatial map of the desktop and active application windows,
 * giving LLMs direct visual understanding of UI layout without transmitting raster image pixels.
 */

import type { SceneGraph, SceneNode, Bounds } from 'vact-sdk';

export interface WireframeOptions {
  cols?: number;
  rows?: number;
}

/**
 * Filter out OCR artifacts and console terminal self-referential text.
 */
export function isConsoleNoise(label?: string): boolean {
  if (!label) return false;
  return /npm\.cmd|vact-agent|tsx src|\[\s*STEP|GEMINI REASONING|Terminate batch job|cd apps|C:\\Users|SYSTEM CORE TELEMETRY|Direct3D11 Vector Engine|Sub-Pixel Bus|Vision Overhead|Action:|status:\s*SUCCESS|The system cannot find the path|logs\/mission_|dev --fast|Parallel Search returned|Active Model:|Session Timestamp|vact-protocol|vactd|C:\\Windows\\system32/i.test(
    label
  );
}

/**
 * Render a 2D text-based visual wireframe grid representing the desktop layout.
 */
export function renderSpatialWireframe(graph: SceneGraph, options: WireframeOptions = {}): string {
  const cols = options.cols ?? 70;
  const rows = options.rows ?? 14;

  const vw = Math.max(100, graph.viewport.width);
  const vh = Math.max(100, graph.viewport.height);

  // 2D Character Grid initialization
  const grid: string[][] = Array.from({ length: rows }, () => Array.from({ length: cols }, () => ' '));

  // Helper to map screen coordinates to grid coordinates
  function toGrid(x: number, y: number): [number, number] {
    const c = Math.max(0, Math.min(cols - 1, Math.floor((x / vw) * cols)));
    const r = Math.max(0, Math.min(rows - 1, Math.floor((y / vh) * rows)));
    return [c, r];
  }

  // Draw desktop outer border
  for (let c = 0; c < cols; c++) {
    grid[0][c] = c === 0 ? '┌' : c === cols - 1 ? '┐' : '─';
    grid[rows - 1][c] = c === 0 ? '└' : c === cols - 1 ? '┘' : '─';
  }
  for (let r = 1; r < rows - 1; r++) {
    grid[r][0] = '│';
    grid[r][cols - 1] = '│';
  }

  // Draw taskbar separator line (around y = vh - 60px)
  const taskbarRow = Math.max(rows - 3, Math.min(rows - 2, Math.floor(((vh - 60) / vh) * rows)));
  if (taskbarRow > 1 && taskbarRow < rows - 1) {
    grid[taskbarRow][0] = '├';
    grid[taskbarRow][cols - 1] = '┤';
    for (let c = 1; c < cols - 1; c++) {
      grid[taskbarRow][c] = '─';
    }
  }

  // Find significant visible interactive nodes to draw
  interface BoxToDraw {
    id: number;
    label: string;
    type: string;
    bounds: Bounds;
    c1: number;
    r1: number;
    c2: number;
    r2: number;
    isTaskbar: boolean;
  }

  const boxes: BoxToDraw[] = [];

  function collectNodes(node: SceneNode) {
    if (node.id === 0) {
      for (const child of node.children ?? []) collectNodes(child);
      return;
    }

    const [x1, y1, x2, y2] = node.bounds;
    const w = x2 - x1;
    const h = y2 - y1;

    // Filter tiny or full-screen root
    if (w < 12 || h < 10) return;
    if (w >= vw && h >= vh) return;

    if (isConsoleNoise(node.label)) return;

    const isTaskbar = y1 >= vh - 60 || y2 >= vh - 60;
    const [c1, r1] = toGrid(x1, y1);
    const [c2, r2] = toGrid(x2, y2);

    const hasLabel = !!node.label && node.label.trim().length > 0;
    const isSignificant =
      node.interactable ||
      node.type === 'BUTTON' ||
      node.type === 'INPUT_FIELD' ||
      isTaskbar ||
      (w >= 200 && h >= 100);

    if (isSignificant || hasLabel) {
      boxes.push({
        id: node.id,
        label: node.label ? node.label.trim() : node.type,
        type: node.type,
        bounds: node.bounds,
        c1,
        r1,
        c2,
        r2,
        isTaskbar,
      });
    }

    for (const child of node.children ?? []) {
      collectNodes(child);
    }
  }

  collectNodes(graph.root);

  // Sort: larger containers first so smaller buttons render on top
  boxes.sort((a, b) => {
    const areaA = (a.bounds[2] - a.bounds[0]) * (a.bounds[3] - a.bounds[1]);
    const areaB = (b.bounds[2] - b.bounds[0]) * (b.bounds[3] - b.bounds[1]);
    return areaB - areaA;
  });

  // Stamp boxes onto grid
  let taskbarColOffset = 1;
  for (const box of boxes.slice(0, 25)) {
    const { id, label, c1, r1, c2, r2, isTaskbar } = box;

    if (isTaskbar) {
      // In taskbar row, render compact badge
      const cleanLabel = label.replace(/\s*-\s*\d+\s*running\s*windows?/i, '');
      const badge = `[#${id}:${cleanLabel.slice(0, 10)}]`;
      if (taskbarColOffset + badge.length < cols - 1) {
        const targetRow = Math.min(rows - 2, taskbarRow);
        for (let i = 0; i < badge.length && taskbarColOffset + i < cols - 1; i++) {
          grid[targetRow][taskbarColOffset + i] = badge[i];
        }
        taskbarColOffset += badge.length + 1;
      }
    } else {
      // Workspace box
      const minR = Math.max(1, r1);
      const maxR = Math.min(taskbarRow - 1, r2);
      const minC = Math.max(1, c1);
      const maxC = Math.min(cols - 2, c2);

      if (maxC > minC + 3 && maxR >= minR) {
        // Draw top/bottom borders
        for (let c = minC; c <= maxC; c++) {
          if (grid[minR][c] === ' ') grid[minR][c] = '─';
          if (maxR > minR && grid[maxR][c] === ' ') grid[maxR][c] = '─';
        }
        for (let r = minR; r <= maxR; r++) {
          if (grid[r][minC] === ' ') grid[r][minC] = '│';
          if (grid[r][maxC] === ' ') grid[r][maxC] = '│';
        }
        grid[minR][minC] = '┌';
        grid[minR][maxC] = '┐';
        if (maxR > minR) {
          grid[maxR][minC] = '└';
          grid[maxR][maxC] = '┘';
        }

        // Put text label inside box
        const text = `#${id} ${label}`.slice(0, maxC - minC - 1);
        for (let i = 0; i < text.length && minC + 1 + i < maxC; i++) {
          grid[minR][minC + 1 + i] = text[i];
        }
      }
    }
  }

  return grid.map((row) => row.join('')).join('\n');
}
