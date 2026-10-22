import type {
  ChatMessage,
  EducationItem,
  Experience,
  Person,
  ProjectItem,
  Resume,
  ResumePage,
  SkillGroup,
  Workspace,
} from "@/types";

const now = Date.now();
const min = 60_000;
const hour = 60 * min;
const day = 24 * hour;

function exp(
  id: string,
  company: string,
  role: string,
  period: string,
  bullets: string[],
  bulletsEn?: string[],
  en?: { companyEn: string; roleEn: string },
): Experience {
  return { id, company, role, period, bullets, bulletsEn, ...en };
}

function proj(
  id: string,
  title: string,
  year: string,
  desc: string,
  tags: string[],
  cover: number,
  descEn?: string,
): ProjectItem {
  return { id, title, year, desc, descEn, tags, cover };
}

function edu(
  id: string,
  school: string,
  degree: string,
  period: string,
  schoolEn?: string,
  degreeEn?: string,
): EducationItem {
  return { id, school, schoolEn, degree, degreeEn, period };
}

function skill(
  id: string,
  label: string,
  value: string,
  en?: { labelEn: string; valueEn: string },
): SkillGroup {
  return { id, label, value, ...en };
}

function page(
  id: string,
  kind: ResumePage["kind"],
  label: string,
  headline: string,
  role: string,
  meta: string[],
  experienceIds: string[],
  projectIds: string[],
  summary?: string,
): ResumePage {
  return { id, kind, label, headline, role, meta, summary, experienceIds, projectIds };
}

/* ------------------------------------------------------------------ */
/* 林夏：完整样例，含会话与改写建议                                      */
/* ------------------------------------------------------------------ */

const linExperiences: Experience[] = [
  exp(
    "e-lin-1",
    "字节跳动",
    "商业化产品设计",
    "2021.06 — 至今",
    [
      "负责 B 端投放平台的设计体系，主导 3 个核心模块重构",
      "投放配置流程从 11 步收敛到 4 步，客户自助率提升 32%",
      "带 2 名设计师，建立投放域的组件库与走查机制",
    ],
    [
      "Own the design system for the B2B ads platform; led rebuilds of 3 core modules",
      "Cut the campaign setup flow from 11 steps to 4, lifting self-serve rate by 32%",
      "Led 2 designers; built the component library and design-review process for the ads domain",
    ],
    { companyEn: "ByteDance", roleEn: "Commercial Product Design" },
  ),
  exp(
    "e-lin-2",
    "小红书",
    "产品设计",
    "2019.07 — 2021.05",
    ["创作者工具从 0 到 1，组件库覆盖 80% 页面", "主导创作者数据看板改版，周活跃提升 18%"],
    [
      "Designed the creator tooling from scratch; component library covered 80% of pages",
      "Led the redesign of the creator analytics dashboard, raising weekly actives by 18%",
    ],
    { companyEn: "Xiaohongshu", roleEn: "Product Design" },
  ),
  exp(
    "e-lin-3",
    "蚂蚁集团",
    "交互设计实习",
    "2018.09 — 2019.05",
    ["参与商家后台的账单模块改版，输出 40+ 个高保真页面"],
    ["Contributed to the merchant billing module redesign; delivered 40+ high-fidelity screens"],
    { companyEn: "Ant Group", roleEn: "Interaction Design Intern" },
  ),
];

const linProjects: ProjectItem[] = ([
  proj(
    "pr-lin-1",
    "投放平台 · 配置链路重构",
    "2023",
    "把 7 个分散配置页收敛为一个流程，抽象出 4 类可复用业务模型，配置耗时下降 63%。",
    ["B 端", "流程设计", "设计系统"],
    0,
    "Consolidated 7 scattered config pages into one flow; abstracted 4 reusable business models, cutting setup time by 63%",
  ),
  proj(
    "pr-lin-2",
    "创作者数据看板",
    "2020",
    "为创作者重建数据叙事：从 12 个孤立指标到 3 层漏斗，次日留存提升 9%。",
    ["数据可视化", "0→1"],
    1,
    "Rebuilt the creator analytics narrative: from 12 isolated metrics to a 3-layer funnel, lifting D1 retention by 9%.",
  ),
  proj(
    "pr-lin-3",
    "设计系统 · Atlas",
    "2022",
    "覆盖 6 条业务线的组件库，56 个组件、3 套密度，落地后设计交付周期缩短 40%。",
    ["设计系统", "组件库"],
    2,
    "A component library covering 6 business lines: 56 components, 3 densities, cutting design delivery time by 40%.",
  ),
] as ProjectItem[]).map((item, index) => ({
  ...item,
  titleEn: ["Ads Platform · Config Flow Rebuild", "Creator Analytics Dashboard", "Design System · Atlas"][index],
}));

