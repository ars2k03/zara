import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface KeyStatus {
  name: string;
  set: boolean;
}

export interface EnvStatus {
  all_set: boolean;
  keys: KeyStatus[];
}

export interface ChatMessage {
  role: "user" | "assistant" | "system";
  content: string;
}

export interface ToolActivityEvent {
  name: string;
  ok: boolean | null;
  summary: string | null;
}

export interface ToolConfirmRequest {
  id: string;
  title: string;
  detail: string;
}

export interface TimerFiredEvent {
  id: string;
  label: string;
  seconds: number;
}

export function envStatus() {
  return invoke<EnvStatus>("env_status");
}

export function sttStart() {
  return invoke<void>("stt_start");
}

export function sttStop() {
  return invoke<string>("stt_stop");
}

export function llmReply(messages: ChatMessage[]) {
  return invoke<string>("llm_reply", { messages });
}

export function toolConfirmResponse(id: string, approved: boolean) {
  return invoke<void>("tool_confirm_response", { id, approved });
}

export async function ttsSpeak(text: string, onStart?: () => void) {
  const unlisten = onStart ? await onTtsStart(onStart) : null;
  try {
    await invoke<void>("tts_speak", { text });
  } finally {
    unlisten?.();
  }
}

export function ttsStop() {
  return invoke<void>("tts_stop");
}

export function onTtsStart(handler: () => void): Promise<UnlistenFn> {
  return listen("tts://start", () => handler());
}

export function onSttInterim(handler: (text: string) => void): Promise<UnlistenFn> {
  return listen<string>("stt://interim", (event) => handler(event.payload));
}

export function onSttFinal(handler: (text: string) => void): Promise<UnlistenFn> {
  return listen<string>("stt://final", (event) => handler(event.payload));
}

export function onSttLevel(handler: (rms: number) => void): Promise<UnlistenFn> {
  return listen<number>("stt://level", (event) => handler(event.payload));
}

export function onToolStart(handler: (event: ToolActivityEvent) => void): Promise<UnlistenFn> {
  return listen<ToolActivityEvent>("tool://start", (event) => handler(event.payload));
}

export function onToolFinish(handler: (event: ToolActivityEvent) => void): Promise<UnlistenFn> {
  return listen<ToolActivityEvent>("tool://finish", (event) => handler(event.payload));
}

export function onToolConfirm(handler: (req: ToolConfirmRequest) => void): Promise<UnlistenFn> {
  return listen<ToolConfirmRequest>("tool://confirm", (event) => handler(event.payload));
}

export function onTimerFired(handler: (event: TimerFiredEvent) => void): Promise<UnlistenFn> {
  return listen<TimerFiredEvent>("timer://fired", (event) => handler(event.payload));
}
