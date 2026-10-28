import type { CSSProperties } from "react";
import { cx } from "@/lib/cx";
import styles from "./slider.module.css";

/**
 * 滑杆（自绘）。
 *
 * 原生 `input[type=range]` 的轨道与滑块是浏览器画的：深浅主题下颜色不由令牌决定，
 * 而且各浏览器的滑块形状不同 —— 在一个全是发丝线的界面里，它会显得像别处的控件。
 *
 * 实现上**保留** 原生 input，只是把它做成透明的一层盖在上面：
 * 拖动、键盘（←→/Home/End）、点击跳位、无障碍语义全部是浏览器原生行为，
 * 我们只负责画轨道、进度和滑块。这比手写一套 pointer 拖拽要稳得多。
 */
export interface SliderProps {
  value: number;
  min?: number;
  max?: number;
  step?: number;
  onChange: (value: number) => void;
  ariaLabel?: string;
  disabled?: boolean;
  className?: string;
}

export function Slider({
  value,
  min = 0,
  max = 1,
  step = 0.01,
  onChange,
  ariaLabel,
  disabled,
  className,
}: SliderProps) {
  const span = max - min;
  const pct = span > 0 ? Math.min(100, Math.max(0, ((value - min) / span) * 100)) : 0;

  return (
    <div
      className={cx(styles.wrap, disabled && styles.off, className)}
      style={{ "--pct": `${pct}%` } as CSSProperties}
    >
      <div className={styles.track}>
        <div className={styles.fill} />
      </div>
      <div className={styles.thumb} />
      <input
        type="range"
        className={styles.native}
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        aria-label={ariaLabel}
        onChange={(event) => onChange(Number(event.target.value))}
      />
    </div>
  );
}
