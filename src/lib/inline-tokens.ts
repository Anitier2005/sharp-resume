import type { Attachment } from "@/types";

/**
 * 行内附件标记。
 *
 * 附件不该被归集到输入框顶上：那样「看这张图」和「这张图」之间没有指代关系。
 * 改成在正文里插入一个 token（`@截图.png`），它既是一段可见的文字，
 * 也是这条消息与那个文件的绑定 —— 删掉这半句，文件就不再随消息发出。
 */

/** 生成一个不撞车的 token：重名时加 (2)(3) */
export function uniqueToken(base: string, taken: (string | undefined)[]): string {
  const used = new Set(taken.filter(Boolean) as string[]);
  if (!used.has(base)) return base;
  let index = 2;
  while (used.has(`${base} (${index})`)) index += 1;
  return `${base} (${index})`;
}

export const tokenOf = (file: Attachment) => file.token ?? `@${file.name}`;

/** 文字里出现的 token，长的排前面，避免 `@a.png` 抢了 `@a.png (2)` 的前缀 */
const escape = (value: string) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

export function tokenPattern(tokens: string[]): RegExp | null {
  const list = [...new Set(tokens)].filter(Boolean).sort((a, b) => b.length - a.length);
  if (!list.length) return null;
  return new RegExp(`(${list.map(escape).join("|")})`, "g");
}

export interface Segment {
  text: string;
  /** 命中 token 时，这里放对应的附件 */
  file?: Attachment;
}

/** 把正文切成「纯文字 / 附件标记」交替的片段 */
export function segmentText(text: string, files: Attachment[]): Segment[] {
  const withToken = files.filter((file) => file.token);
  const pattern = tokenPattern(withToken.map((file) => file.token!));
  if (!pattern) return [{ text }];

  const byToken = new Map(withToken.map((file) => [file.token!, file]));
  return text
    .split(pattern)
    .filter((part) => part !== "")
    .map((part) => {
      const file = byToken.get(part);
      return file ? { text: part, file } : { text: part };
    });
}

/**
 * 追加到正文末尾（拖放场景：指针位置不等于光标位置）。
 * 返回新的正文与附件列表，两边必须一起更新，否则 token 与文件对不上。
 */
export function attachAtEnd(
  text: string,
  files: Attachment[],
  picked: Attachment[],
): { text: string; files: Attachment[] } {
  if (!picked.length) return { text, files };
  const next = [...files];
  const tokens: string[] = [];
  for (const file of picked) {
    const token = uniqueToken(`@${file.name}`, next.map((item) => item.token));
    next.push({ ...file, token });
    tokens.push(token);
  }
  const lead = text && !/\s$/.test(text) ? " " : "";
  return { text: `${text}${lead}${tokens.join(" ")} `, files: next };
}
