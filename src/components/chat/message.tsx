import { useEffect, useMemo, useState, type ReactNode } from "react";
import type { Asset, Attachment, ChatAction, ChatMessage, Person } from "@/types";
import { Check, FileIcon, Plus } from "@/components/ui/icons";
import { ImagePeek, ImageView } from "@/components/ui/image-view";
import { resolveStored, resolveUrl } from "@/lib/assets";
import { attachmentSrc, isImage } from "@/lib/attach";
import { segmentText } from "@/lib/inline-tokens";
import { Markdown } from "@/lib/markdown";
import { cx } from "@/lib/cx";
import { useWorkspace } from "@/store/workspace";
import styles from "./message.module.css";

/**
 * 行内附件的图片地址。
 *
 * 消息里存的是「文件名 + 可能的 material id」：字节要么在材料池（assetId），
 * 要么在本会话的内存缓存里（发出去那一刻读的 data URL）。前者要异步读盘，
 * 所以结果必须缓存 —— 否则每次悬停都重新读一遍，图片一多就卡。
 */
const srcCache = new Map<string, string>();
const srcPending = new Map<string, Promise<string | null>>();

const cacheKeyOf = (file: Attachment) => `${file.id}:${file.assetId ?? ""}`;

/** 材料可能挂在人身上，也可能挂在某条经历里（上传入口不同） */
function assetOf(person: Person | null, assetId?: string): Asset | undefined {
  if (!person || !assetId) return undefined;
  const own = person.assets?.find((item) => item.id === assetId);
  if (own) return own;
  for (const item of person.items) {
    const hit = item.assets?.find((asset) => asset.id === assetId);
    if (hit) return hit;
  }
  return undefined;
}

/** 同步判断「这张图有没有可能看到」：决定要不要挂 hover/click，避免给死标签上手型 */
function hasImage(file: Attachment, person: Person | null): boolean {
  if (!isImage(file.ext)) return false;
  if (file.preview) return true;
  if (assetOf(person, file.assetId)?.kind === "image") return true;
  return Boolean(file.stored);
}

function resolveAttachmentSrc(file: Attachment, person: Person | null): Promise<string | null> {
  const key = cacheKeyOf(file);
  const cached = srcCache.get(key);
  if (cached) return Promise.resolve(cached);
  if (file.preview) {
    srcCache.set(key, file.preview);
    return Promise.resolve(file.preview);
  }
  const running = srcPending.get(key);
  if (running) return running;

  const task = (async () => {
    const asset = assetOf(person, file.assetId);
    if (asset?.kind === "image") {
      const url = await resolveUrl(asset);
      if (url) return url;
    }
    return attachmentSrc(file);
  })().then(
    (value) => {
      srcPending.delete(key);
      // 只缓存成功结果：图片的 data URL 是选文件时异步读的，早一步悬停会读到空
      if (value) srcCache.set(key, value);
      return value;
    },
    () => {
      srcPending.delete(key);
      return null;
    },
  );
  srcPending.set(key, task);
  return task;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** 把数字、百分比、倍数标成重点，模拟编辑器里的高亮 */
function highlight(text: string): ReactNode[] {
  const parts = text.split(/([+＋]?\d+(?:\.\d+)?%|\d+(?:\.\d+)?\s?(?:步|倍|万|亿))/g);
  return parts.map((part, index) =>
    /^[+＋]?\d/.test(part) ? <b key={index}>{part}</b> : <span key={index}>{part}</span>,
  );
}

/** 正文里的一枚附件标记；只有图片才可悬停预览 / 点开大图 */
function InlineAsset({ file, person }: { file: Attachment; person: Person | null }) {
  if (!hasImage(file, person)) {
    return (
      <span className={styles.inlineAsset} title={file.name}>
        <FileIcon size={9} />
        {file.name}
      </span>
    );
  }
  return (
    <ImagePeek
      key={cacheKeyOf(file)}
      className={cx(styles.inlineAsset, styles.inlineImage)}
      title={file.name}
      resolve={() => resolveAttachmentSrc(file, person)}
    >
      <FileIcon size={9} />
      {file.name}
    </ImagePeek>
  );
}

/**
 * 正文里的知识库图片引用。
 *
 * 知识库的图片块被模型引用时只是一行 `![门店照片](asset:ab12.png)`，直接交给
 * markdown 渲染就是一行字（`asset:` 不是有效协议，图片加载必失败）。这里把它
 * 切成「文字段 / 图片段」，图片段换成真图。两种写法都认：带 `![]()` 的，和
 * 模型漏写包裹的裸 `asset:xxx.png`。
 *
 * 裸写法必须带扩展名，否则 `asset:abc` 后面的中文标点会被一起吞进存储名。
 */
const ASSET_REF =
  /!\[([^\]]*)\]\(\s*asset:([^)\s]+?)\s*\)|(?<![\w])asset:([A-Za-z0-9][\w.-]*\.[A-Za-z0-9]{2,5})/g;

