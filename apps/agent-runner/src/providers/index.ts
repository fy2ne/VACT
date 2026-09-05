/**
 * Google Cloud AI Provider Factory
 * Exclusively supports Google Cloud Vertex AI and Google AI Studio (Gemini 2.5 / 2.0).
 */

import type { ILLMProvider } from './base.js';
import { GeminiProvider } from './gemini.js';
import { VertexAIProvider } from './vertex.js';

export type { ILLMProvider, LLMResponse } from './base.js';
export { GeminiProvider } from './gemini.js';
export { VertexAIProvider } from './vertex.js';

export type GoogleBackendType = 'vertex' | 'ai-studio' | 'gemini' | string;

export function getProvider(
  backend: GoogleBackendType = 'vertex',
  model = process.env['GEMINI_MODEL'] ?? process.env['DEFAULT_MODEL'] ?? 'gemini-2.5-flash',
  apiKey?: string,
  projectId?: string,
  location?: string
): ILLMProvider {
  const norm = backend.toLowerCase();

  switch (norm) {
    case 'vertex':
    case 'vertexai':
    case 'gcp':
    case 'google-cloud':
      return new VertexAIProvider(
        model,
        projectId ?? process.env['GOOGLE_CLOUD_PROJECT'] ?? process.env['GCP_PROJECT_ID'],
        location ?? process.env['GOOGLE_CLOUD_LOCATION'] ?? 'us-central1',
        apiKey ?? process.env['VERTEX_API_KEY'] ?? process.env['GEMINI_API_KEY'],
        process.env['GOOGLE_APPLICATION_CREDENTIALS']
      );

    case 'gemini':
    case 'ai-studio':
    case 'aistudio':
    case 'google':
      return new GeminiProvider(model, apiKey ?? process.env['GEMINI_API_KEY'] ?? process.env['VERTEX_API_KEY']);

    default:
      throw new Error(
        `Invalid backend: "${backend}". This engine is exclusively built on Google Cloud AI. Supported backends: 'vertex' (Google Cloud Vertex AI) and 'ai-studio' (Google AI Studio).`
      );
  }
}
