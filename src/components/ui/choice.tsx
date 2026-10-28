import type { ReactNode } from "react";
import { cx } from "@/lib/cx";
import styles from "./choice.module.css";

/**
 * 「从小固定集合里选」的两种形态。
 *
 * 放在同一个文件里，是因为它们的区别只有一条：能不能多选。
 * 分段控件 = 互斥（思维强度、视图模式），筹码 = 可多可少（标签、表单多选）。
 * 两者的边距、圆角、选中态完全一致 —— 分开写就会各自漂移。
 */

/* ============================== 分段控件 ============================== */

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  /** 悬停补充说明（如「跟随系统 120%」）。不传就是没有提示 */
  title?: string;
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  ariaLabel,
  className,
}: {
  value: T;
  options: SegmentedOption<T>[];
  onChange: (value: T) => void;
  ariaLabel?: string;
  className?: string;
}) {
  return (
    <div className={cx(styles.seg, className)} role="group" aria-label={ariaLabel}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          aria-pressed={option.value === value}
          title={option.title}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

/* ============================== 筹码 ============================== */

export function Chip({
  pressed,
  onClick,
  disabled,
  title,
  children,
}: {
  pressed?: boolean;
  onClick?: () => void;
  disabled?: boolean;
  title?: string;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      // 选中态由 CSS 的 [aria-pressed="true"] 渲染：状态只有一个真相源
      className={styles.chip}
      aria-pressed={pressed}
      disabled={disabled}
      title={title}
      onClick={onClick}
    >
      {children}
    </button>
  );
}
