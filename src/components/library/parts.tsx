import type { ReactNode } from "react";
import { ArrowDown, ArrowUp, Check, Trash } from "@/components/ui/icons";
import { Button, Field as KitField, Input, Tooltip } from "@/components/ui";
import { cx } from "@/lib/cx";
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
  // 走组件库的 Field / Input：字段高度、聚焦底色、占位符颜色都只在一处定义
  return (
    <KitField label={label} wide={wide}>
      <Input
        value={value}
        placeholder={placeholder}
        spellCheck={false}
        onChange={(event) => onChange(event.target.value)}
      />
    </KitField>
  );
}

/** 三列字段容器 */
export function Grid({ children }: { children: ReactNode }) {
  return <div className={styles.grid}>{children}</div>;
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
          <Tooltip label="上移">
            <Button
              variant="quiet"
              size="xs"
              icon
              aria-label="上移"
              disabled={actions.first}
              onClick={actions.onUp}
            >
              <ArrowUp size={10} />
            </Button>
          </Tooltip>
          <Tooltip label="下移">
            <Button
              variant="quiet"
              size="xs"
              icon
              aria-label="下移"
              disabled={actions.last}
              onClick={actions.onDown}
            >
              <ArrowDown size={10} />
            </Button>
          </Tooltip>
          <Tooltip label="删除这一条">
            <Button
              variant="quiet"
              size="xs"
              icon
              aria-label="删除"
              className={styles.rm}
              onClick={actions.onRemove}
            >
              <Trash size={10} />
            </Button>
          </Tooltip>
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
