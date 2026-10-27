import {
  cloneElement,
  isValidElement,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
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
  /** 菜单项可能很长（页签折叠、模型列表）时给个上限，超出就滚 */
  maxHeight?: number;
  /** 受控开合：页签折叠菜单要让位给右键菜单时，调用方得能从外面把它收起来 */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}

export function Dropdown({
  trigger,
  children,
  align = "end",
  width,
  maxHeight,
  open: controlled,
  onOpenChange,
}: DropdownProps) {
  const [inner, setInner] = useState(false);
  const open = controlled ?? inner;
  const setOpen = useCallback(
    (next: boolean) => {
      if (controlled === undefined) setInner(next);
      onOpenChange?.(next);
    },
    [controlled, onOpenChange],
  );
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
  }, [open, setOpen]);

  const node = isValidElement(trigger)
    ? cloneElement(trigger, { onClick: () => setOpen(!open), "aria-expanded": open })
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
          style={{
            ...(width ? { minWidth: width } : null),
            ...(maxHeight ? { maxHeight, overflowY: "auto" } : null),
          }}
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
  /** 不可用的项保留在原位，让用户看得到「为什么不行」（比如「至少要留一页」） */
  disabled?: boolean;
  onSelect: () => void;
  onContextMenu?: (event: ReactMouseEvent<HTMLButtonElement>) => void;
}

export function MenuItem({
  label,
  hint,
  icon,
  danger,
  disabled,
  onSelect,
  onContextMenu,
}: MenuItemProps) {
  return (
    <button
      type="button"
      role="menuitem"
      className={cx(styles.item, danger && styles.danger)}
      disabled={disabled}
      onClick={onSelect}
      onContextMenu={onContextMenu}
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
