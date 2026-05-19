export type WebSocketLifecycleState = {
  canConnect: () => boolean;
  onTokenEffectCleanup: () => void;
  onProviderUnmount: () => void;
};

export const createWebSocketLifecycleState = (): WebSocketLifecycleState => {
  let isUnmounted = false;
  return {
    canConnect: () => !isUnmounted,
    // Token changes should reconnect; they must not permanently block connect().
    onTokenEffectCleanup: () => {},
    onProviderUnmount: () => {
      isUnmounted = true;
    },
  };
};
