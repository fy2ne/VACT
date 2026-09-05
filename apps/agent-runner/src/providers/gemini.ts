/**
 * Google AI Studio Gemini Provider Adapter
 * Powered 100% by Google Gemini models (Gemini 2.5 Flash / Pro / 2.0 Flash / 1.5 Flash).
 */

import type { ILLMProvider, LLMResponse } from './base.js';
import type { AgentActionChoice } from '../types.js';

export function normalizeGeminiModel(model: string): string {
  const m = model.toLowerCase().trim();
  if (m === 'gemini-2.0-flash' || m === 'gemini-2.0-flash-001' || m === '2.0-flash') return 'gemini-2.5-flash';
  if (m === 'gemini-2.0-flash-lite' || m === '2.0-flash-lite') return 'gemini-2.5-flash';
  if (m === 'gemini-2.0-pro-exp' || m === 'gemini-2.0-pro-exp-02-05' || m === '2.0-pro') return 'gemini-2.5-pro';
  if (m === 'gemini-3.7' || m === '3.7-flash') return 'gemini-3.7-flash';
  if (m === 'gemini-3.7-thinking') return 'gemini-3.7-flash';
  if (m === 'gemini-2.5' || m === '2.5-flash') return 'gemini-2.5-flash';
  return model;
}

export class GeminiProvider implements ILLMProvider {
  public readonly name = 'Google AI Studio (Gemini)';
  public model: string;
  private readonly _apiKey: string;
  private readonly _fallbackModels: string[];

  public constructor(
    model = process.env['GEMINI_MODEL'] ?? process.env['DEFAULT_MODEL'] ?? 'gemini-2.5-flash',
    apiKey?: string
  ) {
    this.model = normalizeGeminiModel(model);
    this._apiKey = apiKey ?? process.env['GEMINI_API_KEY'] ?? process.env['VERTEX_API_KEY'] ?? process.env['GOOGLE_API_KEY'] ?? '';
    if (!this._apiKey) {
      throw new Error('GEMINI_API_KEY (or GOOGLE_API_KEY) environment variable is missing or empty.');
    }

    // Dynamic fallback chain if the primary model hits free-tier rate limits, 404 deprecation, or transient outages
    const defaultFallbacks = ['gemini-2.5-flash', 'gemini-3.7-flash', 'gemini-1.5-flash', 'gemini-1.5-pro'];
    this._fallbackModels = [this.model, ...defaultFallbacks.filter((m) => m !== this.model)];
  }

  public async decideAction(systemPrompt: string, userPrompt: string): Promise<LLMResponse> {
    let lastError: Error | null = null;

    // Try models in fallback sequence if a quota is completely exhausted or model is deprecated
    for (const currentModel of this._fallbackModels) {
      try {
        const res = await this._callGenerateContent(currentModel, systemPrompt, userPrompt);
        if (currentModel !== this.model) {
          this.model = currentModel;
        }
        return res;
      } catch (err) {
        lastError = err as Error;
        const msg = lastError.message;
        const isQuotaOrNotFound = msg.includes('429') || msg.includes('RESOURCE_EXHAUSTED') || msg.includes('404') || msg.includes('NOT_FOUND') || msg.includes('503');

        if (isQuotaOrNotFound && currentModel !== this._fallbackModels[this._fallbackModels.length - 1]) {
          const nextModel = this._fallbackModels[this._fallbackModels.indexOf(currentModel) + 1];
          console.log(`   \x1b[96m🔄 [Auto-Healing Active]: Switching from ${currentModel} ➜ ${nextModel}...\x1b[0m`);
          this.model = nextModel;
          continue;
        }
        throw lastError;
      }
    }

    throw lastError ?? new Error('All Gemini model endpoints failed.');
  }

  private async _callGenerateContent(
    modelName: string,
    systemPrompt: string,
    userPrompt: string
  ): Promise<LLMResponse> {
    const url = `https://generativelanguage.googleapis.com/v1beta/models/${modelName}:generateContent?key=${this._apiKey}`;

    const body: Record<string, unknown> = {
      systemInstruction: {
        parts: [{ text: systemPrompt }],
      },
      contents: [
        {
          role: 'user',
          parts: [{ text: userPrompt }],
        },
      ],
      generationConfig: {
        temperature: 0.1,
        responseMimeType: 'application/json',
        maxOutputTokens: 512,
      },
    };

    let res: Response | null = null;
    const maxRetries = 3;

    for (let attempt = 1; attempt <= maxRetries; attempt++) {
      try {
        res = await fetch(url, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        });
      } catch (networkErr) {
        if (attempt < maxRetries) {
          const waitMs = attempt * 1000 + Math.random() * 500;
          await new Promise((r) => setTimeout(r, waitMs));
          continue;
        }
        throw new Error(`Network error calling Google Gemini: ${(networkErr as Error).message}`);
      }

      if (res.status === 429) {
        const errJson = (await res.json().catch(() => null)) as {
          details?: Array<{ retryDelay?: string }>;
          error?: { message?: string };
        } | null;
        // Instantly throw to trigger the zero-delay multi-model fallback ladder
        throw new Error(`Google Gemini 429 Quota Exhausted on ${modelName}: ${errJson?.error?.message ?? 'Resource exhausted'}`);
      }

      if (res.status >= 500 && attempt < maxRetries) {
        await new Promise((r) => setTimeout(r, attempt * 1000));
        continue;
      }

      break;
    }

