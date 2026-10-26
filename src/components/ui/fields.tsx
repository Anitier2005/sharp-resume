import { useEffect, useRef, useState } from "react";
import { Close, Grip, Plus } from "@/components/ui/icons";
import type { Field } from "@/types";
import { uid } from "@/lib/uid";
import { cx } from "@/lib/cx";
import styles from "./fields.module.css";

/**
 * 字段列表编辑器：全应用只有这一套。
 *
 * 「人名有什么信息」「一段经历有哪些要点」「页头写什么」在数据结构上是同一件事 ——
 * 一组用户自己命名的键值对。所以不为它们各写一套表单，预置字段也就无从谈起。
 *
 * 交互约定（用户定的）：
 *   - 顺序靠左侧拖拽把手调整，不用上下箭头（箭头占位置、还容易点错）
 *   - 值是多行：随内容自动换行长高，不出现原生 resize 的斜线角标
 *   - 想再高点就拖下边框——整条下边框都能拖，不是只有右下角
 */
export function FieldsEditor({
  fields,
  onChange,
  addLabel = "加一个字段",
  placeholderLabel = "字段名",
  placeholderValue = "内容",
}: {
  fields: Field[];
  onChange: (next: Field[]) => void;
  addLabel?: string;
  placeholderLabel?: string;
  placeholderValue?: string;
}) {
  const listRef = useRef<HTMLDivElement | null>(null);
  /** 正在拖动的那一行；null 表示没在拖 */
  const [dragging, setDragging] = useState<string | null>(null);

  const set = (id: string, patch: Partial<Field>) =>
    onChange(fields.map((field) => (field.id === id ? { ...field, ...patch } : field)));

  /** 指针拖动换位：按指针落在哪一行决定插到哪 */
  const startDrag = (event: React.PointerEvent, id: string) => {
    event.preventDefault();
    setDragging(id);
    const from = fields.findIndex((field) => field.id === id);

    const onMove = (ev: PointerEvent) => {
      const rows = Array.from(listRef.current?.querySelectorAll<HTMLElement>("[data-row]") ?? []);
      const over = rows.findIndex((row) => {
        const box = row.getBoundingClientRect();
        return ev.clientY >= box.top && ev.clientY <= box.bottom;
      });
      if (over < 0 || over === from) return;
      const next = [...fields];
      const [moved] = next.splice(from, 1);
      next.splice(over, 0, moved);
      onChange(next);
    };
    const onUp = () => {
      setDragging(null);
      document.removeEventListener("pointermove", onMove);
      document.removeEventListener("pointerup", onUp);
    };
    document.addEventListener("pointermove", onMove);
    document.addEventListener("pointerup", onUp);
  };

  return (
    <div className={styles.list} ref={listRef}>
      {fields.length === 0 && <span className={styles.empty}>还没有字段。想记什么就加什么。</span>}

      {fields.map((field) => (
        <FieldRow
          key={field.id}
          field={field}
          dragging={dragging === field.id}
          onDragStart={(event) => startDrag(event, field.id)}
          onChange={(patch) => set(field.id, patch)}
          onRemove={() => onChange(fields.filter((item) => item.id !== field.id))}
          placeholderLabel={placeholderLabel}
          placeholderValue={placeholderValue}
        />
      ))}

      <button
        type="button"
        className={styles.add}
        onClick={() => onChange([...fields, { id: uid("f"), label: "", value: "" }])}
      >
        <Plus size={9} /> {addLabel}
      </button>
    </div>
  );
}

function FieldRow({
  field,
  dragging,
  onDragStart,
  onChange,
  onRemove,
  placeholderLabel,
  placeholderValue,
}: {
  field: Field;
  dragging: boolean;
  onDragStart: (event: React.PointerEvent) => void;
  onChange: (patch: Partial<Field>) => void;
  onRemove: () => void;
  placeholderLabel: string;
  placeholderValue: string;
}) {
  const areaRef = useRef<HTMLTextAreaElement | null>(null);
  /** 用户拖出来的高度；null 表示完全跟着内容走 */
  const [floor, setFloor] = useState<number | null>(null);

  // 内容或手拖高度变化时重算：先归零再读 scrollHeight，否则不会缩回去
  useEffect(() => {
    const el = areaRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.max(floor ?? 0, el.scrollHeight)}px`;
  }, [field.value, floor]);

  /** 拖整条下边框改高度 */
  const startResize = (event: React.PointerEvent) => {
    event.preventDefault();
    event.stopPropagation();
    const el = areaRef.current;
    if (!el) return;
    const startY = event.clientY;
    const startH = el.offsetHeight;

    const onMove = (ev: PointerEvent) => {
      setFloor(Math.max(28, startH + (ev.clientY - startY)));
    };
    const onUp = () => {
      document.removeEventListener("pointermove", onMove);
      document.removeEventListener("pointerup", onUp);
    };
    document.addEventListener("pointermove", onMove);
    document.addEventListener("pointerup", onUp);
  };

  return (
    <div className={cx(styles.row, dragging && styles.rowDragging)} data-row={field.id}>
      <span
        className={styles.handle}
        role="button"
        tabIndex={-1}
        aria-label="拖动调整顺序"
        title="拖动调整顺序"
        onPointerDown={onDragStart}
      >
        <Grip size={11} />
      </span>

      <input
        className={styles.label}
        value={field.label}
        placeholder={placeholderLabel}
        spellCheck={false}
        aria-label="字段名"
        onChange={(event) => onChange({ label: event.target.value })}
      />

      <span className={styles.valueWrap}>
        <textarea
          ref={areaRef}
          className={styles.value}
          rows={1}
          value={field.value}
          placeholder={placeholderValue}
          spellCheck={false}
          aria-label="字段值"
          onChange={(event) => onChange({ value: event.target.value })}
        />
        <span
          className={styles.resizer}
          role="separator"
          aria-label="拖动调整这一格的高度"
          onPointerDown={startResize}
        />
      </span>

      <button type="button" className={styles.rm} aria-label="删除字段" onClick={onRemove}>
        <Close size={9} />
      </button>
    </div>
  );
}
