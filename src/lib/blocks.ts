import type { Block, Field, ResumePage, Section, SectionEntry, Workspace } from "@/types";
import { uid } from "@/lib/uid";

/**
 * 块（v5 的基本单位）的读写。
 *
 * 一页就是 `blocks` 这一串东西，**它自己是唯一真相**。页头、分区都只是从这串块里
 * 挑出来的视图：
 *   - 页头 = 第一个 `fields` 块（没有就说明这页还没写页头）
 *   - 分区 = 所有 `section` 块，按它们在 blocks 里的顺序
 *
 * 之所以要「视图」而不是各存一份：存两份就会对不上。渲染、导出、AI 提示词都从这里取，
 * 免得每个地方各写一遍遍历。
 */

/* ---------- 构造 ---------- */

export function newFieldsBlock(fields: Field[] = [], id?: string): Block {
  return { id: id ?? uid("b"), kind: "fields", fields };
}

/**
 * 新分区块。`id` 由调用方给时照用：agent 同一轮里「建分区 → 往里加条目」
 * 靠这个 id 对上，前端自己生成就会和工具手里那个对不上。
 */
export function newSectionBlock(title = "", items: SectionEntry[] = [], id?: string): Block {
  return { id: id ?? uid("sc"), kind: "section", title, items };
}

export function newMediaBlock(assetId: string, caption?: string): Block {
  return { id: uid("b"), kind: "media", assetId, caption };
}

/* ---------- 归一化（v4 → v5） ---------- */

/**
 * 把一页补齐成 v5。幂等：搬完就清掉 header / sections，反复调用不会重复插块。
 *
 * 顺序跟 Rust 的 `ResumePage::normalize` 完全一致：页头块在最前，分区依次跟在后面。
 * 两边一旦不一致，同一份数据走一遍前端再走一遍后端就会变形。
 */
export function normalizePage(page: ResumePage): ResumePage {
  const header = page.header ?? [];
  const sections = page.sections ?? [];
  const blocks = page.blocks ?? [];

  if (header.length === 0 && sections.length === 0) {
    // 没有旧内容要搬。已经干净的页原样返回，避免每次读都造新对象
    if (page.kind && page.blocks && !("header" in page) && !("sections" in page)) return page;
    const next: ResumePage = { ...page, kind: page.kind || "native", blocks };
    delete next.header;
    delete next.sections;
    return next;
  }

  const carried: Block[] = [];
  if (header.length) carried.push(newFieldsBlock(header, `${page.id}-head`));
  for (const section of sections) {
    carried.push({
      id: section.id,
      kind: "section",
      title: section.title,
      items: section.items,
    });
  }

  const next: ResumePage = {
    ...page,
    kind: page.kind || "native",
    blocks: [...blocks, ...carried],
  };
  // 搬完就抹掉，否则下次读又会插一遍
  delete next.header;
  delete next.sections;
  return next;
}

export function normalizeWorkspace(workspace: Workspace): Workspace {
  let changed = false;
  const people = workspace.people.map((person) => ({
    ...person,
    resumes: person.resumes.map((resume) => {
      let dirty = false;
      const pages = resume.pages.map((page) => {
        const next = normalizePage(page);
        if (next !== page) dirty = true;
        return next;
      });
      if (!dirty) return resume;
      changed = true;
      return { ...resume, pages };
    }),
  }));
  if (!changed) return workspace;
  return { ...workspace, people };
}

/* ---------- 视图 ---------- */

/** 页头字段：第一个 fields 块。这页没有页头块就是空数组 */
export function headerOf(page: ResumePage): Field[] {
  const block = page.blocks?.find((item) => item.kind === "fields");
  return block?.fields ?? [];
}

/** 页头块本身（要改它的时候用） */
export function headerBlockOf(page: ResumePage): Block | undefined {
  return page.blocks?.find((item) => item.kind === "fields");
}

/** 所有分区：section 块的投影，顺序即 blocks 里的顺序 */
export function sectionsOf(page: ResumePage): Section[] {
  return (page.blocks ?? [])
    .filter((block) => block.kind === "section")
    .map((block) => ({
      id: block.id,
      title: block.title ?? "",
      items: block.items ?? [],
    }));
}

/** 全页的条目（跨分区），供 AI 定位与会话统计 */
export function entriesOf(page: ResumePage): SectionEntry[] {
  return (page.blocks ?? [])
    .filter((block) => block.kind === "section")
    .flatMap((block) => block.items ?? []);
}

/* ---------- 写入 ---------- */

/** 写页头：改第一个 fields 块；这页还没有页头块就插到最前面 */
export function withHeader(page: ResumePage, fields: Field[]): ResumePage {
  const blocks = page.blocks ?? [];
  const at = blocks.findIndex((block) => block.kind === "fields");
  if (at === -1) return { ...page, blocks: [newFieldsBlock(fields), ...blocks] };
  return {
    ...page,
    blocks: blocks.map((block, index) => (index === at ? { ...block, fields } : block)),
  };
}

/** 对每个 section 块做变换（改标题、改条目、挪位置都走这里） */
export function mapSections(page: ResumePage, fn: (block: Block) => Block): ResumePage {
  return {
    ...page,
    blocks: (page.blocks ?? []).map((block) => (block.kind === "section" ? fn(block) : block)),
  };
}

/** 改某一段条目：跨分区按 entryId 找，找到就是它 */
export function withEntry(page: ResumePage, entryId: string, patch: Partial<SectionEntry>): ResumePage {
  return mapSections(page, (block) => {
    const items = block.items ?? [];
    if (!items.some((entry) => entry.id === entryId)) return block;
    return {
      ...block,
      items: items.map((entry) => (entry.id === entryId ? { ...entry, ...patch } : entry)),
    };
  });
}

/** 删块：按 id。页头块也能删，删完这页就没页头了 */
export function withoutBlock(page: ResumePage, blockId: string): ResumePage {
  return { ...page, blocks: (page.blocks ?? []).filter((block) => block.id !== blockId) };
}
