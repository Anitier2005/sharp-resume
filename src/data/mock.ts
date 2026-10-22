import type { ChatMessage, ResumeDoc, ResumeItem, PortfolioDoc } from "@/types";

const chenMeta = ["上海 · 徐汇", "138 0000 0000", "chenmo@mail.com", "github.com/chenmo"];

export const chenmoZh: ResumeDoc = {
  kind: "resume-zh",
  name: "陈 墨",
  title: "高级前端工程师 · 6 年经验",
  meta: chenMeta,
  summary:
    "6 年前端经验，其中 3 年带团队，长期深耕协同编辑与富文本内核。主导飞书多维表格视图引擎重构，把 12 个视图的渲染逻辑收敛为统一渲染层，首屏耗时从 2.4s 降到 0.9s；设计过支撑 200+ 人同时编辑的冲突处理方案。习惯用数据定义问题，也愿意写文档把经验沉淀下来。",
  experiences: [
    {
      id: "exp-1",
      company: "字节跳动",
      role: "高级前端工程师",
      period: "2023.06 — 至今",
      team: "飞书 · 文档协同团队",
      bullets: [
        "主导多维表格视图引擎重构，将 12 个视图的重复渲染逻辑收敛为统一渲染层，首屏耗时从 2.4s 降至 0.9s，包体积减少 38%。",
        "设计基于 CRDT 的协同编辑冲突处理方案，支撑 200+ 人同时编辑同一文档，冲突回滚率下降 92%。",
        "搭建前端性能监控与告警体系，覆盖 8 个核心场景，季度 P0 性能问题从 11 个降至 2 个。",
        "带 4 人小组负责公式模块，主导 2 次大版本发布，线上事故 0 起。",
      ],
    },
    {
      id: "exp-2",
      company: "蚂蚁集团",
      role: "前端工程师",
      period: "2020.07 — 2023.05",
      team: "体验技术部 · 富文本内核组",
      bullets: [
        "负责富文本编辑器的选区与撤销栈模块，重写选区同步逻辑，复杂文档下的输入延迟从 120ms 降至 28ms。",
        "沉淀编辑器插件协议，支撑 5 条业务线自定义扩展，插件开发周期从 2 周缩短到 3 天。",
        "推动单元测试覆盖率从 41% 提升至 86%，核心模块重构零回归。",
      ],
    },
    {
      id: "exp-3",
      company: "美团",
      role: "前端开发工程师",
      period: "2018.07 — 2020.06",
      team: "到店事业群 · 商家平台",
      bullets: [
        "从 0 到 1 搭建商家活动配置后台，支撑日均 3 万次配置操作，替代原有的人工投放流程。",
        "引入组件库与脚手架，页面平均开发时间从 3 天缩短至 1 天。",
      ],
    },
  ],
  projects: [
    {
      id: "prj-1",
      title: "Mosaic · 开源协同富文本编辑器",
      desc: "基于 Yjs + ProseMirror 的块级编辑器内核，支持多人实时协作、评论与版本回溯，GitHub 3.2k Star。",
      tags: ["React", "Yjs", "ProseMirror"],
      year: "2024 — 至今",
      cover: "violet",
    },
    {
      id: "prj-2",
      title: "Atlas · 企业级设计系统",
      desc: "48 个组件、主题令牌与自动化视觉回归，服务 6 条业务线，接入成本从 3 天降至 2 小时。",
      tags: ["Design System", "Storybook"],
      year: "2022 — 2023",
      cover: "amber",
    },
  ],
  education: [
    {
      id: "edu-1",
      school: "同济大学",
      degree: "软件工程 · 本科",
      period: "2014.09 — 2018.06",
    },
  ],
  skills: [
    { label: "语言", value: "TypeScript / JavaScript / Rust（了解）" },
    { label: "框架", value: "React 19 / Next.js 15 / Vue 3" },
    { label: "工程化", value: "Vite / Turbopack / pnpm Monorepo / CI-CD" },
    { label: "领域", value: "协同编辑 / 富文本 / 性能优化 / AI 应用" },
  ],
};

