const { chromium } = require("playwright");
const EXE =
  "D:/Users/17718/AppData/Local/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-win64/chrome-headless-shell.exe";
const LEGACY = {
  version: 3,
  people: [
    {
      id: "p-old",
      name: "林夏",
      role: "产品设计师",
      email: "lin@example.com",
      location: "深圳",
      years: 6,
      experiences: [
        {
          id: "e1",
          company: "字节跳动",
          role: "产品设计师",
          period: "2023.03 – 至今",
          notes: "带 8 人团队，把发布周期从两周缩到三天\n自建灰度平台，覆盖 30 条业务线",
          facts: [{ id: "f1", label: "规模", text: "30 条业务线" }],
        },
      ],
      projects: [{ id: "pr1", title: "设计系统", year: "2022", desc: "一套组件库", tags: ["B 端"] }],
      education: [{ id: "ed1", school: "某大学", degree: "本科", period: "2014-2018" }],
      skills: [{ id: "s1", label: "工具", value: "Figma" }],
      messages: [],
      resumes: [
        {
          id: "r-old",
          personId: "p-old",
          title: "通用版",
          updatedAt: 1,
          pages: [
            {
              id: "pg1",
              kind: "zh",
              label: "中文",
              headline: "林夏",
              role: "产品设计师",
              meta: ["深圳", "6 年"],
              summary: "6 年 B 端产品设计经验。",
              entries: [
                { id: "en1", experienceId: "e1", bullets: ["负责设计系统落地，覆盖 30 条业务线"] },
              ],
              projectIds: ["pr1"],
            },
          ],
        },
      ],
    },
  ],
  ui: { accent: "indigo", mode: "light", activePersonId: "p-old", activeResumeId: "r-old", expanded: ["p-old"] },
};

(async () => {
  const b = await chromium.launch({ executablePath: EXE });
  const p = await b.newPage({ viewport: { width: 1500, height: 940 }, deviceScaleFactor: 1.5 });
  const errors = [];
  p.on("pageerror", (e) => errors.push(String(e)));
  p.on("console", (m) => m.type() === "error" && errors.push(m.text().slice(0, 150)));

  await p.goto("http://localhost:1420/", { waitUntil: "networkidle" });
  await p.evaluate((legacy) => localStorage.setItem("sharp-resume:workspace:v1", JSON.stringify(legacy)), LEGACY);
  await p.reload({ waitUntil: "networkidle" });
  await p.waitForSelector(".win");
  await p.waitForTimeout(1000);

  await p.screenshot({ path: "E:/ProgramProject/AIresume/.shots/v4-migrated-edit.png" });

  // 抽屉
  await p.locator('[class*="_libEntry_"]').first().click();
  await p.waitForTimeout(500);
  await p.screenshot({ path: "E:/ProgramProject/AIresume/.shots/v4-drawer-person.png" });
  await p.locator('[role="dialog"] nav button:has-text("库")').click();
  await p.waitForTimeout(400);
  // 展开第一条
  await p.locator('[role="dialog"] [class*="_title_"]').first().click();
  await p.waitForTimeout(400);
  await p.screenshot({ path: "E:/ProgramProject/AIresume/.shots/v4-drawer-items.png" });
  await p.keyboard.press("Escape");
  await p.waitForTimeout(300);

  // 预览
  await p.click('[role="tab"]:has-text("预览")');
  await p.waitForTimeout(600);
  await p.screenshot({ path: "E:/ProgramProject/AIresume/.shots/v4-migrated-preview.png" });
  const paper = await p.evaluate(() =>
    document.querySelector('[data-print="sheet"]')?.innerText.replace(/\n+/g, " | "),
  );
  console.log("纸面:", paper);

  console.log(errors.length ? `[errors] ${errors.slice(0, 3).join(" | ")}` : "[console] 无报错");
  await b.close();
})().catch((e) => { console.error("FAILED:", e.message); process.exit(1); });
