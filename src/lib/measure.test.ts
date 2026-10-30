import { describe, expect, it } from "vitest";
import { clearOrigin, contentVersion, measureTip, originOf, pageMetrics, putOrigin } from "./measure";
import type { PageMeasure, Person, ResumePage } from "@/types";

const person = (patch: Partial<Person> = {}): Person => ({
  id: "p1",
  fields: [
    { id: "f1", label: "姓名", value: "张三" },
    { id: "f2", label: "照片", value: "", assetId: "photo.png" },
  ],
  items: [],
  messages: [],
  resumes: [],
  ...patch,
});

const page = (patch: Partial<ResumePage> = {}): ResumePage => ({
  id: "pg1",
  kind: "native",
  label: "第一页",
  blocks: [
    { id: "b1", kind: "fields", fields: [{ id: "f1", label: "姓名", value: "张三" }] },
    {
      id: "b2",
      kind: "section",
      title: "工作经历",
      items: [
        {
          id: "e1",
          fields: [{ id: "f3", label: "公司", value: "某公司" }],
          bullets: ["做了第一件事", "做了第二件事"],
        },
      ],
    },
  ],
  ...patch,
});

describe("contentVersion", () => {
  it("同内容的两个不同对象算出同一个版本号", () => {
    expect(contentVersion(person(), page())).toBe(contentVersion(person(), page()));
  });

  it("改一个要点里的一个字就换版本号", () => {
    const before = contentVersion(person(), page());
    const changed = page({
      blocks: [
        { id: "b1", kind: "fields", fields: [{ id: "f1", label: "姓名", value: "张三" }] },
        {
          id: "b2",
          kind: "section",
          title: "工作经历",
          items: [
            {
              id: "e1",
              fields: [{ id: "f3", label: "公司", value: "某公司" }],
              bullets: ["做了第一件事", "做了第三件事"],
            },
          ],
        },
      ],
    });
    expect(contentVersion(person(), changed)).not.toBe(before);
  });

  it("layout.parts 的键顺序不影响结果", () => {
    const a = page({ layout: { parts: { b1: "<p>一</p>", b2: "<p>二</p>" } } });
    const b = page({ layout: { parts: { b2: "<p>二</p>", b1: "<p>一</p>" } } });
    expect(contentVersion(person(), a)).toBe(contentVersion(person(), b));
  });

  it("只改批注不影响结果（notes 不参与排版高度）", () => {
    const plain = page();
    const noted = page({ notes: [{ id: "n1", text: "这里太挤", at: 1700000000 }] });
    expect(contentVersion(person(), noted)).toBe(contentVersion(person(), plain));
  });

  it("空入参给空串（还没有页可量）", () => {
    expect(contentVersion(null, null)).toBe("");
    expect(contentVersion(person(), null)).toBe("");
  });

  it("输出是 8 位 hex", () => {
    expect(contentVersion(person(), page())).toMatch(/^[0-9a-f]{8}$/);
  });
});

describe("measureTip", () => {
  const ok = (contentPx: number): PageMeasure => ({
    pageId: "pg1",
    status: "ok",
    version: 12,
    content: "a3f19c04",
    onePage: 1000,
    contentPx,
    lineHeight: 17,
  });

  it("溢出：说清超出多少页、约多少行，并且说的是「这一段」而不是整份", () => {
    const tip = measureTip(ok(1180), 1);
    expect(tip).toContain("这一段最多 1 页");
    expect(tip).toContain("超出 0.18");
    expect(tip).toContain("约 11 行正文");
    expect(tip).toContain("未达标");
  });

  it("达标：一页以内且说了「这一段最多 N 页」", () => {
    const tip = measureTip(ok(900), 1);
    expect(tip).toContain("实测 0.90 页");
    expect(tip).toContain("这一段最多 1 页");
    expect(tip).toContain("达标");
  });

  it("容忍带：1 页以上但没超上限，仍算没有溢出", () => {
    const tip = measureTip(ok(1010), 1);
    expect(tip).toContain("2% 容忍内");
    expect(tip).toContain("这一段最多 1 页");
  });

  it("量不到：按 reason 说「尚未实测」，不提「预览」", () => {
    const tip = measureTip({ pageId: "pg1", status: "unknown", version: 0, content: "", reason: "no-preview" }, 1);
    expect(tip).toContain("尚未实测");
    expect(tip).toContain("这一页还没量到");
    expect(tip).not.toContain("预览");
  });

  it("没有目标页数时不讲达标与否，也不提「这一段最多」", () => {
    const tip = measureTip(ok(900));
    expect(tip).toContain("没有目标页数");
    expect(tip).not.toContain("这一段最多");
  });

  it("过期：内容改过就说旧测量作废，不拿旧数糊弄", () => {
    const tip = measureTip(
      {
        pageId: "pg1",
        status: "stale",
        version: 12,
        content: "b7c1d2e3",
        previous: { content: "a3f19c04", onePage: 1000, contentPx: 1180 },
      },
      1,
    );
    expect(tip).toContain("尚未实测");
    expect(tip).toContain("作废");
    expect(tip).not.toContain("超出 0.18");
  });
});

describe("原件页数暂存", () => {
  it("存进去能读到，清掉就没了", () => {
    putOrigin("r1", { name: "简历.pdf", pages: 3 });
    expect(originOf("r1")?.pages).toBe(3);
    // 只读窥视：连读两次都还在
    expect(originOf("r1")?.name).toBe("简历.pdf");
    clearOrigin("r1");
    expect(originOf("r1")).toBeNull();
  });
});

describe("pageMetrics：一页能装多少 / 这页排了多少", () => {
  /** 210mm 在 96dpi 下的宽度，和离屏宿主写的是同一个数 */
  const W = 793.7;
  /** 16mm 页边距，上下两条 */
  const PAD_Y = 120.94;

  it("没有页边距时与旧公式完全一致（不受这次修改影响）", () => {
    const { onePage, contentPx } = pageMetrics({ width: W, height: 2400, padY: 0 });
    expect(onePage).toBe(W * (297 / 210));
    expect(contentPx).toBe(2400);
  });

  it("有页边距时一页变小、页数变大 —— 这才是不偏小 8.9% 的那一版", () => {
    const height = 2056; // 实测：4248 字的稿子，纸高（含上下页边距）
    const withPad = pageMetrics({ width: W, height, padY: PAD_Y });
    const without = pageMetrics({ width: W, height, padY: 0 });

    // 分母少了两条边距 = 一页能装的内容变少
    expect(withPad.onePage).toBeLessThan(without.onePage);
    expect(withPad.onePage).toBeCloseTo(W * (297 / 210) - PAD_Y, 2);
    // 分子也要减：offsetHeight 里的边距不是内容
    expect(withPad.contentPx).toBeCloseTo(height - PAD_Y, 2);

    const pagesWith = withPad.contentPx / withPad.onePage;
    const pagesWithout = without.contentPx / without.onePage;
    // 旧口径 1.83 页，真打印是 2 页；新口径逼近 2（差值来自最后一页没占满）
    expect(pagesWithout).toBeCloseTo(1.83, 1);
    expect(pagesWith).toBeGreaterThan(pagesWithout);
    expect(pagesWith).toBeCloseTo(1.93, 1);
  });

  it("纸只填满上下页边距时内容为 0：这是空页，不许报 1.00 页", () => {
    // hook 里 `contentPx <= 0` 直接走 failed("empty")，不会再算出页数
    const { contentPx } = pageMetrics({ width: W, height: PAD_Y, padY: PAD_Y });
    expect(contentPx).toBe(0);
    expect(contentPx <= 0).toBe(true);
  });
});
