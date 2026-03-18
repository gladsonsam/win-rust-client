import { useEffect, useRef, useCallback } from "react";
import type { WsEvent } from "../lib/types";

export type WsStatus = "connecting" | "connected" | "disconnected";

interface Options {
  onMessage: (ev: WsEvent) => void;
  onStatusChange: (s: WsStatus) => void;
}

export function useWebSocket({ onMessage, onStatusChange }: Options) {
  const wsRef = useRef<WebSocket | null>(null);
  const retryTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Stable refs so the connect closure doesn't stale-close over callbacks.
  const msgCbRef = useRef(onMessage);
  const statusCbRef = useRef(onStatusChange);
  msgCbRef.current = onMessage;
  statusCbRef.current = onStatusChange;

  const connect = useCallback(() => {
    const proto = location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(`${proto}://${location.host}/ws/view`);
    wsRef.current = ws;

    statusCbRef.current("connecting");

    ws.onopen = () => {
      statusCbRef.current("connected");
      if (retryTimer.current) {
        clearTimeout(retryTimer.current);
        retryTimer.current = null;
      }
    };

    ws.onmessage = (e: MessageEvent<string>) => {
      try {
        const raw = JSON.parse(e.data) as Record<string, unknown>;
        // The WS viewer sends `event` for its own messages (init).
        // Agent broadcasts use `type`.  Normalise so all consumers use `ev.event`.
        if (!raw.event && raw.type) raw.event = raw.type;
        msgCbRef.current(raw as WsEvent);
      } catch {
        /* ignore malformed */
      }
    };

    ws.onclose = () => {
      statusCbRef.current("disconnected");
      retryTimer.current = setTimeout(connect, 3000);
    };

    ws.onerror = () => ws.close();
  }, []); // intentionally empty — we use refs for callbacks

  const send = useCallback((data: unknown) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(JSON.stringify(data));
    }
  }, []);

  useEffect(() => {
    connect();
    return () => {
      if (retryTimer.current) clearTimeout(retryTimer.current);
      wsRef.current?.close();
    };
  }, [connect]);

  return { send };
}
