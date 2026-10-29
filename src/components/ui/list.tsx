import type { ReactNode } from "react";
import { Plus } from "./icons";
import { cx } from "@/lib/cx";
import styles from "./list.module.css";

/**
 * 列表与列表行（唯一实现）。
 *
 * 为什么要有它：知识库的库/条目列、模型清单、模型配置都长成「一行可选项目」，
 * 早先各自拼 div + 自己的 CSS，结果行高、缩进、动作按钮出现时机各不相同，
 * 谁想调一次行高都得改三处。这里只定死三件事：
 *
 *   1. 一行 26px 高、左右 6px 内边距、圆角 4 —— 和主页面侧栏的行同一把尺子
 *   2. 悬停染 `--bg-hover`，选中染 `--acc-soft`，不加边框和分隔线
 *   3. 行宽跟着列宽走，不自己撑满外层容器
 *
 * 分隔线、外框这类「卡片感」的东西一概不要：这几个列表是列内的连续项，
 * 不是一组浮起来的卡片。
 */
export function List({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cx(styles.list, className)}>{children}</div>;
}

export interface ListRowProps {
  /** 行首槽位：勾、图标。不传则整行从标题开始 */
  leading?: ReactNode;
  title: ReactNode;
  /** 标题右侧的次要信息（条数、网关、用途） */
  meta?: ReactNode;
  active?: boolean;
  disabled?: boolean;
  onSelect?: () => void;
  /** 行尾动作按钮（齿轮 / 删除）。常显，不做 hover 才出现 —— 藏起来等于没有 */
  actions?: ReactNode;
  className?: string;
}

export function ListRow({
  leading,
  title,
  meta,
  active,
  disabled,
  onSelect,
  actions,
  className,
}: ListRowProps) {
  return (
    <div className={cx(styles.row, active && styles.rowOn, disabled && styles.off, className)}>
      {leading && <span className={styles.lead}>{leading}</span>}
      {/* 主体是按钮：整行可点，且键盘能 Tab 到 */}
      <button type="button" className={styles.main} disabled={disabled} onClick={onSelect}>
        <span className={styles.name}>{title}</span>
        {meta !== undefined && <span className={styles.meta}>{meta}</span>}
      </button>
      {actions && <span className={styles.acts}>{actions}</span>}
    </div>
  );
}

/**
 * 「新建」行：每个列表的第一项。
 *
 * 放首项而不是列头图标：图标看不出是新建，而新建是这些列唯一的产出动作，
 * 值得占一行；放末尾又要滚到底才看得见。
 */
export function ListAdd({
  label,
  onClick,
  disabled,
  className,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <button
      type="button"
      className={cx(styles.add, className)}
      disabled={disabled}
      onClick={onClick}
    >
      <Plus size={10} />
      <span>{label}</span>
    </button>
  );
}

export function ListEmpty({ children }: { children: ReactNode }) {
  return <span className={styles.empty}>{children}</span>;
}
