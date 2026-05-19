import test from 'node:test';
import assert from 'node:assert/strict';

import { appendInboundFrame, collectFramesSince } from '../../../contexts/wsFrameBuffer';
import {
  drainAndProcessRealtimeFrames,
  processChatRealtimeMessage,
  type ChatRealtimeProcessorContext,
  type RealtimeChatMessage,
} from './chatRealtimeProcessor';

function createMockSessionStore() {
  const calls: Array<{ method: string; args: unknown[] }> = [];
  return {
    calls,
    updateStreaming(sessionId: string, text: string) {
      calls.push({ method: 'updateStreaming', args: [sessionId, text] });
    },
    finalizeStreaming(sessionId: string) {
      calls.push({ method: 'finalizeStreaming', args: [sessionId] });
    },
    updateThinking(sessionId: string, text: string) {
      calls.push({ method: 'updateThinking', args: [sessionId, text] });
    },
    finalizeThinking(sessionId: string) {
      calls.push({ method: 'finalizeThinking', args: [sessionId] });
    },
    appendRealtime(sessionId: string, msg: unknown) {
      calls.push({ method: 'appendRealtime', args: [sessionId, msg] });
    },
    replaceSessionId(from: string, to: string) {
      calls.push({ method: 'replaceSessionId', args: [from, to] });
    },
  };
}

function createProcessorContext(
  overrides: Partial<ChatRealtimeProcessorContext> = {},
): ChatRealtimeProcessorContext {
  const sessionStore = createMockSessionStore();
  return {
    provider: 'claude',
    selectedSession: null,
    currentSessionId: 'sess-1',
    activeViewSessionId: 'sess-1',
    setCurrentSessionId: () => {},
    setIsLoading: () => {},
    setCanAbortSession: () => {},
    setClaudeStatus: () => {},
    setTokenBudget: () => {},
    setPendingPermissionRequests: () => {},
    pendingViewSessionRef: { current: null },
    streamTimerRef: { current: null },
    accumulatedStreamRef: { current: '' },
    thinkingTimerRef: { current: null },
    accumulatedThinkingRef: { current: '' },
    sessionStore: sessionStore as any,
    ...overrides,
  };
}

test('processChatRealtimeMessage accumulates stream_delta into session store', () => {
  const accumulatedStreamRef = { current: '' };
  const store = createMockSessionStore();
  const ctx = createProcessorContext({
    accumulatedStreamRef,
    sessionStore: store as any,
  });

  processChatRealtimeMessage(
    { kind: 'stream_delta', sessionId: 'sess-1', content: 'Hel' },
    ctx,
  );
  processChatRealtimeMessage(
    { kind: 'stream_delta', sessionId: 'sess-1', content: 'lo' },
    ctx,
  );

  assert.equal(accumulatedStreamRef.current, 'Hello');
  assert.deepEqual(
    store.calls.filter((c) => c.method === 'updateStreaming').map((c) => c.args[1]),
    ['Hel', 'Hello'],
  );
});

test('processChatRealtimeMessage finalizes streaming on complete', () => {
  let loading = true;
  const accumulatedStreamRef = { current: 'done' };
  const store = createMockSessionStore();
  const ctx = createProcessorContext({
    accumulatedStreamRef,
    sessionStore: store as any,
    setIsLoading: (v) => { loading = v; },
  });

  processChatRealtimeMessage({ kind: 'complete', sessionId: 'sess-1' }, ctx);

  assert.equal(loading, false);
  assert.equal(accumulatedStreamRef.current, '');
  assert.ok(store.calls.some((c) => c.method === 'finalizeStreaming'));
});

test('processChatRealtimeMessage routes tool_use to appendRealtime', () => {
  const store = createMockSessionStore();
  const ctx = createProcessorContext({ sessionStore: store as any });

  processChatRealtimeMessage(
    {
      kind: 'tool_use',
      sessionId: 'sess-1',
      toolName: 'Read',
      toolId: 't1',
      toolInput: { path: '/tmp/x' },
    },
    ctx,
  );

  const append = store.calls.find((c) => c.method === 'appendRealtime');
  assert.ok(append);
  assert.equal((append!.args[1] as any).kind, 'tool_use');
});

test('processChatRealtimeMessage enqueues permission_request UI state', () => {
  let pending: unknown[] = [{ requestId: 'existing', toolName: 'X', sessionId: null, receivedAt: new Date() }];
  let status: unknown = null;
  const ctx = createProcessorContext({
    setPendingPermissionRequests: (updater) => {
      pending = typeof updater === 'function' ? (updater as any)(pending) : updater;
    },
    setClaudeStatus: (s) => { status = s; },
  });

  processChatRealtimeMessage(
    {
      kind: 'permission_request',
      sessionId: 'sess-1',
      requestId: 'req-99',
      toolName: 'Bash',
      input: { command: 'ls' },
    },
    ctx,
  );

  assert.equal(pending.length, 2);
  assert.equal((pending[1] as any).requestId, 'req-99');
  assert.equal((status as any).text, 'Waiting for permission');
});

test('drainAndProcessRealtimeFrames processes buffered WS frames in order', () => {
  const accumulatedStreamRef = { current: '' };
  const store = createMockSessionStore();
  const ctx = createProcessorContext({
    accumulatedStreamRef,
    sessionStore: store as any,
  });

  let frames = appendInboundFrame([], { seq: 1, message: { kind: 'thinking', content: 'hmm' } });
  frames = appendInboundFrame(frames, { seq: 2, message: { kind: 'stream_delta', content: 'A', sessionId: 'sess-1' } });
  frames = appendInboundFrame(frames, { seq: 3, message: { kind: 'stream_delta', content: 'B', sessionId: 'sess-1' } });
  frames = appendInboundFrame(frames, { seq: 4, message: { kind: 'complete', sessionId: 'sess-1' } });

  const batch = collectFramesSince(frames, 0).map((f) => ({
    seq: f.seq,
    message: f.message as RealtimeChatMessage,
  }));
  const nextSeq = drainAndProcessRealtimeFrames(batch, ctx, 0);

  assert.equal(nextSeq, 4);
  assert.equal(accumulatedStreamRef.current, '');
  assert.ok(store.calls.some((c) => c.method === 'finalizeThinking'));
  assert.ok(store.calls.some((c) => c.method === 'finalizeStreaming'));
});

test('processChatRealtimeMessage clears loading state on error', () => {
  let loading = true;
  let canAbort = true;
  const ctx = createProcessorContext({
    setIsLoading: (v) => { loading = v; },
    setCanAbortSession: (v) => { canAbort = v; },
  });

  processChatRealtimeMessage(
    { kind: 'error', sessionId: 'sess-1', content: 'mock-acp-error' },
    ctx,
  );

  assert.equal(loading, false);
  assert.equal(canAbort, false);
});