const linEducation: EducationItem[] = [
  edu("ed-lin-1", "同济大学", "工业设计 · 学士", "2015 — 2019", "Tongji University", "B.Eng. Industrial Design"),
];

const linSkills: SkillGroup[] = [
  skill("sk-lin-1", "设计", "B 端流程设计 / 设计系统 / 数据可视化 / 交互原型", {
    labelEn: "Design",
    valueEn: "B2B flow design / design systems / data visualization / prototyping",
  }),
  skill("sk-lin-2", "工具", "Figma / FigJam / Principle / After Effects", {
    labelEn: "Tools",
    valueEn: "Figma / FigJam / Principle / After Effects",
  }),
  skill("sk-lin-3", "协作", "需求评审 / 设计走查 / 组件治理", {
    labelEn: "Practices",
    valueEn: "Requirement review / design QA / component governance",
  }),
];

const linMessages: ChatMessage[] = [
  {
    id: "m-lin-1",
    role: "user",
    content: "按这个 JD 调一版，重点突出 B 端和复杂业务。",
    time: now - 2 * hour,
    attachments: [{ id: "a-lin-1", name: "JD · 字节跳动 · 产品设计.pdf", size: 254_000, ext: "pdf" }],
  },
  {
    id: "m-lin-2",
    role: "assistant",
    content: "读完了。JD 里三次提到「复杂业务抽象」，但你第二段只写了视觉产出。",
    time: now - 2 * hour + 40_000,
    tool: "已解析 JD · 提取 12 个关键词",
    changes: [
      {
        label: "第 2 段 · 字节跳动",
        before: "负责投放平台的设计体系，主导 3 个核心模块重构。",
        after: "主导投放平台配置链路重构，把 11 步压缩到 4 步，客户自助率 ＋32%。",
        target: { experienceId: "e-lin-1", bulletIndex: 0 },
      },
    ],
    actions: [
      { id: "act-lin-1", label: "应用到第 2 段", kind: "apply" },
      { id: "act-lin-2", label: "我再补一条数据", kind: "hint" },
    ],
  },
];

const linResumes: Resume[] = [
  {
    id: "r-lin-1",
    personId: "p-lin",
    title: "字节 · 产品设计师",
    updatedAt: now - 2 * min,
    messages: linMessages,
    pages: [
      page(
        "pg-lin-1",
        "zh",
        "中文简历",
        "林 夏",
        "Product Designer",
        ["linxia@mail.com", "上海", "5 年经验"],
        ["e-lin-1", "e-lin-2"],
        ["pr-lin-1", "pr-lin-2"],
        "5 年 B 端产品设计经验，擅长把复杂业务收敛成可复用流程。",
      ),
      page(
        "pg-lin-2",
        "en",
        "English",
        "Lin Xia",
        "Product Designer",
        ["linxia@mail.com", "Shanghai", "5 years"],
        ["e-lin-1", "e-lin-2"],
        ["pr-lin-1"],
        "Product designer with 5 years in B2B SaaS, focused on turning complex workflows into reusable systems.",
      ),
      page(
        "pg-lin-3",
        "portfolio",
        "作品集",
        "林 夏",
        "Selected Works · 2020—2024",
        ["linxia@mail.com", "上海"],
        ["e-lin-1", "e-lin-2"],
        ["pr-lin-1", "pr-lin-2", "pr-lin-3"],
        "三个项目，分别关于流程收敛、数据叙事与设计系统。",
      ),
    ],
  },
  {
    id: "r-lin-2",
    personId: "p-lin",
    title: "通用版 · 产品设计师",
    updatedAt: now - day,
    messages: [
      {
        id: "m-lin-g1",
        role: "user",
        content: "做一份不针对具体公司的通用版，重点讲方法论。",
        time: now - day,
      },
      {
        id: "m-lin-g2",
        role: "assistant",
        content: "已经按「问题 → 判断 → 结果」重排了三条经历，把公司名放到次要位置。你看下摘要那句是否要再收一点。",
        time: now - day + 30_000,
        tool: "已重排 3 段经历 · 新增摘要",
      },
    ],
    pages: [
      page(
        "pg-lin-4",
        "zh",
        "中文简历",
        "林 夏",
        "B 端产品设计师",
        ["linxia@mail.com", "上海", "5 年经验"],
        ["e-lin-1", "e-lin-2", "e-lin-3"],
        ["pr-lin-1", "pr-lin-3"],
        "把复杂业务收敛成可复用流程，是过去五年反复在做的一件事。",
      ),
    ],
  },
  {
    id: "r-lin-3",
    personId: "p-lin",
    title: "英文投递版",
    updatedAt: now - 3 * day,
    messages: [],
    pages: [
      page(
        "pg-lin-5",
        "en",
        "English",
        "Lin Xia",
        "Product Designer",
        ["linxia@mail.com", "Shanghai", "5 years"],
        ["e-lin-1", "e-lin-2", "e-lin-3"],
        ["pr-lin-1", "pr-lin-2", "pr-lin-3"],
        "B2B product designer turning complex enterprise workflows into simple, reusable systems.",
      ),
    ],
  },
];

