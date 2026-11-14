type SpeechBubbleProps = {
  role: "user" | "assistant";
  text: string;
};

const LABELS = {
  user: "তুমি",
  assistant: "জারা",
} as const;

export function SpeechBubble({ role, text }: SpeechBubbleProps) {
  const trimmed = text.trim();
  if (!trimmed) return null;

  return (
    <article className={`speech-bubble speech-bubble--${role}`}>
      <p className="speech-bubble__label">{LABELS[role]}</p>
      <p className="speech-bubble__text">{trimmed}</p>
    </article>
  );
}
