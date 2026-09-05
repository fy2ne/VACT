/**
 * Autonomous VACT Agent Execution Engine
 */

import * as path from 'path';
import { VactClient, type SceneNode } from 'vact-sdk';
import type { ILLMProvider } from './providers/base.js';
import type { AgentMissionResult, StepTrace, AgentActionChoice } from './types.js';
import { SYSTEM_PROMPT, buildUserPrompt, formatSceneForLLM } from './prompt.js';
import { ParallelClient } from './tools/parallel.js';
import { MissionLogger } from './logger.js';

export interface AgentRunnerOptions {
  provider: ILLMProvider;
  mission: string;
  maxSteps?: number;
  onStep?: (trace: StepTrace) => void;
  vactClient?: VactClient;
  logsDir?: string;
}

export class AgentRunner {
  private readonly _provider: ILLMProvider;
  private readonly _mission: string;
  private readonly _maxSteps: number;
  private readonly _onStep?: (trace: StepTrace) => void;
  private readonly _parallel = new ParallelClient();
  private readonly _logger: MissionLogger;
  private _client: VactClient;

  public constructor(options: AgentRunnerOptions) {
    this._provider = options.provider;
    this._mission = options.mission;
    this._maxSteps = options.maxSteps ?? 15;
    this._onStep = options.onStep;
    this._logger = new MissionLogger(options.logsDir);
    this._client = options.vactClient ?? new VactClient({ handshakeTimeoutMs: 15000, actionTimeoutMs: 10000 });
  }

  public get logger(): MissionLogger {
    return this._logger;
  }

