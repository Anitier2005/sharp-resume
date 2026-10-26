import {
  cloneElement,
  isValidElement,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
} from "react";
import { cx } from "@/lib/cx";
import styles from "./menu.module.css";

interface DropdownProps {
  trigger: ReactElement<{ onClick?: () => void; "aria-expanded"?: boolean }>;
  children: ReactNode;
  align?: "start" | "end";
  width?: number;
}

export function Dropdown({ trigger, children, align = "end", width }: DropdownProps) {
  const [open, setOpen] = useState(false);
  /** 下方放不下时改为向上弹（输入框在窗口最底部，菜单一定会顶到边） */
  const [flipUp, setFlipUp] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);

  // 展开后量一次：菜单底边超出视口就翻到上方
  useLayoutEffect(() => {
    if (!open) return;
    const el = menuRef.current;
    if (!el) return;
    const box = el.getBoundingClientRect();
    setFlipUp(box.bottom > window.innerHeight - 8);
  }, [open, children]);

  useEffect(() => {
    if (!open) return;
    const onDown = (event: MouseEvent) => {
      if (!ref.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const node = isValidElement(trigger)
    ? cloneElement(trigger, { onClick: () => setOpen((value) => !value), "aria-expanded": open })
    : trigger;

  return (
    <div className={styles.dd} ref={ref}>
      {node}
      {open && (
        <div
          ref={menuRef}
          className={cx(
            styles.menu,
            align === "end" ? styles.end : styles.start,
            flipUp && styles.up,
          )}
          role="menu"
          style={width ? { minWidth: width } : undefined}
          onClick={() => setOpen(false)}
        >
          {children}
        </div>
      )}
    </div>
  );
}

interface MenuItemProps {
  label: string;
  hint?: string;
  icon?: ReactNode;
  danger?: boolean;
  onSelect: () => void;
}

export function MenuItem({ label, hint, icon, danger, onSelect }: MenuItemProps) {
  return (
    <button
      type="button"
      role="menuitem"
      className={cx(styles.item, danger && styles.danger)}
      onClick={onSelect}
    >
      {icon && <span className={styles.mi}>{icon}</span>}
      <span className={styles.ml}>{label}</span>
      {hint && <span className={styles.mh}>{hint}</span>}
    </button>
  );
}

export function MenuLabel({ children }: { children: ReactNode }) {
  return <div className={styles.label}>{children}</div>;
}

export function MenuSep() {
  return <div className={styles.sep} />;
}
