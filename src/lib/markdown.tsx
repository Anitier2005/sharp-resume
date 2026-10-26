import { Fragment, useMemo, type ReactNode } from "react";
import styles from "./markdown.module.css";

/**
 * 极简 Markdown 渲染。
 *
 * 为什么不引 react-markdown / marked：
 *   我们只需要模型实际会输出的那一小撮语法（粗体、行内代码、有序/无序列表、
 *   标题、引用、围栏代码），却要拖进 40KB 依赖 + 一串 remark 插件。
 *   这个项目连 docx 都是手写 zip 的，这里同样保持零依赖。
 *
 * 安全：全程构造 React 元素，没有 dangerouslySetInnerHTML，
 * 因此模型输出里的 HTML 只会被当成字面文本显示，不会被解析。
 *
 * 转义顺序也刻意写死：先处理 `**` 再处理 `*`，否则粗体会被单星号规则切开。
 */

type Block =
  | { kind: "p"; lines: string[] }
  | { kind: "h"; level: number; text: string }
  | { kind: "ul"; items: string[] }
  | { kind: "ol"; start: number; items: string[] }
  | { kind: "quote"; lines: string[] }
  | { kind: "code"; lang: string; code: string }
  | { kind: "hr" };

const HEADING = /^(#{1,6})\s+(.*)$/;
const UL_ITEM = /^\s*[-*+]\s+(.*)$/;
const OL_ITEM = /^\s*(\d+)[.)]\s+(.*)$/;
const QUOTE = /^>\s?(.*)$/;
const HR = /^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/;
const FENCE = /^\s*```(\w*)\s*$/;

/** 行 → 块序列。故意写得直白：状态就一个「当前块」，不留隐式状态机 */
function parseBlocks(text: string): Block[] {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const blocks: Block[] = [];
  let index = 0;

  while (index < lines.length) {
    const line = lines[index];

    if (!line.trim()) {
      index += 1;
      continue;
    }

    // 围栏代码：整段原样收下，内部不再解析任何标记
    const fence = FENCE.exec(line);
    if (fence) {
      const code: string[] = [];
      index += 1;
      while (index < lines.length && !FENCE.test(lines[index])) {
        code.push(lines[index]);
        index += 1;
      }
      index += 1; // 跳过收尾的 ```
      blocks.push({ kind: "code", lang: fence[1] ?? "", code: code.join("\n") });
      continue;
    }

    if (HR.test(line)) {
      blocks.push({ kind: "hr" });
      index += 1;
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      blocks.push({ kind: "h", level: heading[1].length, text: heading[2].trim() });
      index += 1;
      continue;
    }

    if (UL_ITEM.test(line)) {
      const items: string[] = [];
      while (index < lines.length && UL_ITEM.test(lines[index])) {
        items.push(UL_ITEM.exec(lines[index])![1].trim());
        index += 1;
      }
      blocks.push({ kind: "ul", items });
      continue;
    }

    if (OL_ITEM.test(line)) {
      const items: string[] = [];
      const start = Number(OL_ITEM.exec(line)![1]) || 1;
      while (index < lines.length && OL_ITEM.test(lines[index])) {
        items.push(OL_ITEM.exec(lines[index])![2].trim());
        index += 1;
      }
      blocks.push({ kind: "ol", start, items });
      continue;
    }

    if (QUOTE.test(line)) {
      const quoted: string[] = [];
      while (index < lines.length && QUOTE.test(lines[index])) {
        quoted.push(QUOTE.exec(lines[index])![1]);
        index += 1;
      }
      blocks.push({ kind: "quote", lines: quoted });
      continue;
    }

    // 普通段落：一直到空行或下一个块的开始
    const para: string[] = [];
    while (index < lines.length) {
      const current = lines[index];
      if (
        !current.trim() ||
        HEADING.test(current) ||
        UL_ITEM.test(current) ||
        OL_ITEM.test(current) ||
        QUOTE.test(current) ||
        HR.test(current) ||
        FENCE.test(current)
      ) {
        break;
      }
      para.push(current.trim());
      index += 1;
    }
    blocks.push({ kind: "p", lines: para });
  }

  return blocks;
}

/** 行内语法：粗体 → 斜体 → 行内代码 → 链接。顺序不能换 */
const INLINE =
  /(\*\*[^*]+\*\*|__[^_]+__|\*[^*\n]+\*|`[^`\n]+`|\[[^\]\n]+\]\([^)\s]+\))/g;

function inline(text: string, keyPrefix: string): ReactNode[] {
  const parts = text.split(INLINE).filter((part) => part !== "");
  return parts.map((part, i) => {
    const key = `${keyPrefix}-${i}`;

    if ((part.startsWith("**") && part.endsWith("**")) || (part.startsWith("__") && part.endsWith("__"))) {
      return <strong key={key}>{part.slice(2, -2)}</strong>;
    }
    if (part.startsWith("*") && part.endsWith("*") && part.length > 2) {
      return <em key={key}>{part.slice(1, -1)}</em>;
    }
    if (part.startsWith("`") && part.endsWith("`") && part.length > 2) {
      return (
        <code key={key} className={styles.code}>
          {part.slice(1, -1)}
        </code>
      );
    }
    const link = /^\[([^\]]+)\]\(([^)\s]+)\)$/.exec(part);
    if (link) {
      return (
        <a key={key} href={link[2]} target="_blank" rel="noreferrer" className={styles.link}>
          {link[1]}
        </a>
      );
    }
    return <Fragment key={key}>{part}</Fragment>;
  });
}

/** 段落里的单个换行要保留（模型常用「一、二、三」分行列举） */
function lines(list: string[], keyPrefix: string): ReactNode[] {
  return list.flatMap((line, i) => [
    i > 0 ? <br key={`${keyPrefix}-br-${i}`} /> : null,
    <Fragment key={`${keyPrefix}-l-${i}`}>{inline(line, `${keyPrefix}-${i}`)}</Fragment>,
  ]);
}

export function Markdown({ text, className }: { text: string; className?: string }) {
  const blocks = useMemo(() => parseBlocks(text), [text]);

  return (
    <div className={className ? `${styles.md} ${className}` : styles.md}>
      {blocks.map((block, index) => {
        const key = `b-${index}`;
        switch (block.kind) {
          case "h": {
            const Tag = (`h${Math.min(6, block.level + 2)}`) as "h3";
            return (
              <Tag key={key} className={styles.heading}>
                {inline(block.text, key)}
              </Tag>
            );
          }
          case "ul":
            return (
              <ul key={key} className={styles.list}>
                {block.items.map((item, i) => (
                  <li key={`${key}-${i}`}>{inline(item, `${key}-${i}`)}</li>
                ))}
              </ul>
            );
          case "ol":
            return (
              <ol key={key} className={styles.list} start={block.start}>
                {block.items.map((item, i) => (
                  <li key={`${key}-${i}`}>{inline(item, `${key}-${i}`)}</li>
                ))}
              </ol>
            );
          case "quote":
            return (
              <blockquote key={key} className={styles.quote}>
                {lines(block.lines, key)}
              </blockquote>
            );
          case "code":
            return (
              <pre key={key} className={styles.block} data-lang={block.lang || undefined}>
                <code>{block.code}</code>
              </pre>
            );
          case "hr":
            return <hr key={key} className={styles.hr} />;
          default:
            return <p key={key}>{lines(block.lines, key)}</p>;
        }
      })}
    </div>
  );
}
