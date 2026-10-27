import { useEffect, useRef } from "react";
import { cx } from "@/lib/cx";
import shared from "./shared.module.css";
import styles from "./confirm.module.css";

/**
 * 悬浮确认窗：给不可撤销的操作兜一层。
 *
 * 为什么做成「受控浮层」而不是浏览器 `confirm()`：
 *   1. `confirm()` 会阻塞整个渲染进程，流式对话、保存节拍全停；
 *   2. 它长得不像这个应用，用户下意识的「确定」不代表真的看清了要删什么；
 *   3. 需要能一次列出多条（agent 的批量写入），`confirm()` 只有一句话。
 *
 * 刻意不提供全局单例 API：谁要问、问什么、确认后做什么，都由调用方决定，
 * 这样「一次只允许一个确认窗在途」就成了调用方（provider）能看见的普通状态。
 */
export interface ConfirmProps {
  open: boolean;
  title: string;
  description?: string;
  /** 将被删除的具体对象，一条一行。空则不显示列表 */
  items?: string[];
  confirmLabel?: string;
  cancelLabel?: string;
  /** 删除类操作用红色主按钮；普通确认用主色 */
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export function Confirm({
  open,
  title,
  description,
  items,
  confirmLabel = "删除",
  cancelLabel = "取消",
  danger = true,
  onConfirm,
  onCancel,
}: ConfirmProps) {
  // Escape 只在开着的时候接管：关着的确认窗不该吃掉别处的按键
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onCancel]);

  const veilRef = useRef<HTMLDivElement | null>(null);

  /**
   * 滚轮也要拦住。
   *
   * 遮罩能挡点击，但挡不住滚轮——弹窗开着时滚一下，底下的纸面/会话照滚，
   * 用户会以为「确认窗跟着内容动了」。列表本身要能滚，所以只拦列表之外。
   * 用原生 addEventListener 而不是 onWheel：React 给 wheel 挂的是 passive，
   * preventDefault 在里面无效。
   */
  useEffect(() => {
    if (!open) return;
    const veil = veilRef.current;
    if (!veil) return;
    const onWheel = (event: WheelEvent) => {
      const scrollable = veil.querySelector<HTMLElement>("[data-scroll]");
      if (scrollable?.contains(event.target as Node)) return;
      event.preventDefault();
    };
    veil.addEventListener("wheel", onWheel, { passive: false });
    return () => veil.removeEventListener("wheel", onWheel);
  }, [open]);

  if (!open) return null;

  /**
   * Tab 也要关在窗里。
   *
   * 遮罩挡得住鼠标，挡不住键盘 —— 不拦的话 Tab 会把焦点带到下面那张纸上，
   * 用户回车就改到了自己没看见的东西。
   */
  const keepFocus = (event: React.KeyboardEvent) => {
    if (event.key !== "Tab") return;
    const nodes = veilRef.current?.querySelectorAll<HTMLElement>("button");
    if (!nodes?.length) return;
    const first = nodes[0];
    const last = nodes[nodes.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <div
      ref={veilRef}
      className={cx(shared.veil, styles.veil)}
      data-print="hide"
      onMouseDown={(event) => event.target === event.currentTarget && onCancel()}
      onKeyDown={keepFocus}
    >
      <div
        className={styles.box}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        data-selectable
      >
        <div className={styles.title}>{title}</div>
        {description && <p className={styles.desc}>{description}</p>}
        {items && items.length > 0 && (
          <ul className={styles.items} data-scroll>
            {items.map((item, index) => (
              <li key={`${index}-${item}`}>{item}</li>
            ))}
          </ul>
        )}
        <div className={styles.acts}>
          <button type="button" onClick={onCancel} autoFocus>
            {cancelLabel}
          </button>
          <button
            type="button"
            className={danger ? styles.danger : styles.primary}
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
