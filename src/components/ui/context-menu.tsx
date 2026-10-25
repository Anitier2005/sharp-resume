import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import styles from "./menu.module.css";

/**
 * 右键上下文菜单：定位在鼠标处，贴边时向内收。
 *
 * 和 Dropdown 的区别是「锚点」——Dropdown 挂在某个元素下面，
 * 上下文菜单挂在指针位置，所以走 fixed + 手动收敛边界。
 */
export function ContextMenu({
  x,
  y,
  onClose,
  children,
}: {
  x: number;
  y: number;
  onClose: () => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement | null>(null);
  const [pos, setPos] = useState({ x, y });

  // 挂载后量一次尺寸，把菜单收回视口内（右下角右键时不至于被裁掉）
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const pad = 8;
    setPos({
      x: Math.max(pad, Math.min(x, window.innerWidth - rect.width - pad)),
      y: Math.max(pad, Math.min(y, window.innerHeight - rect.height - pad)),
    });
  }, [x, y]);

  useEffect(() => {
    const onDown = (event: MouseEvent) => {
      if (!ref.current?.contains(event.target as Node)) onClose();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    // 拖动分隔条 / 滚动列表时菜单会飘在原地，干脆关掉
    const onScroll = () => onClose();
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    document.addEventListener("scroll", onScroll, true);
    window.addEventListener("blur", onClose);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);

  return (
    <div
      ref={ref}
      className={styles.ctx}
      style={{ left: pos.x, top: pos.y }}
      role="menu"
      onClick={onClose}
    >
      {children}
    </div>
  );
}
