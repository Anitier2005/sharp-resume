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
  /**
   * candidate = 从 PDF 里抽出来、等着被选中的图。渲染层不会拿它当页头照片 ——
   * 不跳过的话，PDF 里第一个 logo / 水印会立刻变成用户的证件照。
   * 老数据没有这个键 = 普通材料。
   */
  role?: "candidate";
  /** candidate 的来源 PDF 资产 id */
  source?: string;
  /** candidate 在来源 PDF 里的页序（1 基） */
  pageIndex?: number;
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
  /**
   * 附件在 `<数据目录>/inbox/` 里的存储名（内容寻址）。
   *
   * 正文解析、OCR、PDF 拆页都由后端按这个名字现取；前端只拿它取缩略图。
   * 浏览器调试模式没有后端，这个字段为空。
   */
  stored?: string;
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

/**
 * 这份简历是哪一种文档。三选一，不许混用（Rust 侧 `normalize` 会纠正，只降级不删页）。
 *
 *   standard —— 参照 Word 排：导出 .docx 可继续编辑
 *   freeform —— 自由版面：只出打印/PDF，Word 置灰
 *   file     —— 单份只读原件（1 个 file 页），不改内容不排版
 */
export type ResumeKind = "standard" | "freeform" | "file";

/** 纸张尺寸名。`custom` 才看 `widthMm` / `heightMm` */
export type PaperSize = "A4" | "Letter" | "custom";

/**
 * 纸张与页边距。**毫米**，`margin` 顺序 `[上, 右, 下, 左]`（与 CSS margin 简写同序）。
 *
 * 为什么用元组不用具名对象：四值在打印 `@page`、预览 `--pad-*`、docx twips 三处
 * 都要按顺序用；具名字段会诱使人写 `paper.margin.left`（不存在）而运行时拿到 undefined。
 */
export interface Paper {
  size: PaperSize;
  widthMm?: number;
  heightMm?: number;
  margin: [number, number, number, number];
}

/** 二级：一份简历 */
export interface Resume {
  id: string;
  personId: string;
  title: string;
  updatedAt: number;
  pages: ResumePage[];
  /** 文档形态。老数据没有这个键 = standard（Rust normalize 会补上） */
  kind?: ResumeKind;
  /** 纸张与页边距。老数据没有 = A4 + 20mm（Rust normalize 会补上） */
  paper?: Paper;
  /**
   * 用户明确说过的目标页数。没有 = 未设置（不要自行假定一页）。
   * `Resume` 没有索引签名，新键必须显式声明，否则从 Rust 侧读回来会被 TS 挡掉。
   */
  targetPages?: number;
  /** 写入时对应「第几条用户消息」，用来区分「这一轮」与「之前」 */
  targetPagesTurn?: number;
  /** 「旧的多页简历带着整份 1 页老目标」那条一次性提示已经看过 */
  targetPagesNotice?: boolean;
  /**
   * 原件样张：用户上传的原件第 1 页的页图（材料池里的一条 Asset）。
   *
   * 它是排版 agent 的**参考物**，不是内容：不进 blocks、不进材料列表、不参与导出。
   * 挂在简历上而不是页上 —— 页的 `layout` 每轮被排版 agent 整体覆盖，挂那里会被写掉。
   */
  originSheet?: Asset;
  /**
   * 原件文档的指针（stored / name / pages）。
   *
   * 现在不再预渲染样张：排版 agent 想看原件长什么样，就自己调 `render_doc_page`
   * 拿那一页的清晰图。指针挂简历上而不是页上 —— 页的 `layout` 每轮被整体覆盖。
   */
  originDoc?: { stored: string; name: string; pages?: number };
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
 * 排版提示：告诉生成器「这块用哪种容器」，语义由人给。
 *
 * 为什么不是「内容分类」：同一段内容，这次想排成两栏、下次想排成表格，
 * 分类应该由下指令的人决定，生成器没有资格替用户判断「这看起来像技能」。
 * 缺省（没有该字段）一律按 plain 处理，所以老数据不需要迁移。
 */
export type BlockRole =
  | "plain"
  | "tight"
  | "band"
  | "tags"
  | "kv"
  | "table"
  | "grid-2"
  | "grid-3"
  | "cols-2"
  | "cols-3"
  | "split"
  | "row";

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
  /** 结构化生成的容器提示；没有 = plain（老数据、手写块都是这种情况） */
  role?: BlockRole;
}

