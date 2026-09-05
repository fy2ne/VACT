/**
 * Tests for VactClient — core client lifecycle and event routing.
 * Uses a MockTransport to avoid Named Pipe dependencies.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import EventEmitter from 'eventemitter3';

import { VactClient } from '../client/VactClient.js';
import type { ITransport, TransportEvents, TransportState } from '../transport/ITransport.js';
import type { ServerMessage, ClientMessage } from '../types/ipc.js';
import type { SceneGraph, SceneNode } from '../types/scene.js';
import { VactTimeoutError, VactStateError } from '../errors/VactError.js';

// ─────────────────────────────────────────────────────────────────────────────
// MockTransport
// ─────────────────────────────────────────────────────────────────────────────

class MockTransport extends EventEmitter<TransportEvents> implements ITransport {
  public state: TransportState = 'disconnected';
  public sent: ClientMessage[] = [];
  public connectCalled = false;
  public closeCalled = false;

  private _connectResolve?: () => void;
  private _connectReject?: (err: Error) => void;

  public connect(): Promise<void> {
    this.connectCalled = true;
    this.state = 'connecting';
    return new Promise((resolve, reject) => {
      this._connectResolve = resolve;
      this._connectReject = reject;
    });
  }

  public send(msg: ClientMessage): Promise<void> {
    this.sent.push(msg);
    return Promise.resolve();
  }

  public close(): void {
    this.closeCalled = true;
    this.state = 'closed';
  }

  // Test helpers
  public acceptConnection(): void {
    this.state = 'connected';
    this._connectResolve?.();
  }

  public rejectConnection(err: Error): void {
    this.state = 'disconnected';
    this._connectReject?.(err);
  }

  public receive(msg: ServerMessage): void {
    this.emit('message', msg);
  }

  public drop(reason = 'test close'): void {
    this.state = 'disconnected';
    this.emit('close', reason);
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test fixtures
// ─────────────────────────────────────────────────────────────────────────────

function makeNode(id: number, type: SceneNode['type'] = 'CONTAINER'): SceneNode {
  return { id, type, bounds: [0, 0, 1920, 1080] };
}

function makeGraph(seq: number): SceneGraph {
  return {
    protocol: 'VACT/1.0',
    seq,
    timestamp_us: seq * 16000,
    viewport: { width: 1920, height: 1080, scale_factor: 1.0 },
    root: makeNode(1),
  };
}

async function connectClient(transport: MockTransport, timeoutMs = 100): Promise<VactClient> {
  const client = new VactClient({ transport, handshakeTimeoutMs: timeoutMs });
  const connectPromise = client.connect();

  // Accept the TCP connection
  transport.acceptConnection();

  // Client sends HANDSHAKE — mock daemon replies with HANDSHAKE_ACK
  await new Promise(r => setTimeout(r, 0)); // flush microtasks
  transport.receive({ type: 'HANDSHAKE_ACK', viewport: { width: 1920, height: 1080, scale_factor: 1.0 } });

  await connectPromise;
  return client;
}

// ─────────────────────────────────────────────────────────────────────────────
// connect + handshake
// ─────────────────────────────────────────────────────────────────────────────

describe('VactClient — connect', () => {
  it('sends a HANDSHAKE message after connecting', async () => {
    const transport = new MockTransport();
    const clientPromise = connectClient(transport);
    const client = await clientPromise;
    expect(transport.sent.some(m => m.type === 'HANDSHAKE')).toBe(true);
    client.close();
  });

  it('emits "connected" event after handshake', async () => {
    const transport = new MockTransport();
    const client = new VactClient({ transport, handshakeTimeoutMs: 100 });
    const connectedSpy = vi.fn();
    client.on('connected', connectedSpy);

    const connectPromise = client.connect();
    transport.acceptConnection();
    await new Promise(r => setTimeout(r, 0));
    transport.receive({ type: 'HANDSHAKE_ACK', viewport: { width: 1920, height: 1080, scale_factor: 1.0 } });
    await connectPromise;

    expect(connectedSpy).toHaveBeenCalledOnce();
    client.close();
  });

  it('throws VactTimeoutError if HANDSHAKE_ACK not received in time', async () => {
    const transport = new MockTransport();
    const client = new VactClient({ transport, handshakeTimeoutMs: 50 });
    const connectPromise = client.connect();
    transport.acceptConnection();

    await expect(connectPromise).rejects.toThrow(VactTimeoutError);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// snapshot events
// ─────────────────────────────────────────────────────────────────────────────

describe('VactClient — snapshot', () => {
  let transport: MockTransport;
  let client: VactClient;

  beforeEach(async () => {
    transport = new MockTransport();
    client = await connectClient(transport);
  });

  it('emits "snapshot" event with the scene graph', () => {
    const spy = vi.fn();
    client.on('snapshot', spy);

    const graph = makeGraph(1);
    transport.receive({ type: 'SNAPSHOT', graph });

    expect(spy).toHaveBeenCalledOnce();
    expect(spy).toHaveBeenCalledWith(graph);
    client.close();
  });

  it('hydrates state after snapshot', () => {
    transport.receive({ type: 'SNAPSHOT', graph: makeGraph(1) });
    expect(client.isHydrated).toBe(true);
    expect(client.getState().seq).toBe(1);
    client.close();
  });

  it('throws VactStateError if getState() called before snapshot', () => {
    expect(() => client.getState()).toThrow(VactStateError);
    client.close();
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// diff events
// ─────────────────────────────────────────────────────────────────────────────

describe('VactClient — diff', () => {
  let transport: MockTransport;
  let client: VactClient;

  beforeEach(async () => {
    transport = new MockTransport();
    client = await connectClient(transport);
    // Load initial snapshot
    transport.receive({ type: 'SNAPSHOT', graph: makeGraph(1) });
  });

  it('emits "diff" event', () => {
    const spy = vi.fn();
    client.on('diff', spy);

    const diff = { type: 'DIFF' as const, seq: 2, ack_seq: 1, mutations: [] };
    transport.receive({ type: 'DIFF', diff });

    expect(spy).toHaveBeenCalledOnce();
    expect(spy).toHaveBeenCalledWith(diff);
    client.close();
  });

  it('advances state seq after stable diff', () => {
    const diff = { type: 'DIFF' as const, seq: 2, ack_seq: 1, mutations: [] };
    transport.receive({ type: 'DIFF', diff });
    expect(client.getState().seq).toBe(2);
    client.close();
  });

  it('emits "error" and resets state on sequence mismatch', () => {
    const errorSpy = vi.fn();
    client.on('error', errorSpy);

    // ack_seq=5 but current seq=1 → mismatch
    const diff = { type: 'DIFF' as const, seq: 10, ack_seq: 5, mutations: [] };
    transport.receive({ type: 'DIFF', diff });

    expect(errorSpy).toHaveBeenCalledOnce();
    expect(client.isHydrated).toBe(false); // state was reset
    client.close();
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// close
// ─────────────────────────────────────────────────────────────────────────────

describe('VactClient — close', () => {
  it('emits "close" event', async () => {
    const transport = new MockTransport();
    const client = await connectClient(transport);
    const spy = vi.fn();
    client.on('close', spy);

    client.close();
    expect(spy).toHaveBeenCalledOnce();
  });

  it('emits "close" event when transport drops', async () => {
    const transport = new MockTransport();
    const client = await connectClient(transport);
    const spy = vi.fn();
    client.on('close', spy);

    transport.drop('vactd crashed');
    expect(spy).toHaveBeenCalledWith('vactd crashed');
    client.close();
  });

  it('resets state on close', async () => {
    const transport = new MockTransport();
    const client = await connectClient(transport);
    transport.receive({ type: 'SNAPSHOT', graph: makeGraph(1) });
    expect(client.isHydrated).toBe(true);

    client.close();
    expect(client.isHydrated).toBe(false);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// query() integration
// ─────────────────────────────────────────────────────────────────────────────

describe('VactClient — query()', () => {
  it('returns a SceneQuery for the current state', async () => {
    const transport = new MockTransport();
    const client = await connectClient(transport);

    const btnNode: SceneNode = {
      id: 2, type: 'BUTTON', bounds: [0, 0, 80, 30], label: 'Submit', interactable: true,
    };
    const graph: SceneGraph = {
      protocol: 'VACT/1.0', seq: 1, timestamp_us: 1000,
      viewport: { width: 1920, height: 1080, scale_factor: 1.0 },
      root: { id: 1, type: 'CONTAINER', bounds: [0, 0, 1920, 1080], children: [btnNode] },
    };
    transport.receive({ type: 'SNAPSHOT', graph });

    const found = client.query().ofType('BUTTON').withLabel('Submit').first();
    expect(found?.id).toBe(2);
    client.close();
  });
});
