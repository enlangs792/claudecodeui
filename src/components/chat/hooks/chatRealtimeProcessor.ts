import type { Dispatch, MutableRefObject, SetStateAction } from 'react';

import type { PendingPermissionRequest, SessionNavigationOptions } from '../types/types';
import type { ProjectSession, LLMProvider } from '../../../types/app';
import type { SessionStore, NormalizedMessage } from '../../../stores/useSessionStore';

export type RealtimeChatMessage = {
  type?: string;
  kind?: string;
  data?: any;
  message?: any;
  sessionId?: string;
  requestId?: string;
  toolName?: string;
  input?: unknown;
  context?: unknown;
  newSessionId?: string;
  content?: string;
  text?: string;
  tokens?: number;
  canInterrupt?: boolean;
  tokenBudget?: unknown;
  aborted?: boolean;
  exitCode?: number;
  actualSessionId?: string;
  status?: any;
  isProcessing?: boolean;
  [key: string]: any;
};

export type ChatRealtimeProcessorContext = {
  provider: LLMProvider;
  selectedSession: ProjectSession | null;
  currentSessionId: string | null;
  activeViewSessionId: string | null;
  setCurrentSessionId: (sessionId: string | null) => void;
  setIsLoading: (loading: boolean) => void;
  setCanAbortSession: (canAbort: boolean) => void;
  setClaudeStatus: (status: { text: string; tokens: number; can_interrupt: boolean } | null) => void;
  setTokenBudget: (budget: Record<string, unknown> | null) => void;
  setPendingPermissionRequests: Dispatch<SetStateAction<PendingPermissionRequest[]>>;
  pendingViewSessionRef: MutableRefObject<{ sessionId: string | null; startedAt: number } | null>;
  streamTimerRef: MutableRefObject<number | null>;
  accumulatedStreamRef: MutableRefObject<string>;
  thinkingTimerRef: MutableRefObject<number | null>;
  accumulatedThinkingRef: MutableRefObject<string>;
  onSessionInactive?: (sessionId?: string | null) => void;
  onSessionProcessing?: (sessionId?: string | null) => void;
  onSessionNotProcessing?: (sessionId?: string | null) => void;
  onNavigateToSession?: (sessionId: string, options?: SessionNavigationOptions) => void;
  onWebSocketReconnect?: () => void;
  sessionStore: SessionStore;
  refreshProjects?: () => void | Promise<void>;
};

function flushAccumulatedThinking(ctx: ChatRealtimeProcessorContext, sessionId: string | null) {
  if (!sessionId) return;
  if (ctx.thinkingTimerRef.current) {
    clearTimeout(ctx.thinkingTimerRef.current);
    ctx.thinkingTimerRef.current = null;
  }
  if (ctx.accumulatedThinkingRef.current) {
    ctx.sessionStore.updateThinking(sessionId, ctx.accumulatedThinkingRef.current, ctx.provider);
    ctx.sessionStore.finalizeThinking(sessionId);
    ctx.accumulatedThinkingRef.current = '';
  }
}

