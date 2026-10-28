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

/** 三横线：页签装不下时的折叠入口。
 *  原来是三点（⋯），和「更多操作」同义、容易被当成菜单按钮；
 *  这里要表达的是「还有其他页排成一列」，横线更像一个折叠起来的列表。 */
export const More = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.9, ...p })}>
    <path d="M4 7h16M4 12h16M4 17h16" />
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

/** 图片：字段里插图的入口，也是「这一项是图不是字」的标记 */
export const Image = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.8, ...p })}>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <circle cx="8.5" cy="9.5" r="1.5" />
    <path d="m4 17 4.5-4.5 3 3L15 11l5 5" />
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

/* 面板显隐：外框 + 一条分割线，表示左/右栏 */
export const PanelLeft = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.6, ...p })}>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M9 4v16" />
  </svg>
);
export const PanelRight = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.6, ...p })}>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M15 4v16" />
  </svg>
);

/* 链接：链环 */
export const Link = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.6, ...p })}>
    <path d="M10 13.5a4 4 0 0 0 5.66 0l2.34-2.34a4 4 0 1 0-5.66-5.66l-1 1" />
    <path d="M14 10.5a4 4 0 0 0-5.66 0L6 12.84a4 4 0 1 0 5.66 5.66l1-1" />
  </svg>
);

/* 拖拽把手：两列小点 */
export const Grip = (p: IconProps) => (
  <svg {...base({ strokeWidth: 0, ...p })}>
    <g fill="currentColor" stroke="none">
      <circle cx="9" cy="6" r="1.1" />
      <circle cx="15" cy="6" r="1.1" />
      <circle cx="9" cy="12" r="1.1" />
      <circle cx="15" cy="12" r="1.1" />
      <circle cx="9" cy="18" r="1.1" />
      <circle cx="15" cy="18" r="1.1" />
    </g>
  </svg>
);

/** 知识库：带书脊的书。
 *  之前是「打开的书」（左右两页），12px 下只剩一个圆角方框，看不出是书。 */
export const BookOpen = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.9, ...p })}>
    <path d="M6.5 3.5H17a2.5 2.5 0 0 1 2.5 2.5v14.5H6.5A2.5 2.5 0 0 1 4 18V6a2.5 2.5 0 0 1 2.5-2.5Z" />
    <path d="M8.5 3.5v17" />
  </svg>
);

/** 设置入口：两根带滑块的横线（调节杆）。
 *  原本画的是齿轮，但 12px 下八个齿会被糊成一个「太阳」，反而看不出是设置。 */
export const Gear = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.9, ...p })}>
    <path d="M3.5 8.5h8M17.5 8.5h3M3.5 15.5h3M12.5 15.5h8" />
    <circle cx="14" cy="8.5" r="2.4" />
    <circle cx="9" cy="15.5" r="2.4" />
  </svg>
);

/** 模型（AI）入口：单颗四角星。多颗星在 12px 下会糊成一片噪点 */
export const Sparkles = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.9, ...p })}>
    <path d="M12 3.5l2 5.6 5.6 2-5.6 2-2 5.6-2-5.6-5.6-2 5.6-2z" />
  </svg>
);

/** 外观：半填充的圆（明暗对比）。调色盘上的几个孔在 12px 下会变成脏点 */
export const Palette = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.9, ...p })}>
    <circle cx="12" cy="12" r="8" />
    <path d="M12 4a8 8 0 0 1 0 16z" fill="currentColor" stroke="none" />
  </svg>
);

/** 返回：详情页左上角。与 ChevronRight 成对，方向语义不能靠 transform 混用 */
export const ChevronLeft = (p: IconProps) => (
  <svg {...base({ strokeWidth: 1.9, ...p })}>
    <path d="M14.5 5.5 8 12l6.5 6.5" />
  </svg>
);