const lin: Person = {
  id: "p-lin",
  name: "林夏",
  nameEn: "Lin Xia",
  role: "产品设计师",
  roleEn: "Product Designer",
  email: "linxia@mail.com",
  location: "上海",
  years: 5,
  experiences: linExperiences,
  projects: linProjects,
  education: linEducation,
  skills: linSkills,
  resumes: linResumes,
};

/* ------------------------------------------------------------------ */
/* 陈默：第二个人，用于验证共享与隔离                                     */
/* ------------------------------------------------------------------ */

const chen: Person = {
  id: "p-chen",
  name: "陈默",
  nameEn: "Chen Mo",
  role: "后端工程师",
  roleEn: "Backend Engineer",
  email: "chenmo@mail.com",
  location: "杭州",
  years: 6,
  experiences: [
    exp(
      "e-chen-1",
      "腾讯",
      "后端工程师",
      "2020.03 — 至今",
      ["负责支付清结算链路，日均处理 2.4 亿笔交易", "主导对账系统重构，差错率从 0.03% 降到 0.002%"],
      [
        "Own the payment clearing and settlement pipeline, handling 240M transactions per day",
        "Led the reconciliation system rebuild, cutting discrepancy rate from 0.03% to 0.002%",
      ],
      { companyEn: "Tencent", roleEn: "Backend Engineer" },
    ),
    exp(
      "e-chen-2",
      "网易",
      "后端开发",
      "2018.07 — 2020.02",
      ["游戏后台的账号与订单服务，QPS 峰值 8 万"],
      ["Account and order services for game backends, peaking at 80K QPS"],
      { companyEn: "NetEase", roleEn: "Backend Developer" },
    ),
  ],
  projects: ([
    proj(
      "pr-chen-1",
      "对账系统重构",
      "2022",
      "从 T+1 批处理改为流式对账，异常发现时间从 24 小时缩短到 5 分钟。",
      ["分布式", "对账", "Go"],
      3,
      "Migrated from T+1 batch to streaming reconciliation, cutting anomaly detection from 24h to 5 minutes.",
    ),
    proj("pr-chen-2", "支付网关限流", "2021", "基于令牌桶的多级限流，大促期间零超时。", ["高可用"], 2),
  ] as ProjectItem[]).map((item, index) => ({
    ...item,
    titleEn: ["Reconciliation System Rebuild", "Payment Gateway Rate Limiting"][index],
  })),
  education: [edu("ed-chen-1", "浙江大学", "计算机科学 · 硕士", "2015 — 2018", "Zhejiang University")],
  skills: [
    skill("sk-chen-1", "语言", "Go / Java / Rust", { labelEn: "Languages", valueEn: "Go / Java / Rust" }),
    skill("sk-chen-2", "领域", "支付 / 清结算 / 高并发", {
      labelEn: "Domains",
      valueEn: "Payments / clearing and settlement / high concurrency",
    }),
  ],
  resumes: [
    {
      id: "r-chen-1",
      personId: "p-chen",
      title: "腾讯 · 后端工程师",
      updatedAt: now - 26 * hour,
      messages: [],
      pages: [
        page(
          "pg-chen-1",
          "zh",
          "中文简历",
          "陈 默",
          "Backend Engineer",
          ["chenmo@mail.com", "杭州", "6 年经验"],
          ["e-chen-1", "e-chen-2"],
          ["pr-chen-1", "pr-chen-2"],
        ),
        page(
          "pg-chen-2",
          "en",
          "English",
          "Chen Mo",
          "Backend Engineer",
          ["chenmo@mail.com", "Hangzhou", "6 years"],
          ["e-chen-1"],
          ["pr-chen-1"],
        ),
      ],
    },
    {
      id: "r-chen-2",
      personId: "p-chen",
      title: "通用版 · 后端",
      updatedAt: now - 4 * day,
      messages: [],
      pages: [
        page(
          "pg-chen-3",
          "zh",
          "中文简历",
          "陈 默",
          "后端工程师",
          ["chenmo@mail.com", "杭州", "6 年经验"],
          ["e-chen-1", "e-chen-2"],
          ["pr-chen-1"],
        ),
      ],
    },
  ],
};

