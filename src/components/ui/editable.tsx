import { useEffect, useRef, type KeyboardEvent } from "react";

interface EditableProps {
  value: string;
  onChange: (value: string) => void;
  className?: string;
  placeholder?: string;
  /** 单行：Enter 提交并失焦；多行：Enter 换行 */
  multiline?: boolean;
  as?: "div" | "span";
  ariaLabel?: string;
}

/**
 * 非受控 contentEditable：只在外部值与 DOM 不一致时写入，
 * 避免每次输入都被 React 覆盖导致光标跳到末尾。
 */
export function Editable({
  value,
  onChange,
  className,
  placeholder,
  multiline = true,
  as = "div",
  ariaLabel,
}: EditableProps) {
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (el.innerText !== value) {
      el.innerText = value;
    }
  }, [value]);

  const commit = () => {
    const el = ref.current;
    if (!el) return;
    const text = el.innerText.replace(/\u00a0/g, " ").replace(/\n{3,}/g, "\n\n").trim();
    if (text !== value) onChange(text);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!multiline && event.key === "Enter") {
      event.preventDefault();
      ref.current?.blur();
    }
    if (event.key === "Escape") {
      if (ref.current) ref.current.innerText = value;
      ref.current?.blur();
    }
  };

  const Tag = as;

  return (
    <Tag
      ref={ref as never}
      role="textbox"
      aria-label={ariaLabel}
      aria-multiline={multiline}
      contentEditable
      suppressContentEditableWarning
      spellCheck={false}
      data-ph={placeholder}
      className={className}
      onBlur={commit}
      onKeyDown={onKeyDown}
      onPaste={(event) => {
        // 粘贴时只取纯文本，避免带入外部样式
        event.preventDefault();
        const text = event.clipboardData.getData("text/plain");
        document.execCommand("insertText", false, multiline ? text : text.replace(/\s+/g, " "));
      }}
    />
  );
}
