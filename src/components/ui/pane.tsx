import type { CSSProperties, ReactNode } from "react";
import { cx } from "@/lib/cx";
import styles from "./pane.module.css";

/**
 * 分栏容器（知识库、模型配置共用）。
 *
 * 抽出来的理由不是少写几行：列宽、边框、滚动容器、列头字号这些东西
 * 一旦各写一份，就会出现「知识库能拖、模型配置拖不动」「两边列头不一样大」
 * 这种只有用户才发现的不一致。这里只定义结构，宽度交给调用方。
 */
export function Columns({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cx(styles.cols, className)}>{children}</div>;
}

export function Pane({
  children,
  width,
  grow,
  bordered,
  hidden,
  className,
  style,
}: {
  children: ReactNode;
  /** 固定宽度（拖拽手柄会改这个值）；不给就是按 flex 分配 */
  width?: number;
  /** 吃掉剩余宽度 */
  grow?: boolean;
  /** 右边界线：分栏之间的「界」 */
  bordered?: boolean;
  hidden?: boolean;
  className?: string;
  style?: CSSProperties;
}) {
  return (
    <section
      className={cx(styles.pane, bordered && styles.bordered, className)}
      style={{
        width,
        flex: grow ? 1 : width === undefined ? undefined : "none",
        display: hidden ? "none" : undefined,
        ...style,
      }}
    >
      {children}
    </section>
  );
}

/** 列头：一个小标题 + 右侧可选说明/动作 */
export function PaneHead({
  title,
  note,
  actions,
  children,
}: {
  title?: ReactNode;
  note?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className={styles.head}>
      {title !== undefined && <span className={styles.title}>{title}</span>}
      {children}
      {note !== undefined && <span className={styles.note}>{note}</span>}
      {actions && <span className={styles.actions}>{actions}</span>}
    </div>
  );
}

/** 列里的滚动区：列表、字段、编辑器都放这里 */
export function PaneScroll({
  children,
  pad,
  className,
}: {
  children: ReactNode;
  /** 内边距：列表列通常 2–4px，字段列 10px */
  pad?: number;
  className?: string;
}) {
  return (
    <div className={cx(styles.scroll, className)} style={pad === undefined ? undefined : { padding: pad }}>
      {children}
    </div>
  );
}

/** 列底：状态 + 一个动作（如知识库的「向量化」） */
export function PaneFoot({ children }: { children: ReactNode }) {
  return <div className={styles.foot}>{children}</div>;
}
