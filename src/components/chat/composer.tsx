import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Attachment } from "@/types";
import { ArrowUp, FileIcon, Plus } from "@/components/ui/icons";
import { Dropdown, MenuItem } from "@/components/ui/menu";
import { MODELS } from "@/lib/model-config";
import { segmentText, uniqueToken } from "@/lib/inline-tokens";
import styles from "./composer.module.css";
import { toAttachment } from "@/lib/attach";

/** 输入框高度偏好在本地记住：不同人写东西的姿势不一样 */
const HEIGHT_KEY = "sharp-resume:composer:h";
const MIN_H = 44;
const MAX_H = 420;

function readHeight(): number | null {
  const raw = localStorage.getItem(HEIGHT_KEY);
  const value = raw ? Number(raw) : NaN;
  return Number.isFinite(value) ? Math.min(MAX_H, Math.max(MIN_H, value)) : null;
}

interface ComposerProps {
  model: string;
  /** 直接切模型（底栏下拉），不用绕到设置页 */
  onPickModel: (id: string) => void;
  onOpenSettings: () => void;
  onSend: (text: string, files: Attachment[]) => void;
  busy: boolean;
  /** 输入内容由会话面板持有，建议动作可以直接写入 */
  text: string;
  onTextChange: (text: string) => void;
  /** 待发送附件由外层持有，方便拖放注入；每个附件在正文里有自己的 token */
  files: Attachment[];
  onFilesChange: (files: Attachment[]) => void;
}

