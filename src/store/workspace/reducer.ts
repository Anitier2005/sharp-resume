import type { Block, BlockRole, Person, Resume, Workspace } from "@/types";
import { mapSections, newSectionBlock, withEntry, withHeader, withoutBlock } from "@/lib/blocks";
import { uid } from "@/lib/uid";
import type { Action } from "./types";
import {
  isEmptyPage,
  mapPage,
  mapPerson,
  mapResume,
  newPerson,
  newPage,
  newResume,
  touch,
  withExpanded,
} from "./helpers";

const patchPerson = (state: Workspace, personId: string, fn: (person: Person) => Person): Workspace => ({
  ...state,
  people: state.people.map((person) => (person.id === personId ? fn(person) : person)),
});

/**
 * 给块写排版提示。
 *
 * 为什么 role 为空时要 delete 而不是写 undefined：
 * 这个字段会原样序列化进存档，`"role": null` 和「没有 role」在 Rust 侧
 * 反序列化成一个样子，但存档里多出的 null 会让「新旧数据对比」多一处噪声。
 */
function withRole(block: Block, role?: BlockRole): Block {
  const next: Block = { ...block };
  if (role) next.role = role;
  else delete next.role;
  return next;
}

/**
 * 当前简历：activeResumeId 失效时退回这个人的第一份。
 * 与 provider 里 `resume` 的派生规则一致 —— 两处不一致的话，
 * reducer 算出来的兜底页和界面显示的页会对不上。
 */
function activeResume(state: Workspace): Resume | null {
  const person =
    state.people.find((item) => item.id === state.ui.activePersonId) ?? state.people[0] ?? null;
  if (!person) return null;
  return person.resumes.find((item) => item.id === state.ui.activeResumeId) ?? person.resumes[0] ?? null;
}

/**
 * 页游标自检：`ui.openPages[0]` 就是「上次看的那一页」的持久化位置
 * （真正的视图态在 provider，不落盘）。
 *
 * 换人、换简历、删页都会让它变成死引用 —— 读它的地方不止一个
 * （启动恢复、给工具层的只读镜像），所以在 reducer 这一层统一收口：
 * 指向的页不在当前简历里就退到第一页，一页都没有就置空。
 * 老数据没有 openPages 时不动它：provider 会按「全部页」处理。
 */
function withPageCursor(state: Workspace): Workspace {
  const stored = state.ui.openPages;
  if (!stored?.length) return state;
  const pages = activeResume(state)?.pages ?? [];
  const alive = new Set(pages.map((page) => page.id));
  if (alive.has(stored[0])) return state;
  const head = pages[0]?.id ?? null;
  const rest = stored.filter((id) => id !== head && alive.has(id));
  return { ...state, ui: { ...state.ui, openPages: head ? [head, ...rest] : rest } };
}

