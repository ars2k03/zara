import { useEffect, useRef, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";

import {
  envStatus,
  llmReply,
  onSttFinal,
  onSttInterim,
  onSttLevel,
  sttStart,
  sttStop,
  ttsSpeak,
  ttsStop,
  type ChatMessage,
  type EnvStatus,
} from "../lib/tauri";

export type VoicePhase =
  | "boot"
  | "missing-keys"
  | "greeting"
  | "listening"
  | "thinking"
  | "speaking"
  | "error";

const GREETING = "হ্যালো, আমি জারা। বলুন, কীভাবে সাহায্য করতে পারি?";
const END_OF_SPEECH_MS = 2000;
const POST_GREETING_WAIT_MS = 1000;
const VOICE_RMS_THRESHOLD = 0.015;

function sleep(ms: number, signal: AbortSignal) {
  return new Promise<void>((resolve, reject) => {
    if (signal.aborted) {
      reject(new DOMException("Aborted", "AbortError"));
      return;
    }
    const timer = window.setTimeout(() => resolve(), ms);
    signal.addEventListener(
      "abort",
      () => {
        window.clearTimeout(timer);
        reject(new DOMException("Aborted", "AbortError"));
      },
      { once: true },
    );
  });
}

async function listenOnce(
  signal: AbortSignal,
  onInterim: (text: string) => void,
  onFinal: (text: string) => void,
) {
  let interim = "";
  let finals = "";
  let heardSpeech = false;
  let lastArmedText = "";
  let silenceTimer: number | null = null;
  let settled = false;

  // VAD is transcript-driven: 2s after the last *new* text, auto-submit.
  // RMS must not touch this timer — ambient noise was resetting it forever.
  const onTranscriptActivity = (text: string) => {
    const trimmed = text.trim();
    if (!trimmed || trimmed === lastArmedText) return;
    lastArmedText = trimmed;
    heardSpeech = true;
    if (silenceTimer !== null) window.clearTimeout(silenceTimer);
    silenceTimer = window.setTimeout(() => {
      void settle(false);
    }, END_OF_SPEECH_MS);
  };

  const unlisteners: UnlistenFn[] = [
    await onSttInterim((text) => {
      interim = text;
      onInterim(text);
      onTranscriptActivity(`${finals} ${text}`.trim());
    }),
    await onSttFinal((text) => {
      finals = `${finals} ${text}`.trim();
      interim = "";
      onFinal(finals);
      onTranscriptActivity(finals);
    }),
    await onSttLevel((rms) => {
      if (rms > VOICE_RMS_THRESHOLD) {
        heardSpeech = true;
      }
    }),
  ];

  let resolveTranscript!: (value: string) => void;
  let rejectTranscript!: (reason?: unknown) => void;
  const transcriptPromise = new Promise<string>((resolve, reject) => {
    resolveTranscript = resolve;
    rejectTranscript = reject;
  });

  const settle = (aborted: boolean) => {
    if (settled) return;
    settled = true;
    if (silenceTimer !== null) window.clearTimeout(silenceTimer);
    signal.removeEventListener("abort", onAbort);
    void (async () => {
      try {
        const stopped = (await sttStop()).trim();
        if (aborted) {
          rejectTranscript(new DOMException("Aborted", "AbortError"));
          return;
        }
        resolveTranscript(stopped || finals || interim.trim());
      } catch (error) {
        rejectTranscript(aborted ? new DOMException("Aborted", "AbortError") : error);
      }
    })();
  };

  const onAbort = () => {
    settle(true);
  };

  try {
    await sttStart();
    if (signal.aborted) {
      settle(true);
    } else {
      signal.addEventListener("abort", onAbort, { once: true });
    }

    return await transcriptPromise.then((transcript) => ({ transcript, heardSpeech }));
  } finally {
    unlisteners.forEach((unlisten) => unlisten());
  }
}

export function useVoiceSession() {
  const [phase, setPhase] = useState<VoicePhase>("boot");
  const [status, setStatus] = useState<EnvStatus | null>(null);
  const [interim, setInterim] = useState("");
  const [finalText, setFinalText] = useState("");
  const [reply, setReply] = useState("");
  const [error, setError] = useState<string | null>(null);

  const historyRef = useRef<ChatMessage[]>([]);
  const generationRef = useRef(0);

  useEffect(() => {
    const controller = new AbortController();
    const generation = ++generationRef.current;
    const live = () => generation === generationRef.current && !controller.signal.aborted;

    const run = async () => {
      try {
        const env = await envStatus();
        if (!live()) return;
        setStatus(env);
        if (!env.all_set) {
          setPhase("missing-keys");
          return;
        }

        setPhase("greeting");
        setReply("");
        await ttsSpeak(GREETING, () => {
          if (live()) setReply(GREETING);
        });
        if (!live()) return;

        await sleep(POST_GREETING_WAIT_MS, controller.signal);

        while (live()) {
          setPhase("listening");
          setInterim("");
          setFinalText("");
          setReply("");
          setError(null);

          const { transcript, heardSpeech } = await listenOnce(
            controller.signal,
            (text) => {
              if (live()) setInterim(text);
            },
            (text) => {
              if (live()) {
                setFinalText(text);
                setInterim("");
              }
            },
          );

          if (!live()) break;
          if (!heardSpeech || !transcript) continue;

          setFinalText(transcript);
          setPhase("thinking");

          const nextHistory: ChatMessage[] = [
            ...historyRef.current,
            { role: "user", content: transcript },
          ];
          const answer = await llmReply(nextHistory);
          if (!live()) break;

          historyRef.current = [...nextHistory, { role: "assistant", content: answer }];
          setPhase("speaking");
          setReply("");
          await ttsSpeak(answer, () => {
            if (live()) setReply(answer);
          });
        }
      } catch (cause) {
        if (!live()) return;
        if (cause instanceof DOMException && cause.name === "AbortError") return;
        const message = cause instanceof Error ? cause.message : String(cause);
        setError(message);
        setPhase("error");
      }
    };

    void run();

    return () => {
      controller.abort();
      void ttsStop();
      generationRef.current += 1;
    };
  }, []);

  return { phase, status, interim, finalText, reply, error };
}