/**
 * 纸面上的批注：排版 agent 留的注记，不是内容。
 *
 * 它属于「过程」而不是「成品」—— 所以不进导出、不进打印，
 * 也需要能单独删掉而不影响任何一块的内容。
 */
export interface PageNote {
  id: string;
  /** 指向某一块；为空表示整页级注记（贴在页角） */
  blockId?: string;
  text: string;
  /** unix 秒，跟 Rust 侧一致；显示时乘 1000 */
  at: number;
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
  /**
   * 页头变体："" | center（姓名与联系方式居中）| bare（无分隔线）| thick（粗分隔线）| aside（照片贴右上）。
   *
   * 必须落进页数据，不能只存在于 parts 的 class 里 —— 导出 Word 时要读它决定
   * 段落对齐与照片位置；藏在 HTML 里的话，Word 侧什么都读不到。
   */
  head?: string;
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
  /**
   * 排版 agent 留在纸面上的批注（不是内容，不参与导出）。
   * 旧数据没有该字段，读的时候当作空数组即可。
   */
  notes?: PageNote[];
  /** v4 遗留：只在读旧数据时出现，`normalizePage` 会把它搬进 blocks */
  header?: Field[];
  /** v4 遗留：同上 */
  sections?: Section[];
}

/**
 * 页面实测：**只有一个来源**（前端 DOM），随 `chat_send` / `layout_submit` 上送。
 *
 * 它不落库 —— 落库的测量值下次启动就变成假数据。判定阈值与文案在 Rust 的
 * `agent/facts.rs`，这里只负责把量到的数照实带过去。
 */
export interface PageMeasure {
  pageId: string;
  /** ok = 量到了；unknown = 量不到（看 reason）；stale = 内容改过，旧值作废（看 previous） */
  status: "ok" | "stale" | "unknown";
  /** 测量版本号，每次成功测量 +1 */
  version: number;
  /** 内容版本（8 位 hex） */
  content: string;
  /** A4 一页在这个容器里的高度（px）= 纸宽 × 297/210 */
  onePage?: number;
  /** 纸面实际高度（px） */
  contentPx?: number;
  /** 正文行高（px），把溢出量折成「多少行正文」 */
  lineHeight?: number;
  /** stale 时带上一次的数 */
  previous?: { content: string; onePage: number; contentPx: number };
  /** unknown 的原因 */
  reason?: "no-preview" | "fonts" | "timeout" | "empty";
}

/** 本轮原件。`pages` 缺失 = 读不出（文件损坏或不是 PDF），**绝不是 0** */
export interface Origin {
  name: string;
  pages?: number;
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
  /**
   * 同一份文件在材料池里的 id（`Asset.id`）。
   *
   * 附件发出去之后就只剩这个名字了，想再看到图（悬停缩略图、点开大图）
   * 必须能从材料池取回字节 —— 只靠 `preview` 那个 object URL，刷新就没了。
   */
  assetId?: string;
  /**
   * 附件在 `<数据目录>/inbox/` 里的存储名（内容寻址）。
   *
   * 正文解析、OCR、PDF 拆页都由后端按这个名字现取，前端碰不到内容；
   * 它同时用来取缩略图（刷新之后 `preview` 已经没了）。
   */
  stored?: string;
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
  /**
   * 这一轮没有正常收尾的原因（白卷 / 截断 / 轮次用尽），由 Rust 侧的终止判定给出。
   *
   * 它和 `content` 分开：正文是模型真的说出口的话，原样保留；
   * 这条是系统对「它没说完」的诊断，不参与模型上下文，只在气泡下方显示。
   */
  note?: string;
  changes?: ChangeItem[];
  actions?: ChatAction[];
  /**
   * 系统提示（「已取消：删除第 2 页等 2 项改动」这类）。
   *
   * 为什么用标记而不是新增 role：会话历史由 Rust 侧按 role 组装成模型输入
   * （`role === "assistant" ? assistant : user`），多一个角色会让整条历史
   * 在模型眼里错位。用一个附加字段，渲染层自己决定长什么样，模型看到的
   * 仍是一条普通的 user/assistant 发言。
   */
  system?: boolean;
  /**
   * 内部消息：agent 之间的往返（比如排版 agent 向会话 agent 提问、它给出的答复）。
   *
   * 它仍然进模型上下文（下一轮它还看得见，否则会前后矛盾），
   * 但**不进对话流** —— 用户看到的只有自己和会话 agent 的对话。
   */
  internal?: boolean;
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