/** Pure message processor shared by the hook and unit tests. */
export function processChatRealtimeMessage(
  msg: RealtimeChatMessage | null | undefined,
  ctx: ChatRealtimeProcessorContext,
): void {
  if (!msg) return;

  if (!msg.kind) {
    const messageType = String(msg.type || '');

    switch (messageType) {
      case 'websocket-reconnected':
        ctx.onWebSocketReconnect?.();
        return;

      case 'pending-permissions-response': {
        const permSessionId = msg.sessionId;
        const isCurrentPermSession =
          permSessionId === ctx.currentSessionId
          || (ctx.selectedSession && permSessionId === ctx.selectedSession.id);
        if (permSessionId && !isCurrentPermSession) return;
        ctx.setPendingPermissionRequests(msg.data || []);
        return;
      }

      case 'session-status': {
        const statusSessionId = msg.sessionId;
        if (!statusSessionId) return;

        const status = msg.status;
        if (status) {
          const statusInfo = {
            text: status.text || 'Working...',
            tokens: status.tokens || 0,
            can_interrupt: status.can_interrupt !== undefined ? status.can_interrupt : true,
          };
          ctx.setClaudeStatus(statusInfo);
          ctx.setIsLoading(true);
          ctx.setCanAbortSession(statusInfo.can_interrupt);
          return;
        }

        const isCurrentSession =
          statusSessionId === ctx.currentSessionId
          || (ctx.selectedSession && statusSessionId === ctx.selectedSession.id);

        if (msg.isProcessing) {
          ctx.onSessionProcessing?.(statusSessionId);
          if (isCurrentSession) {
            ctx.setIsLoading(true);
            ctx.setCanAbortSession(true);
          }
          return;
        }
        ctx.onSessionInactive?.(statusSessionId);
        ctx.onSessionNotProcessing?.(statusSessionId);
        if (isCurrentSession) {
          ctx.setIsLoading(false);
          ctx.setCanAbortSession(false);
          ctx.setClaudeStatus(null);
        }
        return;
      }

      default:
        return;
    }
  }

  const sid = msg.sessionId || ctx.activeViewSessionId;

  if (msg.kind === 'thinking') {
    const text = msg.content || '';
    if (!text) return;
    ctx.accumulatedThinkingRef.current += text;
    if (sid) {
      ctx.sessionStore.updateThinking(sid, ctx.accumulatedThinkingRef.current, ctx.provider);
    }
    return;
  }

  if (msg.kind === 'stream_delta') {
    flushAccumulatedThinking(ctx, sid);
    const text = msg.content || '';
    if (!text) return;
    ctx.accumulatedStreamRef.current += text;
    if (sid) {
      ctx.sessionStore.updateStreaming(sid, ctx.accumulatedStreamRef.current, ctx.provider);
    }
    return;
  }

  if (msg.kind === 'stream_end') {
    flushAccumulatedThinking(ctx, sid);
    if (ctx.streamTimerRef.current) {
      clearTimeout(ctx.streamTimerRef.current);
      ctx.streamTimerRef.current = null;
    }
    if (sid) {
      if (ctx.accumulatedStreamRef.current) {
        ctx.sessionStore.updateStreaming(sid, ctx.accumulatedStreamRef.current, ctx.provider);
      }
      ctx.sessionStore.finalizeStreaming(sid);
    }
    ctx.accumulatedStreamRef.current = '';
    return;
  }

  const shouldPersist =
    msg.kind !== 'session_created'
    && msg.kind !== 'complete'
    && msg.kind !== 'status'
    && msg.kind !== 'permission_request'
    && msg.kind !== 'permission_cancelled';

  if (sid && shouldPersist) {
    if (msg.kind === 'tool_use' || msg.kind === 'text') {
      flushAccumulatedThinking(ctx, sid);
    }
    ctx.sessionStore.appendRealtime(sid, msg as NormalizedMessage);
  }

  switch (msg.kind) {
    case 'session_created': {
      const newSessionId = msg.newSessionId;
      if (!newSessionId) break;

      if (!ctx.currentSessionId) {
        if (typeof sessionStorage !== 'undefined') {
          sessionStorage.setItem('pendingSessionId', newSessionId);
        }
        if (ctx.pendingViewSessionRef.current && !ctx.pendingViewSessionRef.current.sessionId) {
          ctx.pendingViewSessionRef.current.sessionId = newSessionId;
        }
        ctx.setCurrentSessionId(newSessionId);
        ctx.setPendingPermissionRequests((prev) =>
          prev.map((r) => (r.sessionId ? r : { ...r, sessionId: newSessionId })),
        );
      }
      ctx.onNavigateToSession?.(newSessionId);
      break;
    }

    case 'complete': {
      flushAccumulatedThinking(ctx, sid);
      if (ctx.streamTimerRef.current) {
        clearTimeout(ctx.streamTimerRef.current);
        ctx.streamTimerRef.current = null;
      }
      if (sid && ctx.accumulatedStreamRef.current) {
        ctx.sessionStore.updateStreaming(sid, ctx.accumulatedStreamRef.current, ctx.provider);
        ctx.sessionStore.finalizeStreaming(sid);
      }
      ctx.accumulatedStreamRef.current = '';

      ctx.setIsLoading(false);
      ctx.setCanAbortSession(false);
      ctx.setClaudeStatus(null);
      ctx.setPendingPermissionRequests([]);
      ctx.onSessionInactive?.(sid);
      ctx.onSessionNotProcessing?.(sid);

      if (msg.aborted) break;

      const actualSessionId =
        typeof msg.actualSessionId === 'string' && msg.actualSessionId.trim().length > 0
          ? msg.actualSessionId
          : null;
      const pendingSessionId =
        typeof sessionStorage !== 'undefined'
          ? sessionStorage.getItem('pendingSessionId')
          : null;
      const completedSuccessfully = msg.exitCode === undefined || msg.exitCode === 0;
      const isVisibleSession = Boolean(
        sid
        && (
          sid === ctx.activeViewSessionId
          || sid === pendingSessionId
          || ctx.pendingViewSessionRef.current?.sessionId === sid
        ),
      );

      if (actualSessionId && sid && actualSessionId !== sid) {
        ctx.sessionStore.replaceSessionId(sid, actualSessionId);

        if (isVisibleSession) {
          ctx.setCurrentSessionId(actualSessionId);

          if (ctx.pendingViewSessionRef.current) {
            const pendingSession = ctx.pendingViewSessionRef.current.sessionId;
            if (!pendingSession || pendingSession === sid) {
              ctx.pendingViewSessionRef.current.sessionId = actualSessionId;
            }
          }
        }

        if (completedSuccessfully && pendingSessionId === sid && typeof sessionStorage !== 'undefined') {
          sessionStorage.removeItem('pendingSessionId');
        }

        if (isVisibleSession) {
          ctx.onNavigateToSession?.(actualSessionId, { replace: true });
          if (ctx.refreshProjects) {
            setTimeout(() => { void ctx.refreshProjects?.(); }, 500);
          }
        }
        break;
      }

      if (pendingSessionId && !ctx.currentSessionId && completedSuccessfully) {
        const resolvedSessionId = actualSessionId || pendingSessionId;
        ctx.setCurrentSessionId(resolvedSessionId);
        if (actualSessionId) {
          ctx.onNavigateToSession?.(resolvedSessionId, { replace: true });
        }
        if (typeof sessionStorage !== 'undefined') {
          sessionStorage.removeItem('pendingSessionId');
        }
        if (ctx.refreshProjects) {
          setTimeout(() => { void ctx.refreshProjects?.(); }, 500);
        }
      }
      break;
    }

    case 'error': {
      ctx.setIsLoading(false);
      ctx.setCanAbortSession(false);
      ctx.setClaudeStatus(null);
      ctx.onSessionInactive?.(sid);
      ctx.onSessionNotProcessing?.(sid);
      break;
    }

    case 'permission_request': {
      const requestId = msg.requestId;
      if (!requestId) break;
      ctx.setPendingPermissionRequests((prev) => {
        if (prev.some((r: PendingPermissionRequest) => r.requestId === requestId)) return prev;
        return [...prev, {
          requestId,
          toolName: msg.toolName || 'UnknownTool',
          input: msg.input,
          context: msg.context,
          sessionId: sid || null,
          receivedAt: new Date(),
        }];
      });
      ctx.setIsLoading(true);
      ctx.setCanAbortSession(true);
      ctx.setClaudeStatus({ text: 'Waiting for permission', tokens: 0, can_interrupt: true });
      break;
    }

    case 'permission_cancelled': {
      if (msg.requestId) {
        ctx.setPendingPermissionRequests((prev) =>
          prev.filter((r: PendingPermissionRequest) => r.requestId !== msg.requestId),
        );
      }
      break;
    }

    case 'status': {
      if (msg.text === 'token_budget' && msg.tokenBudget) {
        ctx.setTokenBudget(msg.tokenBudget as Record<string, unknown>);
      } else if (msg.text) {
        ctx.setClaudeStatus({
          text: msg.text,
          tokens: msg.tokens || 0,
          can_interrupt: msg.canInterrupt !== undefined ? msg.canInterrupt : true,
        });
        ctx.setIsLoading(true);
        ctx.setCanAbortSession(msg.canInterrupt !== false);
      }
      break;
    }

    default:
      break;
  }
}

/** Drain buffered WS frames through the processor (mirrors hook loop). */
export function drainAndProcessRealtimeFrames(
  frames: Array<{ seq: number; message: RealtimeChatMessage }>,
  ctx: ChatRealtimeProcessorContext,
  lastProcessedSeq: number,
): number {
  let nextSeq = lastProcessedSeq;
  for (const frame of frames) {
    processChatRealtimeMessage(frame.message, ctx);
    nextSeq = Math.max(nextSeq, frame.seq);
  }
  return nextSeq;
}
