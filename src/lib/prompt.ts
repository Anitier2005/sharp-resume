import type { ChangeItem, Person, ResumePage } from "@/types";
import { fieldText } from "@/types";
import { entriesOf, headerOf, sectionsOf } from "@/lib/blocks";

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
