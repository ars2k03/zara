import { ConfirmActionModal } from "./ConfirmActionModal";
import { ListeningIndicator } from "./ListeningIndicator";
import { SpeechBubble } from "./SpeechBubble";
import { ToolActivityBadge } from "./ToolActivityBadge";
import { useToolEvents } from "../hooks/useToolEvents";
import { useVoiceSession, type VoicePhase } from "../hooks/useVoiceSession";

function phaseLabel(phase: VoicePhase) {
  switch (phase) {
    case "boot":
      return "শুরু হচ্ছে";
    case "missing-keys":
      return "কী লাগবে";
    case "greeting":
      return "শুভেচ্ছা";
    case "listening":
      return "শুনছি";
    case "thinking":
      return "ভাবছি";
    case "speaking":
      return "বলছি";
    case "error":
      return "সমস্যা";
  }
}

export function VoiceScreen() {
  const { phase, status, interim, finalText, reply, error } = useVoiceSession();
  const { activeTool, lastTool, pendingConfirm, confirmAction, firedTimers, dismissTimer } =
    useToolEvents(phase);

  const userText = `${finalText} ${interim}`.trim();
  const showUser =
    Boolean(userText) && (phase === "listening" || phase === "thinking" || phase === "speaking");
  const showAssistant = Boolean(reply) && (phase === "greeting" || phase === "speaking");

  return (
    <main className="voice-screen">
      <div className="voice-glow" />
      <header className="voice-brand">
        <img className="voice-logo" src="/logo.svg" alt="" width={72} height={72} />
        <p className="voice-kicker">ভয়েস অ্যাসিস্ট্যান্ট</p>
        <h1>Zara</h1>
        <p className="voice-phase">{phaseLabel(phase)}</p>
      </header>

      <ListeningIndicator active={phase === "listening"} />
      <ToolActivityBadge activeTool={activeTool} lastTool={lastTool} />

      {firedTimers.length > 0 && (
        <div className="timer-alerts" role="alert">
          {firedTimers.map((timer) => (
            <div key={timer.id} className="timer-alert">
              <span>সময় শেষ: {timer.label}</span>
              <button
                type="button"
                className="timer-alert__close"
                onClick={() => dismissTimer(timer.id)}
              >
                ঠিক আছে
              </button>
            </div>
          ))}
        </div>
      )}

      <section className="voice-copy" aria-live="polite">
        {phase === "missing-keys" && status && (
          <div className="voice-keys">
            <p>`.env` ফাইলে এই কীগুলো দাও, তারপর অ্যাপ রিস্টার্ট করো।</p>
            <ul>
              {status.keys.map((key) => (
                <li key={key.name} data-set={key.set}>
                  {key.name}
                </li>
              ))}
            </ul>
          </div>
        )}

        {phase === "error" && error && <p className="voice-error">{error}</p>}

        <div className="speech-stack">
          {showUser && <SpeechBubble role="user" text={userText} />}
          {showAssistant && <SpeechBubble role="assistant" text={reply} />}
        </div>
      </section>

      <ConfirmActionModal
        request={pendingConfirm}
        onRespond={(approved) => {
          void confirmAction(approved);
        }}
      />
    </main>
  );
}
