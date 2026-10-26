import { Close, Plus, ArrowUp, ArrowDown } from "@/components/ui/icons";
import type { Field } from "@/types";
import { uid } from "@/lib/uid";
import styles from "./fields.module.css";

/**
 * 字段列表编辑器：全应用只有这一套。
 *
 * 「人名有什么信息」「一段经历有哪些要点」「页头写什么」在数据结构上是同一件事 ——
 * 一组用户自己命名的键值对。所以不为它们各写一套表单，预置字段也就无从谈起。
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
  const set = (id: string, patch: Partial<Field>) =>
    onChange(fields.map((field) => (field.id === id ? { ...field, ...patch } : field)));

  const move = (index: number, dir: -1 | 1) => {
    const to = index + dir;
    if (to < 0 || to >= fields.length) return;
    const next = [...fields];
    [next[index], next[to]] = [next[to], next[index]];
    onChange(next);
  };

  return (
    <div className={styles.list}>
      {fields.length === 0 && <span className={styles.empty}>还没有字段。想记什么就加什么。</span>}

      {fields.map((field, index) => (
        <div className={styles.row} key={field.id}>
          <input
            className={styles.label}
            value={field.label}
            placeholder={placeholderLabel}
            spellCheck={false}
            aria-label="字段名"
            onChange={(event) => set(field.id, { label: event.target.value })}
          />
          <textarea
            className={styles.value}
            rows={1}
            value={field.value}
            placeholder={placeholderValue}
            spellCheck={false}
            aria-label="字段值"
            onChange={(event) => set(field.id, { value: event.target.value })}
          />
          <span className={styles.acts}>
            <button type="button" aria-label="上移" disabled={index === 0} onClick={() => move(index, -1)}>
              <ArrowUp size={9} />
            </button>
            <button
              type="button"
              aria-label="下移"
              disabled={index === fields.length - 1}
              onClick={() => move(index, 1)}
            >
              <ArrowDown size={9} />
            </button>
            <button
              type="button"
              aria-label="删除字段"
              onClick={() => onChange(fields.filter((item) => item.id !== field.id))}
            >
              <Close size={9} />
            </button>
          </span>
        </div>
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
