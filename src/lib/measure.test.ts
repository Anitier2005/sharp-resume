import { describe, expect, it } from "vitest";
import { clearOrigin, contentVersion, measureTip, originOf, putOrigin } from "./measure";
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

  it("溢出：说清超出多少页、约多少行", () => {
    const tip = measureTip(ok(1180), 1);
    expect(tip).toContain("超出 0.18");
    expect(tip).toContain("约 11 行正文");
    expect(tip).toContain("未达标");
  });

  it("容忍带：1 页以上但没超上限，仍算没有溢出", () => {
    expect(measureTip(ok(1010), 1)).toContain("2% 容忍内");
  });

  it("量不到：按 reason 说「尚未实测」", () => {
    const tip = measureTip({ pageId: "pg1", status: "unknown", version: 0, content: "", reason: "no-preview" }, 1);
    expect(tip).toContain("尚未实测");
    expect(tip).toContain("预览没打开");
  });

  it("没有目标页数时不讲达标与否", () => {
    expect(measureTip(ok(900))).toContain("没有目标页数");
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
