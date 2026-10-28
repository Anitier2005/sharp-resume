import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type HTMLAttributes,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { placeAbove, portalHost } from "./popover";
import styles from "./tooltip.module.css";

/**
 * 悬停提示（替代 title=""）。
 *
 * 原生 title 的问题很具体：延迟由系统定（约 1 秒，出手慢了用户以为没提示）、
 * 深浅主题下都是系统的浅黄底、字号不跟界面缩放、永远不会被截断也说不了换行。
 * 在一个 12px 字号、发丝线的界面里，它是唯一一块「不属于这个应用」的像素。
 *
 * 行为按惯例做：鼠标悬停/键盘聚焦出现，移开立刻消失，滚动与改尺寸直接收起
 * （浮层是 fixed 定位，不跟着锚点走，留着就会飘到别处）。
 */
export function Tooltip({
  label,
  children,
  delay = 320,
  className,
  ...rest
}: {
  label: ReactNode;
  children: ReactNode;
  /** 出现延迟：太小会一路扫过去闪个不停，太大像是坏了 */
  delay?: number;
  className?: string;
} & Omit<HTMLAttributes<HTMLSpanElement>, "className" | "children">) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const wrapRef = useRef<HTMLSpanElement | null>(null);
  const tipRef = useRef<HTMLDivElement | null>(null);
  const timer = useRef<number | null>(null);

  const clear = useCallback(() => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
  }, []);

  const hide = useCallback(() => {
    clear();
    setOpen(false);
    setPos(null);
  }, [clear]);

  const schedule = useCallback(() => {
    clear();
    timer.current = window.setTimeout(() => setOpen(true), delay);
  }, [clear, delay]);

  useEffect(() => clear, [clear]);

  // 先渲染再量：提示的宽度取决于文字，估不出来
  useLayoutEffect(() => {
    if (!open) return;
    const anchor = wrapRef.current?.getBoundingClientRect();
    const tip = tipRef.current;
    if (!anchor || !tip) return;
    setPos(placeAbove(anchor, tip.offsetWidth, tip.offsetHeight));
  }, [open]);

  useEffect(() => {
    if (!open) return;
    window.addEventListener("scroll", hide, true);
    window.addEventListener("resize", hide);
    return () => {
      window.removeEventListener("scroll", hide, true);
      window.removeEventListener("resize", hide);
    };
  }, [open, hide]);

  return (
    // 其余 props 全部落到外层 span 上：`Dropdown` 会用 cloneElement 给 trigger
    // 注入 onClick / aria-expanded，包在 Tooltip 外面时这些必须能传进来，
    // 否则按钮点不动（事件落在 span 上也一样能冒泡到它）。
    <span
      ref={wrapRef}
      {...rest}
      className={className ? `${styles.wrap} ${className}` : styles.wrap}
      onMouseEnter={schedule}
      onMouseLeave={hide}
      onFocus={schedule}
      onBlur={hide}
    >
      {children}
      {open &&
        createPortal(
          <div
            ref={tipRef}
            role="tooltip"
            className={styles.tip}
            style={{
              left: pos?.left ?? 0,
              top: pos?.top ?? 0,
              visibility: pos ? "visible" : "hidden",
            }}
          >
            {label}
          </div>,
          portalHost(),
        )}
    </span>
  );
}
