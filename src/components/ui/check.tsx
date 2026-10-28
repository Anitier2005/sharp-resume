import { Check } from "./icons";
import { cx } from "@/lib/cx";
import styles from "./check.module.css";

/**
 * 勾选与开关。
 *
 * 两者都是「布尔值」，但语义不同，别混用：
 *   Checkbox —— 选项的一部分，通常成组出现、需要反复核对（「这个模型能看图」）；
 *   Switch   —— 一按就生效的状态（「跟随系统主题」）。
 *
 * 共同点是原生控件都只做交互、不做像素：`input` / `button` 保持可点、可聚焦、
 * 有正确的无障碍语义，视觉全部由这里的 span 画，颜色只走令牌。
 */

export function Checkbox({
  checked,
  onChange,
  disabled,
  children,
  className,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  children?: React.ReactNode;
  className?: string;
}) {
  return (
    <label className={cx(styles.row, disabled && styles.off, className)}>
      <input
        type="checkbox"
        className={styles.native}
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span className={cx(styles.box, checked && styles.boxOn)} aria-hidden>
        {checked && <Check size={9} />}
      </span>
      {children && <span className={styles.text}>{children}</span>}
    </label>
  );
}

export function Switch({
  checked,
  onChange,
  disabled,
  children,
  className,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  children?: React.ReactNode;
  className?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      className={cx(styles.row, styles.rowBtn, className)}
      onClick={() => onChange(!checked)}
    >
      <span className={cx(styles.sw, checked && styles.swOn)} aria-hidden>
        <span className={styles.knob} />
      </span>
      {children && <span className={styles.text}>{children}</span>}
    </button>
  );
}
