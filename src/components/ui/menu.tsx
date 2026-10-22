import {
  cloneElement,
  isValidElement,
  useEffect,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
} from "react";

interface DropdownProps {
  trigger: ReactElement<{ onClick?: () => void; "aria-expanded"?: boolean }>;
  children: ReactNode;
  align?: "start" | "end";
  width?: number;
}

export function Dropdown({ trigger, children, align = "end", width }: DropdownProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

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
    <div className="dd" ref={ref}>
      {node}
      {open && (
        <div
          className={`dd-menu ${align}`}
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
      className={`menu-item${danger ? " danger" : ""}`}
      onClick={onSelect}
    >
      {icon && <span className="mi">{icon}</span>}
      <span className="ml">{label}</span>
      {hint && <span className="mh">{hint}</span>}
    </button>
  );
}

export function MenuLabel({ children }: { children: ReactNode }) {
  return <div className="menu-label">{children}</div>;
}

export function MenuSep() {
  return <div className="menu-sep" />;
}
