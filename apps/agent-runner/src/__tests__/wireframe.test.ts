import { describe, it, expect } from 'vitest';
import type { SceneGraph } from 'vact-sdk';
import { renderSpatialWireframe, isConsoleNoise } from '../wireframe.js';

describe('2D Spatial UI Wireframe Renderer', () => {
  const mockScene: SceneGraph = {
    seq: 100,
    timestamp_us: 1000,
    viewport: { width: 1600, height: 900, scale_factor: 1.0 },
    active_window: 'DaVinci Resolve',
    root: {
      id: 0,
      type: 'CONTAINER',
      bounds: [0, 0, 1600, 900],
      children: [
        {
          id: 10,
          type: 'CONTAINER',
          label: 'Timeline Canvas',
          bounds: [100, 200, 1500, 700],
          interactable: true,
          children: [
            {
              id: 20,
              type: 'BUTTON',
              label: 'Import Media',
              bounds: [150, 250, 300, 300],
              interactable: true,
            },
          ],
        },
        {
          id: 36,
          type: 'BUTTON',
          label: 'Search',
          bounds: [59, 867, 85, 893],
          interactable: true,
        },
        {
          id: 57,
          type: 'BUTTON',
          label: 'Chrome',
          bounds: [742, 866, 770, 894],
          interactable: true,
        },
      ],
    },
  };

  it('renders 2D wireframe representation without errors', () => {
    const wireframe = renderSpatialWireframe(mockScene, { cols: 60, rows: 12 });
    expect(wireframe).toContain('┌');
    expect(wireframe).toContain('┘');
    expect(wireframe).toContain('#36:Search');
    expect(wireframe).toContain('#57:Chrome');
  });

  it('filters console noise correctly', () => {
    expect(isConsoleNoise('npm.cmd run dev --fast')).toBe(true);
    expect(isConsoleNoise('[STEP 01/20] GEMINI REASONING')).toBe(true);
    expect(isConsoleNoise('DaVinci Resolve - Edit')).toBe(false);
    expect(isConsoleNoise('Import Media')).toBe(false);
  });
});