    if (!res || !res.ok) {
      const errText = res ? await res.text() : 'No response';
      try {
        const parsedErr = JSON.parse(errText);
        if (parsedErr?.error?.status === 'PERMISSION_DENIED' && errText.includes('SERVICE_DISABLED')) {
          const actUrl = parsedErr.error.details?.find((d: any) => d.activationUrl)?.activationUrl ??
            'https://console.developers.google.com/apis/api/aiplatform.googleapis.com/overview';
          throw new Error(
            `\n\x1b[93m╔══ Google Cloud API Activation Required ═══════════════════════════════════╗\x1b[0m\n` +
            `\x1b[93m║\x1b[0m The Vertex AI / Agent Platform API is not enabled on this GCP project.\n` +
            `\x1b[93m║\x1b[0m 👉 Click here to enable it with 1 click in your browser:\n` +
            `\x1b[93m║\x1b[0m \x1b[96m${actUrl}\x1b[0m\n` +
            `\x1b[93m║\x1b[0m (Alternatively, get an instant Google AI Studio key at https://aistudio.google.com)\n` +
            `\x1b[93m╚═══════════════════════════════════════════════════════════════════════════╝\x1b[0m`
          );
        }
      } catch (e: any) {
        if (e.message.includes('Google Cloud API Activation Required')) throw e;
      }
      throw new Error(`Google Gemini API error (${res?.status ?? 500}): ${errText}`);
    }

    const data = (await res.json()) as {
      candidates?: Array<{
        content?: {
          parts?: Array<{ text?: string; thought?: boolean }>;
        };
      }>;
      usageMetadata?: { totalTokenCount?: number };
    };

    const parts = data.candidates?.[0]?.content?.parts ?? [];
    let rawText = '';
    let thoughtText = '';

    for (const p of parts) {
      if (p.text) {
        if (p.thought) {
          thoughtText += p.text + '\n';
        } else {
          rawText += p.text + '\n';
        }
      }
    }

    if (!rawText.trim() && thoughtText) {
      rawText = thoughtText;
    }
    if (!rawText.trim()) {
      rawText = parts[0]?.text ?? '{}';
    }

    const tokensUsed = data.usageMetadata?.totalTokenCount ?? 0;

    let parsedAction: AgentActionChoice;
    try {
      parsedAction = JSON.parse(rawText.trim()) as AgentActionChoice;
    } catch {
      parsedAction = extractJsonAction(rawText);
    }

    if (process.env['DEBUG'] === '1' || process.env['DEBUG'] === 'true') {
      console.log('\n[DEBUG GEMINI RAW RESPONSE]:', rawText.slice(0, 500));
      console.log('[DEBUG PARSED ACTION]:', JSON.stringify(parsedAction));
    }

    return {
      action: parsedAction,
      rawText,
      tokensUsed,
    };
  }
}

function extractJsonAction(text: string): AgentActionChoice {
  // Strip markdown fences and thinking blocks
  const withoutThinking = text.replace(/<thought>[\s\S]*?<\/thought>/gi, '');
  const cleaned = withoutThinking.replace(/```(?:json)?\s*([\s\S]*?)\s*```/gi, '$1').trim();

  // 1. Direct parse attempt
  try {
    const obj = JSON.parse(cleaned);
    if (obj && typeof obj === 'object') return obj as AgentActionChoice;
  } catch {}

  // 2. Greedy search for outermost JSON object { ... }
  const firstBrace = cleaned.indexOf('{');
  const lastBrace = cleaned.lastIndexOf('}');
  if (firstBrace !== -1 && lastBrace > firstBrace) {
    const candidate = cleaned.slice(firstBrace, lastBrace + 1);
    try {
      return JSON.parse(candidate) as AgentActionChoice;
    } catch {
      // 3. Relaxed cleanup for trailing commas, escaped quotes
      try {
        const relaxed = candidate
          .replace(/,\s*([\}\]])/g, '$1')
          .replace(/[\u0000-\u001F]+/g, ' ');
        return JSON.parse(relaxed) as AgentActionChoice;
      } catch {}
    }
  }

  // 4. Fallback search for action keyword in text
  const clickMatch = text.match(/CLICK[^\d]*(\d+)/i);
  if (clickMatch) {
    return { action: 'CLICK', target_id: parseInt(clickMatch[1], 10), thought: 'Extracted CLICK from reasoning' };
  }

  const typeMatch = text.match(/TYPE[^\d]*(\d+)[^"]*"([^"]+)"/i);
  if (typeMatch) {
    return { action: 'TYPE', target_id: parseInt(typeMatch[1], 10), text: typeMatch[2], thought: 'Extracted TYPE from reasoning' };
  }

  return { action: 'WAIT', message: text.slice(0, 100), thought: 'Awaiting next state' };
}
