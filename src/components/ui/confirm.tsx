import { useEffect } from "react";
import shared from "./shared.module.css";
import styles from "./confirm.module.css";

/**
 * 确认框：给不可撤销的操作兜一层。
 *
 * 不做通用 confirm 系统 —— 目前只有「删除候选人」这一处需要，
 * 写成受控组件由调用方决定文案，比全局 API 更好读。
 */
export function Confirm({
  title,
  desc,
  confirmLabel = "删除",
  onCancel,
  onConfirm,
}: {
  title: string;
  desc: string;
  confirmLabel?: string;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onCancel]);

  return (
    <div
      className={shared.veil}
      data-print="hide"
      onMouseDown={(event) => event.target === event.currentTarget && onCancel()}
    >
      <div className={styles.box} role="dialog" aria-modal="true" aria-label={title}>
        <div className={styles.title}>{title}</div>
        <p className={styles.desc}>{desc}</p>
        <div className={styles.acts}>
          <button type="button" onClick={onCancel} autoFocus>
            取消
          </button>
          <button type="button" className={styles.danger} onClick={onConfirm}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
