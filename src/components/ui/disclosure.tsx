import { useState, type ReactNode } from "react";
import { ChevronRight } from "./icons";
import { cx } from "@/lib/cx";
import styles from "./disclosure.module.css";

/**
 * 折叠区（替代 <details> / <summary>）。
 *
 * 原生 summary 的三角是浏览器画的，位置、大小、颜色都不可控，
 * 而且它的默认焦点框和整个界面的聚焦环不是一套。
 * 这里用 button 实现：结构一样简单，但箭头、间距、焦点都归令牌管。
 */
export function Disclosure({
  summary,
  children,
  defaultOpen = false,
  className,
}: {
  summary: ReactNode;
  children: ReactNode;
  defaultOpen?: boolean;
  className?: string;
}) {
  const [open, setOpen] = useState(defaultOpen);

  return (
    <div className={cx(styles.box, className)}>
      <button
        type="button"
        className={styles.head}
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <ChevronRight size={10} className={styles.caret} />
        <span className={styles.label}>{summary}</span>
      </button>
      {open && <div className={styles.body}>{children}</div>}
    </div>
  );
}
