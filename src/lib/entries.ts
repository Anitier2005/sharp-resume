import type { Experience, Person, ResumeEntry, ResumePage } from "@/types";

/**
 * 经历条目在本页的解析。
 *
 * 「库」与「简历」是两层：库给公司 / 职位 / 时间这些事实，措辞只存在于页面的
 * entry 上。纸面、Markdown、Word、AI 提示词都必须走这里取数，否则各处对
 * 「这一页到底写什么」会各有一套判断，迟早漂移。
 */

export interface ResolvedEntry {
  entry: ResumeEntry;
  exp: Experience;
}

/** 本页引用的经历，按页面顺序；库记录被删掉的悬空条目自动跳过 */
export function resolveEntries(person: Person, page: ResumePage): ResolvedEntry[] {
  return page.entries
    .map((entry) => ({
      entry,
      exp: person.experiences.find((item) => item.id === entry.experienceId),
    }))
    .filter((row): row is ResolvedEntry => Boolean(row.exp));
}

/** 英文页优先英文措辞；没写英文就回落中文，避免英文页整段空白 */
export function wordingOf(entry: ResumeEntry, en: boolean): string[] {
  if (en) return entry.bulletsEn?.length ? entry.bulletsEn : entry.bullets;
  return entry.bullets;
}

/** 本页已经挂上的经历 id，用于「加入经历」里过滤掉重复项 */
export function attachedIds(page: ResumePage): Set<string> {
  return new Set(page.entries.map((entry) => entry.experienceId));
}

/**
 * 从详实记录里起草几条要点：按行切，去掉空行与纯标点行。
 *
 * 刻意只做「搬句子」不做润色 —— 起草是给用户一个可以马上删改的起点，
 * 真正贴合岗位的措辞仍然要人来定。
 */
export function draftFromNotes(notes: string, limit = 8): string[] {
  return notes
    .split(/\r?\n/)
    .map((line) => line.replace(/^[\s·•\-*—]+/, "").trim())
    .filter((line) => line.length > 1 && /[\p{L}\p{N}]/u.test(line))
    .slice(0, limit);
}
