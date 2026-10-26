// 二次检查：等自动保存落盘后，看真实结构；再看预览截图是否被覆盖
const { chromium } = require("playwright");
const EXE =
  "D:/Users/17718/AppData/Local/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-win64/chrome-headless-shell.exe";
(async () => {
  const b = await chromium.launch({ executablePath: EXE });
  const p = await b.newPage({ viewport: { width: 1500, height: 940 } });
  await p.goto("http://localhost:1420/", { waitUntil: "networkidle" });
  await p.waitForSelector(".win");
  await p.waitForTimeout(1500);

  const saved = await p.evaluate(() => {
    const ws = JSON.parse(localStorage.getItem("sharp-resume:workspace:v1") || "null");
    if (!ws) return null;
    const person = ws.people[0];
    const page = person?.resumes?.[0]?.pages?.[0];
    return {
      version: ws.version,
      personFieldKeys: person?.fields?.map((f) => f.label),
      header: page?.header,
      sections: page?.sections?.map((s) => ({
        title: s.title,
        items: s.items.map((i) => ({ refId: i.refId ?? null, fields: i.fields, bullets: i.bullets })),
      })),
      // 关键：老字段一个都不该存在
      legacyKeys: Object.keys(person ?? {}).filter((k) =>
        ["name", "role", "email", "location", "years", "experiences", "projects", "education", "skills"].includes(k),
      ),
      pageLegacyKeys: Object.keys(page ?? {}).filter((k) =>
        ["kind", "headline", "meta", "summary", "entries", "experienceIds", "projectIds"].includes(k),
      ),
    };
  });
  console.log(JSON.stringify(saved, null, 1));
  await b.close();
})().catch((e) => { console.error("FAILED:", e.message); process.exit(1); });
