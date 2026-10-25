import type { Experience, Person, Resume, ResumeEntry, ResumePage, Workspace } from "@/types";
import { uid } from "@/lib/uid";

/**
 * 工作区版本迁移。
 *
 * v1 → v2 的变化：
 *   - 经历库不再存简历措辞：`Experience.bullets` 拆成
 *     `notes`（详实记录，原文照搬不丢内容）+ `facts`（结构化细节，初始为空）
 *   - 简历页不再只是「引用 id」：`page.experienceIds` 变成 `page.entries`，
 *     每条 entry 持有**这一页自己的措辞**，迁移时把原来的 bullets 原样抄进去
 *
 * 迁移必须无损：用户已经写好的句子一句都不能少，只是换了存放位置。
 */

interface LegacyExperience extends Partial<Experience> {
  bullets?: string[];
  bulletsEn?: string[];
}

interface LegacyPage extends Partial<ResumePage> {
  experienceIds?: string[];
}

function toEntry(
  experienceId: string,
  src: { zh: string[]; en?: string[] } | undefined,
): ResumeEntry {
  return {
    id: uid("en"),
    experienceId,
    bullets: [...(src?.zh ?? [])],
    bulletsEn: src?.en?.length ? [...src.en] : undefined,
  };
}

function migratePerson(person: Person): Person {
  const legacyList = (person.experiences ?? []) as LegacyExperience[];

  /** 迁移时留一份「原措辞」表，供页面的 entries 抄写 */
  const wording = new Map<string, { zh: string[]; en?: string[] }>();
  const experiences: Experience[] = legacyList.map((item) => {
    const id = item.id ?? uid("e");
    wording.set(id, { zh: item.bullets ?? [], en: item.bulletsEn });
    return {
      id,
      company: item.company ?? "",
      companyEn: item.companyEn,
      role: item.role ?? "",
      roleEn: item.roleEn,
      period: item.period ?? "",
      notes: (item.bullets ?? []).join("\n"),
      facts: [],
    };
  });

  const resumes: Resume[] = (person.resumes ?? []).map((resume) => ({
    ...resume,
    pages: (resume.pages ?? []).map((page) => {
      const legacy = page as LegacyPage;
      const entries = (legacy.experienceIds ?? []).map((id) =>
        toEntry(id, wording.get(id)),
      );
      return {
        id: legacy.id ?? uid("pg"),
        kind: legacy.kind ?? "zh",
        label: legacy.label ?? "中文简历",
        headline: legacy.headline ?? "",
        role: legacy.role ?? "",
        meta: legacy.meta ?? [],
        summary: legacy.summary,
        entries,
        projectIds: legacy.projectIds ?? [],
      } satisfies ResumePage;
    }),
  }));

  return { ...person, experiences, resumes };
}

/** 把任意读到的数据拉到当前版本；已是 v2 就原样返回 */
export function migrateWorkspace(raw: unknown): Workspace {
  const ws = raw as Partial<Workspace> & { version?: number };
  if (!ws || typeof ws !== "object" || !Array.isArray(ws.people)) {
    throw new Error("工作区数据格式不对");
  }
  if (ws.version === 2) return ws as Workspace;

  return {
    version: 2,
    people: ws.people.map(migratePerson),
    ui: {
      accent: ws.ui?.accent ?? "white",
      mode: ws.ui?.mode ?? "light",
      activePersonId: ws.ui?.activePersonId ?? null,
      activeResumeId: ws.ui?.activeResumeId ?? null,
      expanded: ws.ui?.expanded ?? [],
      textScale: ws.ui?.textScale ?? null,
    },
  };
}
