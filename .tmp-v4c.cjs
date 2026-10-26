// A) 全新建 → 填字段 → 等落盘 → 查结构；B) v3 老数据迁移
const { chromium } = require("playwright");
const EXE =
  "D:/Users/17718/AppData/Local/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-win64/chrome-headless-shell.exe";

const LEGACY = {
  version: 3,
  people: [
    {
      id: "p-old",
      name: "老数据",
      role: "产品设计师",
      email: "old@example.com",
      location: "深圳",
      years: 6,
      experiences: [
        {
          id: "e1",
          company: "旧公司",
          role: "设计师",
          period: "2020-2023",
          notes: "带 8 人团队\n把发布周期缩到三天",
          facts: [{ id: "f1", label: "规模", text: "30 条业务线" }],
        },
      ],
      projects: [{ id: "pr1", title: "旧项目", year: "2022", desc: "做了个东西", tags: ["B 端"] }],
      education: [{ id: "ed1", school: "某大学", degree: "本科", period: "2014-2018" }],
      skills: [{ id: "s1", label: "工具", value: "Figma" }],
      messages: [{ id: "m1", role: "user", content: "帮我看看", time: 1 }],
      resumes: [
        {
          id: "r-old",
          personId: "p-old",
          title: "旧简历",
          updatedAt: 1,
          pages: [
            {
              id: "pg1",
              kind: "zh",
              label: "中文",
              headline: "老数据",
              role: "产品设计师",
              meta: ["深圳", "6 年"],
              summary: "一句话摘要",
              entries: [{ id: "en1", experienceId: "e1", bullets: ["原句一", "原句二"] }],
              projectIds: ["pr1"],
            },
          ],
        },
      ],
    },
  ],
  ui: { accent: "indigo", mode: "light", activePersonId: "p-old", activeResumeId: "r-old" },
};

(async () => {
  const browser = await chromium.launch({ executablePath: EXE });
  const page = await browser.newPage({ viewport: { width: 1500, height: 940 }, deviceScaleFactor: 1.5 });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => m.type() === "error" && errors.push(m.text().slice(0, 160)));

  /* ---------- B) 先验迁移：直接塞老数据 ---------- */
  await page.goto("http://localhost:1420/", { waitUntil: "networkidle" });
  await page.evaluate((legacy) => {
    localStorage.setItem("sharp-resume:workspace:v1", JSON.stringify(legacy));
  }, LEGACY);
  await page.reload({ waitUntil: "networkidle" });
  await page.waitForSelector(".win");
  await page.waitForTimeout(900);

  const migrated = await page.evaluate(() => {
    const side = document.querySelector("aside");
    return {
      sidebar: side ? side.innerText.replace(/\s+/g, " ").slice(0, 90) : null,
      paper: document.querySelector('[data-print="sheet"]')?.innerText?.replace(/\n+/g, " | ").slice(0, 200) ?? null,
    };
  });
  console.log("迁移后侧栏:", migrated.sidebar);
  console.log("迁移后纸面:", migrated.paper);
  await page.screenshot({ path: "E:/ProgramProject/AIresume/.shots/v4-migrated.png" });

  /* ---------- A) 全新建流程 ---------- */
  await page.evaluate(() => localStorage.removeItem("sharp-resume:workspace:v1"));
  await page.reload({ waitUntil: "networkidle" });
  await page.waitForSelector(".win");
  await page.waitForTimeout(600);

  await page.click("text=新增候选人");
  await page.waitForTimeout(500);

  await page.locator('button:has-text("加一个字段")').first().click();
  await page.waitForTimeout(200);
  await page.locator('[aria-label="字段名"]').first().fill("姓名");
  await page.locator('[aria-label="字段值"]').first().fill("张三");

  await page.locator('button:has-text("加一个分区")').click();
  await page.waitForTimeout(300);
  await page.locator('[class*="_sectionTitle_"]').first().fill("工作经历");

  await page.locator('button:has-text("新建条目")').first().click();
  await page.waitForTimeout(300);
  await page.locator('button:has-text("加一个字段")').nth(1).click();
  await page.waitForTimeout(250);
  await page.locator('[aria-label="字段名"]').nth(1).fill("公司");
  await page.locator('[aria-label="字段值"]').nth(1).fill("某某科技");
  await page.locator('button:has-text("加一个字段")').nth(1).click();
  await page.waitForTimeout(250);
  await page.locator('[aria-label="字段名"]').nth(2).fill("时间");
  await page.locator('[aria-label="字段值"]').nth(2).fill("2023.03 – 至今");

  await page.locator('button:has-text("加一句")').first().click();
  await page.waitForTimeout(200);
  await page.locator('[class*="_bulletRow_"] textarea').first().fill("把发布周期从两周缩到三天");

  // 等自动保存真的落盘
  await page.waitForTimeout(2600);

  const fresh = await page.evaluate(() => {
    const ws = JSON.parse(localStorage.getItem("sharp-resume:workspace:v1") || "null");
    const person = ws.people[0];
    const page0 = person.resumes[0].pages[0];
    return {
      version: ws.version,
      personKeys: Object.keys(person).sort(),
      pageKeys: Object.keys(page0).sort(),
      header: page0.header.map((f) => `${f.label}=${f.value}`),
      sections: page0.sections.map((s) => ({
        title: s.title,
        items: s.items.map((i) => ({
          fields: i.fields.map((f) => `${f.label}=${f.value}`),
          bullets: i.bullets,
        })),
      })),
    };
  });
  console.log("全新落盘:", JSON.stringify(fresh, null, 1));

  await page.click('[role="tab"]:has-text("预览")');
  await page.waitForTimeout(600);
  await page.screenshot({ path: "E:/ProgramProject/AIresume/.shots/v4-preview.png" });

  console.log(errors.length ? `[errors] ${errors.slice(0, 4).join(" | ")}` : "[console] 无报错");
  await browser.close();
})().catch((e) => { console.error("FAILED:", e.message); process.exit(1); });
