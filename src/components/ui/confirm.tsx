import { Modal } from "./modal";
import { Button } from "./button";
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
  // 关着就不挂载：不注册 ESC、不抢焦点、不拦截滚轮
  if (!open) return null;

  return (
    <Modal
      title={title}
      subtitle={description}
      onCancel={onCancel}
      footer={
        <>
          {/* 危险操作默认停在「取消」上：回车不该替用户做决定 */}
          <Button size="lg" onClick={onCancel} data-autofocus>
            {cancelLabel}
          </Button>
          <Button size="lg" variant={danger ? "danger" : "primary"} onClick={onConfirm}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      {items && items.length > 0 ? (
        <ul className={styles.items}>
          {items.map((item, index) => (
            <li key={`${index}-${item}`}>{item}</li>
          ))}
        </ul>
      ) : null}
    </Modal>
  );
}
