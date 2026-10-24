import type { ReactNode } from "react";
import { ArrowDown, ArrowUp, Check, Trash } from "@/components/ui/icons";
import { cx } from "@/lib/cx";
import shared from "@/components/ui/shared.module.css";
import styles from "./library.module.css";

/** 单行文本字段 */
export function Field({
  label,
  value,
  placeholder,
  onChange,
  wide,
}: {
  label: string;
  value: string;
  placeholder?: string;
  onChange: (value: string) => void;
  wide?: boolean;
}) {
  return (
    <label className={cx(shared.fld, wide && shared.wide)}>
      <span className={shared.fldK}>{label}</span>
      <input
        value={value}
        placeholder={placeholder}
        spellCheck={false}
        onChange={(event) => onChange(event.target.value)}
      />
    </label>
  );
}

/** 三列字段容器 */
export function Grid({ children }: { children: ReactNode }) {
  return <div className={shared.fldGrid}>{children}</div>;
}

/** 面板头：标题 + 说明 + 右侧新增按钮 */
export function PaneHead({
  title,
  desc,
  action,
}: {
  title: string;
  desc: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className={styles.paneHead}>
      <div>
        <b>{title}</b>
        <span>{desc}</span>
      </div>
      {action}
    </div>
  );
}

export interface ItemActions {
  onUp: () => void;
  onDown: () => void;
  onRemove: () => void;
  first: boolean;
  last: boolean;
}

/** 条目头：序号 + 标题 + 备注 + 右侧（本页勾选 / 上移下移删除） */
export function ItemHead({
  index,
  title,
  note,
  pick,
  actions,
}: {
  index: number;
  title: string;
  note?: string;
  pick?: ReactNode;
  actions: ItemActions;
}) {
  return (
    <div className={styles.itemHead}>
      <span className={styles.idx}>{String(index + 1).padStart(2, "0")}</span>
      <span className={styles.title}>{title || "未命名"}</span>
      {note && <span className={styles.note}>{note}</span>}
      <span className={styles.end}>
        {pick}
        <span className={styles.acts}>
          <button type="button" aria-label="上移" disabled={actions.first} onClick={actions.onUp}>
            <ArrowUp size={10} />
          </button>
          <button type="button" aria-label="下移" disabled={actions.last} onClick={actions.onDown}>
            <ArrowDown size={10} />
          </button>
          <button type="button" aria-label="删除" className={styles.rm} onClick={actions.onRemove}>
            <Trash size={10} />
          </button>
        </span>
      </span>
    </div>
  );
}

/** 「本页」勾选：只在有当前页时出现 */
export function PagePick({
  checked,
  onChange,
  label = "本页",
}: {
  checked: boolean;
  onChange: () => void;
  label?: string;
}) {
  return (
    <button
      type="button"
      className={cx(styles.pick, checked && styles.on)}
      aria-pressed={checked}
      onClick={onChange}
    >
      <span className={styles.tick}>{checked && <Check size={8} />}</span>
      {label}
    </button>
  );
}