/* ------------------------------------------------------------------ */
/* 其余候选人：结构完整但内容精简，用于验证列表滚动与折叠                 */
/* ------------------------------------------------------------------ */

function simplePerson(
  id: string,
  name: string,
  role: string,
  years: number,
  location: string,
  titles: string[],
): Person {
  const pid = `p-${id}`;
  const exps: Experience[] = [
    exp(`e-${id}-1`, titles[0].split(" · ")[0] || "某公司", role, "2021 — 至今", [
      `${role}，负责核心模块的迭代与重构`,
    ]),
    exp(`e-${id}-2`, "早期经历", role, "2018 — 2021", ["参与业务从 0 到 1 的建设"]),
  ];
  const prjs: ProjectItem[] = [
    proj(`pr-${id}-1`, "核心项目", "2023", "一段项目描述，说明问题、做法与结果。", ["项目"], 0),
  ];
  const resumes: Resume[] = titles.map((title, i) => ({
    id: `r-${id}-${i + 1}`,
    personId: pid,
    title,
    updatedAt: now - (i + 1) * day,
    messages: [],
    pages: [
      page(
        `pg-${id}-${i + 1}`,
        i === 0 ? "zh" : "en",
        i === 0 ? "中文简历" : "English",
        i === 0 ? name : name,
        role,
        [`${id}@mail.com`, location, `${years} 年经验`],
        exps.map((e) => e.id),
        prjs.map((p) => p.id),
      ),
    ],
  }));

  return {
    id: pid,
    name,
    nameEn: name,
    role,
    roleEn: role,
    email: `${id}@mail.com`,
    location,
    years,
    experiences: exps,
    projects: prjs,
    education: [edu(`ed-${id}-1`, "某大学", "学士", "2014 — 2018")],
    skills: [skill(`sk-${id}-1`, "技能", "待补充")],
    resumes,
  };
}

export const SEED: Workspace = {
  version: 1,
  people: [
    lin,
    chen,
    simplePerson("zhou", "周识", "前端工程师", 4, "北京", ["快手 · 前端工程师", "通用版 · 前端"]),
    simplePerson("he", "何以安", "数据分析师", 3, "深圳", ["美团 · 数据分析"]),
    simplePerson("su", "苏黎", "品牌设计师", 7, "广州", ["品牌视觉负责人"]),
    simplePerson("zheng", "郑野", "算法工程师", 5, "上海", ["推荐算法工程师", "英文投递版"]),
    simplePerson("bai", "白露", "内容运营", 2, "成都", ["内容运营 · 通用版"]),
    simplePerson("gu", "顾南", "技术产品经理", 8, "北京", ["技术产品经理", "通用版", "英文投递版"]),
  ],
  ui: {
    accent: "white",
    mode: "light",
    activePersonId: "p-lin",
    activeResumeId: "r-lin-1",
    expanded: ["p-lin"],
  },
};
