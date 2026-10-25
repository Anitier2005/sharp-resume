/**
 * 应用标识：一枚「纸」的抽象 —— 圆角方块 + 两条文字行。
 * 用令牌上色（--acc / --acc-fg），所以跟着主题与自定义强调色走。
 */
export function Logo({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" aria-hidden="true" focusable="false">
      <rect x="1.25" y="1.25" width="13.5" height="13.5" rx="3.6" fill="var(--acc)" />
      <path
        d="M5.1 6.1h5.8M5.1 9.6h3.5"
        stroke="var(--acc-fg)"
        strokeWidth="1.5"
        strokeLinecap="round"
        opacity="0.92"
      />
    </svg>
  );
}
