/**
 * VACT Agent Comprehensive Telemetry & Mission Logger
 * 
 * Automatically logs every session, step, thought, token count, scene graph,
 * and execution latency into timestamped .json and .md files in the /logs directory.
 */

import * as fs from 'fs';
import * as path from 'path';
import type { AgentActionChoice, AgentMissionResult, StepTrace } from './types.js';

export interface StepLogEntry {
  step: number;
  timestamp: string;
  durationMs: number;
  tokensUsed: number;
  cumulativeTokens: number;
  thought?: string;
  action: AgentActionChoice;
  targetNodeInfo?: string;
  activeWindow?: string;
  sceneSummary?: string;
  promptSent?: string;
  rawResponse?: string;
  route?: string;
  success: boolean;
  error?: string;
}

export interface MissionLogData {
  sessionTimestamp: string;
  mission: string;
  provider: string;
  model: string;
  maxSteps: number;
  startTime: string;
  endTime?: string;
  totalDurationMs?: number;
  totalTokensUsed: number;
  completed: boolean;
  finalMessage?: string;
  steps: StepLogEntry[];
}

export class MissionLogger {
  private _logsDir: string;
  private _jsonPath: string = '';
  private _mdPath: string = '';
  private _data!: MissionLogData;
  private _startTimeMs: number = 0;
  private _cumulativeTokens: number = 0;

  public constructor(customLogsDir?: string) {
    this._logsDir = customLogsDir ?? path.resolve(process.cwd(), 'logs');
    this._ensureDirectory();
  }

  private _ensureDirectory(): void {
    if (!fs.existsSync(this._logsDir)) {
      fs.mkdirSync(this._logsDir, { recursive: true });
    }
  }

  public get mdPath(): string {
    return this._mdPath;
  }

  public get jsonPath(): string {
    return this._jsonPath;
  }

