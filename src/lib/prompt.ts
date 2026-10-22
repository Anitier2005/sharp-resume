import type { ChangeItem, Person, ResumePage } from "@/types";
import type { ChatMsg } from "@/lib/ai";

const KIND_LABEL: Record<ResumePage["kind"], string> = {
  zh: "中文简历",
  en: "英文简历",
  portfolio: "作品集",
};

function experienceBlock(person: Person, page: ResumePage): string {
  const lines: string[] = [];
  person.experiences.forEach((exp, index) => {
    const onPage = page.experienceIds.includes(exp.id);
    lines.push(`[经历 ${index + 1}]${onPage ? "（在本页）" : ""} ${exp.company} · ${exp.role} · ${exp.period}`);
    exp.bullets.forEach((bullet, i) => lines.push(`  中${i + 1}. ${bullet}`));
    (exp.bulletsEn ?? []).forEach((bullet, i) => lines.push(`  EN${i + 1}. ${bullet}`));
  });
  if (!lines.length) lines.push("（还没有经历）");

  const projects = person.projects.map(
    (item, index) =>
      `[作品 ${index + 1}]${page.projectIds.includes(item.id) ? "（在本页）" : ""} ${item.title} · ${item.year}｜${item.desc}`,
  );
  if (projects.length) {
    lines.push("", "作品：", ...projects);
  }

  if (person.education.length) {
    lines.push(
      "",
      "教育：",
      ...person.education.map((item) => `· ${item.school} · ${item.degree} · ${item.period}`),
    );
  }
  if (person.skills.length) {
    lines.push("", "技能：", ...person.skills.map((item) => `· ${item.label}：${item.value}`));
  }
  return lines.join("\n");
}

export function buildSystemPrompt(person: Person, page: ResumePage, resumeTitle: string): string {
  return `你是一位资深简历顾问，正在帮 ${person.name} 改简历。这个工作台里，「经历库」属于人这一级，被名下所有简历共享；当前打开的是「${resumeTitle} · ${page.label}」（${KIND_LABEL[page.kind]}）。

回答规则：
1. 用中文，先给一句判断（问题在哪、能打几分），再给 2–3 条能立刻执行的建议。不要空话与套话。
2. 绝对不要编造数字、公司、时间、奖项。需要数据的地方用「__」留空，并提醒用户补。
3. 只讨论下面这份材料里真实存在的内容。
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

function findTarget(before: string, person: Person): ChangeItem["target"] {
  const key = norm(before);
  for (const exp of person.experiences) {
    const zhIndex = exp.bullets.findIndex((bullet) => norm(bullet) === key);
    if (zhIndex >= 0) return { experienceId: exp.id, bulletIndex: zhIndex, lang: "zh" };
    const enIndex = (exp.bulletsEn ?? []).findIndex((bullet) => norm(bullet) === key);
    if (enIndex >= 0) return { experienceId: exp.id, bulletIndex: enIndex, lang: "en" };
  }
  // 模型偶尔会多带一个句末标点，退一步用包含关系
  for (const exp of person.experiences) {
    const zhIndex = exp.bullets.findIndex((bullet) => key.length > 6 && norm(bullet).includes(key));
    if (zhIndex >= 0) return { experienceId: exp.id, bulletIndex: zhIndex, lang: "zh" };
    const enIndex = (exp.bulletsEn ?? []).findIndex(
      (bullet) => key.length > 6 && norm(bullet).includes(key),
    );
    if (enIndex >= 0) return { experienceId: exp.id, bulletIndex: enIndex, lang: "en" };
  }
  return undefined;
}

const FENCE = /```(?:json|JSON)?\s*(\[[\s\S]*?\])\s*```/;

/** 拆出正文与可应用的改写块 */
export function splitReply(
  reply: string,
  person: Person,
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
      target: findTarget(before, person),
    });
  }

  return { content: reply.replace(match[0], "").trim(), changes };
}
