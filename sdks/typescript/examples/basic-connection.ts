/**
 * Basic Connection Example — VACT TypeScript SDK (`vact-sdk`)
 *
 * Demonstrates:
 * 1. Establishing an IPC connection to `vactd` over Windows Named Pipes.
 * 2. Receiving the initial Vector Scene Graph snapshot.
 * 3. Streaming temporal delta diffs in real-time (< 2KB payloads).
 * 4. Graceful teardown on SIGINT.
 */

import { VactClient } from '../src/index.js';

async function main(): Promise<void> {
  console.log('🚀 Connecting to VACT Daemon (vactd)...');

  const client = new VactClient();

  // Listen for initial full scene graph snapshot
  client.on('snapshot', (graph) => {
    console.log('\n📸 [SNAPSHOT] Initial Scene Graph Received:');
    console.log(`   Protocol:       ${graph.protocol}`);
    console.log(`   Frame Seq:      #${graph.seq}`);
    console.log(`   Active Window:  ${graph.active_window ?? 'None'}`);
    console.log(`   Viewport:       ${graph.viewport.width}x${graph.viewport.height} @ ${graph.viewport.scale_factor}x`);
    console.log(`   Root Type:      ${graph.root.type} (ID: ${graph.root.id})`);
    console.log(`   Total Children: ${graph.root.children?.length ?? 0}`);
  });

  // Listen for real-time temporal diffs
  client.on('diff', (diff) => {
    if (diff.mutations.length === 0) {
      // Stable frame (no visual mutation detected)
      return;
    }

    console.log(`\n⚡ [DIFF] Frame #${diff.seq} (ack: #${diff.ack_seq}) — ${diff.mutations.length} mutation(s):`);
    for (const mutation of diff.mutations) {
      switch (mutation.op) {
        case 'INSERT':
          console.log(`   ➕ INSERT: Node #${mutation.node.id} (${mutation.node.type}) under parent #${mutation.parent_id}`);
          break;
        case 'UPDATE':
          console.log(`   🔄 UPDATE: Node #${mutation.id} -> Patch: ${JSON.stringify(mutation.patch)}`);
          break;
        case 'REMOVE':
          console.log(`   ❌ REMOVE: Node #${mutation.id}`);
          break;
      }
    }
  });

  client.on('error', (err) => {
    console.error('⚠️ [CLIENT ERROR]:', err.message);
  });

  client.on('close', (reason) => {
    console.log(`🔌 Connection closed: ${reason}`);
  });

  try {
    await client.connect();
    console.log('✅ Connected to vactd successfully! Streaming scene graph deltas (Press Ctrl+C to exit)...\n');
  } catch (err) {
    console.error('❌ Failed to connect to vactd. Is the daemon running? (run: cargo run -p vactd)');
    console.error(err);
    process.exit(1);
  }

  // Graceful shutdown on Ctrl+C
  process.on('SIGINT', () => {
    console.log('\nShutting down VACT client...');
    client.close();
    process.exit(0);
  });
}

main().catch(console.error);
