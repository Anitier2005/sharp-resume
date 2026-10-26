// v4 验收：零预设 → 自己加字段 → 自己加分区 → 预览 → 落盘结构
const { chromium } = require("playwright");
const EXE =
  "D:/Users/17718/AppData/Local/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-win64/chrome-headless-shell.exe";
const OUT = "E:/ProgramProject/AIresume/.shots";

(async () => {
  const browser = await chromium.launch({ executablePath: EXE });
  const page = await browser.newPage({ viewport: { width: 1500, height: 940 }, deviceScaleFactor: 1.5 });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => m.type() === "error" && errors.push(m.text().slice(0, 160)));

  await page.goto("http://localhost:1420/", { waitUntil: "networkidle" });
  await page.evaluate(() => localStorage.removeItem("sharp-resume:workspace:v1"));
  await page.reload({ waitUntil: "networkidle" });
  await page.waitForSelector(".win");
  await page.waitForTimeout(600);

  // 空库时右栏应提示新建
  console.log("空态提示:", await page.locator('[class*="_empty_"]').first().textContent().catch(() => "无"));

  await page.click("text=新增候选人");
  await page.waitForTimeout(600);

  const dump = async () => {
    return page.evaluate(() => {
      const raw = localStorage.getItem("sharp-resume:workspace:v1");
      if (!raw) return null;
      const ws = JSON.parse(raw);
      const person = ws.people[0];
      return {
        version: ws.version,
        personKeys: Object.keys(person).sort(),
        fields: person.fields,
        pages: person.resumes?.[0]?.pages?.map((p) => ({
          label: p.label,
          header: p.header,
          sections: p.sections.map((s) => ({ title: s.title, items: s.items.length })),
        })),
      };
    });
  };
  console.log("建人后结构:", JSON.stringify(await dump()));

  // 页头：加一个字段
  await page.locator('button:has-text("加一个字段")').first().click();
  await page.waitForTimeout(200);
  const hdr = page.locator('[aria-label="字段名"]').first();
  await hdr.fill("姓名");
  const hdrVal = page.locator('[aria-label="字段值"]').first();
  await hdrVal.fill("张三");
  await page.waitForTimeout(200);

  // 分区：加一个
  await page.locator('button:has-text("加一个分区")').click();
  await page.waitForTimeout(300);
  const secTitle = page.locator('[class*="_sectionTitle_"]').first();
  await secTitle.fill("工作经历");
  await page.waitForTimeout(200);

  // 条目：新建条目
  await page.locator('button:has-text("新建条目")').first().click();
  await page.waitForTimeout(300);
  const flds = page.locator('[aria-label="字段名"]');
  console.log("字段名输入框数量:", await flds.count());

  // 条目自己没有字段，先给它加一个（第 1 个「加一个字段」属于页头）
  await page.locator('button:has-text("加一个字段")').nth(1).click();
  await page.waitForTimeout(250);
  await page.locator('[aria-label="字段名"]').nth(1).fill("公司");
  await page.locator('[aria-label="字段值"]').nth(1).fill("某某科技");
  // 再加第二个字段：时间
  await page.locator('button:has-text("加一个字段")').nth(1).click();
  await page.waitForTimeout(250);
  await page.locator('[aria-label="字段名"]').nth(2).fill("时间");
  await page.locator('[aria-label="字段值"]').nth(2).fill("2023.03 – 至今");
  await page.waitForTimeout(200);

  // 加一句措辞
  await page.locator('button:has-text("加一句")').first().click();
  await page.waitForTimeout(200);
  const bullet = page.locator('[class*="_bulletRow_"] textarea').first();
  await bullet.fill("把发布周期从两周缩到三天，覆盖 30 条业务线");
  await page.waitForTimeout(400);

  console.log("填完后结构:", JSON.stringify(await dump(), null, 1));

  await page.screenshot({ path: `${OUT}/v4-edit.png` });

  // 预览
  await page.click('[role="tab"]:has-text("预览")');
  await page.waitForTimeout(600);
  const paper = await page.evaluate(() => {
    const el = document.querySelector('[data-print="sheet"]');
    if (!el) return null;
    return {
      text: el.innerText.replace(/\n+/g, " | ").slice(0, 160),
      editable: el.querySelectorAll('[contenteditable="true"]').length,
    };
  });
  console.log("纸面:", JSON.stringify(paper));
  await page.screenshot({ path: `${OUT}/v4-preview.png` });

  // 从库里引用：先开关闭预览回到编辑，测「从库里引用」空库提示
  await page.click('[role="tab"]:has-text("编辑")');
  await page.waitForTimeout(300);

  // 侧栏经历库入口
  const libEntry = page.locator('[class*="_libEntry_"]').first();
  console.log("侧栏资料入口:", await libEntry.count());
  await libEntry.click();
  await page.waitForTimeout(500);
  const tabs = await page.evaluate(() =>
    [...document.querySelectorAll('[role="dialog"][aria-label="资料"] nav button')].map((b) =>
      b.textContent.trim(),
    ),
  );
  console.log("抽屉面板:", JSON.stringify(tabs));
  await page.screenshot({ path: `${OUT}/v4-drawer.png` });

  console.log(errors.length ? `[errors] ${errors.slice(0, 4).join(" | ")}` : "[console] 无报错");
  await browser.close();
})().catch((e) => {
  console.error("FAILED:", e.message);
  process.exit(1);
});
