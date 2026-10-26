# DESIGN · 自由字段与材料体系（v4）

> 状态：待确认。本文件是这次架构转向的契约，确认后按 TASK 顺序落地。
> 前三个版本的问题：数据结构是**预设 schema**（company/role/period/bullets），
> 用户的真实材料装不进去 —— 照片、链接、图片、证书、奖项、田野调查、GitHub 仓库，
> 全都要硬塞进「公司/职位/时间 + 三条 bullet」这个模子里。

## 0. 一句话

**实体只管理「字段」，不管字段叫什么；页面只管理「怎么排」，不预设排什么。**

## 1. 核心数据模型

```ts
/** 字段：用户自己的键值对，label 由用户起名 */
interface Field {
  id: string;
  label: string;
  value: string;
  /** 呈现方式，不是内容类型：text 一行、long 段落、link 可点、image 图片 */
  kind?: "text" | "long" | "link" | "image";
}

/** 材料：图片 / 链接 / 文件 / 文本片段，可挂在人、条目、字段上 */
type Asset =
  | { id: string; kind: "image"; src: string; name?: string; caption?: string }
  | { id: string; kind: "link"; url: string; title?: string }
  | { id: string; kind: "file"; name: string; path: string; size?: number }
  | { id: string; kind: "text"; text: string };

/** 库条目：一段经历 / 一件作品 / 一条教育 / 一个奖项，本质相同 */
interface LibraryItem {
  id: string;
  /** 用户自己的归类，自由文本（"工作""项目""获奖""证书"…），不枚举 */
  group: string;
  fields: Field[];
  /** 详实记录：跟着对话当时的语言写，不强制中英双份 */
  notes: string;
  assets: Asset[];
  /** 库内排序 */
  order: number;
}
```

`Person` 持有一个 `library: LibraryItem[]`（替代 `experiences/projects/education/skills` 四个固定数组）。
`Person.photo?: Asset`（候选人照片）。`Person.assets: Asset[]`（补充材料，如作品集 PDF、推荐信）。

**为什么合并**：公司的「工作经历」、开源项目的「作品」、培训的「教育」、资格证 —— 对简历来说都是
「一个标题 + 几行说明 + 一些证据」。分成四套结构只是把用户的选择题提前做了，而且必然缺一个。

## 2. 页面模型

```ts
interface ResumePage {
  id: string;
  /** 页签名，用户自己起，默认「未命名页」 */
  label: string;
  /** 纸面使用的语言（仅用于排版细节：字距、日期格式），不决定内容 */
  lang?: string;
  header: Field[];          // 姓名 / 职位 / 邮箱 / 城市 …（同样是自由字段）
  sections: PageSection[];  // 页面上的分区
}

interface PageSection {
  id: string;
  title: string;            // 「工作经历」「Experience」「获奖」…用户填
  layout: "timeline" | "list" | "cards" | "gallery";
  items: SectionItem[];
}

interface SectionItem {
  id: string;
  /** 引用库条目；为空表示这一页临时写的条目（不回写库） */
  refId?: string;
  /** 本页这一条的字段：引用时默认复制库里的，改了只影响本页 */
  fields: Field[];
  /** 本页措辞，按句存，供 AI 单句改写 */
  bullets: string[];
  assets: Asset[];
}
```

- **没有 `PageKind`**：不再有「中文简历 / English / 作品集」三种类型。作品集就是
  「几个 gallery 分区的页面」；英文简历就是「一个 label=English、措辞写英文的页面」。
- 新建页面时**只问名字**（可留空 → 「未命名页」），不问类型。
- `kind` 字段渲染规则：
  - 第 1 个字段 = 标题行（加粗）
  - 第 2 个字段 = 右对齐（时间/地点）
  - 其余 = `label：value` 逐行；`kind:"long"` 独占一段
  - 这是**排版约定**，不是 schema —— 用户调顺序就能改主次。

## 3. 经历库的语言

- 去掉「中文 | EN」开关。
- `notes` 跟随**当时对话使用的语言**原样记录（AI 提炼时也不翻译）。
- 措辞（bullets）属于页面：一个页面一套，语言自然跟着页面走。