export const chenmoEn: ResumeDoc = {
  kind: "resume-en",
  name: "Chen Mo",
  title: "Senior Frontend Engineer · 6 years",
  meta: ["Shanghai, China", "+86 138 0000 0000", "chenmo@mail.com", "github.com/chenmo"],
  summary:
    "Frontend engineer with 6 years of experience, including 3 years leading small teams. I work on collaborative editing and rich-text internals: I led the view-engine rewrite of Feishu Base, cutting first paint from 2.4s to 0.9s, and designed a CRDT-based conflict strategy that supports 200+ concurrent editors.",
  experiences: [
    {
      id: "exp-en-1",
      company: "ByteDance",
      role: "Senior Frontend Engineer",
      period: "Jun 2023 — Present",
      team: "Feishu · Document Collaboration",
      bullets: [
        "Led the rewrite of the Base view engine, unifying 12 view implementations into one rendering layer; first paint 2.4s → 0.9s, bundle size −38%.",
        "Designed a CRDT-based conflict resolution strategy supporting 200+ concurrent editors, reducing conflict rollbacks by 92%.",
        "Built a frontend performance monitoring and alerting system across 8 critical flows; quarterly P0 perf issues 11 → 2.",
        "Led a team of 4 on the formula module and shipped 2 major releases with zero production incidents.",
      ],
    },
    {
      id: "exp-en-2",
      company: "Ant Group",
      role: "Frontend Engineer",
      period: "Jul 2020 — May 2023",
      team: "Experience Technology · Rich Text Core",
      bullets: [
        "Owned selection and undo-stack modules of the rich-text editor; rewrote selection sync, cutting input latency from 120ms to 28ms.",
        "Designed a plugin protocol adopted by 5 business lines, shortening extension development from 2 weeks to 3 days.",
        "Raised unit test coverage from 41% to 86% with zero regressions in core refactors.",
      ],
    },
  ],
  projects: [
    {
      id: "prj-en-1",
      title: "Mosaic — Open-source Collaborative Editor",
      desc: "A block-based editor core built on Yjs and ProseMirror with real-time collaboration and version history. 3.2k GitHub stars.",
      tags: ["React", "Yjs", "ProseMirror"],
      year: "2024 — Now",
      cover: "violet",
    },
    {
      id: "prj-en-2",
      title: "Atlas — Enterprise Design System",
      desc: "48 components, design tokens and visual regression testing, serving 6 product lines. Adoption time cut from 3 days to 2 hours.",
      tags: ["Design System", "Storybook"],
      year: "2022 — 2023",
      cover: "amber",
    },
  ],
  education: [
    {
      id: "edu-en-1",
      school: "Tongji University",
      degree: "B.Eng. in Software Engineering",
      period: "Sep 2014 — Jun 2018",
    },
  ],
  skills: [
    { label: "Languages", value: "TypeScript / JavaScript / Rust (basic)" },
    { label: "Frameworks", value: "React 19 / Next.js 15 / Vue 3" },
    { label: "Tooling", value: "Vite / Turbopack / pnpm monorepo / CI-CD" },
    { label: "Focus", value: "Collaboration / Rich text / Performance / AI apps" },
  ],
};

export const chenmoPortfolio: PortfolioDoc = {
  kind: "portfolio",
  name: "陈墨",
  role: "Senior Frontend Engineer",
  year: "PORTFOLIO / 2026",
  intro:
    "6 年前端工程师，专注协同编辑、富文本内核与开发者体验。相信好的工具应该让人忘记它的存在。",
  projects: [
    {
      id: "pf-1",
      title: "飞书多维表格 · 视图引擎",
      desc: "把 12 种视图收敛成一套渲染内核，首屏 2.4s → 0.9s",
      tags: ["架构重构", "性能"],
      year: "2024",
      cover: "violet",
    },
    {
      id: "pf-2",
      title: "Mosaic 编辑器",
      desc: "3.2k Star 的开源协同富文本内核，支持 200 人实时协作",
      tags: ["开源", "CRDT"],
      year: "2025",
      cover: "teal",
    },
    {
      id: "pf-3",
      title: "Atlas 设计系统",
      desc: "48 个组件服务 6 条业务线，接入成本从 3 天到 2 小时",
      tags: ["设计系统", "工程化"],
      year: "2023",
      cover: "amber",
    },
    {
      id: "pf-4",
      title: "Latency Lab",
      desc: "一个把前端性能数据做成可视化实验的个人项目",
      tags: ["可视化", "Side project"],
      year: "2026",
      cover: "rose",
    },
  ],
  contact: ["chenmo@mail.com", "github.com/chenmo", "上海 · 徐汇"],
};