  public async run(): Promise<AgentMissionResult> {
    const startTime = Date.now();
    const steps: StepTrace[] = [];
    const history: string[] = [];
    const retrievedAssets: string[] = [];
    let totalTokens = 0;
    let completed = false;
    let finalMessage: string | undefined;

    // 0. Initialize telemetry session log file in /logs
    const logInfo = this._logger.startMission(this._mission, this._provider.name, this._provider.model, this._maxSteps);
    const relLogPath = path.relative(process.cwd(), logInfo.mdPath).replace(/\\/g, '/');
    console.log(`\x1b[96m📁 [MISSION TELEMETRY LOG]:\x1b[0m \x1b[1m\x1b[93m${relLogPath}\x1b[0m`);

    // 1. Connect to VACT daemon with auto-retry
    let connected = false;
    let lastError: Error | null = null;
    const maxConnectAttempts = 10;
    for (let attempt = 1; attempt <= maxConnectAttempts; attempt++) {
      try {
        await this._client.connect();
        connected = true;
        break;
      } catch (err) {
        lastError = err as Error;
        if (attempt < maxConnectAttempts) {
          process.stdout.write(`\r⏳ Connecting to vactd daemon (attempt ${attempt}/${maxConnectAttempts})...`);
          await new Promise((r) => setTimeout(r, 1000));
        }
      }
    }

    if (!connected) {
      const connErr = `[DAEMON CONNECTION ERROR] Could not connect to vactd: ${lastError?.message}. Ensure "cargo run -p vactd -- start" is running in Terminal 1.`;
      this._logger.finishMission({
        completed: false,
        finalMessage: connErr,
        totalDurationMs: Date.now() - startTime,
        totalTokensUsed: 0,
      });
      throw new Error(connErr);
    }
    process.stdout.write(`\r✅ Connected to VACT Direct3D11 Vector Engine successfully!          \n`);

    // 2. Wait for initial scene graph snapshot to hydrate
    let sceneGraph;
    try {
      sceneGraph = await this._client.waitForHydration(12000);
    } catch (err) {
      const hydErr = `[SDK HYDRATION ERROR] Daemon connected, but did not receive initial scene snapshot: ${(err as Error).message}`;
      this._logger.finishMission({
        completed: false,
        finalMessage: hydErr,
        totalDurationMs: Date.now() - startTime,
        totalTokensUsed: 0,
      });
      throw new Error(hydErr);
    }

    // 3. Autonomous Execution Loop
    for (let stepNum = 1; stepNum <= this._maxSteps; stepNum++) {
      const stepStart = Date.now();
      
      // Refresh current live scene graph
      const currentGraph = this._client.getState();
      const userPrompt = buildUserPrompt(this._mission, currentGraph, history, retrievedAssets);
      const sceneSummary = formatSceneForLLM(currentGraph);

      // Ask LLM for next action
      process.stdout.write(`\n⏳ [Step ${stepNum}/${this._maxSteps}] Waiting for ${this._provider.name.toUpperCase()} (${this._provider.model}) reasoning...\r`);
      let llmRes;
      try {
        llmRes = await this._provider.decideAction(SYSTEM_PROMPT, userPrompt);
      } catch (err) {
        // Log warning and retry once with a simple wait
        console.warn(`\n⚠️ Provider reasoning warning: ${(err as Error).message}. Retrying step with auto-healing...`);
        try {
          await new Promise((r) => setTimeout(r, 1500));
          llmRes = await this._provider.decideAction(SYSTEM_PROMPT, userPrompt);
        } catch (retryErr) {
          const failMsg = `[LLM PROVIDER ERROR] (${this._provider.name}): ${(retryErr as Error).message}`;
          this._logger.finishMission({
            completed: false,
            finalMessage: failMsg,
            totalDurationMs: Date.now() - startTime,
            totalTokensUsed: totalTokens,
          });
          throw new Error(failMsg);
        }
      }
      process.stdout.write(' '.repeat(85) + '\r');
      totalTokens += llmRes.tokensUsed;

      const choice: AgentActionChoice = llmRes.action;
      const stepDuration = Date.now() - stepStart;

      // Handle FINISH action
      if (choice.action === 'FINISH') {
        completed = true;
        finalMessage = choice.message ?? 'Mission marked finished by model';
        const trace: StepTrace = {
          step: stepNum,
          thought: choice.thought,
          action: choice,
          durationMs: stepDuration,
          success: true,
        };
        steps.push(trace);
        this._logger.logStep({
          step: stepNum,
          durationMs: stepDuration,
          tokensUsed: llmRes.tokensUsed,
          thought: choice.thought,
          action: choice,
          activeWindow: currentGraph.active_window,
          sceneSummary,
          promptSent: userPrompt,
          rawResponse: llmRes.rawText,
          route: 'DIRECT',
          success: true,
        });
        this._onStep?.(trace);
        break;
      }

      // Handle WAIT action
      if (choice.action === 'WAIT') {
        await new Promise((r) => setTimeout(r, 600));
        const trace: StepTrace = {
          step: stepNum,
          thought: choice.thought,
          action: choice,
          durationMs: stepDuration,
          success: true,
        };
        steps.push(trace);
        history.push(`Step ${stepNum}: Waited 600ms for UI settle`);
        this._logger.logStep({
          step: stepNum,
          durationMs: stepDuration,
          tokensUsed: llmRes.tokensUsed,
          thought: choice.thought,
          action: choice,
          activeWindow: currentGraph.active_window,
          sceneSummary,
          promptSent: userPrompt,
          rawResponse: llmRes.rawText,
          route: 'DIRECT',
          success: true,
        });
        this._onStep?.(trace);
        continue;
      }

      // Dispatch physical hardware action or built-in tool via vact-sdk
      let route: 'DIRECT' | 'GHOST' | undefined;
      let success = false;
      let errorMsg: string | undefined;

      const targetId = choice.target_id ?? 0;
      const targetNode = choice.target_id !== undefined ? findNode(currentGraph.root, choice.target_id) : undefined;
      const vh = currentGraph.viewport.height;
      const isTaskbarNode = !!(targetNode && (targetNode.bounds[1] >= vh - 60 || targetNode.bounds[3] >= vh - 60));

      const isWinKey = choice.action === 'KEY' && (choice.vk === 0x5b || choice.vk === 91);
      const lastWasWinKey = history.length > 0 && history[history.length - 1].includes('KEY') && (history[history.length - 1].includes('5b') || history[history.length - 1].includes('91'));

      const isTaskbarClick = choice.action === 'CLICK' && isTaskbarNode;
      const lastWasSameTaskbarClick = history.length > 0 && history[history.length - 1].includes(`CLICK on Node #${targetId}`);

      try {
        if (isWinKey && lastWasWinKey) {
          // Skip redundant Win key toggle (which would close the search menu)
          route = 'DIRECT';
          success = true;
          errorMsg = 'Debounced consecutive Windows key toggle to keep search menu open';
        } else if (isTaskbarClick && lastWasSameTaskbarClick) {
          // Debounce rapid duplicate taskbar click which would minimize the newly opening window
          route = 'DIRECT';
          success = true;
          errorMsg = 'Debounced consecutive taskbar click to prevent minimizing opening window';
        } else {
          switch (choice.action) {
            case 'CLICK': {
              const res = await this._client.agent().click(targetId);
              route = res.route;
              success = res.ok;
              errorMsg = res.error;
              break;
            }
            case 'TYPE': {
              const res = await this._client.agent().type(targetId, choice.text ?? '');
              route = res.route;
              success = res.ok;
              errorMsg = res.error;
              break;
            }
            case 'SCROLL': {
              const res = await this._client.agent().scroll(targetId, choice.delta_y ?? -120);
              route = res.route;
              success = res.ok;
              errorMsg = res.error;
              break;
            }
            case 'KEY': {
              const res = await this._client.agent().key(targetId, choice.vk ?? 13);
              route = res.route;
              success = res.ok;
              errorMsg = res.error;
              break;
            }
            case 'FOCUS': {
              const res = await this._client.agent().focus(targetId);
              route = res.route;
              success = res.ok;
              errorMsg = res.error;
              break;
            }
            case 'LEARN': {
              const label = choice.label ?? 'Custom Control';
              const res = await this._client.agent().learn(targetId, label, choice.action_result);
              route = res.route;
              success = res.ok;
              errorMsg = res.error;
              break;
            }
            case 'SEARCH': {
              const query = choice.query ?? choice.text ?? choice.thought ?? 'cinematic 4K drone footage 60fps';
              const searchRes = await this._parallel.search(query);
              route = 'DIRECT';
              success = true;
              for (const item of searchRes.results) {
                const summary = `[${item.title}] ${item.snippet}`;
                if (!retrievedAssets.includes(summary)) {
                  retrievedAssets.push(summary);
                }
              }
              const resultSnippets = searchRes.results.map((r) => `[${r.title}] ${r.snippet}`).join(' | ');
              errorMsg = `Parallel Search returned ${searchRes.results.length} asset(s): ${resultSnippets.slice(0, 150)}...`;
              break;
            }
          }
        }
      } catch (err) {
        success = false;
        errorMsg = err instanceof Error ? err.message : String(err);
      }

      const nodeInfo = targetNode
        ? `${targetNode.type}${targetNode.label ? ' "' + targetNode.label + '"' : ''} at [${targetNode.bounds.join(',')}]`
        : undefined;

      const trace: StepTrace = {
        step: stepNum,
        thought: choice.thought,
        action: choice,
        durationMs: stepDuration,
        nodeInfo,
        route,
        success,
        error: errorMsg,
      };

      steps.push(trace);

      // Record step in comprehensive mission logger
      this._logger.logStep({
        step: stepNum,
        durationMs: stepDuration,
        tokensUsed: llmRes.tokensUsed,
        thought: choice.thought,
        action: choice,
        targetNodeInfo: nodeInfo,
        activeWindow: currentGraph.active_window,
        sceneSummary,
        promptSent: userPrompt,
        rawResponse: llmRes.rawText,
        route,
        success,
        error: errorMsg,
      });

      this._onStep?.(trace);

      if (choice.action === 'SEARCH') {
        history.push(
          `Step ${stepNum}: Executed built-in SEARCH for "${choice.query ?? ''}" -> RETRIEVED ${retrievedAssets.length} ASSET(S). (Online search complete! Next step: Focus video editor or click Taskbar Search to launch editor, import clips, and align audio)`
        );
      } else {
        history.push(
          `Step ${stepNum}: Executed ${choice.action} on Node #${choice.target_id ?? 0} (${nodeInfo ?? ''}) -> ${success ? 'SUCCESS' : 'FAILED: ' + errorMsg}`
        );
      }

      // Adaptive Pacing: 1.8s inter-step breathing window so requests stay comfortably under rate limits
      let waitMs = 1800;
      if (choice.action === 'KEY' && (choice.vk === 13 || choice.vk === 0x0d || choice.vk === 0x5b)) {
        waitMs = 2000;
      } else if (isTaskbarClick) {
        waitMs = 2200;
      } else if (choice.action === 'SEARCH') {
        waitMs = 1500;
      }
      await new Promise((r) => setTimeout(r, waitMs));
    }

    const totalDurationMs = Date.now() - startTime;

    this._logger.finishMission({
      completed,
      finalMessage,
      totalDurationMs,
      totalTokensUsed: totalTokens,
    });

    this._client.close();

    return {
      mission: this._mission,
      provider: this._provider.name,
      model: this._provider.model,
      steps,
      completed,
      totalDurationMs,
      totalTokensUsed: totalTokens,
      finalMessage,
      logPath: logInfo.mdPath,
    };
  }
}

function findNode(node: SceneNode, id: number): SceneNode | undefined {
  if (node.id === id) return node;
  for (const child of node.children ?? []) {
    const found = findNode(child, id);
    if (found) return found;
  }
  return undefined;
}
