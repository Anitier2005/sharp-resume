import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { flushSync } from "react-dom";
import type { Attachment } from "@/types";
import { ArrowUp, FileIcon, Plus } from "@/components/ui/icons";
import { ImagePeek } from "@/components/ui/image-view";
import { Dropdown, MenuItem } from "@/components/ui/menu";
import { Tooltip } from "@/components/ui";
import { cx } from "@/lib/cx";
import { listAllModels, listProfiles } from "@/lib/model-config";
import { segmentText, tokenRanges, uniqueToken } from "@/lib/inline-tokens";
import styles from "./composer.module.css";
import { isImage, toAttachment } from "@/lib/attach";

/** 输入框高度偏好在本地记住：不同人写东西的姿势不一样 */
const HEIGHT_KEY = "sharp-resume:composer:h";
/** 这两个值现在直接是 textarea 的高度（早先是整块 box 的高度） */
const MIN_H = 24;
const MAX_H = 380;

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

  /** 拖上边框改高度：往上拖就是加高，松手才落盘 */
  const startResize = useCallback((event: React.PointerEvent) => {
    event.preventDefault();
    const el = areaRef.current;
    if (!el) return;
    const startY = event.clientY;
    const startH = el.offsetHeight;
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
  const onPaste = async (event: React.ClipboardEvent<HTMLTextAreaElement>) => {
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
      picked.push(await toAttachment(named));
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

  /**
   * 退格/Delete 碰到附件标记时**整枚删除**。
   * 高亮只在整串文件名完整匹配时才画：一个字一个字地啃，啃掉第一个字
   * 标签底色就没了，剩下的文件名变成普通文字悬在正文里
   * （用户报过「只删了标签、名称有残留」）——所以要么不删，要么整枚删。
   */
  const deleteTokenRange = (from: number, to: number) => {
    // flushSync：新正文与新光标立刻落进 DOM，连按退格也能连删多枚
    flushSync(() => onTextChange(text.slice(0, from) + text.slice(to)));
    const el = areaRef.current;
    el?.focus();
    el?.setSelectionRange(from, from);
  };

  /** 删 [start, end) 时把贴着的一个空格一起带走：行首带右侧空格，其余吃左侧 */
  const swallowSpace = (start: number, end: number): [number, number] => {
    if (start === 0 && text[end] === " ") return [start, end + 1];
    if (start > 0 && text[start - 1] === " ") return [start - 1, end];
    return [start, end];
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.nativeEvent.isComposing) return; // 中文输入法组词中，不抢键
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      submit();
      return;
    }
    if (event.key !== "Backspace" && event.key !== "Delete") return;

    const el = event.currentTarget;
    const start = el.selectionStart;
    const end = el.selectionEnd;
    const ranges = tokenRanges(text, files);

    if (start !== end) {
      // 选区碰到标记就按整枚算：哪怕只选中了文件名中间两个字
      const touched = ranges.filter((r) => r.start < end && start < r.end);
      if (!touched.length) return;
      event.preventDefault();
      const from = Math.min(start, ...touched.map((r) => r.start));
      const to = Math.max(end, ...touched.map((r) => r.end));
      // 两侧都是空格时带走一个，删除后不留双空格
      const adj = from > 0 && text[from - 1] === " " && text[to] === " " ? from - 1 : from;
      deleteTokenRange(adj, to);
      return;
    }

    const backward = event.key === "Backspace";
    // 光标贴着标记（退格：在末尾或中间；Delete：在开头或中间）→ 整枚删。
    // 光标在标记开头按退格、在末尾按 Delete，先正常删旁边的空格，下一键才删整枚。
    const hit = ranges.find((r) =>
      backward ? start > r.start && start <= r.end : start >= r.start && start < r.end,
    );
    if (!hit) return;
    event.preventDefault();
    const [from, to] = swallowSpace(hit.start, hit.end);
    deleteTokenRange(from, to);
  };

  // 下拉里列的是「模型：配置」而不是裸模型名 —— 同一个模型可能有好几套网关，
  // 只写模型名时用户不知道自己正在用哪把 key。
  // 数量很少（模型 × 配置），每次渲染重建比维护失效缓存更省心。
  const options = listAllModels().flatMap((item) =>
    listProfiles(item).map((profile) => ({
      id: `${item.id}|${profile.id}`,
      label: `${item.label}：${profile.label}`,
    })),
  );
  const areaHeight = manual == null ? undefined : Math.max(MIN_H, manual);
  const segments = useMemo(() => segmentText(text, files), [text, files]);

  return (
    <div className={styles.composer}>
      <div className={styles.composerIn}>
        <div className={styles.box}>
          <div className={styles.editor}>
            {/* 高亮层：只画标记底色，正文仍然由 textarea 显示（字形位置完全一致） */}
            <div className={styles.mirror} aria-hidden ref={mirrorRef}>
              {segments.map((part, index) => {
                const file = part.file;
                if (!file) return <span key={index}>{part.text}</span>;
                // 只有图片才有「看一眼 / 点开」的意义；其余附件保持纯标记
                const image = isImage(file.ext) ? file.preview : undefined;
                if (!image) {
                  return (
                    <span className={styles.token} key={index}>
                      {part.text}
                    </span>
                  );
                }
                return (
                  <ImagePeek
                    key={`${file.id}:${index}`}
                    className={cx(styles.token, styles.tokenImage)}
                    title={file.name}
                    resolve={() => file.preview ?? null}
                  >
                    {part.text}
                  </ImagePeek>
                );
              })}
              {"\n"}
            </div>

            {/* 上边框即把手：鼠标碰到光标变成上下箭头，往上拖加高（双击恢复自动） */}
            <span
              className={styles.resizer}
              role="separator"
              aria-label="拖动上边框调整输入框高度（双击恢复自动）"
              onPointerDown={startResize}
              onDoubleClick={resetHeight}
            />

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
              onKeyDown={onKeyDown}
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
                <Tooltip label="切换模型">
                  <button type="button" className={styles.pick}>
                    {model}
                  </button>
                </Tooltip>
              }
            >
              {options.map((item) => (
                <MenuItem key={item.id} label={item.label} onSelect={() => onPickModel(item.id)} />
              ))}
              <MenuItem label="模型与密钥设置…" onSelect={onOpenSettings} />
            </Dropdown>

            <span className={styles.grow} />

            {files.length > 0 && (
              <Tooltip label="正文里带了几枚附件标记">
                <span className={styles.count}>
                  <FileIcon size={9} /> {files.length}
                </span>
              </Tooltip>
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
              const files = Array.from(event.target.files ?? []);
              void Promise.all(files.map(toAttachment)).then((picked) => addFiles(picked));
              event.target.value = "";
            }}
          />
        </div>
      </div>
    </div>
  );
}
