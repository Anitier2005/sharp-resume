import type {
  AccentKey,
  Asset,
  BlockRole,
  ChatMessage,
  Field,
  LibraryItem,
  PageLayout,
  PageNote,
  PageStyle,
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
  | { type: "createResume"; personId: string; title?: string; resumeId?: string }
  | { type: "renameResume"; personId: string; resumeId: string; title: string }
  | { type: "deleteResume"; personId: string; resumeId: string }
  | { type: "deletePerson"; personId: string }
  /**
   * 目标页数：用户说「压到一页」时由 `set_target_pages` 写进来。
   * `targetPages` 省略 = 清除（回到「未设置」）—— 不是 0，也不是 null。
   */
  | {
      type: "resumeTargetPages";
      personId: string;
      resumeId: string;
      targetPages?: number;
      /** 写入时对应「第几条用户消息」；缺省 0 */
      turn?: number;
    }
  /* ---------- 人的字段与库 ---------- */
  | { type: "personFields"; personId: string; fields: Field[] }
  | { type: "libraryItems"; personId: string; items: LibraryItem[] }
  | { type: "personAssets"; personId: string; assets: Asset[] }
  | { type: "itemAssets"; personId: string; itemId: string; assets: Asset[] }
  /* ---------- 页面 ---------- */
  | {
      type: "addPage";
      personId: string;
      resumeId: string;
      /** 新页页名；空则沿用「未命名页」 */
      label?: string;
      /**
       * 页的类型：native = 我们自己排版；file = 用户上传的整页原件。
       * 缺省 native（老调用点不用改）。
       */
      kind?: "native" | "file";
      /** file 页引用的材料 id（页面上就印这一份） */
      assetId?: string;
      /**
       * 建完是否把用户带到新页。
       * 注意：真正的「当前页」游标在 provider 里（视图态，不落盘），
       * reducer 只能把新页顶到 `ui.openPages[0]`，由 provider 对齐。
       */
      open?: boolean;
      /** 预生成 id：调用方（agent 通道）需要在 dispatch 之后立刻指向这一页 */
      id?: string;
    }
  | { type: "removePage"; personId: string; resumeId: string; pageId: string }
  | { type: "pageLabel"; personId: string; resumeId: string; pageId: string; label: string }
  | { type: "pageHeader"; personId: string; resumeId: string; pageId: string; header: Field[] }
  | { type: "pageStyle"; personId: string; resumeId: string; pageId: string; style: PageStyle }
  /** 成品层：整页/块级手写覆盖。整体替换，合并逻辑在工具那侧做完 */
  | { type: "pageLayout"; personId: string; resumeId: string; pageId: string; layout: PageLayout }
  /** 工具产出的新材料（图片转化等）。按 id 去重后并进人的材料列表 */
  | { type: "addAssets"; personId: string; assets: Asset[] }
  /**
   * 排版提示：只改「这块用哪种容器」，不动内容。
   * `role` 省略 = 清掉提示，回落 plain。
   */
  | {
      type: "blockRole";
      personId: string;
      resumeId: string;
      pageId: string;
      blockId: string;
      role?: BlockRole;
    }
  /**
   * 纸面批注：按 note.id 去重追加 —— 同一批动作可能重放（界面重连、事件重投），
   * 不去重会在纸面上叠出一模一样的第二个点。
   */
  | {
      type: "pageNote";
      personId: string;
      resumeId: string;
      pageId: string;
      note: PageNote;
    }
  /* ---------- 分区与条目 ---------- */
  | {
      type: "addSection";
      personId: string;
      resumeId: string;
      pageId: string;
      title: string;
      /**
       * 工具预生成的分区块 id。
       * 同一轮里紧跟着的 `add_entry({sectionId})` 用的就是这个 id ——
       * 让 reducer 自己 uid() 的话，那条 op 找不到容器、被静默跳过，
       * 而工具那边已经返回成功了。
       */
      id?: string;
      /** 插到这个块之后；没给就追加到页尾 */
      afterBlockId?: string;
    }
  | {
      type: "removeSection";
      personId: string;
      resumeId: string;
      pageId: string;
      sectionId: string;
    }
  | {
      /** 改写某个块自己的字段（页头块之外的字段组用这条） */
      type: "blockFields";
      personId: string;
      resumeId: string;
      pageId: string;
      blockId: string;
      fields: Field[];
    }
  | {
      /** 删任意块：页头块、媒体块也走这条 */
      type: "removeBlock";
      personId: string;
      resumeId: string;
      pageId: string;
      blockId: string;
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