  public startMission(mission: string, provider: string, model: string, maxSteps: number): { mdPath: string; jsonPath: string } {
    this._ensureDirectory();
    this._startTimeMs = Date.now();
    this._cumulativeTokens = 0;

    const now = new Date();
    const pad = (n: number) => String(n).padStart(2, '0');
    const timestampStr = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}_${pad(now.getHours())}-${pad(now.getMinutes())}-${pad(now.getSeconds())}`;

    this._jsonPath = path.join(this._logsDir, `mission_${timestampStr}.json`);
    this._mdPath = path.join(this._logsDir, `mission_${timestampStr}.md`);

    this._data = {
      sessionTimestamp: timestampStr,
      mission,
      provider,
      model,
      maxSteps,
      startTime: now.toISOString(),
      totalTokensUsed: 0,
      completed: false,
      steps: [],
    };

    // Write initial JSON
    fs.writeFileSync(this._jsonPath, JSON.stringify(this._data, null, 2), 'utf-8');

    // Write initial Markdown Header
    const initialMd = [
      `# 🎬 VACT Agent Telemetry Log`,
      ``,
      `**Session Timestamp:** \`${timestampStr}\`  `,
      `**Started At:** \`${now.toLocaleString()}\`  `,
      `**Mission Directive:** \`${mission}\`  `,
      `**AI Engine / Provider:** \`${provider}\`  `,
      `**Active Model:** \`${model}\`  `,
      `**Max Allocated Steps:** \`${maxSteps}\`  `,
      `**Sub-Pixel Bus:** \`VACT Direct3D11 60 FPS GPU Vector DAG\`  `,
      `**Vision Token Overhead:** \`$0.00 (Zero raster frame tokens — 100% Vector AST)\`  `,
      ``,
      `---`,
      ``,
      `## 📊 Per-Step Perception & Execution Trace`,
      ``,
    ].join('\n');

    fs.writeFileSync(this._mdPath, initialMd, 'utf-8');

    return { mdPath: this._mdPath, jsonPath: this._jsonPath };
  }

  public logStep(entry: {
    step: number;
    durationMs: number;
    tokensUsed: number;
    thought?: string;
    action: AgentActionChoice;
    targetNodeInfo?: string;
    activeWindow?: string;
    sceneSummary?: string;
    promptSent?: string;
    rawResponse?: string;
    route?: string;
    success: boolean;
    error?: string;
  }): void {
    this._cumulativeTokens += entry.tokensUsed;
    const now = new Date();

    const fullEntry: StepLogEntry = {
      step: entry.step,
      timestamp: now.toISOString(),
      durationMs: entry.durationMs,
      tokensUsed: entry.tokensUsed,
      cumulativeTokens: this._cumulativeTokens,
      thought: entry.thought,
      action: entry.action,
      targetNodeInfo: entry.targetNodeInfo,
      activeWindow: entry.activeWindow,
      sceneSummary: entry.sceneSummary,
      promptSent: entry.promptSent,
      rawResponse: entry.rawResponse,
      route: entry.route,
      success: entry.success,
      error: entry.error,
    };

    this._data.steps.push(fullEntry);
    this._data.totalTokensUsed = this._cumulativeTokens;

    // Flush JSON
    try {
      fs.writeFileSync(this._jsonPath, JSON.stringify(this._data, null, 2), 'utf-8');
    } catch {}

    // Append to Markdown
    const stepNumPad = String(entry.step).padStart(2, '0');
    const statusIcon = entry.success ? '✅ SUCCESS' : '❌ FAILED';
    const actionStr = `\`${entry.action.action}\` ${entry.action.target_id !== undefined ? `(Node #${entry.action.target_id})` : ''} ${entry.action.text ? `-> "${entry.action.text}"` : ''} ${entry.action.query ? `-> query: "${entry.action.query}"` : ''} ${entry.action.vk ? `-> VK 0x${entry.action.vk.toString(16).toUpperCase()}` : ''}`;

    const mdStepBlock = [
      `### Step ${stepNumPad} — ${statusIcon} (⏱️ ${entry.durationMs}ms | 🧠 ${entry.tokensUsed} tokens)`,
      ``,
      `- **Active Window:** \`${entry.activeWindow ?? 'Unknown'}\``,
      `- **Dispatched Action:** ${actionStr}`,
      `- **Hardware Bus:** \`${entry.route ?? 'Direct'}\``,
      entry.targetNodeInfo ? `- **Target Node Info:** \`${entry.targetNodeInfo}\`` : null,
      entry.error ? `- **Notice / Error:** \`${entry.error}\`` : null,
      ``,
      `#### 🧠 Gemini Reasoning / Thought:`,
      `> ${entry.thought ? entry.thought.replace(/\n/g, '\n> ') : '*(No explicit thought returned)*'}`,
      ``,
      entry.sceneSummary ? `<details><summary><b>🖥️ Desktop Scene State Snapshot (Step ${stepNumPad})</b></summary>\n\n\`\`\`\n${entry.sceneSummary}\n\`\`\`\n</details>\n` : null,
      entry.rawResponse ? `<details><summary><b>🤖 Raw Model Output & Action Payload</b></summary>\n\n\`\`\`json\n${entry.rawResponse}\n\`\`\`\n</details>\n` : null,
      `---`,
      ``,
    ].filter((line) => line !== null).join('\n');

    try {
      fs.appendFileSync(this._mdPath, mdStepBlock, 'utf-8');
    } catch {}
  }

  public finishMission(result: {
    completed: boolean;
    finalMessage?: string;
    totalDurationMs: number;
    totalTokensUsed: number;
  }): void {
    const now = new Date();
    this._data.endTime = now.toISOString();
    this._data.completed = result.completed;
    this._data.finalMessage = result.finalMessage;
    this._data.totalDurationMs = result.totalDurationMs;
    this._data.totalTokensUsed = result.totalTokensUsed;

    // Final JSON flush
    try {
      fs.writeFileSync(this._jsonPath, JSON.stringify(this._data, null, 2), 'utf-8');
    } catch {}

    // Append Markdown Summary
    const durationSec = (result.totalDurationMs / 1000).toFixed(2);
    const avgStepMs = (result.totalDurationMs / Math.max(1, this._data.steps.length)).toFixed(0);

    const summaryBlock = [
      `## 🏁 Mission Execution Summary`,
      ``,
      `| Metric | Value |`,
      `| :--- | :--- |`,
      `| **Mission Status** | ${result.completed ? '🏆 **COMPLETED**' : '⚠️ **STOPPED / MAX STEPS REACHED**'} |`,
      `| **Total Duration** | \`${durationSec}s\` (\`${result.totalDurationMs}ms\`) |`,
      `| **Total Steps Executed** | \`${this._data.steps.length} / ${this._data.maxSteps}\` |`,
      `| **Average Step Latency** | \`${avgStepMs}ms\` |`,
      `| **Total Gemini Tokens** | \`${result.totalTokensUsed}\` |`,
      `| **Vision Token Overhead** | \`$0.00 (Zero Screenshot Tokens)\` |`,
      result.finalMessage ? `| **Final Outcome Message** | *${result.finalMessage}* |` : ``,
      ``,
      `---`,
      `*Generated automatically by VACT Protocol Telemetry Core.*`,
      ``,
    ].filter(Boolean).join('\n');

    try {
      fs.appendFileSync(this._mdPath, summaryBlock, 'utf-8');
    } catch {}
  }
}
