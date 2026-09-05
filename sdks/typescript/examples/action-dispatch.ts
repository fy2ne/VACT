/**
 * Action Dispatch Example — VACT TypeScript SDK (`vact-sdk`)
 *
 * Demonstrates:
 * 1. Querying the live Vector Scene Graph using the fluent `SceneQuery` DSL.
 * 2. Dispatching synthetic hardware actions (CLICK, TYPE, SCROLL, KEY) via `AgentContext`.
 * 3. Handling hybrid direct mode / ghost mode execution confirmations from `vactd`.
 */

import { VactClient } from '../src/index.js';

async function main(): Promise<void> {
  const client = new VactClient();
  await client.connect();

  console.log('✅ Connected to VACT daemon.');

  // Wait for initial snapshot hydration
  if (!client.isHydrated) {
    await new Promise<void>((resolve) => client.once('snapshot', () => resolve()));
  }

  const graph = client.getState();
  console.log(`Active Foreground Window: ${graph.active_window ?? 'Unknown'}`);

  // Query scene graph using fluent DSL
  console.log('\n🔍 Finding interactive elements in current view...');

  const searchBox = client.query()
    .ofType('INPUT_FIELD')
    .interactable()
    .first();

  if (searchBox) {
    console.log(`🎯 Found Input Field #${searchBox.id} (${searchBox.label ?? 'unlabeled'}) at [${searchBox.bounds.join(', ')}]`);
    
    // Focus and type query
    console.log(`⌨️ Dispatching TYPE action to Node #${searchBox.id}...`);
    const typeRes = await client.agent().type(searchBox.id, 'VACT hardware acceleration');
    console.log(`   Result: route=${typeRes.route}, ok=${typeRes.ok}`);
  }

  // Find a button with "search" or "submit" label
  const submitBtn = client.query()
    .ofType('BUTTON')
    .withLabel(/search|submit|go|ok/i)
    .notDisabled()
    .first();

  if (submitBtn) {
    console.log(`🎯 Found Button #${submitBtn.id} [${submitBtn.label}] at [${submitBtn.bounds.join(', ')}]`);
    
    console.log(`🖱️ Dispatching CLICK action to Node #${submitBtn.id}...`);
    const clickRes = await client.agent().click(submitBtn.id);
    console.log(`   Result: route=${clickRes.route}, ok=${clickRes.ok}`);
  }

  client.close();
}

main().catch(console.error);