export function Composer({
  model,
  onPickModel,
  onOpenSettings,
  onSend,
  busy,
  text,
  onTextChange,
  files,
  onFilesChange,
}: ComposerProps) {
  const areaRef = useRef<HTMLTextAreaElement | null>(null);
  const mirrorRef = useRef<HTMLDivElement | null>(null);
  const fileRef = useRef<HTMLInputElement | null>(null);
  /** 用户手动拖过的高度；null 表示按内容自动长高 */
  const [manual, setManual] = useState<number | null>(() => readHeight());

  // 自动长高只在不手动调过时生效
  useEffect(() => {
    const el = areaRef.current;
    if (!el || manual != null) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(220, el.scrollHeight)}px`;
  }, [text, manual]);

  // 高亮层要跟正文一起滚，否则行内标签会和文字错位
  const syncScroll = () => {
    const el = areaRef.current;
    const mirror = mirrorRef.current;
    if (el && mirror) mirror.scrollTop = el.scrollTop;
  };

  /** 拖顶部把手改高度，松手才落盘 */
  const startResize = useCallback((event: React.PointerEvent) => {
    event.preventDefault();
    const startY = event.clientY;
    const box = (event.currentTarget as HTMLElement).parentElement;
    const startH = box?.getBoundingClientRect().height ?? 96;
    let latest = startH;

    const onMove = (ev: PointerEvent) => {
      latest = Math.min(MAX_H, Math.max(MIN_H, startH + (startY - ev.clientY)));
      setManual(latest);
    };
    const onUp = () => {
      document.removeEventListener("pointermove", onMove);
      document.removeEventListener("pointerup", onUp);
      localStorage.setItem(HEIGHT_KEY, String(Math.round(latest)));
    };
    document.addEventListener("pointermove", onMove);
    document.addEventListener("pointerup", onUp);
  }, []);

  /** 双击把手恢复自动高度 */
  const resetHeight = () => {
    localStorage.removeItem(HEIGHT_KEY);
    setManual(null);
    const el = areaRef.current;
    if (el) {
      el.style.height = "auto";
      el.style.height = `${Math.min(220, el.scrollHeight)}px`;
    }
  };

  /**
   * 附件进正文：在光标处插入 `@文件名`，并把附件挂上同一个 token。
   * 不再是「顶上排一排 chip」——那样它和句子没有关系。
   */
  const addFiles = useCallback(
    (picked: Attachment[]) => {
      if (!picked.length) return;
      const el = areaRef.current;
      const start = el?.selectionStart ?? text.length;
      const end = el?.selectionEnd ?? start;

      const next = [...files];
      const tokens: string[] = [];
      for (const file of picked) {
        const token = uniqueToken(`@${file.name}`, next.map((item) => item.token));
        next.push({ ...file, token });
        tokens.push(token);
      }

      const before = text.slice(0, start);
      const after = text.slice(end);
      // 前后已有空白就不再补，免得出现两个连续空格
      const lead = before && !/\s$/.test(before) ? " " : "";
      const trail = after && /^\s/.test(after) ? "" : " ";
      const chunk = tokens.join(" ");
      const insert = `${lead}${chunk}${trail}`;

      onTextChange(`${before}${insert}${after}`);
      onFilesChange(next);

      const caret = (before + insert).length;
      requestAnimationFrame(() => {
        el?.focus();
        el?.setSelectionRange(caret, caret);
      });
    },
    [files, onFilesChange, onTextChange, text],
  );

  // 正文里删掉了某个标记 → 对应附件也不再随消息发出（这是「指代」的另一半）
  useEffect(() => {
    const stale = files.filter((file) => file.token && !text.includes(file.token));
    if (!stale.length) return;
    onFilesChange(files.filter((file) => !stale.includes(file)));
  }, [text, files, onFilesChange]);

  /** 粘贴：图片/文件一律变成行内标记；纯文本粘贴交回浏览器默认行为 */
  const onPaste = (event: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const items = Array.from(event.clipboardData?.items ?? []);
    const picked: Attachment[] = [];
    for (const item of items) {
      if (item.kind !== "file") continue;
      const file = item.getAsFile();
      if (!file) continue;
      const named =
        file.name && file.name !== "image.png"
          ? file
          : new File(
              [file],
              `粘贴-${Date.now()}.${(file.type.split("/")[1] || "png").replace("jpeg", "jpg")}`,
              { type: file.type },
            );
      picked.push(toAttachment(named));
    }
    if (!picked.length) return;
    event.preventDefault();
    addFiles(picked);
  };

  const submit = () => {
    const value = text.trim();
    if (!value && files.length === 0) return;
    if (busy) return;
    onSend(value, files);
    onTextChange("");
    onFilesChange([]);
  };

  const options = useMemo(() => MODELS.map((item) => ({ id: item.id, label: item.label })), []);
  const areaHeight = manual == null ? undefined : Math.max(MIN_H, manual - 46);
  const segments = useMemo(() => segmentText(text, files), [text, files]);

  return (
    <div className={styles.composer}>
      <div className={styles.composerIn}>
        <div className={styles.box}>
          <div
            className={styles.grip}
            role="separator"
            aria-label="拖动调整输入框高度（双击恢复自动）"
            onPointerDown={startResize}
            onDoubleClick={resetHeight}
          />

          <div className={styles.editor}>
            {/* 高亮层：只画标记底色，正文仍然由 textarea 显示（字形位置完全一致） */}
            <div className={styles.mirror} aria-hidden ref={mirrorRef}>
              {segments.map((part, index) =>
                part.file ? (
                  <span className={styles.token} key={index}>
                    {part.text}
                  </span>
                ) : (
                  <span key={index}>{part.text}</span>
                ),
              )}
              {"\n"}
            </div>

            <textarea
              ref={areaRef}
              className={styles.input}
              rows={1}
              value={text}
              style={areaHeight ? { height: areaHeight, overflowY: "auto" } : undefined}
              placeholder="描述你想改的地方；粘贴截图或拖入文件会变成正文里的一枚标记"
              aria-label="消息输入"
              onChange={(event) => onTextChange(event.target.value)}
              onPaste={onPaste}
              onScroll={syncScroll}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  submit();
                }
              }}
            />
          </div>

          <div className={styles.bar}>
            <button
              type="button"
              className={styles.plus}
              aria-label="添加文件"
              onClick={() => fileRef.current?.click()}
            >
              <Plus size={11} />
            </button>

            <Dropdown
              width={236}
              trigger={
                <button type="button" className={styles.pick} title="切换模型">
                  {model}
                </button>
              }
            >
              {options.map((item) => (
                <MenuItem key={item.id} label={item.label} onSelect={() => onPickModel(item.id)} />
              ))}
              <MenuItem label="模型与密钥设置…" onSelect={onOpenSettings} />
            </Dropdown>

            <span className={styles.grow} />

            {files.length > 0 && (
              <span className={styles.count} title="正文里带了几枚附件标记">
                <FileIcon size={9} /> {files.length}
              </span>
            )}

            <button
              type="button"
              className={styles.send}
              aria-label="发送"
              disabled={busy || (!text.trim() && files.length === 0)}
              onClick={submit}
            >
              <ArrowUp size={11} />
            </button>
          </div>

          <input
            ref={fileRef}
            type="file"
            multiple
            hidden
            onChange={(event) => {
              const picked = Array.from(event.target.files ?? []).map(toAttachment);
              addFiles(picked);
              event.target.value = "";
            }}
          />
        </div>
        <div className={styles.hint}>
          <kbd>Enter</kbd> 发送 · <kbd>Shift</kbd>
          <kbd>Enter</kbd> 换行 · 顶部把手拖动可调高度
        </div>
      </div>
    </div>
  );
}
