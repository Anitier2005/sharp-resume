/** 页面类型：中文简历 / 英文简历 / 作品集 */
export type PageKind = "zh" | "en" | "portfolio";

/** 库里的细节事实：一条一个点（背景 / 动作 / 结果 / 数据…），供改简历时挑着用 */
export interface ExperienceFact {
  id: string;
  /** 事实类型，自由文本，常见值见 FACT_LABELS */
  label: string;
  text: string;
}

/**
 * 共享经历：属于「人」，被这个人名下所有简历共享。
 *
 * 注意这里**没有** bullets —— 经历库是「详实记录」，不是简历输出。
 * 简历上的措辞在 ResumeEntry 里，每份简历、每个岗位各写一版。
 */
export interface Experience {
  id: string;
  company: string;
  companyEn?: string;
  role: string;
  roleEn?: string;
  period: string;
  /** 详实记录：用户母语自由书写，可以很长、很碎、很口语，不进简历 */
  notes: string;
  /** 结构化细节，一条一个点 */
  facts: ExperienceFact[];
}

/**
 * 简历页里对某段经历的措辞：拿库里的记录改写成这个岗位要强调的说法。
 * 库里不留副本，所以同一段经历在不同简历里可以写得完全不同。
 */
export interface ResumeEntry {
  id: string;
  /** 引用哪段库经历（公司 / 职位 / 时间从库里取） */
  experienceId: string;
  /** 本简历的措辞，按句存，便于单句改写 */
  bullets: string[];
  /** 英文措辞，缺省时英文页回落到中文 */
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
  /** 会话属于「人」：换岗位简历不换会话，问过的话不会凭空消失 */
  messages: ChatMessage[];
  resumes: Resume[];
}

/** 二级：简历，一份简历 = 一组页面（会话在人这一级，见 Person.messages） */
export interface Resume {
  id: string;
  personId: string;
  title: string;
  updatedAt: number;
  pages: ResumePage[];
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
  /** 本页放哪几段经历，以及每一段在这个岗位下怎么讲 */
  entries: ResumeEntry[];
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
  /** 绑定到本页某条 entry 的某一句，应用时写回这一个页面（不碰经历库） */
  target?: { entryId: string; bulletIndex: number };
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
  /** 3 起：经历库与简历措辞分离、会话归到「人」（见 lib/migrate.ts） */
  version: 3;
  people: Person[];
  ui: {
    accent: AccentKey;
    /** 自定义强调色（#rrggbb）。有值时覆盖 accent 预设色 */
    accentHex?: string;
    mode: "light" | "dark";
    activePersonId: string | null;
    activeResumeId: string | null;
    /** 侧栏展开了哪些候选人 */
    expanded?: string[];
    /**
     * UI 字号倍率。缺省（undefined / null）表示自动：
     * 跟随 Windows「文本大小」设置，且不低于可读下限。
     * 用户在外观里选过档位后存入具体数值，之后不再受系统设置影响。
     */
    textScale?: number | null;
    /** 三栏宽度（px）。拖分隔条改变，持久化在这里 */
    sideW?: number;
    docW?: number;
    /** 左栏 / 右侧简历区是否显示 */
    showSide?: boolean;
    showDoc?: boolean;
  };
}

export type AccentKey = "white" | "indigo" | "pine" | "clay";

/** 细节事实的常用类型，只是输入提示，可以自己填 */
export const FACT_LABELS = ["背景", "动作", "结果", "数据", "团队", "技术栈", "难点"];

/** 新建一段经历时的空白骨架 */
export function blankExperience(id: string): Experience {
  return {
    id,
    company: "",
    companyEn: "",
    role: "",
    roleEn: "",
    period: "",
    notes: "",
    facts: [],
  };
}

/** UI 字号档位。semantic 值直接作为 CSS 的 --text-scale。 */
export const TEXT_SCALES: { key: string; label: string; value: number }[] = [
  { key: "sm", label: "小", value: 1.05 },
  { key: "md", label: "标准", value: 1.2 },
  { key: "lg", label: "大", value: 1.35 },
  { key: "xl", label: "特大", value: 1.5 },
];

/** 自动档取系统值与可读下限的较大者；应用内选过档位则用档位值。 */
export const AUTO_TEXT_SCALE_FLOOR = 1.2;

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