const pmZh: ResumeDoc = {
  ...chenmoZh,
  title: "策略产品经理 · 转型中",
  summary:
    "6 年互联网经验，前端工程师出身，近两年主导协同办公产品的表格能力规划。能从技术可行性与用户价值两端判断需求，做过 3 次从 0 到 1 的能力建设，擅长把模糊的业务诉求拆成可验证的产品方案。",
  experiences: [
    {
      id: "pm-exp-1",
      company: "小红书",
      role: "策略产品经理（协同办公）",
      period: "2025.03 — 至今",
      team: "效率工具组",
      bullets: [
        "负责内部协同文档的表格能力规划，上线 6 个高频场景，月活从 1.2 万提升至 3.8 万。",
        "主导 AI 写作助手的需求定义与灰度验证，采纳率从 18% 提升至 43%。",
      ],
    },
    {
      id: "pm-exp-2",
      company: "字节跳动",
      role: "前端工程师 → 产品方向",
      period: "2023.06 — 2025.02",
      team: "飞书 · 文档协同团队",
      bullets: [
        "作为技术侧接口人参与多维表格年度规划，输出 4 份竞品与用户调研。",
        "推动公式模块的可用性改造，相关客诉下降 61%。",
      ],
    },
  ],
  skills: [
    { label: "产品", value: "需求分析 / 竞品调研 / 数据验证 / PRD" },
    { label: "技术", value: "TypeScript / SQL / 原型设计" },
    { label: "领域", value: "协同办公 / AI 应用 / 效率工具" },
  ],
};

const linxiaZh: ResumeDoc = {
  kind: "resume-zh",
  name: "林 夏",
  title: "UI/UX 设计师 · 5 年经验",
  meta: ["杭州 · 西湖", "139 0000 0000", "linxia@mail.com", "linxia.design"],
  summary:
    "5 年体验设计经验，服务过工具类与内容类产品。擅长把复杂业务抽象成清晰的界面语言，主导过 2 次设计系统从 0 到 1 的建设。习惯用原型和数据对话，而不是用风格稿说服人。",
  experiences: [
    {
      id: "lx-exp-1",
      company: "蚂蚁集团",
      role: "高级体验设计师",
      period: "2022.04 — 至今",
      team: "体验技术部 · 企业服务",
      bullets: [
        "负责企业级数据看板的体验重构，任务完成率从 72% 提升至 91%。",
        "主导设计系统 Atlas 的组件规范与文档，覆盖 6 条业务线、48 个组件。",
        "建立可用性测试流程，季度完成 12 场用户访谈，推动 9 项体验优化落地。",
      ],
    },
    {
      id: "lx-exp-2",
      company: "网易",
      role: "UI 设计师",
      period: "2020.07 — 2022.03",
      team: "云音乐 · 内容社区",
      bullets: [
        "负责社区创作工具的界面设计，支撑 3 次大型活动上线。",
        "输出移动端设计规范，设计交付效率提升 40%。",
      ],
    },
  ],
  projects: [
    {
      id: "lx-prj-1",
      title: "Mori · 冥想 App 概念设计",
      desc: "从情绪记录到呼吸练习的完整体验，包含 32 个界面与动效规范。",
      tags: ["Mobile", "Concept"],
      year: "2025",
      cover: "teal",
    },
    {
      id: "lx-prj-2",
      title: "好食 · 社区团购改版",
      desc: "重构下单链路，把 5 步操作压缩到 3 步，转化率提升 18%。",
      tags: ["E-commerce", "UX"],
      year: "2024",
      cover: "amber",
    },
  ],
  education: [
    { id: "lx-edu-1", school: "中国美术学院", degree: "视觉传达 · 本科", period: "2016.09 — 2020.06" },
  ],
  skills: [
    { label: "设计", value: "Figma / Sketch / Principle / After Effects" },
    { label: "研究", value: "用户访谈 / 可用性测试 / 数据分析" },
    { label: "协作", value: "设计系统 / 组件规范 / 跨端交付" },
  ],
};

