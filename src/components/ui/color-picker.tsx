import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { hexToHsv, hsvToHex, isHexColor, type Hsv } from "@/lib/color";
import { cx } from "@/lib/cx";
import styles from "./color-picker.module.css";

/**
 * 自绘取色器。
 *
 * 之前用 `<input type="color">`，弹出的是系统色板 —— 它不可样式化，
 * 深浅主题下都长 Windows 的样子，和整个界面不是一个语言。
 *
 * 内部状态刻意用 HSV 而不是 HEX：从 HEX 反推，灰色（s=0）的色相信息已经丢了，
 * 用户把色相条转一圈再拖回来会跳成红色。所以 h/s/v 是这里的真相，
 * 只在两处转成 HEX：提交给 onChange、以及回显到输入框。
 *
 * 浮层走 portal + fixed 定位：设置面板 `.box` 是 overflow:hidden，
 * 就地渲染会被裁掉半截。位置按触发按钮的 rect 算，越界翻转。
 */
export interface ColorPickerProps {
  /** #rrggbb（带 #），由调用方持有 */
  value: string;
  onChange: (hex: string) => void;
  /** 触发色块的无障碍名 */
  label?: string;
}

const POPOVER_W = 232;
const POPOVER_H = 214;

export function ColorPicker({ value, onChange, label = "自定义颜色" }: ColorPickerProps) {
  const [open, setOpen] = useState(false);
  /** HSV 是内部真相；打开那一刻从 value 播种，之后只在提交时转回 HEX */
  const [hsv, setHsv] = useState<Hsv>({ h: 220, s: 70, v: 70 });
  const [hexDraft, setHexDraft] = useState(value);
  const anchorRef = useRef<HTMLButtonElement | null>(null);
  /** 浮层位置：打开时按按钮 rect 算一次就够 —— 拖色板期间按钮不会动 */
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const svRef = useRef<HTMLDivElement | null>(null);
  const hueRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    setHsv((prev) => {
      const next = hexToHsv(value);
      // value 是外部真相，但只在打开那一刻对齐一次；拖动中外部变化不追
      return prev && next ? prev : (next ?? prev);
    });
    setHexDraft(value);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // ESC 关闭。捕获阶段拦下，不让设置面板自己的 ESC 监听跟着关两层
  useEffect(() => {
    if (!open) return;
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      setOpen(false);
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [open]);

  // 点击浮层与触发块之外就关。用 mousedown 而不是 click：按住的瞬间就该有反馈
  useEffect(() => {
    if (!open) return;
    const onDown = (event: MouseEvent) => {
      const target = event.target as Node;
      if (anchorRef.current?.contains(target)) return;
      if (svRef.current?.contains(target) || hueRef.current?.contains(target)) return;
      // 浮层本体（含 HEX 输入）也不算外面
      const pop = document.querySelector<HTMLElement>(`[data-color-pop]`);
      if (pop?.contains(target)) return;
      setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  useLayoutEffect(() => {
    if (!open) {
      setPos(null);
      return;
    }
    const rect = anchorRef.current?.getBoundingClientRect();
    if (!rect) return;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    // 先贴按钮左缘、放按钮下方；右边放不下就右对齐按钮；下面放不下翻到上面
    let left = rect.left;
    if (left + POPOVER_W > vw - 8) left = rect.right - POPOVER_W;
    left = Math.max(8, left);
    let top = rect.bottom + 6;
    if (top + POPOVER_H > vh - 8) top = rect.top - POPOVER_H - 6;
    setPos({ left: Math.round(left), top: Math.round(Math.max(8, top)) });
  }, [open]);

  const commit = (next: Hsv) => {
    setHsv(next);
    const hex = hsvToHex(next.h, next.s, next.v);
    setHexDraft(hex);
    onChange(hex);
  };

  /** 在一条「轨道」上按指针位置取 0–1 的比例。拖出去也要能继续算（capture 住） */
  const track = (el: HTMLElement, event: PointerEvent | React.PointerEvent) => {
    const rect = el.getBoundingClientRect();
    return {
      x: Math.min(1, Math.max(0, ((event as PointerEvent).clientX - rect.left) / rect.width)),
      y: Math.min(1, Math.max(0, ((event as PointerEvent).clientY - rect.top) / rect.height)),
    };
  };

  const dragSv = (event: React.PointerEvent) => {
    const el = svRef.current;
    if (!el) return;
    el.setPointerCapture(event.pointerId);
    const apply = (e: PointerEvent | React.PointerEvent) => {
      const { x, y } = track(el, e);
      commit({ ...hsv, s: Math.round(x * 100), v: Math.round((1 - y) * 100) });
    };
    apply(event);
    const onMove = (e: PointerEvent) => apply(e);
    const onUp = () => {
      el.removeEventListener("pointermove", onMove);
      el.removeEventListener("pointerup", onUp);
      el.removeEventListener("pointercancel", onUp);
    };
    el.addEventListener("pointermove", onMove);
    el.addEventListener("pointerup", onUp);
    el.addEventListener("pointercancel", onUp);
  };

  const dragHue = (event: React.PointerEvent) => {
    const el = hueRef.current;
    if (!el) return;
    el.setPointerCapture(event.pointerId);
    const apply = (e: PointerEvent | React.PointerEvent) => {
      const { x } = track(el, e);
      commit({ ...hsv, h: Math.round(x * 360) });
    };
    apply(event);
    const onMove = (e: PointerEvent) => apply(e);
    const onUp = () => {
      el.removeEventListener("pointermove", onMove);
      el.removeEventListener("pointerup", onUp);
      el.removeEventListener("pointercancel", onUp);
    };
    el.addEventListener("pointermove", onMove);
    el.addEventListener("pointerup", onUp);
    el.addEventListener("pointercancel", onUp);
  };

  const submitHex = () => {
    if (!isHexColor(hexDraft)) {
      // 输错了就回显成当前色，不静默吞 —— 用户得知道刚才那个值没生效
      setHexDraft(hsvToHex(hsv.h, hsv.s, hsv.v));
      return;
    }
    const next = hexToHsv(hexDraft);
    if (next) commit(next);
  };

  const host = typeof document === "undefined" ? null : document.querySelector<HTMLElement>(".win");

  return (
    <>
      <button
        ref={anchorRef}
        type="button"
        className={cx(styles.swatch, open && styles.swatchOn)}
        style={{ background: value }}
        aria-label={label}
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      />
      {open &&
        host &&
        createPortal(
          <div
            data-color-pop
            className={styles.pop}
            style={{ left: pos?.left ?? -9999, top: pos?.top ?? -9999 }}
          >
            <div
              ref={svRef}
              className={styles.sv}
              style={{ ["--h" as string]: hsv.h }}
              onPointerDown={dragSv}
            >
              <span
                className={styles.knob}
                style={{ left: `${hsv.s}%`, top: `${100 - hsv.v}%` }}
              />
            </div>
            <div ref={hueRef} className={styles.hue} onPointerDown={dragHue}>
              <span className={styles.knob} style={{ left: `${(hsv.h / 360) * 100}%` }} />
            </div>
            <div className={styles.row}>
              <span className={styles.hash}>#</span>
              <input
                className={styles.hex}
                value={hexDraft.replace(/^#/, "")}
                spellCheck={false}
                maxLength={6}
                aria-label="颜色十六进制值"
                onChange={(event) => setHexDraft(`#${event.target.value.replace(/[^0-9a-fA-F]/g, "")}`)}
                onBlur={submitHex}
                onKeyDown={(event) => {
                  if (event.key === "Enter") submitHex();
                }}
              />
            </div>
          </div>,
          host,
        )}
    </>
  );
}
