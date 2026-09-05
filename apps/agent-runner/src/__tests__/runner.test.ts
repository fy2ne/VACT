/**
 * Unit Tests for AgentRunner & Google Provider Factory
 */

import { describe, it, expect } from 'vitest';
import { formatSceneForLLM, buildUserPrompt } from '../prompt.js';
import { getProvider } from '../providers/index.js';
import type { SceneGraph } from 'vact-sdk';

const mockSceneGraph: SceneGraph = {
  protocol: 'VACT/1.0',
  seq: 1042,
  timestamp_us: 1000000,
  active_window: 'Blender 4.2',
  viewport: { width: 1920, height: 1080, scale_factor: 1.0 },
  root: {
    id: 1,
    type: 'CONTAINER',
    bounds: [0, 0, 1920, 1080],
    children: [
      {
        id: 14,
        type: 'INPUT_FIELD',
        bounds: [340, 980, 1240, 1030],
        label: 'Camera Focal Length',
        value: '50mm',
        interactable: true,
        focused: true,
      },
      {
        id: 18,
        type: 'BUTTON',
        bounds: [1600, 980, 1680, 1030],
        label: 'Render Frame',
        interactable: true,
        disabled: false,
      },
    ],
  },
};

describe('Prompt & Scene Serialization', () => {
  it('formats scene graph into compact text representation', () => {
    const formatted = formatSceneForLLM(mockSceneGraph);
    expect(formatted).toContain('Active Foreground Window: "Blender 4.2"');
    expect(formatted).toContain('[ID: #14] INPUT_FIELD "Camera Focal Length"');
    expect(formatted).toContain('[ID: #18] BUTTON "Render Frame"');
    expect(formatted).toContain('[FOCUSED]');
  });

  it('correctly formats bounds as [x1, y1, w, h] and annotates taskbar apps', () => {
    const taskbarScene: SceneGraph = {
      protocol: 'VACT/1.0',
      seq: 200,
      timestamp_us: 2000000,
      active_window: 'Desktop',
      viewport: { width: 1920, height: 1080, scale_factor: 1.0 },
      root: {
        id: 1,
        type: 'CONTAINER',
        bounds: [0, 0, 1920, 1080],
        children: [
          {
            id: 2,
            type: 'ICON',
            bounds: [100, 1040, 140, 1076],
            label: 'DaVinci Resolve',
            interactable: true,
          },
          {
            id: 3,
            type: 'ICON',
            bounds: [50, 1040, 90, 1076],
            label: 'Search',
            interactable: true,
          },
        ],
      },
    };

    const formatted = formatSceneForLLM(taskbarScene);
    // Bounds should be w: 40 (140 - 100), h: 36 (1076 - 1040)
    expect(formatted).toContain('bounds=[x:100, y:1040, w:40, h:36]');
    expect(formatted).toContain('[TASKBAR APP LAUNCHER: DaVinci Resolve - CLICK TO OPEN/FOCUS]');
    expect(formatted).toContain('[TASKBAR SEARCH ICON - CLICK TO SEARCH]');
  });

  it('builds complete user prompt with history and retrieved assets', () => {
    const prompt = buildUserPrompt(
      'Search drone footage and import',
      mockSceneGraph,
      ['Step 1: Found assets'],
      ['[Asset 1] 4K drone footage']
    );
    expect(prompt).toContain('MISSION: "Search drone footage and import"');
    expect(prompt).toContain('📦 RETRIEVED ASSET INVENTORY');
    expect(prompt).toContain('[Asset 1] 4K drone footage');
    expect(prompt).toContain('Step 1: Found assets');
  });
});

describe('Google Provider Factory', () => {
  it('instantiates Gemini provider (Google AI Studio) with default model', () => {
    const p = getProvider('ai-studio', 'gemini-2.5-flash', 'dummy_key');
    expect(p.name).toContain('Google AI Studio');
    expect(p.model).toBe('gemini-2.5-flash');
  });

  it('instantiates Google Cloud Vertex AI provider with project and region', () => {
    const p = getProvider('vertex', 'gemini-2.5-pro', undefined, 'test-gcp-project', 'us-central1');
    expect(p.name).toBe('Google Cloud Vertex AI');
    expect(p.model).toBe('gemini-2.5-pro');
    expect((p as any).projectId).toBe('test-gcp-project');
    expect((p as any).location).toBe('us-central1');
  });

  it('throws error for non-Google providers (claude, openai, ollama)', () => {
    expect(() => getProvider('claude' as any)).toThrow(/exclusively built on Google Cloud AI/);
    expect(() => getProvider('openai' as any)).toThrow(/exclusively built on Google Cloud AI/);
    expect(() => getProvider('ollama' as any)).toThrow(/exclusively built on Google Cloud AI/);
  });
});
