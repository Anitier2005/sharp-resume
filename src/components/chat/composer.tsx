import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Attachment } from "@/types";
import { ArrowUp, Close, FileIcon, Plus } from "@/components/ui/icons";
import { Dropdown, MenuItem } from "@/components/ui/menu";
import { MODELS } from "@/lib/model-config";
import styles from "./composer.module.css";
import { formatSize, toAttachment } from "@/lib/attach";

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
  /** 待发送附件由外层持有，方便拖放注入 */
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

  /** 粘贴：图片走附件（带缩略图），其余文件同样收下 */
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
          : new File([file], `粘贴-${Date.now()}.${(file.type.split("/")[1] || "png").replace("jpeg", "jpg")}`, {
              type: file.type,
            });
      picked.push(toAttachment(named));
    }
    if (!picked.length) return; // 纯文本粘贴交回浏览器默认行为
    event.preventDefault();
    onFilesChange([...files, ...picked]);
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

          {files.length > 0 && (
            <div className={styles.pendingFiles}>
              {files.map((file) => (
                <span className={styles.pending} key={file.id}>
                  {file.preview ? (
                    <img className={styles.thumb} src={file.preview} alt={file.name} />
                  ) : (
                    <FileIcon size={10} />
                  )}
                  {file.name}
                  <span className={styles.sz}>{formatSize(file.size)}</span>
                  <button
                    type="button"
                    aria-label={`移除 ${file.name}`}
                    onClick={() => onFilesChange(files.filter((item) => item.id !== file.id))}
                  >
                    <Close size={9} />
                  </button>
                </span>
              ))}
            </div>
          )}

          <textarea
            ref={areaRef}
            className={styles.input}
            rows={1}
            value={text}
            style={areaHeight ? { height: areaHeight, overflowY: "auto" } : undefined}
            placeholder="描述你想改的地方；也可以直接粘贴截图、拖入 JD 或旧简历"
            aria-label="消息输入"
            onChange={(event) => onTextChange(event.target.value)}
            onPaste={onPaste}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                submit();
              }
            }}
          />

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
                <MenuItem
                  key={item.id}
                  label={item.label}
                  onSelect={() => onPickModel(item.id)}
                />
              ))}
              <MenuItem label="模型与密钥设置…" onSelect={onOpenSettings} />
            </Dropdown>

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
              if (picked.length) onFilesChange([...files, ...picked]);
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
