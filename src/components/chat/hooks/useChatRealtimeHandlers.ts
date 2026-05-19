import { useEffect, useRef } from 'react';
import type { Dispatch, MutableRefObject, SetStateAction } from 'react';

import { usePaletteOps } from '../../../contexts/PaletteOpsContext';
import type { PendingPermissionRequest, SessionNavigationOptions } from '../types/types';
import type { ProjectSession, LLMProvider } from '../../../types/app';
import type { SessionStore } from '../../../stores/useSessionStore';
import {
  drainAndProcessRealtimeFrames,
  type RealtimeChatMessage,
} from './chatRealtimeProcessor';

type PendingViewSession = {
  sessionId: string | null;
  startedAt: number;
};

interface UseChatRealtimeHandlersArgs {
  /** Increments once per inbound WS frame so rapid stream_delta frames are not dropped. */
  messageSeq: number;
  latestMessage: RealtimeChatMessage | null;
  drainMessagesSince: (lastSeq: number) => Array<{ seq: number; message: RealtimeChatMessage }>;
  provider: LLMProvider;
  selectedSession: ProjectSession | null;
  currentSessionId: string | null;
  setCurrentSessionId: (sessionId: string | null) => void;
  setIsLoading: (loading: boolean) => void;
  setCanAbortSession: (canAbort: boolean) => void;
  setClaudeStatus: (status: { text: string; tokens: number; can_interrupt: boolean } | null) => void;
  setTokenBudget: (budget: Record<string, unknown> | null) => void;
  setPendingPermissionRequests: Dispatch<SetStateAction<PendingPermissionRequest[]>>;
  pendingViewSessionRef: MutableRefObject<PendingViewSession | null>;
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
}

export function useChatRealtimeHandlers({
  messageSeq,
  latestMessage,
  drainMessagesSince,
  provider,
  selectedSession,
  currentSessionId,
  setCurrentSessionId,
  setIsLoading,
  setCanAbortSession,
  setClaudeStatus,
  setTokenBudget,
  setPendingPermissionRequests,
  pendingViewSessionRef,
  streamTimerRef,
  accumulatedStreamRef,
  thinkingTimerRef,
  accumulatedThinkingRef,
  onSessionInactive,
  onSessionProcessing,
  onSessionNotProcessing,
  onNavigateToSession,
  onWebSocketReconnect,
  sessionStore,
}: UseChatRealtimeHandlersArgs) {
  const paletteOps = usePaletteOps();
  const lastProcessedSeqRef = useRef(0);

  useEffect(() => {
    if (messageSeq === 0) return;
    if (messageSeq <= lastProcessedSeqRef.current && !latestMessage) return;

    const frames = drainMessagesSince(lastProcessedSeqRef.current);
    if (frames.length === 0 && latestMessage) {
      frames.push({ seq: messageSeq, message: latestMessage });
    }
    if (frames.length === 0) return;

    const activeViewSessionId =
      selectedSession?.id || currentSessionId || pendingViewSessionRef.current?.sessionId || null;

    lastProcessedSeqRef.current = drainAndProcessRealtimeFrames(
      frames,
      {
        provider,
        selectedSession,
        currentSessionId,
        activeViewSessionId,
        setCurrentSessionId,
        setIsLoading,
        setCanAbortSession,
        setClaudeStatus,
        setTokenBudget,
        setPendingPermissionRequests,
        pendingViewSessionRef,
        streamTimerRef,
        accumulatedStreamRef,
        thinkingTimerRef,
        accumulatedThinkingRef,
        onSessionInactive,
        onSessionProcessing,
        onSessionNotProcessing,
        onNavigateToSession,
        onWebSocketReconnect,
        sessionStore,
        refreshProjects: () => paletteOps.refreshProjects(),
      },
      lastProcessedSeqRef.current,
    );
  }, [
    messageSeq,
    latestMessage,
    drainMessagesSince,
    provider,
    selectedSession,
    currentSessionId,
    setCurrentSessionId,
    setIsLoading,
    setCanAbortSession,
    setClaudeStatus,
    setTokenBudget,
    setPendingPermissionRequests,
    pendingViewSessionRef,
    streamTimerRef,
    accumulatedStreamRef,
    thinkingTimerRef,
    accumulatedThinkingRef,
    onSessionInactive,
    onSessionProcessing,
    onSessionNotProcessing,
    onNavigateToSession,
    onWebSocketReconnect,
    sessionStore,
    paletteOps,
  ]);
}
