import test from 'node:test';
import assert from 'node:assert/strict';

import { appendInboundFrame, collectFramesSince } from './wsFrameBuffer';
import { createWebSocketLifecycleState } from './webSocketLifecycle';

test('collectFramesSince returns all unseen frames in order', () => {
  let frames: Array<{ seq: number; message: unknown }> = [];
  frames = appendInboundFrame(frames, { seq: 1, message: { kind: 'session_created' } }, 10);
  frames = appendInboundFrame(frames, { seq: 2, message: { kind: 'stream_delta', content: '你' } }, 10);
  frames = appendInboundFrame(frames, { seq: 3, message: { kind: 'stream_delta', content: '好' } }, 10);
  frames = appendInboundFrame(frames, { seq: 4, message: { kind: 'complete' } }, 10);

  const unseen = collectFramesSince(frames, 1);
  assert.deepEqual(unseen.map((frame) => frame.seq), [2, 3, 4]);
  assert.equal((unseen[0].message as any).kind, 'stream_delta');
  assert.equal((unseen[2].message as any).kind, 'complete');
});

test('collectFramesSince treats non-positive lastSeq as zero', () => {
  let frames: Array<{ seq: number; message: unknown }> = [];
  frames = appendInboundFrame(frames, { seq: 1, message: { kind: 'stream_delta', content: 'a' } }, 10);
  const all = collectFramesSince(frames, 0);
  assert.equal(all.length, 1);
  assert.equal((all[0].message as any).content, 'a');
});

test('appendInboundFrame enforces bounded replay window', () => {
  let frames: Array<{ seq: number; message: unknown }> = [];
  for (let i = 1; i <= 5; i += 1) {
    frames = appendInboundFrame(frames, { seq: i, message: { i } }, 3);
  }
  assert.deepEqual(frames.map((frame) => frame.seq), [3, 4, 5]);
});

test('drain simulation preserves rapid stream_delta ordering', () => {
  let frames: Array<{ seq: number; message: unknown }> = [];
  let lastSeq = 0;
  const chunks = ['Hel', 'lo', ', ', 'wor', 'ld'];
  for (const [i, content] of chunks.entries()) {
    frames = appendInboundFrame(frames, { seq: i + 1, message: { kind: 'stream_delta', content } }, 4096);
  }
  frames = appendInboundFrame(frames, { seq: chunks.length + 1, message: { kind: 'complete' } }, 4096);

  const drained: string[] = [];
  while (true) {
    const batch = collectFramesSince(frames, lastSeq);
    if (batch.length === 0) break;
    for (const frame of batch) {
      if ((frame.message as any).kind === 'stream_delta') {
        drained.push((frame.message as any).content);
      }
      lastSeq = Math.max(lastSeq, frame.seq);
    }
  }

  assert.equal(drained.join(''), 'Hello, world');
});

test('token-change cleanup does not block reconnect', () => {
  const lifecycle = createWebSocketLifecycleState();

  assert.equal(lifecycle.canConnect(), true);
  lifecycle.onTokenEffectCleanup();
  assert.equal(lifecycle.canConnect(), true);

  lifecycle.onProviderUnmount();
  assert.equal(lifecycle.canConnect(), false);
});