const linxiaPortfolio: PortfolioDoc = {
  kind: "portfolio",
  name: "林夏",
  role: "UI/UX Designer",
  year: "SELECTED WORK / 2026",
  intro: "做工具，也做内容。喜欢把复杂的流程拆开、重排，直到它看起来理所当然。",
  projects: [
    {
      id: "lx-pf-1",
      title: "Mori 冥想",
      desc: "情绪记录与呼吸练习，32 个界面、一套完整的动效语言",
      tags: ["Mobile App", "Concept"],
      year: "2025",
      cover: "teal",
    },
    {
      id: "lx-pf-2",
      title: "好食社区团购",
      desc: "下单链路从 5 步压到 3 步，转化率 +18%",
      tags: ["E-commerce", "UX"],
      year: "2024",
      cover: "amber",
    },
    {
      id: "lx-pf-3",
      title: "Atlas 设计系统",
      desc: "48 个组件、6 条业务线，从 0 到 1 建立规范",
      tags: ["Design System"],
      year: "2023",
      cover: "violet",
    },
    {
      id: "lx-pf-4",
      title: "城市散步地图",
      desc: "一个记录城市角落的个人项目，包含 120 个地点",
      tags: ["Side project", "Visual"],
      year: "2026",
      cover: "rose",
    },
  ],
  contact: ["linxia@mail.com", "linxia.design", "杭州 · 西湖"],
};

const r1Messages: ChatMessage[] = [
  {
    id: "m1",
    role: "assistant",
    time: "10:24",
    content:
      "你好，我是 **Sharp AI**。这份简历的 3 个页面已经进入上下文：中文简历、English Resume、作品集。\n\n你可以直接说想改哪一段，也可以把 JD、旧简历、项目文档拖进来 —— 我会先读，再动手改。",
  },
  {
    id: "m2",
    role: "user",
    time: "10:26",
    content:
      "按这份 JD 帮我把工作经历重写一版，突出协同编辑和性能优化，整体控制在一页内。",
    attachments: [
      {
        id: "f1",
        name: "JD-字节跳动-前端工程师.pdf",
        size: 284_000,
        ext: "pdf",
      },
    ],
  },
  {
    id: "m3",
    role: "assistant",
    time: "10:27",
    content:
      "读完了。这个岗位最看重三件事：**协同编辑的底层经验**、**可量化的性能收益**、以及**能否带小团队交付**。我把「字节跳动」这段按这三条重写了，删掉了两份重复的职责描述。",
    fileMeta: {
      name: "JD-字节跳动-前端工程师.pdf",
      pages: 4,
      keywords: ["React 19", "协同编辑", "性能优化", "团队管理", "Node.js"],
    },
    changesTitle: "已更新「工作经历 · 字节跳动」",
    changes: [
      { label: "重构渲染层", detail: "补充「12 个视图 → 统一渲染层」与首屏 2.4s → 0.9s 的量化结果" },
      { label: "协同编辑", detail: "把「参与」改为「主导」，补上 200+ 人同时编辑与冲突回滚率指标" },
      { label: "团队管理", detail: "明确 4 人小组、2 次大版本发布与线上事故 0 起" },
    ],
  },
  {
    id: "m4",
    role: "user",
    time: "10:31",
    content: "英文版也同步一下，语气再克制一点，别用太多形容词。",
  },
  {
    id: "m5",
    role: "assistant",
    time: "10:32",
    content:
      "已同步 **English Resume**：把 \"led a team of 4\" 这类表述保留，删掉了所有 \"excellent / passionate\" 这类形容词，动词全部改成过去式开头的短句。作品集页的项目顺序也按岗位相关度调了一遍。\n\n右侧切到 English Resume 可以直接看。",
  },
];

