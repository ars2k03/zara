type ListeningIndicatorProps = {
  active: boolean;
};

export function ListeningIndicator({ active }: ListeningIndicatorProps) {
  return (
    <div className={`listening-indicator${active ? " is-active" : ""}`} aria-hidden={!active}>
      <span />
      <span />
      <span />
    </div>
  );
}
