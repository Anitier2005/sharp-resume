import type { ReactNode } from "react";
import { Check } from "./icons";
import { cx } from "@/lib/cx";
import styles from "./list.module.css";

/**
 * 列表与列表行。
 *
 * 「一组可选项目」在应用里出现了很多次（模型清单、材料列表、页面清单），
 * 每次都由调用方自己拼 div + 分隔线 + 选中底色，结果行高、缩进、勾的位置
 * 各不相同。这里只定三件事：发丝线分隔、选中行整行染底色 + 右侧勾、
 * 右侧动作区不参与点击。
 */
export function List({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cx(styles.list, className)}>{children}</div>;
}

export interface ListRowProps {
  title: ReactNode;
  /** 标题右侧的次要信息（来源、页数、大小） */
  meta?: ReactNode;
  active?: boolean;
  disabled?: boolean;
  onSelect?: () => void;
  /** 行尾的动作按钮（编辑 / 删除）。不传则整行都是点击区 */
  actions?: ReactNode;
  className?: string;
}

export function ListRow({
  title,
  meta,
  active,
  disabled,
  onSelect,
  actions,
  className,
}: ListRowProps) {
  return (
    <div
      className={cx(styles.row, active && styles.rowOn, disabled && styles.off, className)}
    >
      {/* 主体是按钮：整行可点，且键盘能 Tab 到 */}
      <button type="button" className={styles.main} disabled={disabled} onClick={onSelect}>
        <span className={styles.name}>{title}</span>
        {meta && <span className={styles.meta}>{meta}</span>}
      </button>
      {active && <Check size={10} className={styles.check} />}
      {actions && <div className={styles.acts}>{actions}</div>}
    </div>
  );
}

export function ListEmpty({ children }: { children: ReactNode }) {
  return <span className={styles.empty}>{children}</span>;
}
