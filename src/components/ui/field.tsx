import { useState, type InputHTMLAttributes, type ReactNode, type TextareaHTMLAttributes } from "react";
import { cx } from "@/lib/cx";
import { Eye, EyeOff } from "./icons";
import styles from "./field.module.css";

/**
 * 表单字段：标签 / 控件 / 说明 三段竖排。
 *
 * 之前是每个调用方自己拼 `<label className={shared.fld}><span className={shared.fldK}>…`，
 * 48 处各写一遍，于是标签字距、控件高度、说明颜色三样东西在不同面板里各不相同。
 * 收成组件后，「一个字段长什么样」只有这一个答案。
 *
 * Field 渲染成 <label>：点标签即聚焦控件，不需要再挂 htmlFor。
 */
export function Field({
  label,
  hint,
  wide,
  className,
  children,
}: {
  label?: ReactNode;
  /** 控件下方的小字说明：为什么填、留空会怎样 */
  hint?: ReactNode;
  /** 在栅格里占满整行 */
  wide?: boolean;
  className?: string;
  children: ReactNode;
}) {
  return (
    <label className={cx(styles.field, wide && styles.wide, className)}>
      {label && <span className={styles.key}>{label}</span>}
      {children}
      {hint && <span className={styles.hint}>{hint}</span>}
    </label>
  );
}

export function Input({ className, ...rest }: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={cx(styles.ctrl, styles.input, className)} {...rest} />;
}

export function Textarea({
  className,
  rows = 3,
  ...rest
}: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea rows={rows} className={cx(styles.ctrl, styles.area, className)} {...rest} />;
}

/**
 * 密钥输入：默认遮住，右侧一个眼睛切换。
 *
 * 为什么必须有这个开关：密钥是一长串看不出对错的字符，粘贴错了只能靠「数一数」
 * 判断；而把它一直明文显示，旁边的人一眼就能记下来。
 */
export function PasswordInput({
  className,
  ...rest
}: InputHTMLAttributes<HTMLInputElement>) {
  const [shown, setShown] = useState(false);
  return (
    <span className={styles.pwd}>
      <input
        {...rest}
        type={shown ? "text" : "password"}
        className={cx(styles.ctrl, styles.input, styles.pwdInput, className)}
      />
      <button
        type="button"
        className={styles.eye}
        aria-label={shown ? "隐藏密钥" : "显示密钥"}
        title={shown ? "隐藏密钥" : "显示密钥"}
        onClick={() => setShown((value) => !value)}
      >
        {shown ? <EyeOff size={12} /> : <Eye size={12} />}
      </button>
    </span>
  );
}

/** 只读展示用的值（上下文窗口这类不可改的字段） */
export function ReadonlyValue({ children }: { children: ReactNode }) {
  return <span className={styles.readonly}>{children}</span>;
}
