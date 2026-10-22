import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement> & { size?: number };

function base({ size = 12, ...rest }: IconProps) {
  return {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    ...rest,
  };
}

export const Plus = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.8, ...p })}>
    <path d="M12 5v14M5 12h14" />
  </svg>
);

export const Search = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.8, ...p })}>
    <circle cx="11" cy="11" r="7" />
    <path d="m20 20-3.6-3.6" />
  </svg>
);

export const User = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.7, ...p })}>
    <circle cx="12" cy="8" r="3.4" />
    <path d="M5 20c0-3.6 3.1-5.6 7-5.6s7 2 7 5.6" />
  </svg>
);

export const ChevronRight = (p: IconProps) => (
  <svg {...base({ strokeWidth: 2, ...p })}>
    <path d="m9 6 6 6-6 6" />
  </svg>
);

export const ChevronDown = (p: IconProps) => (
  <svg {...base({ strokeWidth: 2, ...p })}>
    <path d="m6 9 6 6 6-6" />
  </svg>
);

export const FileIcon = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.8, ...p })}>
    <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8Z" />
    <path d="M14 3v5h5" />
  </svg>
);

export const Check = (p: IconProps) => (
  <svg {...base({ strokeWidth: 2, ...p })}>
    <path d="m5 13 4 4L19 7" />
  </svg>
);

export const ArrowUp = (p: IconProps) => (
  <svg {...base({ strokeWidth: 2.2, ...p })}>
    <path d="M12 19V5" />
    <path d="m6 11 6-6 6 6" />
  </svg>
);

export const ArrowDown = (p: IconProps) => (
  <svg {...base({ strokeWidth: 2.2, ...p })}>
    <path d="M12 5v14" />
    <path d="m6 13 6 6 6-6" />
  </svg>
);

export const Close = (p: IconProps) => (
  <svg {...base({ strokeWidth: 2, ...p })}>
    <path d="M6 6l12 12M18 6L6 18" />
  </svg>
);

export const Trash = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.7, ...p })}>
    <path d="M4 7h16" />
    <path d="M9 7V5h6v2" />
    <path d="M6 7l1 13h10l1-13" />
  </svg>
);

export const Pencil = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.7, ...p })}>
    <path d="M4 20h4l10-10a2.8 2.8 0 0 0-4-4L4 16Z" />
  </svg>
);

export const Layers = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.7, ...p })}>
    <path d="m12 3 9 5-9 5-9-5 9-5Z" />
    <path d="m3 13 9 5 9-5" />
  </svg>
);

export const Download = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.8, ...p })}>
    <path d="M12 4v10" />
    <path d="m7 10 5 5 5-5" />
    <path d="M5 19h14" />
  </svg>
);

/* 窗口控制：与设计稿一致的 12×12 细线 */
export const WinMin = () => (
  <svg width="10" height="10" viewBox="0 0 12 12">
    <path d="M2 6h8" stroke="currentColor" strokeWidth="1" />
  </svg>
);
export const WinMax = () => (
  <svg width="10" height="10" viewBox="0 0 12 12">
    <rect x="2.5" y="2.5" width="7" height="7" fill="none" stroke="currentColor" strokeWidth="1" />
  </svg>
);
export const WinClose = () => (
  <svg width="10" height="10" viewBox="0 0 12 12">
    <path d="M3 3l6 6M9 3l-6 6" stroke="currentColor" strokeWidth="1" />
  </svg>
);
