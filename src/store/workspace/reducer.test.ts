import { describe, expect, it } from "vitest";

import { cleanOps } from "@/lib/agent-actions";
import type { Block, Resume, Workspace } from "@/types";
import { reducer } from "./reducer";

/**
 * 只测新加的三个 op 的**语义**（替换 / 拒删 / 无副作用）。
 *
 * 为什么不用 jsdom 去渲染组件：这三个 op 是纯函数，输入 workspace、输出 workspace，
 * 用假 DOM 反而测不到真东西。
 */

function page(id: string, blocks: Block[] = []): Resume["pages"][number] {
  return { id, kind: "native", label: id, blocks };
}

function workspace(pages: Resume["pages"]): Workspace {
  const resume: Resume = {
    id: "r1",
    personId: "p1",
    title: "简历",
    updatedAt: 1,
    pages,
  };
  const state: Workspace = {
    version: 5,
    assets: [],
    people: [
      { id: "p1", fields: [], assets: [], items: [], messages: [], resumes: [resume] },
    ],
    ui: {
      accent: "white",
      mode: "light",
      activePersonId: "p1",
      activeResumeId: "r1",
    },
  };
  return state;
}

const block = (id: string): Block => ({ id, kind: "fields", fields: [] });

describe("pageBlocks", () => {
  it("整页全量替换：顺序与 id 就是给进来的那一份", () => {
    const before = workspace([page("pg1", [block("b1"), block("b2")])]);
    const after = reducer(before, {
      type: "pageBlocks",
      personId: "p1",
      resumeId: "r1",
      pageId: "pg1",
      blocks: [block("b3"), block("b1")],
    });
    const blocks = after.people[0].resumes[0].pages[0].blocks;
    expect(blocks.map((item) => item.id)).toEqual(["b3", "b1"]);
  });

  it("只动目标页，别的页原样", () => {
    const before = workspace([page("pg1", [block("b1")]), page("pg2", [block("b2")])]);
    const after = reducer(before, {
      type: "pageBlocks",
      personId: "p1",
      resumeId: "r1",
      pageId: "pg2",
      blocks: [],
    });
    expect(after.people[0].resumes[0].pages[1].blocks).toEqual([]);
    expect(after.people[0].resumes[0].pages[0].blocks.map((item) => item.id)).toEqual(["b1"]);
  });
});

describe("removeEmptyPage", () => {
  it("真空页才删", () => {
    const before = workspace([page("pg1", [block("b1")]), page("pg2")]);
    const after = reducer(before, {
      type: "removeEmptyPage",
      personId: "p1",
      resumeId: "r1",
      pageId: "pg2",
    });
    expect(after.people[0].resumes[0].pages.map((item) => item.id)).toEqual(["pg1"]);
  });

  it("非空页被 reducer 拒掉，且连 updatedAt 都不动", () => {
    const before = workspace([page("pg1", [block("b1")]), page("pg2")]);
    const after = reducer(before, {
      type: "removeEmptyPage",
      personId: "p1",
      resumeId: "r1",
      pageId: "pg1",
    });
    expect(after).toBe(before);
    expect(after.people[0].resumes[0].updatedAt).toBe(1);
  });

  it("file 页（只有 assetId、没有 block）不算空页", () => {
    const filePage: Resume["pages"][number] = {
      id: "pg2",
      kind: "file",
      label: "原件",
      assetId: "a-cert",
      blocks: [],
    };
    const before = workspace([page("pg1", [block("b1")]), filePage]);
    const after = reducer(before, {
      type: "removeEmptyPage",
      personId: "p1",
      resumeId: "r1",
      pageId: "pg2",
    });
    expect(after).toBe(before);
    expect(after.people[0].resumes[0].pages).toHaveLength(2);
  });
});

