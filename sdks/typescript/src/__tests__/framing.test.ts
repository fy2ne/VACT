/**
 * Tests for the wire framing codec — encodeFrame + FrameDecoder
 */

import { describe, it, expect, vi } from 'vitest';
import { encodeFrame, FrameDecoder } from '../transport/framing.js';
import { VactProtocolError } from '../errors/VactError.js';

// ─────────────────────────────────────────────────────────────────────────────
// encodeFrame
// ─────────────────────────────────────────────────────────────────────────────

describe('encodeFrame', () => {
  it('produces a 4-byte LE length header followed by UTF-8 JSON', () => {
    const payload = { type: 'HANDSHAKE', protocol: 'VACT/1.0', capabilities: [] };
    const frame = encodeFrame(payload);
    const expectedJson = JSON.stringify(payload);
    const expectedBody = Buffer.from(expectedJson, 'utf8');

    // First 4 bytes = body length as LE u32
    expect(frame.readUInt32LE(0)).toBe(expectedBody.byteLength);
    // Remaining bytes = the JSON body
    expect(frame.subarray(4).toString('utf8')).toBe(expectedJson);
  });

  it('correctly encodes payload with unicode characters', () => {
    const payload = { text: '你好 emoji 🚀' };
    const frame = encodeFrame(payload);
    const body = Buffer.from(JSON.stringify(payload), 'utf8');
    expect(frame.readUInt32LE(0)).toBe(body.byteLength);
    expect(frame.subarray(4).toString('utf8')).toBe(JSON.stringify(payload));
  });

  it('encodes empty object', () => {
    const frame = encodeFrame({});
    const body = Buffer.from('{}', 'utf8');
    expect(frame.readUInt32LE(0)).toBe(body.byteLength);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// FrameDecoder — single complete frame
// ─────────────────────────────────────────────────────────────────────────────

describe('FrameDecoder — single frame', () => {
  it('emits a complete frame when all bytes arrive at once', () => {
    const frames: string[] = [];
    const decoder = new FrameDecoder((json) => frames.push(json));

    const payload = { type: 'HANDSHAKE_ACK', viewport: { width: 1920, height: 1080, scale_factor: 1.0 } };
    decoder.push(encodeFrame(payload));

    expect(frames).toHaveLength(1);
    expect(JSON.parse(frames[0]!)).toEqual(payload);
  });

  it('has 0 buffered bytes after a complete frame', () => {
    const decoder = new FrameDecoder(() => {});
    decoder.push(encodeFrame({ msg: 'test' }));
    expect(decoder.bufferedBytes).toBe(0);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// FrameDecoder — fragmented delivery (TCP coalescing simulation)
// ─────────────────────────────────────────────────────────────────────────────

describe('FrameDecoder — fragmented delivery', () => {
  it('reassembles a frame split across two chunks', () => {
    const frames: string[] = [];
    const decoder = new FrameDecoder((json) => frames.push(json));

    const payload = { seq: 42, type: 'DIFF' };
    const full = encodeFrame(payload);

    // Split at arbitrary point inside the body
    const splitAt = 6;
    decoder.push(full.subarray(0, splitAt));
    expect(frames).toHaveLength(0); // incomplete
    decoder.push(full.subarray(splitAt));
    expect(frames).toHaveLength(1);
    expect(JSON.parse(frames[0]!)).toEqual(payload);
  });

  it('handles header split across chunks', () => {
    const frames: string[] = [];
    const decoder = new FrameDecoder((json) => frames.push(json));

    const payload = { data: 'x'.repeat(100) };
    const full = encodeFrame(payload);

    // Split inside the 4-byte header
    decoder.push(full.subarray(0, 2));
    expect(frames).toHaveLength(0);
    decoder.push(full.subarray(2));
    expect(frames).toHaveLength(1);
  });

  it('handles multiple frames in one chunk (coalesced)', () => {
    const frames: string[] = [];
    const decoder = new FrameDecoder((json) => frames.push(json));

    const f1 = encodeFrame({ seq: 1 });
    const f2 = encodeFrame({ seq: 2 });
    const f3 = encodeFrame({ seq: 3 });
    const combined = Buffer.concat([f1, f2, f3]);

    decoder.push(combined);
    expect(frames).toHaveLength(3);
    expect(JSON.parse(frames[0]!)).toEqual({ seq: 1 });
    expect(JSON.parse(frames[1]!)).toEqual({ seq: 2 });
    expect(JSON.parse(frames[2]!)).toEqual({ seq: 3 });
  });

  it('handles a frame split across 10 single-byte pushes', () => {
    const frames: string[] = [];
    const decoder = new FrameDecoder((json) => frames.push(json));

    const payload = { hello: 'world' };
    const full = encodeFrame(payload);

    for (let i = 0; i < full.byteLength; i++) {
      decoder.push(full.subarray(i, i + 1));
    }

    expect(frames).toHaveLength(1);
    expect(JSON.parse(frames[0]!)).toEqual(payload);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// FrameDecoder — error handling
// ─────────────────────────────────────────────────────────────────────────────

describe('FrameDecoder — error handling', () => {
  it('throws VactProtocolError for oversized frames', () => {
    const decoder = new FrameDecoder(() => {});

    // Craft a frame header claiming 20 MB payload
    const fakeHeader = Buffer.allocUnsafe(4);
    fakeHeader.writeUInt32LE(20 * 1024 * 1024, 0);

    expect(() => decoder.push(fakeHeader)).toThrow(VactProtocolError);
  });

  it('resets buffer after oversized frame error', () => {
    const decoder = new FrameDecoder(() => {});

    const fakeHeader = Buffer.allocUnsafe(4);
    fakeHeader.writeUInt32LE(20 * 1024 * 1024, 0);

    try { decoder.push(fakeHeader); } catch { /* expected */ }
    expect(decoder.bufferedBytes).toBe(0);
  });

  it('calls onFrame callback with the exact JSON string', () => {
    const cb = vi.fn();
    const decoder = new FrameDecoder(cb);

    const json = '{"type":"SNAPSHOT"}';
    const body = Buffer.from(json, 'utf8');
    const header = Buffer.allocUnsafe(4);
    header.writeUInt32LE(body.byteLength, 0);

    decoder.push(Buffer.concat([header, body]));
    expect(cb).toHaveBeenCalledOnce();
    expect(cb).toHaveBeenCalledWith(json);
  });
});
