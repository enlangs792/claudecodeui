import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { useAuth } from '../components/auth/context/AuthContext';
import { IS_PLATFORM } from '../constants/config';
import { appendInboundFrame, collectFramesSince } from './wsFrameBuffer';
import {
  createWebSocketLifecycleState,
  type WebSocketLifecycleState,
} from './webSocketLifecycle';

type WebSocketContextType = {
  ws: WebSocket | null;
  sendMessage: (message: any) => boolean;
  /** Monotonic counter; increments for every inbound WS frame (avoids coalescing drops). */
  messageSeq: number;
  /** Most recent inbound message (same as last element processed for this seq). */
  latestMessage: any | null;
  /** Return all inbound frames with seq greater than `lastSeq`. */
  drainMessagesSince: (lastSeq: number) => Array<{ seq: number; message: any }>;
  isConnected: boolean;
};

const WebSocketContext = createContext<WebSocketContextType | null>(null);

export const useWebSocket = () => {
  const context = useContext(WebSocketContext);
  if (!context) {
    throw new Error('useWebSocket must be used within a WebSocketProvider');
  }
  return context;
};

const buildWebSocketUrl = (token: string | null) => {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  if (IS_PLATFORM) return `${protocol}//${window.location.host}/ws`; // Platform mode: Use same domain as the page (goes through proxy)
  if (!token) return null;
  return `${protocol}//${window.location.host}/ws?token=${encodeURIComponent(token)}`; // OSS mode: Use same host:port that served the page
};

const useWebSocketProviderState = (): WebSocketContextType => {
  const wsRef = useRef<WebSocket | null>(null);
  const lifecycleRef = useRef<WebSocketLifecycleState>(createWebSocketLifecycleState());
  const hasConnectedRef = useRef(false); // Track if we've ever connected (to detect reconnects)
  const inboundFramesRef = useRef<Array<{ seq: number; message: any }>>([]);
  const seqRef = useRef(0);
  const [latestMessage, setLatestMessage] = useState<any>(null);
  const [messageSeq, setMessageSeq] = useState(0);
  const [isConnected, setIsConnected] = useState(false);
  const reconnectTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const { token } = useAuth();

  useEffect(() => {
    connect();

    return () => {
      lifecycleRef.current.onTokenEffectCleanup();
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current);
        reconnectTimeoutRef.current = null;
      }
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, [token]); // everytime token changes, we reconnect

  useEffect(() => {
    return () => {
      lifecycleRef.current.onProviderUnmount();
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current);
      }
      if (wsRef.current) {
        wsRef.current.close();
      }
    };
  }, []);

  const connect = useCallback(() => {
    if (!lifecycleRef.current.canConnect()) return; // Prevent connection if unmounted
    try {
      // Construct WebSocket URL
      const wsUrl = buildWebSocketUrl(token);

      if (!wsUrl) return console.warn('No authentication token found for WebSocket connection');
      
      const websocket = new WebSocket(wsUrl);

      websocket.onopen = () => {
        setIsConnected(true);
        wsRef.current = websocket;
        if (hasConnectedRef.current) {
          // This is a reconnect — signal so components can catch up on missed messages
          setLatestMessage({ type: 'websocket-reconnected', timestamp: Date.now() });
          setMessageSeq((seq) => seq + 1);
        }
        hasConnectedRef.current = true;
      };

      websocket.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data);
          seqRef.current += 1;
          inboundFramesRef.current = appendInboundFrame(inboundFramesRef.current, {
            seq: seqRef.current,
            message: data,
          });
          setLatestMessage(data);
          setMessageSeq(seqRef.current);
        } catch (error) {
          console.error('Error parsing WebSocket message:', error);
        }
      };

      websocket.onclose = () => {
        setIsConnected(false);
        wsRef.current = null;
        
        // Attempt to reconnect after 3 seconds
        reconnectTimeoutRef.current = setTimeout(() => {
          if (!lifecycleRef.current.canConnect()) return; // Prevent reconnection if unmounted
          connect();
        }, 3000);
      };

      websocket.onerror = (error) => {
        console.error('WebSocket error:', error);
      };

    } catch (error) {
      console.error('Error creating WebSocket connection:', error);
    }
  }, [token]); // everytime token changes, we reconnect

  const sendMessage = useCallback((message: any) => {
    const socket = wsRef.current;
    if (socket && socket.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify(message));
      return true;
    } else {
      console.warn('WebSocket not connected');
      return false;
    }
  }, []);

  const drainMessagesSince = useCallback((lastSeq: number) => {
    return collectFramesSince(inboundFramesRef.current, lastSeq);
  }, []);

  const value: WebSocketContextType = useMemo(() =>
  ({
    ws: wsRef.current,
    sendMessage,
    messageSeq,
    latestMessage,
    drainMessagesSince,
    isConnected
  }), [sendMessage, messageSeq, latestMessage, drainMessagesSince, isConnected]);

  return value;
};

export const WebSocketProvider = ({ children }: { children: React.ReactNode }) => {
  const webSocketData = useWebSocketProviderState();
  
  return (
    <WebSocketContext.Provider value={webSocketData}>
      {children}
    </WebSocketContext.Provider>
  );
};

export default WebSocketContext;
