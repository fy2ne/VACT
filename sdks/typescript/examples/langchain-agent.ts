/**
 * LangChain.js Integration Example — VACT TypeScript SDK (`vact-sdk`)
 *
 * Demonstrates how to wrap VACT into LangChain DynamicStructuredTool tools,
 * allowing LLMs (Claude, GPT-4, Gemini) to perceive UI state as structured vector DAGs
 * with 0 screenshots, 0 cloud vision tokens, and sub-20ms interaction loops.
 */

import { VactClient } from '../src/index.js';

/**
 * Helper to build LLM tools for LangChain / custom agent harnesses
 */
export function createVactAgentTools(client: VactClient) {
  return {
    /**
     * Tool: get_desktop_scene
     * Returns the semantic vector tree representation of the active UI.
     */
    get_desktop_scene: {
      name: 'get_desktop_scene',
      description: 'Returns the current UI state as a semantic vector scene graph with bounding boxes, labels, and interactive node IDs. Use this instead of taking screenshots.',
      execute: async () => {
        if (!client.isHydrated) {
          return { error: 'Scene graph not yet hydrated. Daemon is connecting.' };
        }
        const state = client.getState();
        return {
          seq: state.seq,
          active_window: state.active_window,
          viewport: state.viewport,
          root: state.root,
        };
      },
    },

    /**
     * Tool: click_ui_element
     * Dispatches a deterministic sub-ms click to a node in the scene graph.
     */
    click_ui_element: {
      name: 'click_ui_element',
      description: 'Click on an interactive UI element by its stable node ID from the scene graph.',
      parameters: {
        node_id: { type: 'number', description: 'Stable ID of the node to click' },
      },
      execute: async ({ node_id }: { node_id: number }) => {
        const res = await client.agent().click(node_id);
        return { success: res.ok, route: res.route, error: res.error };
      },
    },

    /**
     * Tool: type_text
     * Types characters directly into the target input field or element.
     */
    type_text: {
      name: 'type_text',
      description: 'Types text into a specified input field or interactive node by ID.',
      parameters: {
        node_id: { type: 'number', description: 'Target node ID' },
        text: { type: 'string', description: 'The text string to type' },
      },
      execute: async ({ node_id, text }: { node_id: number; text: string }) => {
        const res = await client.agent().type(node_id, text);
        return { success: res.ok, route: res.route, error: res.error };
      },
    },
  };
}

async function main(): Promise<void> {
  const client = new VactClient();
  await client.connect();

  console.log('🤖 Initialized VACT Agent Tools for LangChain / Autonomous Agent Harnesses.');
  const tools = createVactAgentTools(client);

  // Demonstrate tool execution without screenshot overhead
  console.log('\n--- LLM Step 1: Perceive UI state ---');
  const sceneResult = await tools.get_desktop_scene.execute();
  console.log('Scene context extracted (< 2KB token payload):', JSON.stringify(sceneResult).slice(0, 200) + '...');

  client.close();
}

main().catch(console.error);
