/**
 * Common Types for VACT Agent Runner
 */

export type ProviderName = 'gemini' | 'claude' | 'openai' | 'deepseek' | 'ollama';

export interface AgentActionChoice {
  /** The action to execute: CLICK | TYPE | SCROLL | KEY | FOCUS | LEARN | SEARCH | FINISH | WAIT */
  action: 'CLICK' | 'TYPE' | 'SCROLL' | 'KEY' | 'FOCUS' | 'LEARN' | 'SEARCH' | 'FINISH' | 'WAIT';
  /** Target Node ID from the scene graph */
  target_id?: number;
  /** Text payload for TYPE */
  text?: string;
  /** Search query for SEARCH (Parallel Partner API) */
  query?: string;
  /** Scroll delta for SCROLL */
  delta_y?: number;
  /** Virtual key code for KEY */
  vk?: number;
  /** Semantic label for LEARN */
  label?: string;
  /** Action result outcome for LEARN */
  action_result?: string;
  /** Final summary message when action is FINISH */
  message?: string;
  /** Reasoning explanation from LLM */
  thought?: string;
}

export interface StepTrace {
  step: number;
  thought?: string;
  action: AgentActionChoice;
  durationMs: number;
  nodeInfo?: string;
  route?: 'DIRECT' | 'GHOST';
  success: boolean;
  error?: string;
}

export interface AgentMissionResult {
  mission: string;
  provider: string;
  model: string;
  steps: StepTrace[];
  completed: boolean;
  totalDurationMs: number;
  totalTokensUsed: number;
  finalMessage?: string;
  logPath?: string;
}