describe("resumeNotice", () => {
  it("写 targetPagesNotice 不碰 targetPages（这就是它单开一个 op 的原因）", () => {
    const base = workspace([page("pg1", [block("b1")])]);
    const withTarget: Workspace = {
      ...base,
      people: [
        {
          ...base.people[0],
          resumes: [{ ...base.people[0].resumes[0], targetPages: 1, targetPagesTurn: 7 }],
        },
      ],
    };
    const after = reducer(withTarget, {
      type: "resumeNotice",
      personId: "p1",
      resumeId: "r1",
      key: "targetPagesNotice",
      value: true,
    });
    const resume = after.people[0].resumes[0];
    expect(resume.targetPagesNotice).toBe(true);
    expect(resume.targetPages).toBe(1);
    expect(resume.targetPagesTurn).toBe(7);
  });

  it("白名单外的 key 与非布尔 value 在 cleanOps 就被丢掉", () => {
    const ok = cleanOps([
      { type: "resumeNotice", personId: "p1", resumeId: "r1", key: "targetPagesNotice", value: true },
    ]);
    expect(ok).toHaveLength(1);

    const badKey = cleanOps([
      { type: "resumeNotice", personId: "p1", resumeId: "r1", key: "targetPages", value: true },
    ]);
    expect(badKey).toHaveLength(0);

    const badValue = cleanOps([
      { type: "resumeNotice", personId: "p1", resumeId: "r1", key: "targetPagesNotice", value: 1 },
    ]);
    expect(badValue).toHaveLength(0);
  });
});

describe("cleanOps 认得新 op", () => {
  it("pageBlocks 的 blocks 必须是数组，removeEmptyPage 三个 id 都要有", () => {
    expect(
      cleanOps([
        { type: "pageBlocks", personId: "p1", resumeId: "r1", pageId: "pg1", blocks: [] },
      ]),
    ).toHaveLength(1);
    expect(
      cleanOps([{ type: "pageBlocks", personId: "p1", resumeId: "r1", pageId: "pg1" }]),
    ).toHaveLength(0);
    expect(
      cleanOps([{ type: "removeEmptyPage", personId: "p1", resumeId: "r1", pageId: "pg1" }]),
    ).toHaveLength(1);
    expect(cleanOps([{ type: "removeEmptyPage", personId: "p1", resumeId: "r1" }])).toHaveLength(0);
  });
});

describe("blockStyle", () => {
  it("合并：先设对齐、再改字号，两项都在", () => {
    const before = workspace([page("pg1", [block("b1")])]);
    const centered = reducer(before, {
      type: "blockStyle",
      personId: "p1",
      resumeId: "r1",
      pageId: "pg1",
      blockId: "b1",
      style: { align: "center" },
    });
    const sized = reducer(centered, {
      type: "blockStyle",
      personId: "p1",
      resumeId: "r1",
      pageId: "pg1",
      blockId: "b1",
      style: { size: 12 },
    });
    expect(sized.people[0].resumes[0].pages[0].blocks[0].style).toEqual({
      align: "center",
      size: 12,
    });
  });

  it("空对象 = 清掉，且存档里不留 style 键", () => {
    const before = workspace([page("pg1", [block("b1")])]);
    const dubbed = reducer(before, {
      type: "blockStyle",
      personId: "p1",
      resumeId: "r1",
      pageId: "pg1",
      blockId: "b1",
      style: { bold: true },
    });
    const cleared = reducer(dubbed, {
      type: "blockStyle",
      personId: "p1",
      resumeId: "r1",
      pageId: "pg1",
      blockId: "b1",
      style: {},
    });
    expect("style" in cleared.people[0].resumes[0].pages[0].blocks[0]).toBe(false);
  });

  it("只动目标块", () => {
    const before = workspace([page("pg1", [block("b1"), block("b2")])]);
    const after = reducer(before, {
      type: "blockStyle",
      personId: "p1",
      resumeId: "r1",
      pageId: "pg1",
      blockId: "b2",
      style: { align: "right" },
    });
    const blocks = after.people[0].resumes[0].pages[0].blocks;
    expect(blocks[0].style).toBeUndefined();
    expect(blocks[1].style).toEqual({ align: "right" });
  });
});

describe("cleanOps 认得 blockStyle", () => {
  it("空对象合法，越界字号与错对齐整条丢掉", () => {
    const shape = { type: "blockStyle", personId: "p1", resumeId: "r1", pageId: "pg1", blockId: "b1" };
    expect(cleanOps([{ ...shape, style: {} }])).toHaveLength(1);
    expect(cleanOps([{ ...shape, style: { align: "center", size: 12, bold: true } }])).toHaveLength(1);
    expect(cleanOps([{ ...shape, style: { size: 99 } }])).toHaveLength(0);
    expect(cleanOps([{ ...shape, style: { align: "middle" } }])).toHaveLength(0);
    expect(cleanOps([{ ...shape, style: "center" }])).toHaveLength(0);
    expect(cleanOps([{ ...shape }])).toHaveLength(0);
  });
});
