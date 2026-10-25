import type {
  ChatMessage,
  Experience,
  ExperienceFact,
  Person,
  Resume,
  ResumeEntry,
  ResumePage,
  Workspace,
} from "@/types";
import { uid } from "@/lib/uid";

/**
 * 工作区版本迁移 —— 单遍、可重复执行。
 *
 * 历史包袱（按时间顺序）：
 *   v1  经历库把简历措辞直接存成 `Experience.bullets`；简历页只存 `experienceIds`
 *   v2  把措辞拆出来：库只留 `notes` + `facts`，页面用 `entries` 持有自己的措辞
 *   v3  会话从「简历」上移到「人」（换岗位不该换会话）
 *
 * 三个版本都必须能落到当前版本，所以这里不写「v1→v2→v3」的阶梯，
 * 而是逐字段判断「有没有、要不要补」—— 每一步都幂等，重复跑不会二次改写。
 * 迁移必须无损：用户写过的句子一句都不能少，只是换个位置存。
 */

type LegacyExperience = Partial<Experience> & {
  bullets?: string[];
  bulletsEn?: string[];
};

type LegacyPage = Partial<ResumePage> & {
  experienceIds?: string[];
};

type LegacyResume = Omit<Partial<Resume>, "pages"> & {
  pages?: LegacyPage[];
  messages?: ChatMessage[];
};

type LegacyPerson = Omit<Partial<Person>, "experiences" | "resumes" | "messages"> & {
  experiences?: LegacyExperience[];
  resumes?: LegacyResume[];
  messages?: ChatMessage[];
};

type LegacyWorkspace = {
  version?: number;
  people?: LegacyPerson[];
  ui?: Partial<Workspace["ui"]>;
};

function toEntry(
  experienceId: string,
  src: { zh: string[]; en?: string[] } | undefined,
  fallbackId: string,
): ResumeEntry {
  return {
    id: fallbackId,
    experienceId,
    bullets: [...(src?.zh ?? [])],
    bulletsEn: src?.en?.length ? [...src.en] : undefined,
  };
}

function migratePerson(person: LegacyPerson, index: number): Person {
  /** 迁移时留一份「原措辞」表，供页面的 entries 抄写 */
  const wording = new Map<string, { zh: string[]; en?: string[] }>();

  const experiences: Experience[] = (person.experiences ?? []).map((item) => {
    const id = item.id ?? uid("e");
    wording.set(id, { zh: item.bullets ?? [], en: item.bulletsEn });
    const facts: ExperienceFact[] = Array.isArray(item.facts) ? item.facts : [];
    return {
      id,
      company: item.company ?? "",
      companyEn: item.companyEn,
      role: item.role ?? "",
      roleEn: item.roleEn,
      period: item.period ?? "",
      // v1 没有 notes，把旧措辞原样当记录留下，一句不丢
      notes: item.notes ?? (item.bullets ?? []).join("\n"),
      facts,
    };
  });

  const resumes: Resume[] = (person.resumes ?? []).map((resume) => {
    const resumeId = resume.id ?? uid("r");
    const pages: ResumePage[] = (resume.pages ?? []).map((legacy, pi) => {
      const entries =
        legacy.entries ??
        (legacy.experienceIds ?? []).map((expId, ei) =>
          toEntry(expId, wording.get(expId), `${resumeId}-${pi}-${ei}`),
        );
      return {
        id: legacy.id ?? uid("pg"),
        kind: legacy.kind ?? "zh",
        label: legacy.label ?? "简历页",
        headline: legacy.headline ?? "",
        role: legacy.role ?? "",
        meta: legacy.meta ?? [],
        summary: legacy.summary,
        entries,
        projectIds: legacy.projectIds ?? [],
      };
    });
    return {
      id: resumeId,
      personId: resume.personId ?? person.id ?? `p-${index}`,
      title: resume.title ?? "通用版",
      updatedAt: resume.updatedAt ?? Date.now(),
      pages,
    };
  });

  /** v3：把散在各份简历上的会话合并到人这一级，按时间排序 */
  const messages: ChatMessage[] =
    person.messages ??
    (person.resumes ?? [])
      .flatMap((resume) => resume.messages ?? [])
      .sort((a, b) => a.time - b.time);

  return {
    id: person.id ?? uid("p"),
    name: person.name ?? "新候选人",
    nameEn: person.nameEn ?? "New Candidate",
    role: person.role ?? "",
    roleEn: person.roleEn ?? "",
    email: person.email ?? "",
    location: person.location ?? "",
    years: person.years ?? 0,
    experiences,
    projects: person.projects ?? [],
    education: person.education ?? [],
    skills: person.skills ?? [],
    messages,
    resumes,
  };
}

/** 把任意读到的数据拉到当前版本；已是 v3 就原样返回 */
export function migrateWorkspace(raw: unknown): Workspace {
  const ws = raw as LegacyWorkspace | undefined;
  if (!ws || typeof ws !== "object" || !Array.isArray(ws.people)) {
    throw new Error("工作区数据格式不对");
  }
  if (ws.version === 3) return ws as Workspace;

  return {
    version: 3,
    people: ws.people.map(migratePerson),
    ui: {
      accent: ws.ui?.accent ?? "white",
      mode: ws.ui?.mode ?? "light",
      activePersonId: ws.ui?.activePersonId ?? null,
      activeResumeId: ws.ui?.activeResumeId ?? null,
      expanded: ws.ui?.expanded ?? [],
      textScale: ws.ui?.textScale ?? null,
      sideW: ws.ui?.sideW,
      docW: ws.ui?.docW,
      showSide: ws.ui?.showSide,
      showDoc: ws.ui?.showDoc,
    },
  };
}
