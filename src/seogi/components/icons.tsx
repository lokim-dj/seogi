// The few line icons the screens use, drawn at 16 units on a thin stroke.

type IconProps = { className?: string };

const line = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.4,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};

function Icon({
  children,
  className,
  viewBox = "0 0 16 16",
}: IconProps & { children: React.ReactNode; viewBox?: string }) {
  return (
    <svg viewBox={viewBox} aria-hidden="true" className={className}>
      {children}
    </svg>
  );
}

export const SearchIcon = (p: IconProps) => (
  <Icon {...p}>
    <g {...line}>
      <circle cx="7" cy="7" r="4.5" />
      <path d="M10.4 10.4 13.5 13.5" />
    </g>
  </Icon>
);

export const SlidersIcon = (p: IconProps) => (
  <Icon {...p}>
    <g {...line}>
      <path d="M2.5 4.5h6M11.5 4.5h2M2.5 11.5h2M7.5 11.5h6" />
      <circle cx="10" cy="4.5" r="1.5" />
      <circle cx="6" cy="11.5" r="1.5" />
    </g>
  </Icon>
);

export const ImportIcon = (p: IconProps) => (
  <Icon {...p}>
    <g {...line}>
      <path d="M8 2.5v7M5.2 6.8 8 9.6l2.8-2.8" />
      <path d="M2.5 9.5v2.5a1.5 1.5 0 0 0 1.5 1.5h8a1.5 1.5 0 0 0 1.5-1.5V9.5" />
    </g>
  </Icon>
);

export const PlayGlyph = (p: IconProps) => (
  <Icon {...p} viewBox="0 0 10 10">
    <path d="M2 1.2v7.6L8.6 5z" fill="currentColor" />
  </Icon>
);

export const PauseGlyph = (p: IconProps) => (
  <Icon {...p} viewBox="0 0 10 10">
    <path d="M2.2 1.5h2v7h-2zM5.8 1.5h2v7h-2z" fill="currentColor" />
  </Icon>
);

export const BackIcon = (p: IconProps) => (
  <Icon {...p}>
    <path d="M10 3.5 5.5 8l4.5 4.5" {...line} strokeWidth={1.5} />
  </Icon>
);

export const DownIcon = (p: IconProps) => (
  <Icon {...p}>
    <path d="M4 6l4 4 4-4" {...line} strokeWidth={1.5} />
  </Icon>
);

export const RightIcon = (p: IconProps) => (
  <Icon {...p}>
    <path d="M6 4l4 4-4 4" {...line} strokeWidth={1.5} />
  </Icon>
);

export const FlagGlyph = (p: IconProps) => (
  <Icon {...p}>
    <g {...line}>
      <path d="M3.5 14V2.5" />
      <path d="M3.5 3h8l-1.8 3 1.8 3h-8" />
    </g>
  </Icon>
);

export const StopGlyph = (p: IconProps) => (
  <Icon {...p}>
    <rect x="4" y="4" width="8" height="8" rx="1.5" fill="currentColor" />
  </Icon>
);

export const CopyIcon = (p: IconProps) => (
  <Icon {...p}>
    <g fill="none" stroke="currentColor" strokeWidth="1.4">
      <rect x="5.5" y="5.5" width="8" height="8" rx="1.8" />
      <path d="M10.5 3.5V3a1.5 1.5 0 0 0-1.5-1.5H4A1.5 1.5 0 0 0 2.5 3v5A1.5 1.5 0 0 0 4 9.5h.5" />
    </g>
  </Icon>
);

export const MoreIcon = (p: IconProps) => (
  <Icon {...p}>
    <g fill="currentColor">
      <circle cx="3.5" cy="8" r="1.2" />
      <circle cx="8" cy="8" r="1.2" />
      <circle cx="12.5" cy="8" r="1.2" />
    </g>
  </Icon>
);

export const CloseIcon = (p: IconProps) => (
  <Icon {...p}>
    <path d="M4 4l8 8M12 4l-8 8" {...line} />
  </Icon>
);