const r2Messages: ChatMessage[] = [
  {
    id: "r2-m1",
    role: "assistant",
    time: "昨天 21:36",
    content:
      "这份是纯英文版本，目前只有 1 页。投 Stripe 这类 fintech 公司，建议把 **payments / reliability** 相关的经历往前放 —— 需要我从中文简历里抽两条过来吗？",
  },
  {
    id: "r2-m2",
    role: "user",
    time: "昨天 21:38",
    content: "好，顺便把 Summary 改得更像基础设施工程师，不要写太多产品侧的东西。",
  },
  {
    id: "r2-m3",
    role: "assistant",
    time: "昨天 21:40",
    content:
      "改好了。Summary 现在聚焦在 **editor core / reliability / performance** 三个关键词上，句子长度控制在 22 词以内。",
    changesTitle: "已更新「Summary」",
    changes: [
      { label: "重写 Summary", detail: "从「产品协作」转向「编辑器内核与稳定性」" },
      { label: "压缩长度", detail: "由 5 句 118 词压缩为 3 句 62 词" },
    ],
  },
];

const r3Messages: ChatMessage[] = [
  {
    id: "r3-m1",
    role: "assistant",
    time: "10 月 18 日",
    content:
      "这是一份转型用的产品简历。技术背景是加分项，但现在的写法太像「前端工程师的履历」。建议把每段经历都改成 **问题 → 判断 → 结果** 的结构。",
  },
  {
    id: "r3-m2",
    role: "user",
    time: "10 月 18 日",
    content: "那你先帮我改第一段，我想看看差别。",
  },
  {
    id: "r3-m3",
    role: "assistant",
    time: "10 月 18 日",
    content:
      "第一段改好了。开头先说清楚「协同办公表格能力」这个业务问题，再写你做的三个判断，最后落到月活 1.2 万 → 3.8 万的数据。",
    changesTitle: "已更新「工作经历 · 小红书」",
    changes: [
      { label: "补充业务背景", detail: "开头增加协同办公场景与目标用户说明" },
      { label: "突出判断", detail: "把「负责」拆成 3 个具体的产品决策" },
    ],
  },
];

const r4Messages: ChatMessage[] = [
  {
    id: "r4-m1",
    role: "assistant",
    time: "10 月 12 日",
    content:
      "这份作品集目前有 4 个项目，但项目描述偏「做了什么」，缺少「为什么这么做」。设计岗位的面试官更想看你的判断过程。",
  },
  {
    id: "r4-m2",
    role: "user",
    time: "10 月 12 日",
    content: "我把作品集 PDF 传给你，你帮我按这个思路改一下项目描述。",
    attachments: [{ id: "r4-f1", name: "林夏-作品集-2026.pdf", size: 8_400_000, ext: "pdf" }],
  },
  {
    id: "r4-m3",
    role: "assistant",
    time: "10 月 12 日",
    content:
      "收到，12 页都读完了。我会保留原有的视觉排版，只重写每个项目的描述段落，并在「Mori」这个项目上补一段设计取舍。",
    fileMeta: {
      name: "林夏-作品集-2026.pdf",
      pages: 12,
      keywords: ["Mori", "设计系统", "用户访谈"],
    },
  },
];