export function reducer(state: Workspace, action: Action): Workspace {
  switch (action.type) {
    case "hydrate":
      return action.workspace;

    case "selectPerson": {
      const person = state.people.find((item) => item.id === action.personId);
      // activeResumeId 必须属于 activePersonId：否则右侧会停在上一个人的简历上
      const keeps = person?.resumes.some((item) => item.id === state.ui.activeResumeId) ?? false;
      const resumeId = keeps ? state.ui.activeResumeId : person?.resumes[0]?.id ?? null;
      return withPageCursor({
        ...state,
        ui: { ...state.ui, activePersonId: action.personId, activeResumeId: resumeId },
      });
    }

    case "togglePerson": {
      const list = state.ui.expanded ?? [];
      return {
        ...state,
        ui: {
          ...state.ui,
          expanded: list.includes(action.personId)
            ? list.filter((id) => id !== action.personId)
            : [...list, action.personId],
        },
      };
    }

    case "selectResume":
      return {
        ...state,
        ui: {
          ...state.ui,
          activePersonId: action.personId,
          activeResumeId: action.resumeId,
          expanded: withExpanded(state, action.personId),
        },
      };

    case "setAccent":
      return { ...state, ui: { ...state.ui, accent: action.accent, accentHex: action.accentHex } };

    case "setMode":
      return { ...state, ui: { ...state.ui, mode: action.mode } };

    case "setTextScale":
      return { ...state, ui: { ...state.ui, textScale: action.textScale } };

    case "patchUi":
      return { ...state, ui: { ...state.ui, ...action.patch } };

    case "createPerson": {
      const person = newPerson();
      return {
        ...state,
        people: [...state.people, person],
        ui: {
          ...state.ui,
          activePersonId: person.id,
          activeResumeId: person.resumes[0].id,
          expanded: withExpanded(state, person.id),
        },
      };
    }

    case "createResume":
      return patchPerson(state, action.personId, (person) => {
        // resumeId 由工具预生成：同一轮里的 renameResume / addPage 会引用它，
        // 拿不到才退回自己生成（保证老数据回放不缺字段）
        const resume = newResume(person.id, action.title, action.resumeId);
        return {
          ...person,
          resumes: [...person.resumes, resume],
        };
      });

    case "renameResume":
      return mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        title: action.title,
      }));

    case "deleteResume": {
      const person = state.people.find((item) => item.id === action.personId);
      const remaining = person?.resumes.filter((resume) => resume.id !== action.resumeId) ?? [];
      const wasActive = state.ui.activeResumeId === action.resumeId;
      return {
        ...state,
        people: state.people.map((item) =>
          item.id !== action.personId ? item : { ...item, resumes: remaining },
        ),
        ui: wasActive ? { ...state.ui, activeResumeId: remaining[0]?.id ?? null } : state.ui,
      };
    }

    case "deletePerson": {
      const remaining = state.people.filter((person) => person.id !== action.personId);
      const wasActive = state.ui.activePersonId === action.personId;
      if (!wasActive) return { ...state, people: remaining };
      const next = remaining[0] ?? null;
      return withPageCursor({
        ...state,
        people: remaining,
        ui: {
          ...state.ui,
          activePersonId: next?.id ?? null,
          activeResumeId: next?.resumes[0]?.id ?? null,
        },
      });
    }

    case "resumeTargetPages":
      return mapResume(state, action.personId, action.resumeId, (resume) => {
        // 必须整份展开：这两个键落在 Resume.extra 里，手写重建对象会把未知键一起丢掉
        const next = { ...resume };
        if (action.targetPages === undefined) {
          delete next.targetPages;
          delete next.targetPagesTurn;
        } else {
          next.targetPages = action.targetPages;
          next.targetPagesTurn = action.turn ?? 0;
        }
        return next;
      });

    case "pageBlocks":
      // 整页全量替换：顺序与 id 由调用方（工具侧）算好，这里只换数组。
      // 不做「按 id 合并」—— 合并要猜谁的顺序对，而调用方已经知道答案。
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) => ({
        ...page,
        blocks: action.blocks,
      }));

    case "removeEmptyPage": {
      let removed = false;
      const next = mapResume(state, action.personId, action.resumeId, (resume) => {
        const page = resume.pages.find((item) => item.id === action.pageId);
        // 双保险：工具侧已经判过空，这里再判一次。
        // 旧版本 agent 重放、或者确认窗里的旧批次，都可能带来一条「其实非空」的删除。
        // 最后一页也不删：空简历比空页更难解释。
        if (!page || !isEmptyPage(page) || resume.pages.length <= 1) return resume;
        removed = true;
        return { ...resume, pages: resume.pages.filter((item) => item.id !== action.pageId) };
      });
      // 被拒时原样返回：连 updatedAt 都不许动，否则「什么都没发生」也会让排版重跑
      return removed ? withPageCursor(next) : state;
    }

    case "resumeNotice":
      // 只写这一个标记，不碰 targetPages（那个 op 的缺省值是「清除」，混用就有副作用）
      return mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        [action.key]: action.value,
      }));

    /* ---------- 人的字段与库 ---------- */

    case "personFields":
      return patchPerson(state, action.personId, (person) => ({ ...person, fields: action.fields }));

    case "libraryItems":
      return patchPerson(state, action.personId, (person) => ({ ...person, items: action.items }));

    case "personAssets":
      return patchPerson(state, action.personId, (person) => ({ ...person, assets: action.assets }));

    case "itemAssets":
      return patchPerson(state, action.personId, (person) => ({
        ...person,
        items: person.items.map((item) =>
          item.id === action.itemId ? { ...item, assets: action.assets } : item,
        ),
      }));

    /* ---------- 页面 ---------- */

    case "addPage": {
      // id 在这里定下来：`open` 要把它写进 ui.openPages，而那是 state 级的事，
      // 拿不到「刚生成的那一页」就没法指过去
      const id = action.id ?? uid("pg");
      // assetId 只在真有原件时写：native 页不带这个字段，
      // 存档里少一条 `"assetId": null` 的噪声（同 withRole 的处理）
      const fresh = { ...newPage(id, action.label), kind: action.kind ?? "native" };
      const page = action.assetId ? { ...fresh, assetId: action.assetId } : fresh;
      const next = mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        pages: [...resume.pages, page],
      }));
      if (!action.open) return next;
      // openPages[0] 就是「上次看的那一页」这个持久化游标（见 provider 的注释）；
      // 真正的视图态在 provider 里，由调用方 dispatch 之后再切
      const rest = (next.ui.openPages ?? []).filter((pageId) => pageId !== id);
      return { ...next, ui: { ...next.ui, openPages: [id, ...rest] } };
    }

    case "blockRole":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) => ({
        ...page,
        blocks: (page.blocks ?? []).map((block) =>
          block.id === action.blockId ? withRole(block, action.role) : block,
        ),
      }));

    case "pageNote":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) => {
        const notes = page.notes ?? [];
        // 已存在就原样返回：这一批动作被重投时不该长出第二条一模一样的批注
        if (notes.some((note) => note.id === action.note.id)) return page;
        return { ...page, notes: [...notes, action.note] };
      });

    // 排版只由「页排版 agent」写：会话 agent 想改排版得走 request_layout
    case "pageStyle":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) => ({
        ...page,
        style: action.style,
      }));

    case "pageLayout":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) => ({
        ...page,
        layout: action.layout,
      }));

    // 原件指针落在简历上：`layout` 每次都被排版 agent 整体覆盖，挂那里会被写掉
    case "originDoc":
      return mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        originDoc: action.originDoc,
      }));

    case "addAssets":
      // 按 id 去重：同一次工具调用的事件可能重复送达（界面重连、事件重放），
      // 直接 push 会在材料区留下两张一模一样的图
      return mapPerson(state, action.personId, (person) => {
        const have = new Set((person.assets ?? []).map((asset) => asset.id));
        const fresh = action.assets.filter((asset) => !have.has(asset.id));
        if (!fresh.length) return person;
        return { ...person, assets: [...(person.assets ?? []), ...fresh] };
      });

    case "removePage": {
      const next = mapResume(state, action.personId, action.resumeId, (resume) => ({
        ...resume,
        pages:
          resume.pages.length <= 1
            ? resume.pages
            : resume.pages.filter((page) => page.id !== action.pageId),
      }));
      // 删掉的正好是「上次看的那一页」时，游标得自己退一格：
      // 否则存档里会留一个死 id，下次启动会先闪一帧空页再回退
      return withPageCursor(next);
    }

    case "pageLabel":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) => ({
        ...page,
        label: action.label,
      }));

    case "pageHeader": {
      // blockId 是工具在「这页还没有页头块」时预生成的 id，必须透传：
      // 后端这一轮的工作副本里已经有这个 id 的块，前端自己造一个就和它走散了。
      // 空串/非字符串等于没给 —— 退回让 blocks.ts 自己生成，绝不能用空串当块 id
      const blockId =
        typeof action.blockId === "string" && action.blockId ? action.blockId : undefined;
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) =>
        withHeader(page, action.header, blockId),
      );
    }

    /* ---------- 分区与条目（都是块） ---------- */

    case "addSection":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) => {
        const block = newSectionBlock(action.title, [], action.id);
        const blocks = page.blocks ?? [];
        // afterBlockId 给了就插到那个块后面；找不到（块已被删）就退回追加，
        // 比默默丢掉一条新分区要好 —— 内容不会凭空消失
        const at = action.afterBlockId
          ? blocks.findIndex((item) => item.id === action.afterBlockId)
          : -1;
        const next =
          at < 0
            ? [...blocks, block]
            : [...blocks.slice(0, at + 1), block, ...blocks.slice(at + 1)];
        return { ...page, blocks: next };
      });

    case "removeSection":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) =>
        withoutBlock(page, action.sectionId),
      );

    case "removeBlock":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) =>
        withoutBlock(page, action.blockId),
      );

    case "blockFields":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) => ({
        ...page,
        blocks: (page.blocks ?? []).map((block) =>
          block.id === action.blockId ? { ...block, fields: action.fields } : block,
        ),
      }));

    case "sectionTitle":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) =>
        mapSections(page, (block) =>
          block.id === action.sectionId ? { ...block, title: action.title } : block,
        ),
      );

    case "sectionItems":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) =>
        mapSections(page, (block) =>
          block.id === action.sectionId ? { ...block, items: action.items } : block,
        ),
      );

    case "entryPatch":
      return mapPage(state, action.personId, action.resumeId, action.pageId, (page) =>
        withEntry(page, action.entryId, action.patch),
      );

    /* ---------- 会话 ---------- */

    case "appendMessage":
      return patchPerson(state, action.personId, (person) => ({
        ...person,
        messages: [...person.messages, action.message],
      }));

    case "patchMessage":
      return patchPerson(state, action.personId, (person) => ({
        ...person,
        messages: person.messages.map((message) =>
          message.id === action.messageId ? { ...message, ...action.patch } : message,
        ),
      }));

    default:
      return state;
  }
}

void touch;
