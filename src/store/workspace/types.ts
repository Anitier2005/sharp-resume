import type {
  AccentKey,
  Asset,
  ChatMessage,
  Field,
  LibraryItem,
  Person,
  ResumePage,
  Section,
  SectionEntry,
  Workspace,
} from "@/types";

export type Action =
  | { type: "hydrate"; workspace: Workspace }
  | { type: "selectPerson"; personId: string }
  | { type: "togglePerson"; personId: string }
  | { type: "selectResume"; personId: string; resumeId: string }
  | { type: "setAccent"; accent: AccentKey; accentHex?: string }
  | { type: "setMode"; mode: "light" | "dark" }
  | { type: "setTextScale"; textScale: number | null }
  | { type: "patchUi"; patch: Partial<Workspace["ui"]> }
  | { type: "createPerson" }
  | { type: "createResume"; personId: string }
  | { type: "renameResume"; personId: string; resumeId: string; title: string }
  | { type: "deleteResume"; personId: string; resumeId: string }
  | { type: "deletePerson"; personId: string }
  /* ---------- 人的字段与库 ---------- */
  | { type: "personFields"; personId: string; fields: Field[] }
  | { type: "libraryItems"; personId: string; items: LibraryItem[] }
  | { type: "personAssets"; personId: string; assets: Asset[] }
  | { type: "itemAssets"; personId: string; itemId: string; assets: Asset[] }
  /* ---------- 页面 ---------- */
  | { type: "addPage"; personId: string; resumeId: string }
  | { type: "removePage"; personId: string; resumeId: string; pageId: string }
  | { type: "pageLabel"; personId: string; resumeId: string; pageId: string; label: string }
  | { type: "pageHeader"; personId: string; resumeId: string; pageId: string; header: Field[] }
  /* ---------- 分区与条目 ---------- */
  | { type: "addSection"; personId: string; resumeId: string; pageId: string; title: string }
  | {
      type: "removeSection";
      personId: string;
      resumeId: string;
      pageId: string;
      sectionId: string;
    }
  | {
      type: "sectionTitle";
      personId: string;
      resumeId: string;
      pageId: string;
      sectionId: string;
      title: string;
    }
  | {
      type: "sectionItems";
      personId: string;
      resumeId: string;
      pageId: string;
      sectionId: string;
      items: SectionEntry[];
    }
  | {
      type: "entryPatch";
      personId: string;
      resumeId: string;
      pageId: string;
      entryId: string;
      patch: Partial<SectionEntry>;
    }
  /* ---------- 会话（人这一级） ---------- */
  | { type: "appendMessage"; personId: string; message: ChatMessage }
  | { type: "patchMessage"; personId: string; messageId: string; patch: Partial<ChatMessage> };

export type { Field, LibraryItem, ResumePage, Section, SectionEntry, Person };
