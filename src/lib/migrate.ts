import type {
  Field,
  LibraryItem,
  Person,
  Resume,
  ResumePage,
  Section,
  SectionEntry,
  Workspace,
} from "@/types";
import { uid } from "@/lib/uid";

/**
 * 工作区迁移：任意历史版本 → v4（纯字段模型）。
 *
 * 历史：
 *   v1  经历库存 bullets，页面只存 experienceIds
 *   v2  措辞拆到页面 entries，库存 notes + facts
 *   v3  会话从简历上移到人
 *   v4  **字段化**：人的信息、库条目、页头、分区条目全变成自由字段；
 *       去掉页面类型（中文/英文/作品集）与语言维度
 *
 * 单向、幂等：v4 直接原样返回；更早的按字段有没有来判断，重复跑不会二次改写。
 * 迁移必须无损：原有内容全部变成字段值，一句不丢（只是键名由用户接管）。
 */

type Loose = Record<string, unknown>;

const text = (value: unknown): string => (typeof value === "string" ? value.trim() : "");

function field(label: string, value: unknown): Field | null {
  const content = text(value);
  return content ? { id: uid("f"), label, value: content } : null;
}

function compact(list: (Field | null)[]): Field[] {
  return list.filter((item): item is Field => item !== null);
}

/** v1 的 bullets / v2 的 notes 都在这一层 */
function migrateExperience(item: Loose): LibraryItem {
  const facts = Array.isArray(item.facts) ? (item.facts as Loose[]) : [];
  const fields = compact([
    field("公司", item.company),
    field("职位", item.role),
    field("时间", item.period),
    ...facts.map((fact) => field(text(fact.label) || "细节", fact.text)),
  ]);
  const notes = text(item.notes) || (Array.isArray(item.bullets) ? (item.bullets as string[]).join("\n") : "");
  return { id: text(item.id) || uid("it"), fields, notes };
}

function migrateProject(item: Loose): LibraryItem {
  const tags = Array.isArray(item.tags) ? (item.tags as string[]).join("、") : "";
  // desc 在 v3 是**会印在简历上的**描述，所以迁成字段而不是 notes ——
  // notes 是只给自己看的素材，落到那里等于从纸上消失。
  const fields = compact([
    field("项目", item.title),
    field("时间", item.year),
    field("简介", item.desc),
    field("标签", tags),
  ]);
  return { id: text(item.id) || uid("it"), fields, notes: "" };
}

function migrateEducation(item: Loose): LibraryItem {
  const fields = compact([
    field("学校", item.school),
    field("学位", item.degree),
    field("时间", item.period),
  ]);
  return { id: text(item.id) || uid("it"), fields, notes: "" };
}

function migrateSkill(item: Loose): LibraryItem {
  const fields = compact([field("类别", item.label), field("内容", item.value)]);
  return { id: text(item.id) || uid("it"), fields, notes: "" };
}

/** 旧页面的 entries（引用经历 + 本页措辞）→ 分区条目 */
function migrateEntry(entry: Loose): SectionEntry {
  const zh = Array.isArray(entry.bullets) ? (entry.bullets as string[]) : [];
  const en = Array.isArray(entry.bulletsEn) ? (entry.bulletsEn as string[]) : [];
  return {
    id: text(entry.id) || uid("se"),
    refId: text(entry.experienceId) || undefined,
    fields: [],
    // 不再区分语言：哪个有内容用哪个，都有就合并（去重）
    bullets: [...new Set([...zh, ...en].map((line) => line.trim()).filter(Boolean))],
  };
}

function migratePage(page: Loose, order: number): ResumePage {
  const header = compact([
    field("姓名", page.headline),
    field("职位", page.role),
    ...(Array.isArray(page.meta) ? (page.meta as string[]).map((item) => field("", item)) : []),
    field("摘要", page.summary),
  ]);

  const sections: Section[] = [];
  const oldEntries = Array.isArray(page.entries) ? (page.entries as Loose[]) : [];

  if (oldEntries.length) {
    sections.push({
      id: uid("sc"),
      title: "工作经历",
      items: oldEntries.map((entry) => migrateEntry(entry)),
    });
  }
  if (Array.isArray(page.experienceIds) && (page.experienceIds as string[]).length) {
    // 极早期数据：只存了 id，没有措辞
    sections.push({
      id: uid("sc"),
      title: "工作经历",
      items: (page.experienceIds as string[]).map((id) => ({
        id: uid("se"),
        refId: id,
        fields: [],
        bullets: [],
      })),
    });
  }
  if (Array.isArray(page.projectIds) && (page.projectIds as string[]).length) {
    sections.push({
      id: uid("sc"),
      title: "作品",
      items: (page.projectIds as string[]).map((id) => ({
        id: uid("se"),
        refId: id,
        fields: [],
        bullets: [],
      })),
    });
  }

  return {
    id: text(page.id) || uid("pg"),
    label: text(page.label) || `第 ${order + 1} 页`,
    header,
    sections,
  };
}

function migratePerson(person: Loose): Person {
  const experiences = Array.isArray(person.experiences) ? (person.experiences as Loose[]) : [];
  const projects = Array.isArray(person.projects) ? (person.projects as Loose[]) : [];
  const education = Array.isArray(person.education) ? (person.education as Loose[]) : [];
  const skills = Array.isArray(person.skills) ? (person.skills as Loose[]) : [];

  const fields = Array.isArray(person.fields)
    ? (person.fields as Field[])
    : compact([
        field("姓名", person.name),
        field("职位", person.role),
        field("邮箱", person.email),
        field("城市", person.location),
        field("年限", person.years ? `${person.years} 年` : ""),
      ]);

  const items = Array.isArray(person.items)
    ? (person.items as LibraryItem[])
    : [
        ...experiences.map(migrateExperience),
        ...projects.map(migrateProject),
        ...education.map(migrateEducation),
        ...skills.map(migrateSkill),
      ];

  const resumes: Resume[] = (Array.isArray(person.resumes) ? (person.resumes as Loose[]) : []).map(
    (resume) => ({
      id: text(resume.id) || uid("r"),
      personId: text(resume.personId) || text(person.id) || uid("p"),
      title: text(resume.title) || "未命名简历",
      updatedAt: typeof resume.updatedAt === "number" ? resume.updatedAt : Date.now(),
      pages: (Array.isArray(resume.pages) ? (resume.pages as Loose[]) : []).map(migratePage),
    }),
  );

  return {
    id: text(person.id) || uid("p"),
    fields,
    items,
    messages: Array.isArray(person.messages) ? (person.messages as Person["messages"]) : [],
    resumes,
  };
}

export function migrateWorkspace(raw: unknown): Workspace {
  const src = raw as { version?: number; people?: Loose[]; ui?: Workspace["ui"] } | undefined;
  if (!src || typeof src !== "object" || !Array.isArray(src.people)) {
    throw new Error("工作区数据格式不对");
  }
  if (src.version === 4) return raw as Workspace;

  return {
    version: 4,
    people: src.people.map(migratePerson),
    ui: {
      accent: src.ui?.accent ?? "white",
      accentHex: src.ui?.accentHex,
      mode: src.ui?.mode ?? "light",
      activePersonId: src.ui?.activePersonId ?? null,
      activeResumeId: src.ui?.activeResumeId ?? null,
      expanded: src.ui?.expanded ?? [],
      textScale: src.ui?.textScale ?? null,
      sideW: src.ui?.sideW,
      docW: src.ui?.docW,
      showSide: src.ui?.showSide,
      showDoc: src.ui?.showDoc,
    },
  };
}
