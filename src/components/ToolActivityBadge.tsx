import type { ToolActivityEvent } from "../lib/tauri";

type ToolActivityBadgeProps = {
  activeTool: string | null;
  lastTool: ToolActivityEvent | null;
};

export function ToolActivityBadge({ activeTool, lastTool }: ToolActivityBadgeProps) {
  if (activeTool) {
    return (
      <div className="tool-badge tool-badge--active" role="status">
        <span className="tool-badge__dot" />
        <span>কাজ চলছে: {activeTool}</span>
      </div>
    );
  }

  if (!lastTool) return null;

  return (
    <div
      className={`tool-badge ${lastTool.ok === false ? "tool-badge--error" : "tool-badge--done"}`}
      role="status"
    >
      <span className="tool-badge__dot" />
      <span>
        {lastTool.name}
        {lastTool.summary ? ` — ${lastTool.summary}` : ""}
      </span>
    </div>
  );
}
