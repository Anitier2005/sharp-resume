/**
 * 数据模型 v5：只管理「字段」，不预设任何具体内容。
 *
 * 设计原则（用户定的）：
 *   1. 一个人有什么信息、一段经历有哪些要点，由用户自己加字段决定，代码里不许出现
 *      「姓名」「公司」「职位」「时间」这类预设键
 *   2. 没有语言维度：不区分中英，措辞与分区标题都是用户写的普通文本
 *   3. 新建简历 / 新建页不预置模板，从零开始
 *
 * v5 的改动只有一处：**一页 = 一串块**。页头不再是一个特等公民字段，
 * 它就是第一个 `fields` 块；分区是 `section` 块；上传的整页原件是 `media` 块。
 * 这样「页头 → 分区 → 作品图 → 又一个分区」这种顺序才能表达出来。
 */

/**
 * 材料：图片、链接、文件。
 *
 * 简历之外的东西都靠它进来 —— 证件照、证书扫描件、作品链接、补充材料 PDF。
 * `stored` 在桌面端是 `<数据目录>/assets/` 里的文件名，浏览器调试模式是 data URL。
 */
export interface Asset {
  id: string;
  kind: "image" | "link" | "file";
  /** 原始文件名或链接标题，只用于显示 */
  name: string;
  /** 桌面端：assets 下的存储名；浏览器：data URL；link 类型则是网址 */
  stored: string;
  /** 可选备注，比如「2024 年 6 月取得」 */
  note?: string;
}

/** 字段：用户自己的键值对 */
export interface Field {
  id: string;
  label: string;
  value: string;
  /**
   * 这一项是图片时，指向资产池里的那份文件；有它时 `value` 当图注。
   *
   * 为什么字段要能装图片：证件照、作品截图、证书扫描件在用户眼里就是
   * 「页面上的一项内容」，让他先传到材料区、再指望程序猜哪张是头像，
   * 是把一个结构问题推给人去绕。
   */
  assetId?: string;
}

/** 人的一级：这个人是谁，完全由字段描述 */
export interface Person {
  id: string;
  fields: Field[];
  /** 属于这个人的材料：证件照、证书、作品链接…所有简历共用 */
  assets?: Asset[];
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
  /** 这一条自己的材料：证书扫描件、作品截图、外链 */
  assets?: Asset[];
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

/** 简历页：一串块，按顺序排下去 */
/** 页面排版参数：由「页排版 agent」决定，会话 agent 不改它 */
export interface PageStyle {
  /** 疏密：紧凑 / 标准 / 宽松，决定行高与段间距 */
  density?: "compact" | "normal" | "roomy";
  /** 页边距倍数，0.8–1.6，1 = 默认 */
  margin?: number;
  /** 页头对齐 */
  align?: "left" | "center";
}

/**
 * 页里的块。
 *
 * 形状以 Rust 的 `model::Block` 为准：扁平结构，用 `kind` 区分，
 * 每种块只用到自己那几个字段，其余留空。**不认识的 kind 要原样留着**，
 * 别在读写循环里把它扫掉 —— 老版本遇到新版本写的块不该毁数据。
 */
export interface Block {
  id: string;
  /** fields（字段组，第一块就是页头）/ section（分区）/ media（图片或原件） */
  kind: string;
  fields?: Field[];
  /** section 块的分区标题 */
  title?: string;
  items?: SectionEntry[];
  /** media 块引用的材料 id */
  assetId?: string;
  caption?: string;
  /** contain / cover，只是排版提示 */
  fit?: string;
}

/**
 * 成品层：这一页的版面。
 *
 * 与 `style` 那个「参数层」的区别：参数层只能从几档里挑，成品层是
 * 「整页 / 某一块的真实 HTML」，排成什么样由排版 agent 自己决定。
 * 生成器只在没有覆盖的地方介入 —— 所以内容变了，成品不会失效，
 * 顶多是「按旧内容的量算出来的版面装新内容」，那是排版 agent 下一轮要处理的事。
 */
export interface PageLayout {
  /** classic / compact / roomy / custom；缺省 = classic */
  preset?: string;
  /** 整页接管：存在时生成器完全不介入，parts 与 preset 都不再生效 */
  html?: string;
  /** 块 id → 这一块手写的 HTML；没有的块照常自动生成 */
  parts?: Record<string, string>;
}

export interface ResumePage {
  id: string;
  /** native（我们自己排版）/ file（用户上传的整页原件） */
  kind: string;
  label: string;
  /** file 页引用的材料 id */
  assetId?: string;
  note?: string;
  /** 旧的三档疏密，只在读历史数据时出现；新写入一律走 layout */
  style?: PageStyle;
  /** 成品层：排版 agent 写的版面覆盖 */
  layout?: PageLayout;
  /** 唯一真相：页头、分区、图片都在这里按顺序排 */
  blocks: Block[];
  /** v4 遗留：只在读旧数据时出现，`normalizePage` 会把它搬进 blocks */
  header?: Field[];
  /** v4 遗留：同上 */
  sections?: Section[];
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
  /** 这一页这一条自己带的材料 */
  assets?: Asset[];
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
  /** 正文里的行内标记（如 `@截图.png`）。有它才知道「这句话指的是哪个附件」 */
  token?: string;
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
  /** 5 起：一页 = 一串块（页头也是块） */
  version: 5;
  /**
   * 资产池的元数据。文件本体在 `<数据目录>/pool/`，这里只记「它是什么」。
   * file 页与 media 块通过 id 引用这里的条目；老数据里材料还嵌在人和条目上。
   */
  assets?: Asset[];
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
    /** 右栏当前是编辑还是预览。会话要知道用户在看哪个视图（定位头 + 页 agent 的触发条件） */
    view?: "edit" | "preview";
    /**
     * 页签栏里「快速检索」的页：按最近使用排序，第一个是当前页。
     *
     * 它不是页的完整列表 —— 关掉标签只是从这里移除，页本身还在 resume.pages 里，
     * 所以这份列表可以不含全部页；老数据没有这个字段时按全部页处理。
     */
    openPages?: string[];
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
