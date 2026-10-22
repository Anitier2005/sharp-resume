/** 页面类型：中文简历 / 英文简历 / 作品集 */
export type PageKind = "zh" | "en" | "portfolio";

/** 共享经历：属于「人」，被这个人名下所有简历引用 */
export interface Experience {
  id: string;
  company: string;
  companyEn?: string;
  role: string;
  roleEn?: string;
  period: string;
  /** 中文描述，按句存，便于单独改写 */
  bullets: string[];
  /** 英文描述，缺省时英文页回落到中文 */
  bulletsEn?: string[];
}

/** 共享作品：属于「人」 */
export interface ProjectItem {
  id: string;
  title: string;
  titleEn?: string;
  year: string;
  desc: string;
  descEn?: string;
  tags: string[];
  /** 作品集封面用的色相序号 0-3 */
  cover: number;
}

export interface EducationItem {
  id: string;
  school: string;
  schoolEn?: string;
  degree: string;
  degreeEn?: string;
  period: string;
}

export interface SkillGroup {
  id: string;
  label: string;
  labelEn?: string;
  value: string;
  valueEn?: string;
}

/** 一级：人 */
export interface Person {
  id: string;
  name: string;
  nameEn: string;
  role: string;
  roleEn: string;
  email: string;
  location: string;
  years: number;
  experiences: Experience[];
  projects: ProjectItem[];
  education: EducationItem[];
  skills: SkillGroup[];
  resumes: Resume[];
}

/** 二级：简历，一份简历 = 一条独立会话 + 一组页面 */
export interface Resume {
  id: string;
  personId: string;
  title: string;
  updatedAt: number;
  pages: ResumePage[];
  messages: ChatMessage[];
}

/** 页面：同一份简历下的中文 / 英文 / 作品集 */
export interface ResumePage {
  id: string;
  kind: PageKind;
  label: string;
  /** 页头姓名（中文页用 name，英文页用 nameEn） */
  headline: string;
  /** 目标岗位，显示为职位行 */
  role: string;
  /** 邮箱 · 城市 · 年限 */
  meta: string[];
  /** 一句话摘要，可选 */
  summary?: string;
  /** 引用哪些共享经历，按顺序 */
  experienceIds: string[];
  /** 引用哪些共享作品，按顺序 */
  projectIds: string[];
}

export interface Attachment {
  id: string;
  name: string;
  size: number;
  ext: string;
}

/** AI 给出的一处改写 */
export interface ChangeItem {
  label: string;
  before: string;
  after: string;
  /** 绑定到某条经历的某一句，应用时写回共享库 */
  target?: { experienceId: string; bulletIndex: number };
}

export interface ChatAction {
  id: string;
  label: string;
  kind: "apply" | "hint" | "undo";
}

export interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  time: number;
  attachments?: Attachment[];
  /** AI 的工具行，如「已解析 JD · 提取 12 个关键词」 */
  tool?: string;
  changes?: ChangeItem[];
  actions?: ChatAction[];
}

export interface Workspace {
  version: 1;
  people: Person[];
  ui: {
    accent: AccentKey;
    mode: "light" | "dark";
    activePersonId: string | null;
    activeResumeId: string | null;
    /** 侧栏展开了哪些候选人 */
    expanded?: string[];
  };
}

export type AccentKey = "white" | "indigo" | "pine" | "clay";

export const ACCENTS: { key: AccentKey; label: string; color: string }[] = [
  { key: "white", label: "白", color: "#FFFFFF" },
  { key: "indigo", label: "靛", color: "#3B50B4" },
  { key: "pine", label: "松", color: "#2A6650" },
  { key: "clay", label: "朱", color: "#B2452C" },
];

export const PAGE_KIND_LABEL: Record<PageKind, string> = {
  zh: "中文简历",
  en: "English",
  portfolio: "作品集",
};

/** 侧栏里显示的页面缩写 */
export function pageShort(kind: PageKind): string {
  return kind === "zh" ? "中" : kind === "en" ? "EN" : "作品";
}
