import { useEffect, useRef } from "react";
import type { ElementType, ClipboardEvent, FocusEvent, KeyboardEvent } from "react";

import { cn } from "@/lib/utils";

interface EditableProps {
  value: string;
  onChange: (next: string) => void;
  editing: boolean;
  as?: ElementType;
  className?: string;
  multiline?: boolean;
  ariaLabel?: string;
}

/**
 * 轻量可编辑文本块。
 * 用 ref 写入文本，避免 React children 与 contentEditable 争夺 DOM 造成光标跳动。
 */
export function Editable({
  value,
  onChange,
  editing,
  as: Component = "div",
  className,
  multiline = true,
  ariaLabel,
}: EditableProps) {
  const ref = useRef<HTMLElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (document.activeElement === el) return;
    if (el.innerText !== value) {
      el.innerText = value;
    }
  }, [value, editing]);

  const handleBlur = (event: FocusEvent<HTMLElement>) => {
    const next = event.currentTarget.innerText.replace(/\u00a0/g, " ").trim();
    if (next !== value) onChange(next);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (!multiline && event.key === "Enter") {
      event.preventDefault();
      event.currentTarget.blur();
    }
    if (event.key === "Escape") {
      event.currentTarget.blur();
    }
  };

  const handlePaste = (event: ClipboardEvent<HTMLElement>) => {
    event.preventDefault();
    const text = event.clipboardData.getData("text/plain");
    document.execCommand("insertText", false, text);
  };

  return (
    <Component
      ref={ref}
      role={editing ? "textbox" : undefined}
      aria-label={ariaLabel}
      aria-multiline={editing ? multiline : undefined}
      tabIndex={editing ? 0 : undefined}
      contentEditable={editing}
      suppressContentEditableWarning
      spellCheck={false}
      onBlur={handleBlur}
      onKeyDown={handleKeyDown}
      onPaste={handlePaste}
      className={cn(
        "whitespace-pre-wrap",
        editing && "cursor-text",
        !editing && "cursor-default",
        className,
      )}
    />
  );
}
