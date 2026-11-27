// The × the floating microphone key is dropped on to hide it: a small ink
// circle at the bottom centre of the screen, shown by the app only while the
// key is being dragged. Clicks go through it. The app sets `data-over` on the
// page while the key is over it, and the × turns red.

export default function DropTarget() {
  return (
    <div className="mic-target-stage">
      <div className="mic-target">
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <path d="M3.5 3.5 12.5 12.5 M12.5 3.5 3.5 12.5" />
        </svg>
        <span className="mic-target-name">×</span>
      </div>
      <style>{TARGET_CSS}</style>
    </div>
  );
}

// The key's ink palette whatever the system's theme, like the key itself.
const TARGET_CSS = `
.mic-target-stage {
  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  user-select: none;
  -webkit-user-select: none;
}
.mic-target {
  width: 44px;
  height: 44px;
  border-radius: 50%;
  border: 1px solid oklch(0.34 0.008 65);
  background: oklch(0.19 0.007 65 / 0.92);
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: 0 2px 8px oklch(0.1 0.01 60 / 0.35);
  transition: transform 140ms ease-out, border-color 140ms ease-out;
}
.mic-target path {
  fill: none;
  stroke: oklch(0.93 0.008 75 / 0.8);
  stroke-width: 1.5;
  stroke-linecap: round;
  transition: stroke 140ms ease-out;
}
.mic-target-name {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip-path: inset(50%);
}
:root[data-over] .mic-target {
  border-color: oklch(0.63 0.2 29);
  transform: scale(1.12);
}
:root[data-over] .mic-target path { stroke: oklch(0.63 0.2 29); }
@media (prefers-reduced-motion: reduce) {
  .mic-target, .mic-target path { transition: none; }
  :root[data-over] .mic-target { transform: none; }
}
`;
