"use client";

import { useEffect, type ReactNode } from "react";
import { useStore } from "../store";
import { getBlockStream } from "./client";

/**
 * Owns the singleton block-stream connection for the whole app.
 * Mount once at the root layout. On mount it:
 *   1. wires `connection` + `block` events into the Zustand store
 *   2. opens the WebSocket
 *
 * On unmount it tears the connection down. Components don't talk to
 * the WS directly — they read store slices.
 */
export function RealtimeProvider({ children }: { children: ReactNode }) {
  useEffect(() => {
    const stream = getBlockStream();
    const { setConnection, applyBlock } = useStore.getState();

    const offConnection = stream.on("connection", (event) => {
      setConnection({
        state: event.state,
        lastSeenHeight: stream.getLastSeenHeight(),
      });
    });

    const offBlock = stream.on("block", (event) => {
      applyBlock(event.block);
    });

    stream.connect();

    return () => {
      offConnection();
      offBlock();
      stream.disconnect();
    };
  }, []);

  return <>{children}</>;
}