export const mockResumes: ResumeItem[] = [
  {
    id: "r1",
    name: "陈墨 · 高级前端",
    role: "前端工程师 · 6 年",
    company: "目标：字节跳动 · 飞书",
    updatedAt: "2 分钟前",
    accent: "from-violet-500 to-indigo-500",
    pages: [
      {
        id: "r1-p1",
        kind: "resume-zh",
        label: "中文简历",
        badge: "中",
        updatedAt: "2 分钟前",
        doc: chenmoZh,
      },
      {
        id: "r1-p2",
        kind: "resume-en",
        label: "English Resume",
        badge: "EN",
        updatedAt: "3 分钟前",
        doc: chenmoEn,
      },
      {
        id: "r1-p3",
        kind: "portfolio",
        label: "作品集",
        badge: "PORT",
        updatedAt: "昨天",
        doc: chenmoPortfolio,
      },
    ],
    messages: r1Messages,
  },
  {
    id: "r2",
    name: "陈墨 · Senior FE",
    role: "Frontend Engineer · 6 yrs",
    company: "目标：Stripe · 远程",
    updatedAt: "昨天 21:40",
    accent: "from-sky-500 to-cyan-500",
    pages: [
      {
        id: "r2-p1",
        kind: "resume-en",
        label: "English Resume",
        badge: "EN",
        updatedAt: "昨天 21:40",
        doc: {
          ...chenmoEn,
          summary:
            "Frontend engineer with 6 years of experience building and maintaining editor infrastructure at scale. Led the rewrite of a collaborative view engine used by millions of documents, cutting first paint from 2.4s to 0.9s, and designed conflict resolution that keeps 200+ concurrent editors consistent.",
          skills: [
            { label: "Languages", value: "TypeScript / JavaScript / Rust" },
            { label: "Focus", value: "Editor core / Reliability / Performance" },
            { label: "Tooling", value: "Vite / Turbopack / CI-CD" },
          ],
        },
      },
    ],
    messages: r2Messages,
  },
  {
    id: "r3",
    name: "陈墨 · 产品经理",
    role: "策略产品 · 转型中",
    company: "目标：小红书 · 效率工具",
    updatedAt: "10 月 18 日",
    accent: "from-amber-500 to-orange-500",
    pages: [
      {
        id: "r3-p1",
        kind: "resume-zh",
        label: "中文简历",
        badge: "中",
        updatedAt: "10 月 18 日",
        doc: pmZh,
      },
    ],
    messages: r3Messages,
  },
  {
    id: "r4",
    name: "林夏 · UI/UX 设计师",
    role: "体验设计 · 5 年",
    company: "目标：独立品牌",
    updatedAt: "10 月 12 日",
    accent: "from-rose-500 to-pink-500",
    pages: [
      {
        id: "r4-p1",
        kind: "resume-zh",
        label: "中文简历",
        badge: "中",
        updatedAt: "10 月 12 日",
        doc: linxiaZh,
      },
      {
        id: "r4-p2",
        kind: "portfolio",
        label: "作品集",
        badge: "PORT",
        updatedAt: "10 月 12 日",
        doc: linxiaPortfolio,
      },
    ],
    messages: r4Messages,
  },
];

export const emptyZhDoc: ResumeDoc = {
  kind: "resume-zh",
  name: "你的名字",
  title: "求职意向 · 职位名称",
  meta: ["城市", "手机号", "邮箱", "个人主页"],
  summary: "用 2–3 句话说明你的经验、擅长方向和最有说服力的一个结果。",
  experiences: [
    {
      id: "new-exp-1",
      company: "公司名称",
      role: "职位",
      period: "2023.01 — 至今",
      team: "部门 / 业务线",
      bullets: ["写清楚你做了什么、怎么做的、带来了什么可量化的结果。"],
    },
  ],
  projects: [],
  education: [
    { id: "new-edu-1", school: "学校名称", degree: "专业 · 学历", period: "2016.09 — 2020.06" },
  ],
  skills: [{ label: "技能", value: "在这里填写你的技能栈" }],
};

export const emptyEnDoc: ResumeDoc = {
  ...emptyZhDoc,
  kind: "resume-en",
  name: "Your Name",
  title: "Target Role · X years",
  meta: ["City", "Phone", "Email", "Website"],
  summary: "Two or three sentences on your experience and one measurable outcome.",
  education: [
    { id: "new-edu-en", school: "University", degree: "B.Eng. in ...", period: "2016 — 2020" },
  ],
};

export const emptyPortfolioDoc: PortfolioDoc = {
  kind: "portfolio",
  name: "你的名字",
  role: "Your Role",
  year: "PORTFOLIO / 2026",
  intro: "一句话介绍你在做什么、擅长什么。",
  projects: [],
  contact: ["邮箱", "主页", "城市"],
};
