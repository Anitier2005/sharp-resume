import type { ChangeItem, Person, ResumePage } from "@/types";
import { fieldText } from "@/types";
import type { ChatMsg } from "@/lib/ai";
import { entriesOf, headerOf, sectionsOf } from "@/lib/blocks";
import { skillsPrompt } from "@/lib/skills";

/** 材料：只描述"这一页现在有什么"，规则在 prompts/system.md 里 */
export function buildContext(person: Person, page: ResumePage): string {
  const lines: string[] = ["【页头】"];
  const header = headerOf(page);
  lines.push(...(header.length ? header.map(fieldText) : ["（空）"]));

  for (const section of sectionsOf(page)) {
    lines.push("", `【${section.title || "未命名分区"}】`);
    if (!section.items.length) {
      lines.push("（空）");
      continue;
    }
    section.items.forEach((entry, index) => {
      const source = person.items.find((item) => item.id === entry.refId);
      const fields = entry.fields.length ? entry.fields : source?.fields ?? [];
      const label = fields.length ? fieldText(fields[0]) : `条目 ${index + 1}`;
      lines.push(`- ${label}`);
      for (const field of fields.slice(1)) lines.push(`    ${fieldText(field)}`);
      entry.bullets.forEach((bullet, at) => {
        if (bullet.trim()) lines.push(`    ${at + 1}. ${bullet}`);
      });
    });
  }

  if (person.items.length) {
    lines.push("", "【库里的记录】（用户写给自己的素材，只用来核对事实，不要直接抄进简历）");
    for (const item of person.items) {
      lines.push(`· ${item.fields.map(fieldText).join(" | ") || "（无字段）"}`);
      if (item.notes.trim()) {
        const clipped = item.notes.length > 400 ? `${item.notes.slice(0, 400)}…` : item.notes;
        clipped.split(/\r?\n/).forEach((line) => lines.push(`    ${line}`));
      }
    }
  }

  return lines.join("\n");
}

/**
 * 浏览器调试模式仍需要完整 system prompt（桌面端由 Rust 拼 `prompts/system.md`）。
 * 这里保持与那份模板同样的规则要点，避免两种模式的回答风格相差太远。
 */
export function buildSystemPrompt(context: string): string {
  return [
    "你是一位资深简历顾问，正在和一个候选人一起改他的简历。",
    "用中文回答；先给一句判断（问题在哪），再给 2–3 条能立刻执行的建议。",
    "绝对不要编造数字、公司、时间、奖项、证书；需要数据的地方用「__」留空并提醒补。",
    "只讨论下面材料里真实存在的内容。",
    "",
    // 这条只在「让用户挑版本」时用。默认走工具直接写 —— 浏览器模式下没有工具层，
    // 所以这里保留它作为唯一的落地方式，措辞与 prompts/system.md 保持一致：
    // 桌面端默认直接改库，只有多版本对比才回这张卡。
    "用户要你「给几个版本挑一个」时，在回答最后附一个 json 代码块：",
    '[{"label":"第 1 段 · 公司名","before":"原文逐字复制","after":"改写后的句子"}]',
    "before 必须与原文逐字一致；没有可替换的句子就不要输出这个代码块。",
    "其余情况直接给结论和改法，不要为了凑格式输出这张卡。",
    "",
    "---",
    "",
    "## 当前材料",
    "",
    context,
    "",
    skillsPrompt(),
  ]
    .filter(Boolean)
    .join("\n");
}

export function toApiMessages(
  system: string,
  history: { role: "user" | "assistant"; content: string }[],
  prompt: string,
  attachments: { name: string; text?: string }[],
  /** 图片 data URL。只在模型支持视觉时由调用方传进来，走协议的 image_url 段 */
  images: string[] = [],
): ChatMsg[] {
  const recent = history.slice(-6).map((message) => ({
    role: message.role,
    content: message.content,
  }));

  const blocks = attachments.map((file) =>
    file.text
      ? `──── ${file.name} ────\n${file.text.trim()}\n──── 文件结束 ────`
      : `（附件 ${file.name}：非纯文本，读不到内容）`,
  );

  const text = blocks.length ? `${prompt}\n\n本次上传的附件：\n${blocks.join("\n\n")}` : prompt;

  // 图片走多模态段而不是塞进正文：data URL 动辄几百 KB，
  // 拼进字符串会让模型（和 token 计数）都很难受 —— 协议本来就有专门的段类型。
  const userContent: ChatMsg["content"] = images.length
    ? [
        { type: "text", text },
        ...images.map((url) => ({ type: "image_url" as const, image_url: { url } })),
      ]
    : text;

  return [{ role: "system", content: system }, ...recent, { role: "user", content: userContent }];
}

const norm = (value: string) => value.replace(/\s+/g, "").replace(/[，。；、,.．:：!！?？"'“”‘’]/g, "");

/** 按本页措辞定位可改写的那一句 */
function findTarget(before: string, person: Person, page: ResumePage): ChangeItem["target"] {
  const key = norm(before);
  if (!key) return undefined;
  const entries = entriesOf(page);
  void person;

  for (const entry of entries) {
    const index = entry.bullets.findIndex((bullet) => norm(bullet) === key);
    if (index >= 0) return { entryId: entry.id, bulletIndex: index };
  }
  for (const entry of entries) {
    const index = entry.bullets.findIndex((bullet) => key.length > 6 && norm(bullet).includes(key));
    if (index >= 0) return { entryId: entry.id, bulletIndex: index };
  }
  return undefined;
}

const FENCE = /```(?:json|JSON)?\s*(\[[\s\S]*?\])\s*```/;

/** 拆出正文与可应用的改写块 */
export function splitReply(
  reply: string,
  person: Person,
  page: ResumePage,
): { content: string; changes: ChangeItem[] } {
  const match = reply.match(FENCE);
  if (!match) return { content: reply.trim(), changes: [] };

  let rows: unknown;
  try {
    rows = JSON.parse(match[1]);
  } catch {
    return { content: reply.trim(), changes: [] };
  }
  if (!Array.isArray(rows)) return { content: reply.trim(), changes: [] };

  const changes: ChangeItem[] = [];
  for (const row of rows) {
    const item = row as { label?: unknown; before?: unknown; after?: unknown };
    if (typeof item.before !== "string" || typeof item.after !== "string") continue;
    const before = item.before.trim();
    const after = item.after.trim();
    if (!before || !after || before === after) continue;
    changes.push({
      label: typeof item.label === "string" && item.label.trim() ? item.label.trim() : "改写建议",
      before,
      after,
      target: findTarget(before, person, page),
    });
  }

  return { content: reply.replace(match[0], "").trim(), changes };
}
