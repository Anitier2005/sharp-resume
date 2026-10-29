import type { PointerEvent as ReactPointerEvent, ReactNode } from "react";
import { clamp } from "@/lib/clamp";
import styles from "./grip.module.css";

/**
 * 可拖拽的分栏手柄。
 *
 * 抽出来是因为「拖列宽」这件事在两处出现（知识库的库列/内容列、模型配置的模型列），
 * 各写一遍必然出现一边能拖、一边拖不动 —— 用户第一反应会是「这页没用同一个组件」。
 *
 * 宽度由调用方持有（它可能还要落 localStorage），这里只负责把拖拽换算成 px 差值。
 */
export function Grip({
  label,
  width,
  min,
  max,
  onChange,
  hidden,
  children,
}: {
  label: string;
  /** 拖动前的宽度，按下时取值 */
  width: number;
  min: number;
  max: number;
  onChange: (next: number) => void;
  hidden?: boolean;
  /** 可选的叠放内容（比如展开按钮），传给调用方的少，留个口子 */
  children?: ReactNode;
}) {
  const start = (event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    const startX = event.clientX;
    const base = width;
    const onMove = (move: PointerEvent) => {
      const delta = move.clientX - startX;
      onChange(clamp(base + delta, min, max));
    };
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      document.body.style.cursor = "";
    };
    document.body.style.cursor = "col-resize";
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  return (
    <div
      className={styles.grip}
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      onPointerDown={start}
      style={hidden ? { display: "none" } : undefined}
    >
      {children}
    </div>
  );
}
