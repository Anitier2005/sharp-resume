import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cx } from "@/lib/cx";
import styles from "./button.module.css";

/**
 * 按钮。
 *
 * 为什么要收成一个组件：之前 `.btnGhost` / `.btnPrimary` 是两份全局类，
 * 各处的 <button> 自己决定要不要挂、挂哪一个，于是同一个界面里出现了
 * 26px 和 22px 两种高度、三种圆角、还有干脆不带类名的裸按钮。视觉上的
 * 「不对劲」几乎都来自这种逐个手写的偏差，而不是设计问题。
 *
 * 四个变体是按**语义**分的，不是按颜色分的：
 *   primary  —— 这一屏的主动作，一屏最多一个；
 *   ghost    —— 有边框的次级动作（取消、稍后再说）；
 *   quiet    —— 没有边框的图标/文字动作（行尾编辑删除、顶栏小按钮）；
 *   danger   —— 会造成数据损失的确认动作。
 */
export type ButtonVariant = "primary" | "ghost" | "quiet" | "danger";
export type ButtonSize = "xs" | "sm" | "md";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** 只有一个图标：做成正方形，宽高一致（否则图标会被 padding 推偏） */
  icon?: boolean;
  /** 文字左侧的图标 */
  leading?: ReactNode;
}

export function Button({
  variant = "ghost",
  size = "md",
  icon,
  leading,
  className,
  type = "button",
  children,
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      className={cx(styles.btn, styles[variant], styles[size], icon && styles.iconOnly, className)}
      {...rest}
    >
      {leading}
      {children}
    </button>
  );
}
