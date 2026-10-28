import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown } from "./icons";
import { placeBelow, portalHost } from "./popover";
import { cx } from "@/lib/cx";
import styles from "./select.module.css";

/**
 * 下拉选择（自绘）。
 *
 * 为什么不用原生 <select>：它的弹出层是操作系统画的，深浅主题、字体、行高
 * 全都不受控 —— 界面上其他地方都是发丝线与令牌色，一点开就是一块灰色系统菜单，
 * 这是最明显的「不一个语言」。原生下拉也塞不进「名称 + 来源 + 勾」这种行内结构。
 *
 * 键盘行为按 listbox 惯例做：↓/↑ 移动、Enter 选中、Esc 关闭、Home/End 跳首尾。
 * 焦点始终留在触发按钮上（不把焦点搬进浮层），这样 Esc 与 Tab 的行为可预测。
 */
export interface SelectOption {
  value: string;
  label: string;
  /** 右侧的次要信息（来源、维度、文件大小…） */
  meta?: string;
  disabled?: boolean;
}

export interface SelectProps {
  value: string;
  onChange: (value: string) => void;
  options: SelectOption[];
  placeholder?: string;
  ariaLabel?: string;
  size?: "sm" | "md";
  disabled?: boolean;
  /** 浮层最小宽度；默认与触发按钮同宽 */
  width?: number;
  className?: string;
}

export function Select({
  value,
  onChange,
  options,
  placeholder = "请选择",
  ariaLabel,
  size = "md",
  disabled,
  width,
  className,
}: SelectProps) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [pos, setPos] = useState<{ left: number; top: number; width: number } | null>(null);
  const anchorRef = useRef<HTMLButtonElement | null>(null);
  const popRef = useRef<HTMLDivElement | null>(null);

  const selectedIndex = options.findIndex((option) => option.value === value);
  const current = selectedIndex >= 0 ? options[selectedIndex] : undefined;

  const close = useCallback(() => {
    setOpen(false);
    setPos(null);
  }, []);

  const pick = (option: SelectOption) => {
    if (option.disabled) return;
    onChange(option.value);
    close();
  };

  const show = () => {
    setActive(selectedIndex >= 0 ? selectedIndex : 0);
    setOpen(true);
  };

  // 位置每次都重量：面板可能滚动，缓存 rect 会让浮层飘到别处
  useLayoutEffect(() => {
    if (!open) return;
    const anchor = anchorRef.current?.getBoundingClientRect();
    const pop = popRef.current;
    if (!anchor || !pop) return;
    const box = placeBelow(anchor, Math.max(anchor.width, width ?? 0), pop.offsetHeight);
    setPos({ ...box, width: Math.max(anchor.width, width ?? 0) });
  }, [open, width, options.length]);

  // 点外面收起来。用 mousedown：按下的瞬间就该有反应
  useEffect(() => {
    if (!open) return;
    const onDown = (event: MouseEvent) => {
      const target = event.target as Node;
      if (anchorRef.current?.contains(target)) return;
      if (popRef.current?.contains(target)) return;
      close();
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open, close]);

  // Esc 只关浮层，不关它所在的模态窗；滚动/改尺寸时浮层跟着锚点走不了，直接收
  useEffect(() => {
    if (!open) return;
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      close();
    };
    const onMove = () => close();
    document.addEventListener("keydown", onKey, true);
    window.addEventListener("scroll", onMove, true);
    window.addEventListener("resize", onMove);
    return () => {
      document.removeEventListener("keydown", onKey, true);
      window.removeEventListener("scroll", onMove, true);
      window.removeEventListener("resize", onMove);
    };
  }, [open, close]);

  const onKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>) => {
    if (!open) {
      if (["ArrowDown", "ArrowUp", "Enter", " "].includes(event.key)) {
        event.preventDefault();
        show();
      }
      return;
    }
    const move = (next: number) => {
      event.preventDefault();
      const clamped = Math.min(Math.max(next, 0), options.length - 1);
      setActive(clamped);
    };
    switch (event.key) {
      case "ArrowDown":
        move(active + 1);
        break;
      case "ArrowUp":
        move(active - 1);
        break;
      case "Home":
        move(0);
        break;
      case "End":
        move(options.length - 1);
        break;
      case "Enter":
      case " ":
        event.preventDefault();
        if (options[active]) pick(options[active]);
        break;
      case "Tab":
        close();
        break;
      default:
        break;
    }
  };

  return (
    <div className={cx(styles.wrap, size === "sm" && styles.sm, className)}>
      <button
        type="button"
        ref={anchorRef}
        className={styles.trigger}
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        onClick={() => (open ? close() : show())}
        onKeyDown={onKeyDown}
      >
        <span className={cx(styles.value, !current && styles.placeholder)}>
          {current?.label ?? placeholder}
        </span>
        <ChevronDown size={10} className={styles.caret} />
      </button>

      {open &&
        createPortal(
          <div
            ref={popRef}
            className={styles.pop}
            role="listbox"
            aria-label={ariaLabel}
            style={{
              width: pos?.width,
              left: pos?.left ?? 0,
              top: pos?.top ?? 0,
              // 首帧还没量出位置：先藏起来，量完再显示，避免闪一下再跳
              visibility: pos ? "visible" : "hidden",
            }}
          >
            {options.map((option, index) => {
              const on = option.value === value;
              return (
                <div
                  key={option.value}
                  role="option"
                  aria-selected={on}
                  aria-disabled={option.disabled}
                  className={cx(
                    styles.opt,
                    on && styles.optOn,
                    index === active && styles.optActive,
                  )}
                  onMouseEnter={() => setActive(index)}
                  onClick={() => pick(option)}
                >
                  <span className={styles.optLabel}>{option.label}</span>
                  {option.meta && <span className={styles.optMeta}>{option.meta}</span>}
                  {on && <Check size={10} className={styles.optCheck} />}
                </div>
              );
            })}
          </div>,
          portalHost(),
        )}
    </div>
  );
}
