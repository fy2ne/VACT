/**
 * Base LLM Provider Interface
 */

import type { AgentActionChoice } from '../types.js';

export interface LLMResponse {
  action: AgentActionChoice;
  rawText: string;
  tokensUsed: number;
}

export interface ILLMProvider {
  readonly name: string;
  readonly model: string;

  /**
   * Decide the next action given the current mission prompt, scene graph representation, and history.
   */
  decideAction(systemPrompt: string, userPrompt: string): Promise<LLMResponse>;
}
