/**
 * Unit Tests for MissionLogger
 */

import { describe, it, expect, afterEach } from 'vitest';
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { MissionLogger } from '../logger.js';

describe('MissionLogger', () => {
  const tempDir = path.join(os.tmpdir(), `vact-test-logs-${Date.now()}`);

  afterEach(() => {
    try {
      if (fs.existsSync(tempDir)) {
        fs.rmSync(tempDir, { recursive: true, force: true });
      }
    } catch {}
  });

  it('creates timestamped .md and .json log files', () => {
    const logger = new MissionLogger(tempDir);
    const { mdPath, jsonPath } = logger.startMission(
      'Search drone footage and align audio',
      'Google AI Studio',
      'gemini-2.5-flash',
      10
    );

    expect(fs.existsSync(mdPath)).toBe(true);
    expect(fs.existsSync(jsonPath)).toBe(true);

    const initialMd = fs.readFileSync(mdPath, 'utf-8');
    expect(initialMd).toContain('VACT Agent Telemetry Log');
    expect(initialMd).toContain('Search drone footage and align audio');
    expect(initialMd).toContain('gemini-2.5-flash');
  });

  it('records step telemetry with reasoning, tokens, duration and action details', () => {
    const logger = new MissionLogger(tempDir);
    const { mdPath, jsonPath } = logger.startMission(
      'Search drone footage and align audio',
      'Google AI Studio',
      'gemini-2.5-flash',
      10
    );

    logger.logStep({
      step: 1,
      durationMs: 1420,
      tokensUsed: 312,
      thought: 'Executing built-in Parallel Search for 4K drone footage',
      action: { action: 'SEARCH', query: 'cinematic 4K drone footage 60fps' },
      activeWindow: 'DaVinci Resolve Studio',
      sceneSummary: 'Active Window: DaVinci Resolve Studio\nWorkspace Controls: ...',
      route: 'DIRECT',
      success: true,
      error: 'Parallel Search returned 5 assets',
    });

    const mdContent = fs.readFileSync(mdPath, 'utf-8');
    expect(mdContent).toContain('Step 01 — ✅ SUCCESS (⏱️ 1420ms | 🧠 312 tokens)');
    expect(mdContent).toContain('`SEARCH`');
    expect(mdContent).toContain('Executing built-in Parallel Search for 4K drone footage');
    expect(mdContent).toContain('DaVinci Resolve Studio');

    const jsonContent = JSON.parse(fs.readFileSync(jsonPath, 'utf-8'));
    expect(jsonContent.steps).toHaveLength(1);
    expect(jsonContent.steps[0].step).toBe(1);
    expect(jsonContent.steps[0].tokensUsed).toBe(312);
    expect(jsonContent.steps[0].cumulativeTokens).toBe(312);
  });

  it('finalizes mission execution with summary table and completion metrics', () => {
    const logger = new MissionLogger(tempDir);
    const { mdPath, jsonPath } = logger.startMission(
      'Search drone footage and align audio',
      'Google AI Studio',
      'gemini-2.5-flash',
      10
    );

    logger.logStep({
      step: 1,
      durationMs: 1200,
      tokensUsed: 250,
      thought: 'Finished work',
      action: { action: 'FINISH', message: 'All media imported and aligned' },
      route: 'DIRECT',
      success: true,
    });

    logger.finishMission({
      completed: true,
      finalMessage: 'All media imported and aligned',
      totalDurationMs: 1200,
      totalTokensUsed: 250,
    });

    const mdContent = fs.readFileSync(mdPath, 'utf-8');
    expect(mdContent).toContain('Mission Execution Summary');
    expect(mdContent).toContain('🏆 **COMPLETED**');
    expect(mdContent).toContain('All media imported and aligned');

    const jsonContent = JSON.parse(fs.readFileSync(jsonPath, 'utf-8'));
    expect(jsonContent.completed).toBe(true);
    expect(jsonContent.totalTokensUsed).toBe(250);
  });
});
