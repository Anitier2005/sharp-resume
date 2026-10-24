import type { AccentKey, ChatMessage, Experience, PageKind, Person, ResumePage, Workspace } from "@/types";

/** 页面引用字段：决定「这一页放哪些经历 / 作品」 */
export type PageRefField = "experienceIds" | "projectIds";

export type Action =
  | { type: "hydrate"; workspace: Workspace }
  | { type: "selectPerson"; personId: string }
  | { type: "togglePerson"; personId: string }
  | { type: "selectResume"; personId: string; resumeId: string }
  | { type: "setAccent"; accent: AccentKey }
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
  | { type: "appendMessage"; personId: string; resumeId: string; message: ChatMessage }
  | { type: "patchMessage"; personId: string; resumeId: string; messageId: string; patch: Partial<ChatMessage> };
