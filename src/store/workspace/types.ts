import type {
  AccentKey,
  ChatMessage,
  Experience,
  PageKind,
  Person,
  ResumeEntry,
  ResumePage,
  Workspace,
} from "@/types";

/** 页面引用字段：作品仍是「勾选 + 排序」的字符串列表；经历改由 entries 承载 */
export type PageRefField = "projectIds";

export type Action =
  | { type: "hydrate"; workspace: Workspace }
  | { type: "selectPerson"; personId: string }
  | { type: "togglePerson"; personId: string }
  | { type: "selectResume"; personId: string; resumeId: string }
  | { type: "setAccent"; accent: AccentKey; accentHex?: string }
  | { type: "setMode"; mode: "light" | "dark" }
  | { type: "setTextScale"; textScale: number | null }
  | { type: "createPerson" }
  | { type: "createResume"; personId: string }
  | { type: "renameResume"; personId: string; resumeId: string; title: string }
  | { type: "deleteResume"; personId: string; resumeId: string }
  | { type: "addPage"; personId: string; resumeId: string; kind: PageKind }
  | { type: "removePage"; personId: string; resumeId: string; pageId: string }
  | { type: "updatePage"; personId: string; resumeId: string; pageId: string; patch: Partial<ResumePage> }
  | { type: "updateExperience"; personId: string; expId: string; patch: Partial<Experience> }
  | { type: "patchPerson"; personId: string; patch: Partial<Person> }
  | { type: "removeExperience"; personId: string; expId: string }
  /** 把一段库经历加进这一页（同时开一条空措辞） */
  | {
      type: "addEntry";
      personId: string;
      resumeId: string;
      pageId: string;
      experienceId: string;
      /** 起草用的初始措辞；不给就是空 */
      bullets?: string[];
    }
  | { type: "removeEntry"; personId: string; resumeId: string; pageId: string; entryId: string }
  | {
      type: "moveEntry";
      personId: string;
      resumeId: string;
      pageId: string;
      entryId: string;
      dir: -1 | 1;
    }
  | {
      type: "updateEntry";
      personId: string;
      resumeId: string;
      pageId: string;
      entryId: string;
      patch: Partial<ResumeEntry>;
    }
  | {
      type: "togglePageItem";
      personId: string;
      resumeId: string;
      pageId: string;
      field: PageRefField;
      itemId: string;
    }
  | {
      type: "movePageItem";
      personId: string;
      resumeId: string;
      pageId: string;
      field: PageRefField;
      itemId: string;
      dir: -1 | 1;
    }
  /** 会话属于「人」，所以这两个动作不认 resumeId */
  | { type: "appendMessage"; personId: string; message: ChatMessage }
  | { type: "patchMessage"; personId: string; messageId: string; patch: Partial<ChatMessage> }
  | { type: "deletePerson"; personId: string }
  /** 通用 UI 状态补丁：栏宽、面板显隐等零散偏好都走这里 */
  | { type: "patchUi"; patch: Partial<Workspace["ui"]> };
