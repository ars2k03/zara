import { useCallback, useEffect, useRef, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";

import {
  onTimerFired,
  onToolConfirm,
  onToolFinish,
  onToolStart,
  toolConfirmResponse,
  type TimerFiredEvent,
  type ToolActivityEvent,
  type ToolConfirmRequest,
} from "../lib/tauri";
import type { VoicePhase } from "./useVoiceSession";

const TOOL_BADGE_HOLD_MS = 3500;

export function useToolEvents(phase?: VoicePhase) {
  const [activeTool, setActiveTool] = useState<string | null>(null);
  const [lastTool, setLastTool] = useState<ToolActivityEvent | null>(null);
  const [pendingConfirm, setPendingConfirm] = useState<ToolConfirmRequest | null>(null);
  const [firedTimers, setFiredTimers] = useState<TimerFiredEvent[]>([]);
  const clearTimerRef = useRef<number | null>(null);

  const clearHideTimer = useCallback(() => {
    if (clearTimerRef.current !== null) {
      window.clearTimeout(clearTimerRef.current);
      clearTimerRef.current = null;
    }
  }, []);

  useEffect(() => {
    if (phase === "listening") {
      clearHideTimer();
      setLastTool(null);
    }
  }, [phase, clearHideTimer]);

  useEffect(() => {
    let disposed = false;
    const unlisteners: UnlistenFn[] = [];

    const bind = async () => {
      const subs = await Promise.all([
        onToolStart((event) => {
          if (disposed) return;
          clearHideTimer();
          setLastTool(null);
          setActiveTool(event.name);
        }),
        onToolFinish((event) => {
          if (disposed) return;
          clearHideTimer();
          setActiveTool(null);
          setLastTool(event);
          clearTimerRef.current = window.setTimeout(() => {
            setLastTool(null);
            clearTimerRef.current = null;
          }, TOOL_BADGE_HOLD_MS);
        }),
        onToolConfirm((req) => {
          if (disposed) return;
          setPendingConfirm(req);
        }),
        onTimerFired((event) => {
          if (disposed) return;
          setFiredTimers((prev) => [...prev, event]);
        }),
      ]);

      if (disposed) {
        subs.forEach((unlisten) => unlisten());
        return;
      }
      unlisteners.push(...subs);
    };

    void bind();

    return () => {
      disposed = true;
      clearHideTimer();
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [clearHideTimer]);

  const confirmAction = useCallback(
    async (approved: boolean) => {
      if (!pendingConfirm) return;
      const target = pendingConfirm;
      setPendingConfirm(null);
      await toolConfirmResponse(target.id, approved);
    },
    [pendingConfirm],
  );

  const dismissTimer = useCallback((id: string) => {
    setFiredTimers((prev) => prev.filter((item) => item.id !== id));
  }, []);

  return {
    activeTool,
    lastTool,
    pendingConfirm,
    confirmAction,
    firedTimers,
    dismissTimer,
  };
}
