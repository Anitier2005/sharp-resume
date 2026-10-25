import type { ChangeItem, Person, ResumePage } from "@/types";
import { resolveEntries, wordingOf } from "@/lib/entries";
import type { ChatMsg } from "@/lib/ai";

const KIND_LABEL: Record<ResumePage["kind"], string> = {
  zh: "中文简历",
  en: "英文简历",
  portfolio: "作品集",
};

/**
 * 喂给模型的两类材料必须分清楚，否则它会拿库里的记录当成品去"改"：
 *   1. 本页措辞 —— 这是简历正文，是可改写的对象
 *   2. 经历库记录 —— 这是用户写给自己的素材，只用来核对事实与挖亮点
 */
function experienceBlock(person: Person, page: ResumePage): string {
  const lines: string[] = [];
  const rows = resolveEntries(person, page);
  const en = page.kind === "en";
  const onPage = new Set(rows.map((row) => row.entry.experienceId));

  lines.push("【本页简历正文】（措辞属于这一份简历，可以直接改写）");
  if (!rows.length) lines.push("（这一页还没有放经历）");
  rows.forEach(({ entry, exp }, index) => {
    const company = en ? exp.companyEn || exp.company : exp.company;
    const role = en ? exp.roleEn || exp.role : exp.role;
    lines.push(`[经历 ${index + 1}] ${company} · ${role} · ${exp.period}`);
    const wording = wordingOf(entry, en);
    if (!wording.length) lines.push("  （还没写措辞）");
    wording.forEach((bullet, i) => lines.push(`  ${i + 1}. ${bullet}`));
  });

  if (person.projects.length) {
    lines.push("", "【本页作品】");
    const picked = person.projects.filter((item) => page.projectIds.includes(item.id));
    if (!picked.length) lines.push("（没有选作品）");
    picked.forEach((item, index) =>
      lines.push(`[作品 ${index + 1}] ${item.title} · ${item.year}｜${item.desc}`),
    );
  }

  if (person.education.length) {
    lines.push(
      "",
      "【教育】",
      ...person.education.map((item) => `· ${item.school} · ${item.degree} · ${item.period}`),
    );
  }
  if (person.skills.length) {
    lines.push("", "【技能】", ...person.skills.map((item) => `· ${item.label}：${item.value}`));
  }

  lines.push(
    "",
    "【经历库记录】（用户写给自己的素材，只用来核对事实、挖可强调的点；不要直接抄进简历，也不要当成已定稿的句子）",
  );
  if (!person.experiences.length) lines.push("（库里还没有经历）");
  person.experiences.forEach((exp, index) => {
    const mark = onPage.has(exp.id) ? "（本页已使用）" : "（本页未使用）";
    lines.push(`{经历 ${index + 1}} ${exp.company} · ${exp.role} · ${exp.period}${mark}`);
    const notes = exp.notes.trim();
    if (notes) {
      // 记录可能很长，截断以免上下文爆掉；够模型判断方向即可
      const clipped = notes.length > 400 ? `${notes.slice(0, 400)}…` : notes;
      clipped.split(/\r?\n/).forEach((line) => lines.push(`    ${line}`));
    }
    exp.facts.forEach((fact) => lines.push(`    · ${fact.label || "细节"}：${fact.text}`));
  });

  return lines.join("\n");
}

export function buildSystemPrompt(person: Person, page: ResumePage, resumeTitle: string): string {
  return `你是一位资深简历顾问，正在帮 ${person.name} 改简历。这个工作台里，「经历库」属于人这一级，被名下所有简历共享；当前打开的是「${resumeTitle} · ${page.label}」（${KIND_LABEL[page.kind]}）。

回答规则：
1. 用中文，先给一句判断（问题在哪、能打几分），再给 2–3 条能立刻执行的建议。不要空话与套话。
2. 绝对不要编造数字、公司、时间、奖项。需要数据的地方用「__」留空，并提醒用户补。
3. 只讨论下面这份材料里真实存在的内容。「经历库记录」是用户写给自己看的素材，可以用来核对事实、挖可强调的点，但不要把它当成简历正文，也不要把与目标岗位无关的细节搬上去。
4. before 请从「本页简历正文」里逐字复制；改写后要贴合这个岗位在意的东西。
4. 如果要给出可以直接替换原句的改写，在回答最后附一个 json 代码块，形如：

\`\`\`json
[{"label":"第 1 段 · 公司名","before":"原文逐字复制","after":"改写后的句子"}]
\`\`\`

before 必须与原文逐字一致，不要加省略号、不要改标点；没有可替换的句子就不要输出这个代码块。

页头：${page.headline} · ${page.role} · ${page.meta.join(" / ")}
摘要：${page.summary?.trim() || "（空）"}

材料：
${experienceBlock(person, page)}`;
}

/** 把历史消息转成接口要的格式，只保留最近若干轮，避免上下文过长 */
export function toApiMessages(
  system: string,
  history: { role: "user" | "assistant"; content: string }[],
  prompt: string,
  attachments: { name: string; text?: string }[],
): ChatMsg[] {
  const recent = history.slice(-6).map((message) => ({
    role: message.role,
    content: message.content,
  }));

  const blocks = attachments.map((file) =>
    file.text
      ? `──── ${file.name} ────\n${file.text.trim()}\n──── 文件结束 ────`
      : `（附件 ${file.name}：非纯文本，读不到内容，需要用户把关键信息贴进来）`,
  );

  const text = blocks.length ? `${prompt}\n\n本次上传的附件：\n${blocks.join("\n\n")}` : prompt;

  return [
    { role: "system", content: system },
    ...recent,
    { role: "user", content: text },
  ];
}

const norm = (value: string) => value.replace(/\s+/g, "").replace(/[，。；、,.．:：!！?？"'“”‘’]/g, "");

function findTarget(
  before: string,
  person: Person,
  page: ResumePage,
): ChangeItem["target"] {
  const key = norm(before);
  if (!key) return undefined;
  const rows = resolveEntries(person, page);

  // 先按逐字相等找，找不到再用包含关系兜底（模型偶尔会多带句末标点）
  for (const { entry } of rows) {
    const list = [...entry.bullets, ...(entry.bulletsEn ?? [])];
    const index = list.findIndex((bullet) => norm(bullet) === key);
    if (index >= 0) return { entryId: entry.id, bulletIndex: index };
  }
  for (const { entry } of rows) {
    const list = [...entry.bullets, ...(entry.bulletsEn ?? [])];
    const index = list.findIndex((bullet) => key.length > 6 && norm(bullet).includes(key));
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
