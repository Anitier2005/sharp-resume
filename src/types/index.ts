/**
 * 数据模型 v4：只管理「字段」，不预设任何具体内容。
 *
 * 设计原则（用户定的）：
 *   1. 一个人有什么信息、一段经历有哪些要点，由用户自己加字段决定，代码里不许出现
 *      「姓名」「公司」「职位」「时间」这类预设键
 *   2. 没有语言维度：不区分中英，措辞与分区标题都是用户写的普通文本
 *   3. 新建简历 / 新建页不预置模板，从零开始
 */

/** 字段：用户自己的键值对 */
export interface Field {
  id: string;
  label: string;
  value: string;
}

/** 人的一级：这个人是谁，完全由字段描述 */
export interface Person {
  id: string;
  fields: Field[];
  /** 库：经历 / 作品 / 教育 / 证书…本质都是「一组字段 + 一段记录」 */
  items: LibraryItem[];
  /** 会话属于人 */
  messages: ChatMessage[];
  resumes: Resume[];
}

/** 库条目 */
export interface LibraryItem {
  id: string;
  fields: Field[];
  /** 详实记录：当时怎么聊就怎么写 */
  notes: string;
}

/** 二级：一份简历 */
export interface Resume {
  id: string;
  personId: string;
  title: string;
  updatedAt: number;
  pages: ResumePage[];
}

/** 简历页：页头字段 + 若干分区 */
export interface ResumePage {
  id: string;
  label: string;
  header: Field[];
  sections: Section[];
}

/** 分区：标题由用户起（"工作经历" / "Awards" / "开源项目"…） */
export interface Section {
  id: string;
  title: string;
  items: SectionEntry[];
}

/** 分区里的一条：可以引用库条目，也可以只在本页存在 */
export interface SectionEntry {
  id: string;
  /** 引用库条目 id；为空表示这一页临时写的 */
  refId?: string;
  fields: Field[];
  /** 本页措辞，按句存，供 AI 单句改写 */
  bullets: string[];
}

export interface Attachment {
  id: string;
  name: string;
  size: number;
  ext: string;
  /** 图片的临时缩略图（object URL）。只在待发送时存在，落盘前会被剥掉 */
  preview?: string;
}

/** AI 给出的一处改写 */
export interface ChangeItem {
  label: string;
  before: string;
  after: string;
  /** 绑定到本页某条的某一句 */
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
  tool?: string;
  changes?: ChangeItem[];
  actions?: ChatAction[];
}

export interface Workspace {
  /** 4 起：字段化，去掉语言与页面类型 */
  version: 4;
  people: Person[];
  ui: {
    accent: AccentKey;
    accentHex?: string;
    mode: "light" | "dark";
    activePersonId: string | null;
    activeResumeId: string | null;
    expanded?: string[];
    textScale?: number | null;
    sideW?: number;
    docW?: number;
    showSide?: boolean;
    showDoc?: boolean;
  };
}

export type AccentKey = "white" | "indigo" | "pine" | "clay";

/** 界面字号档位 */
export const TEXT_SCALES: { key: string; label: string; value: number }[] = [
  { key: "sm", label: "小", value: 1.05 },
  { key: "md", label: "标准", value: 1.2 },
  { key: "lg", label: "大", value: 1.35 },
  { key: "xl", label: "特大", value: 1.5 },
];

export const AUTO_TEXT_SCALE_FLOOR = 1.2;

export const ACCENTS: { key: AccentKey; label: string; color: string }[] = [
  { key: "white", label: "白", color: "#FFFFFF" },
  { key: "indigo", label: "靛", color: "#3B50B4" },
  { key: "pine", label: "松", color: "#2A6650" },
  { key: "clay", label: "朱", color: "#B2452C" },
];

/** 新字段的空白骨架（label 留空，让用户自己起名） */
export const blankField = (id: string): Field => ({ id, label: "", value: "" });

/** 新人物的显示名：取第一个有值的字段，没有就叫「未命名」 */
export function displayName(fields: Field[]): string {
  const hit = fields.find((field) => field.value.trim());
  return hit ? hit.value.trim().slice(0, 24) : "未命名";
}

/** 展示用：字段的「标签：值」，标签为空就只显示值 */
export function fieldText(field: Field): string {
  const value = field.value.trim();
  if (!field.label.trim()) return value;
  return value ? `${field.label}：${value}` : `${field.label}：`;
}
