export type PageKind = "resume-zh" | "resume-en" | "portfolio";

export interface Experience {
  id: string;
  company: string;
  role: string;
  period: string;
  team?: string;
  bullets: string[];
}

export interface ProjectItem {
  id: string;
  title: string;
  desc: string;
  tags: string[];
  year: string;
  cover: string;
}

export interface EducationItem {
  id: string;
  school: string;
  degree: string;
  period: string;
}

export interface SkillGroup {
  label: string;
  value: string;
}

export interface ResumeDoc {
  kind: "resume-zh" | "resume-en";
  name: string;
  title: string;
  meta: string[];
  summary: string;
  experiences: Experience[];
  projects: ProjectItem[];
  education: EducationItem[];
  skills: SkillGroup[];
}

export interface PortfolioDoc {
  kind: "portfolio";
  name: string;
  role: string;
  year: string;
  intro: string;
  projects: ProjectItem[];
  contact: string[];
}

export type Doc = ResumeDoc | PortfolioDoc;

export interface ResumePage {
  id: string;
  kind: PageKind;
  label: string;
  badge?: string;
  updatedAt: string;
  doc: Doc;
}

export interface ChatAttachment {
  id: string;
  name: string;
  size: number;
  ext: string;
}

export interface ChangeItem {
  label: string;
  detail: string;
}

export interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  time: string;
  attachments?: ChatAttachment[];
  changes?: ChangeItem[];
  changesTitle?: string;
  fileMeta?: { name: string; pages: number; keywords: string[] };
}

export interface ResumeItem {
  id: string;
  name: string;
  role: string;
  company: string;
  updatedAt: string;
  accent: string;
  pages: ResumePage[];
  messages: ChatMessage[];
}

export const PAGE_KIND_META: Record<
  PageKind,
  { label: string; short: string; icon: "zh" | "en" | "portfolio" }
> = {
  "resume-zh": { label: "中文简历", short: "中", icon: "zh" },
  "resume-en": { label: "English Resume", short: "EN", icon: "en" },
  portfolio: { label: "作品集", short: "作品", icon: "portfolio" },
};