## 4. 变更感知（AI 知道用户改了什么）

每次结构性编辑产生一条变更记录，合并后在下一次发消息时作为 system 消息注入：

```
[用户的手动变更] 自上次对话以来：
- 经历「字节跳动」：新增字段「团队规模=12 人」；措辞第 2 句被改写
- 页面「English」：新增分区「Awards」，加入 2 条
- 候选人：上传了照片
```

实现：`ui.pendingChanges: ChangeRecord[]`，`send()` 时序列化进 prompt 后清空。
不做逐次推送（不打断模型、不刷 token），只在下一轮注入一次。

## 5. 设置页（统一入口）

一个弹窗，三个 tab：

| tab | 内容 |
|---|---|
| 模型 | 服务商 / Base URL / API Key / 模型选择（来自配置文件）/ 温度 / 上下文窗口 / 思维强度 |
| 外观 | 明暗、强调色（预设 4 套 + **自定义调色盘**：取色器 + hex 输入）、字号 |
| 工作区 | 数据目录、清空数据、导出备份 |

替代现在顶栏那个「外观」下拉和聊天里那个「AI 设置」弹窗。

## 6. 配置与打包（可改、打包即硬编码）

```
config/
  models.json       # 模型清单：id / 名称 / contextWindow / maxOutput / reasoning 档位 / 默认温度
prompts/
  system.md         # 会话系统提示词
  skills/*.md       # 技能/资料，按文件注入
testdata/
  ai.json           # 测试环境（baseUrl / model / 参数），可进仓库
  mock/*.json       # 各版本工作区样例，迁移回归用
  secrets/          # 本机密钥，.gitignore 排除，打包时拦截
    ai.local.json   # 只需要写 apiKey
```

- 全部用 `import ... ?raw` / `import models from "../config/models.json"` 静态引入
  → **打包时编进 bundle**，发行版不依赖外部文件。
- 开发时改这些文件刷新即生效（vite 会 HMR）。

## 7. 输入框

- 高度可调（拖上边缘，记住高度）。
- 直接粘贴：图片（转 base64 或存 assets）、链接（识别为 link 附件）、文件、长文本。
- 底栏「未配置模型 · 点此设置」改为**模型选择器**：下拉切换已配置模型，末项「管理模型…」进设置页。

## 8. 迁移（v3 → v4）

| 旧 | 新 |
|---|---|
| `experiences[]` | `library[]`，`group="工作"`，company→field「公司」、role→「职位」、period→「时间」，notes/facts→notes + field |
| `projects[]` | `library[]`，`group="作品"`，assets 收 tags |
| `education[]` | `library[]`，`group="教育"` |
| `skills[]` | `library[]`，`group="技能"` |
| `page.entries[]` | `page.sections[0].items[]`（分区名用页面语言的内置默认值） |
| `entry.bulletsEn` | 原英文页的 `bullets` |

无损原则不变：一句话都不丢，只是换位置。

## 9. 验收

1. 新建候选人 → 只填姓名 → 能直接在经历库里「加字段」，不出现任何预设字段。
2. 一段经历能挂 3 张图 + 2 个链接，纸面上以 gallery 呈现，导出 Word/PDF 时图片跟着走。
3. 简历页能显示候选人照片。
4. 简历里加一个「获奖」分区，条目挂证书图片，库里有凭证入口。
5. 新建页面只问名字，不问类型；作品集 = 用户自己建一个 gallery 页面。
6. 改完右侧表单，下一轮对话里模型能说出「你刚才把第 2 句改了」。
7. 设置页能选到 `minimax-m3.1-flash` 并跑通；能自定义任意强调色。
8. `config/models.json` 改完刷新生效；release 包里不需要外部文件。

## 10. 风险

- **最大风险**：自由字段让「排版」失去抓手 —— 用户可能加 12 个字段，纸面挤成一团。
  对策：字段 kind + 顺序约定（第一节），并给「预览」即时反馈；不做自动布局算法。
- 材料存磁盘（图片/文件）需要 fs 权限与目录规划，`SharpResumeData/assets/<hash>` 内容寻址，避免重名。
- 一次做完全部会拖太久：按 TASK 分四批，每批可独立验收。
