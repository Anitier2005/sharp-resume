import type { Person, ResumePage } from "@/types";
import { fieldText } from "@/types";

/**
 * 变更感知：把「右侧手改了什么」变成结构化差异，喂给模型。
 *
 * 为什么需要它：材料每轮都整份发给模型，模型只能看到「现在是什么样」，
 * 看不到「你刚把它改成了什么样」。于是它会重复建议改你已经改过的句子，
 * 或者对刚删掉的内容继续发表意见。这里补上「增量」这一维。
 *
 * 两个设计要点：
 *   1. **key 用 id，不用标题**。用标题当 key 时，一旦首字段改名，
 *      底下所有句子都会同时「消失 + 新增」，看起来像整段被重写。
 *   2. **label 用可读说法**。模型不需要认识内部 id，只要知道
 *      「工作经历 · 字节跳动 · 第 2 句」是哪一句。
 */

/** 一处内容：稳定 key → 可读位置 + 文本 */
export interface Spot {
  label: string;
  value: string;
}

export type Flat = Record<string, Spot>;

/** 一条差异 */
export interface Delta {
  /** 可读位置，如「页头 · 职位」「工作经历 · 字节跳动 · 第 2 句」 */
  where: string;
  before: string;
  after: string;
}

const clip = (value: string, max = 140) => {
  const text = value.trim();
  return text.length > max ? `${text.slice(0, max)}…` : text;
};

/** 摊平：key 稳定（id 或序号），label 可读 */
export function flatten(person: Person, page: ResumePage): Flat {
  const out: Flat = {
    "page:label": { label: "页名", value: page.label },
  };

  for (const field of page.header) {
    out[`header:${field.id}`] = {
      label: `页头 · ${field.label || "未命名字段"}`,
      value: field.label ? field.value : fieldText(field),
    };
  }

  for (const section of page.sections) {
    const secName = section.title.trim() || "未命名分区";
    out[`section:${section.id}`] = { label: `分区 · ${secName}`, value: section.title };

    for (const entry of section.items) {
      const source = person.items.find((item) => item.id === entry.refId);
      const fields = entry.fields.length ? entry.fields : source?.fields ?? [];
      const title = fields[0]?.value?.trim() || "未命名条目";

      for (const field of fields) {
        // 继承来的字段用库里的 id —— 它在库里改，这里也能感知到
        out[`entry:${entry.id}:field:${field.id}`] = {
          label: `${secName} · ${title} · ${field.label || "字段"}`,
          value: field.label ? field.value : fieldText(field),
        };
      }
      entry.bullets.forEach((bullet, index) => {
        out[`entry:${entry.id}:bullet:${index}`] = {
          label: `${secName} · ${title} · 第 ${index + 1} 句`,
          value: bullet,
        };
      });
    }
  }

  for (const field of person.fields) {
    out[`person:${field.id}`] = {
      label: `人物 · ${field.label || "未命名字段"}`,
      value: field.label ? field.value : fieldText(field),
    };
  }

  return out;
}

/**
 * 前后两份做差。上限 40 条 —— 一次改太多时，与其把模型淹掉，
 * 不如只给最能说明问题的那几条（按字段顺序）。
 */
export function diff(before: Flat, after: Flat): Delta[] {
  const out: Delta[] = [];
  const keys = new Set([...Object.keys(before), ...Object.keys(after)]);

  for (const key of keys) {
    const was = before[key];
    const now = after[key];
    if ((was?.value ?? "") === (now?.value ?? "")) continue;
    out.push({
      where: (now ?? was)?.label ?? key,
      before: clip(was?.value ?? ""),
      after: clip(now?.value ?? ""),
    });
    if (out.length >= 40) break;
  }
  return out;
}

/** 渲染成喂给模型的一小段（没有变更时返回空串） */
export function renderDelta(deltas: Delta[]): string {
  if (!deltas.length) return "";
  const lines = ["【自上次对话后，你在右侧手动改过的地方】"];
  for (const item of deltas) {
    if (!item.before) lines.push(`+ ${item.where}：${item.after}`);
    else if (!item.after) lines.push(`- ${item.where}（原来是：${item.before}）`);
    else lines.push(`~ ${item.where}：${item.before} → ${item.after}`);
  }
  lines.push("（这些是你自己刚做的改动；请基于改动后的版本继续，不要重复建议已经改过的地方。）");
  return lines.join("\n");
}