/** 代码围栏：围栏里的 `asset:xxx` 是示例文本，不该变图片 */
const FENCE_OPEN = /^\s{0,3}(`{3,}|~{3,})/;
const FENCE_CLOSE = /^\s{0,3}(`{3,}|~{3,})\s*$/;

type Block =
  | { kind: "text"; text: string }
  | { kind: "image"; stored: string; raw: string; alt: string };

function splitAssetBlocks(text: string): Block[] {
  const blocks: Block[] = [];
  let buf = "";
  let fence: { char: string; len: number } | null = null;

  // 纯空白的片段（图片前后那半个换行）丢掉，否则会在图片两侧多出空段落
  const takeText = () => {
    if (buf.trim()) blocks.push({ kind: "text", text: buf });
    buf = "";
  };

  for (const line of text.split("\n")) {
    if (fence) {
      buf += `${line}\n`;
      const close = FENCE_CLOSE.exec(line);
      if (close && close[1][0] === fence.char && close[1].length >= fence.len) fence = null;
      continue;
    }
    const open = FENCE_OPEN.exec(line);
    if (open) {
      fence = { char: open[1][0], len: open[1].length };
      buf += `${line}\n`;
      continue;
    }

    let cursor = 0;
    // 全局正则会记住 lastIndex，逐行用之前必须归零
    ASSET_REF.lastIndex = 0;
    for (let match = ASSET_REF.exec(line); match; match = ASSET_REF.exec(line)) {
      buf += line.slice(cursor, match.index);
      takeText();
      blocks.push({
        kind: "image",
        stored: (match[2] ?? match[3]).trim(),
        raw: match[0],
        alt: (match[1] ?? "").trim(),
      });
      cursor = match.index + match[0].length;
    }
    buf += `${line.slice(cursor)}\n`;
  }

  takeText();
  // 整段都被切成图片时 blocks 非空；这里只兜「全是空白」的极端情况
  return blocks.length ? blocks : [{ kind: "text", text }];
}

/**
 * 图片地址缓存。
 *
 * 同一条消息会因为滚动、切会话、重新渲染反复挂载，每挂一次就读一遍盘的话，
 * 一屏图能把池子读爆。解析失败的也记下来（记成 null）：缺图是内容问题，
 * 重试也不会自己变好，反而会拖慢消息流。
 */
const assetSrcCache = new Map<string, string | null>();
const assetPending = new Map<string, Promise<string | null>>();

function loadAssetSrc(stored: string): Promise<string | null> {
  if (assetSrcCache.has(stored)) return Promise.resolve(assetSrcCache.get(stored) ?? null);
  const running = assetPending.get(stored);
  if (running) return running;

  const task = resolveStored(stored).then(
    (url) => {
      assetPending.delete(stored);
      assetSrcCache.set(stored, url);
      return url;
    },
    () => {
      assetPending.delete(stored);
      assetSrcCache.set(stored, null);
      return null;
    },
  );
  assetPending.set(stored, task);
  return task;
}

/** 一张知识库图片：缩略图，点击看大图；读不到就老老实实显示原来那行文本 */
function AssetImage({ stored, raw, alt }: { stored: string; raw: string; alt: string }) {
  const known = assetSrcCache.has(stored);
  const [src, setSrc] = useState<string | null>(known ? assetSrcCache.get(stored) ?? null : null);
  const [settled, setSettled] = useState(known);
  // 地址拿到了但字节解不开（不是图片、文件被换过）也要退到原文，别留个破图图标
  const [broken, setBroken] = useState(false);
  const [zoom, setZoom] = useState(false);

  useEffect(() => {
    let alive = true;
    void loadAssetSrc(stored).then((url) => {
      // 异步回来时这一块可能已经被换掉（消息重排），别往旧组件里塞地址
      if (!alive) return;
      setSrc(url);
      setSettled(true);
    });
    return () => {
      alive = false;
    };
  }, [stored]);

  if (!settled) return <span className={styles.assetPending} aria-busy="true" />;
  if (!src || broken) return <span className={styles.assetMissing}>{raw}</span>;

  const label = alt || stored;
  return (
    <>
      <button
        type="button"
        className={styles.assetThumb}
        onClick={() => setZoom(true)}
        aria-label={`放大「${label}」`}
      >
        <img
          className={styles.assetImg}
          src={src}
          alt={label}
          draggable={false}
          onError={() => setBroken(true)}
        />
      </button>
      {zoom && <ImageView src={src} name={label} onClose={() => setZoom(false)} />}
    </>
  );
}

