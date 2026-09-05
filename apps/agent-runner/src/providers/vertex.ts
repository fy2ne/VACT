/**
 * Google Cloud Vertex AI Enterprise Provider
 * Authenticates via Google Cloud Application Default Credentials (ADC) or Service Account.
 */

import { GoogleAuth } from 'google-auth-library';
import type { ILLMProvider, LLMResponse } from './base.js';
import type { AgentActionChoice } from '../types.js';
import { normalizeGeminiModel } from './gemini.js';

export class VertexAIProvider implements ILLMProvider {
  public readonly name = 'Google Cloud Vertex AI';
  public model: string;
  public readonly projectId: string;
  public readonly location: string;
  private readonly _apiKey?: string;
  private readonly _auth?: GoogleAuth;
  private readonly _fallbackModels: string[];

  public constructor(
    model = process.env['GEMINI_MODEL'] ?? process.env['DEFAULT_MODEL'] ?? 'gemini-2.5-flash',
    projectId = process.env['GOOGLE_CLOUD_PROJECT'] ?? process.env['GCP_PROJECT_ID'] ?? '',
    location = process.env['GOOGLE_CLOUD_LOCATION'] ?? 'us-central1',
    apiKey = process.env['VERTEX_API_KEY'] ?? process.env['GEMINI_API_KEY'],
    keyFile = process.env['GOOGLE_APPLICATION_CREDENTIALS']
  ) {
    this.model = normalizeGeminiModel(model);
    this.projectId = projectId;
    this.location = location;
    this._apiKey = apiKey;

    if (!this._apiKey) {
      if (!this.projectId || this.projectId === 'default') {
        throw new Error(
          'GOOGLE_CLOUD_PROJECT is required for Vertex AI. Please set your Google Cloud Project ID in .env or environment variables.'
        );
      }

      this._auth = new GoogleAuth({
        scopes: ['https://www.googleapis.com/auth/cloud-platform'],
        keyFilename: keyFile || undefined,
        projectId: this.projectId,
      });
    }

    const defaultFallbacks = ['gemini-2.5-flash', 'gemini-3.7-flash', 'gemini-1.5-flash', 'gemini-1.5-pro'];
    this._fallbackModels = [this.model, ...defaultFallbacks.filter((m) => m !== this.model)];
  }

  public async decideAction(systemPrompt: string, userPrompt: string): Promise<LLMResponse> {
    let lastError: Error | null = null;

    for (const currentModel of this._fallbackModels) {
      try {
        const res = await this._callVertex(currentModel, systemPrompt, userPrompt);
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
          console.log(`   \x1b[96m🔄 [Auto-Healing Active]: Switching Vertex model from ${currentModel} ➜ ${nextModel}...\x1b[0m`);
          this.model = nextModel;
          continue;
        }
        throw lastError;
      }
    }

    throw lastError ?? new Error('All Vertex AI model endpoints failed.');
  }

  private async _callVertex(modelName: string, systemPrompt: string, userPrompt: string): Promise<LLMResponse> {
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
    };

    let url: string;

    if (this._apiKey) {
      headers['x-goog-api-key'] = this._apiKey;
      // High-performance direct endpoint for API keys
      url = `https://generativelanguage.googleapis.com/v1beta/models/${modelName}:generateContent?key=${this._apiKey}`;
    } else if (this._auth) {
      const client = await this._auth.getClient();
      const tokenResponse = await client.getAccessToken();
      const token = tokenResponse.token;

      if (!token) {
        throw new Error(
          'Could not obtain Google Cloud access token. Run: gcloud auth application-default login'
        );
      }
      headers['Authorization'] = `Bearer ${token}`;
      url = `https://${this.location}-aiplatform.googleapis.com/v1/projects/${this.projectId}/locations/${this.location}/publishers/google/models/${modelName}:generateContent`;
    } else {
      throw new Error('No authentication method configured for Vertex AI.');
    }

    const body = {
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
      },
    };

    const res = await fetch(url, {
      method: 'POST',
      headers,
      body: JSON.stringify(body),
    });

    if (!res.ok) {
      const errText = await res.text();
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
      throw new Error(`Google Cloud Vertex AI Error (${res.status}): ${errText}`);
    }

    const data = (await res.json()) as {
      candidates?: Array<{ content?: { parts?: Array<{ text?: string; thought?: boolean }> } }>;
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
      console.log('\n[DEBUG VERTEX RAW RESPONSE]:', rawText.slice(0, 500));
      console.log('[DEBUG PARSED ACTION]:', JSON.stringify(parsedAction));
    }

    return { action: parsedAction, rawText, tokensUsed };
  }
}

function extractJsonAction(text: string): AgentActionChoice {
  const withoutThinking = text.replace(/<thought>[\s\S]*?<\/thought>/gi, '');
  const cleaned = withoutThinking.replace(/```(?:json)?\s*([\s\S]*?)\s*```/gi, '$1').trim();

  try {
    const obj = JSON.parse(cleaned);
    if (obj && typeof obj === 'object') return obj as AgentActionChoice;
  } catch {}

  const firstBrace = cleaned.indexOf('{');
  const lastBrace = cleaned.lastIndexOf('}');
  if (firstBrace !== -1 && lastBrace > firstBrace) {
    const candidate = cleaned.slice(firstBrace, lastBrace + 1);
    try {
      return JSON.parse(candidate) as AgentActionChoice;
    } catch {
      try {
        const relaxed = candidate
          .replace(/,\s*([\}\]])/g, '$1')
          .replace(/[\u0000-\u001F]+/g, ' ');
        return JSON.parse(relaxed) as AgentActionChoice;
      } catch {}
    }
  }

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