/**
 * 助手正文：按图片段切开后，文字段仍走原来的 Markdown 组件。
 *
 * 不重写整条 Markdown 链路（表格、代码块、脚注都靠它），只是把图片引用从
 * 文本里摘出来单独渲染 —— markdown 自己渲染 `asset:` 只会得到一张破图。
 */
function AssistantBody({ text }: { text: string }) {
  const blocks = useMemo(() => splitAssetBlocks(text), [text]);
  if (blocks.length === 1 && blocks[0].kind === "text") return <Markdown text={text} />;

  return (
    <>
      {blocks.map((block, index) =>
        block.kind === "image" ? (
          <AssetImage
            key={`${block.stored}#${index}`}
            stored={block.stored}
            raw={block.raw}
            alt={block.alt}
          />
        ) : (
          <Markdown key={`text#${index}`} text={block.text} />
        ),
      )}
    </>
  );
}

export function UserTurn({ message }: { message: ChatMessage }) {
  // person 不去改 panel.tsx 的调用签名：消息本来就属于这个人，直接从工作区取
  const { person } = useWorkspace();
  const attachments = message.attachments ?? [];
  // 有 token 的附件就长在正文里；没 token 的是旧消息，退回下面那排
  const inline = attachments.filter((file) => file.token);
  const legacy = attachments.filter((file) => !file.token);
  const segments = segmentText(message.content ?? "", inline);

  return (
    <div className={styles.turnUser}>
      {message.content && (
        <div className={styles.prompt} data-selectable>
          {segments.map((part, index) =>
            part.file ? (
              <InlineAsset key={index} file={part.file} person={person} />
            ) : (
              <span key={index}>{part.text}</span>
            ),
          )}
        </div>
      )}

      {legacy.map((file) => (
        <div className={styles.attach} key={file.id} title={file.name}>
          <FileIcon size={10} />
          <span className={styles.an}>{file.name}</span>
          <span className={styles.sz}>{formatSize(file.size)}</span>
        </div>
      ))}
    </div>
  );
}

export function AssistantTurn({
  message,
  onAction,
}: {
  message: ChatMessage;
  onAction: (action: ChatAction) => void;
}) {
  return (
    <div className={styles.turnAi}>
      {message.tool && (
        <div className={styles.tool}>
          <Check size={11} />
          {message.tool}
        </div>
      )}
      {message.content && <AssistantBody text={message.content} />}

      {message.changes?.map((change, index) => (
        <div className={styles.change} key={index}>
          <span className={styles.lb}>{change.label}</span>
          <span className={styles.old}>{change.before}</span>
          <span className={styles.new}>{highlight(change.after)}</span>
        </div>
      ))}

      {message.actions && message.actions.length > 0 && (
        <div className={styles.acts}>
          {message.actions.map((action) => (
            <button
              key={action.id}
              type="button"
              className={cx(styles.act, action.kind === "apply" && styles.actPrimary)}
              onClick={() => onAction(action)}
            >
              {action.kind === "apply" ? <Check size={12} /> : <Plus size={12} />}
              {action.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * 系统提示：会话流里的「发生了什么」，不是谁说的话。
 *
 * 它借用消息列表存（不新增存储），但形态上要一眼看出来不是对话内容 ——
 * 居中的一行灰字，不带头像、不带气泡，也不能被当成上下文引用。
 */
export function SystemTurn({ message }: { message: ChatMessage }) {
  return (
    <div className={styles.turnSys} data-selectable>
      {message.content}
    </div>
  );
}

export function TypingTurn() {
  return (
    <div className={styles.turnAi}>
      <div className={styles.typing}>
        <span />
        <span />
        <span />
      </div>
    </div>
  );
}

export function EmptyTurn({ name, onPick }: { name: string; onPick: (text: string) => void }) {
  return (
    <div className={styles.emptyTurn}>
      <p>这是 {name} 的独立会话。</p>
      <p className={styles.dim}>
        描述要改的地方，或拖入 JD、旧简历、截图。同一候选人的其他简历不会受到影响，但经历库是共用的。
      </p>
      <div className={styles.emptyHints}>
        {["按 JD 重写摘要", "把第二段改得更量化", "压缩到一页"].map((text) => (
          <button key={text} type="button" onClick={() => onPick(text)}>
            {text}
          </button>
        ))}
      </div>
    </div>
  );
}
