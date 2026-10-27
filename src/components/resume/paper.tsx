import type { Person, ResumePage } from "@/types";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { compilePage, resolveAssetRefs } from "@/lib/compile";
import { cx } from "@/lib/cx";
import styles from "./paper.module.css";
import resumeStyles from "./resume.module.css";

/**
 * 纸面：只负责「装上成品」。
 *
 * 这里不再有任何 `if (块类型)` 的分支 —— 版面由 Rust 编译层产出
 * （`src-tauri/src/render`）。以前这里是 React 手写的一棵渲染树，
 * 而 Word 导出是另一套硬编码规则，两边各排各的；现在只有一个是真相。
 *
 * 与「打印 / 存为 PDF」共用同一份 DOM：`window.print()` 打的就是这个节点。
 */
interface PaperProps {
  person: Person;
  page: ResumePage;
}

interface Loaded {
  html: string;
  css: string;
  mode: "structured" | "manual";
}

/** 「我看过了」的记录：按页存，换页时旧记录自然失效，不需要用 effect 去清 */
interface SeenNotes {
  pageId: string;
  ids: Set<string>;
}

const NO_NOTES: ReadonlySet<string> = new Set<string>();

/**
 * 时间显示：`at` 是 unix 秒（跟 Rust 一致），但历史上也有过毫秒，
 * 用数量级判断一下，别把 2026 年显示成 1970 年。
 */
function noteTime(at: number): string {
  const ms = at > 1e12 ? at : at * 1000;
  const date = new Date(ms);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleString("zh-CN", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function Paper({ person, page }: PaperProps) {
  const [doc, setDoc] = useState<Loaded | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** 批注的宿主：绝对定位以它为参照，才能和纸面上的块对齐 */
  const hostRef = useRef<HTMLDivElement | null>(null);
  /** 展开的是哪一条 */
  const [openId, setOpenId] = useState<string | null>(null);
  /**
   * 关掉的批注只记在这里，不落库：批注是排版 agent 的「过程记录」，
   * 用户看过就算了。真删数据要走一条删除 op，不该在「点一下」里顺手发生。
   */
  const [seen, setSeen] = useState<SeenNotes>({ pageId: page.id, ids: new Set() });

  useEffect(() => {
    let alive = true;
    // 编译要走一次 IPC：打字时每个键都发一次太浪费，等停下来再算
    const timer = window.setTimeout(() => {
      void (async () => {
        try {
          const compiled = await compilePage(page, person);
          if (!alive || !compiled) return;
          const html = await resolveAssetRefs(compiled.html, person);
          if (!alive) return;
          setDoc({ html, css: compiled.css, mode: compiled.mode });
          setError(null);
        } catch (cause) {
          // 编译失败绝不能静默变成一片空白：用户会以为是自己的内容没了
          if (!alive) return;
          setError(cause instanceof Error ? cause.message : String(cause));
        }
      })();
    }, 120);
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [page, person]);

  const dismissed = seen.pageId === page.id ? seen.ids : NO_NOTES;

  /**
   * 整页空白：没有块，也没有手写成品。
   *
   * 这不是错误 —— agent 新建出来的页就是这样（`add_page` 只给一张空纸）。
   * 但界面上必须说出来，否则「一张白纸」和「渲染挂了」长得一模一样。
   */
  const blank = (page.blocks ?? []).length === 0 && !page.layout?.html?.trim();

  const notes = useMemo(
    () => (page.notes ?? []).filter((note) => !dismissed.has(note.id)),
    [page.notes, dismissed],
  );

  /** 关掉一条：记成当前页的「已读」，别的页的记录不受影响 */
  const dismiss = useCallback(
    (id: string) => {
      setSeen((prev) => {
        const base = prev.pageId === page.id ? prev.ids : NO_NOTES;
        return { pageId: page.id, ids: new Set(base).add(id) };
      });
      setOpenId(null);
    },
    [page.id],
  );

  /**
   * 把每个批注点贴到它该在的位置。
   *
   * 为什么必须量而不能纯 CSS：批注要贴在「某个块」的右侧中线，而块是编译
   * 产物、高度由内容决定 —— CSS 没有「兄弟元素的中线」这种选择器。
   * 位置直接写进 DOM 而不进 state：这是「用测量结果摆一下外部对象」，
   * 进 state 会多一轮渲染，还会在块高变化时反复抖动。
   */
  const measure = useCallback(() => {
    const host = hostRef.current;
    if (!host) return;
    const base = host.getBoundingClientRect().top;
    let stacked = 0;
    for (const el of host.querySelectorAll<HTMLElement>("[data-note-id]")) {
      const note = notes.find((item) => item.id === el.dataset.noteId);
      if (!note) continue;
      // 手写 parts 的块没有 data-block，找不到锚点就退化成页级批注，不能丢
      const anchor = note.blockId
        ? host.querySelector<HTMLElement>(`[data-block="${note.blockId.replace(/"/g, '\\"')}"]`)
        : null;
      if (anchor) {
        const rect = anchor.getBoundingClientRect();
        el.style.top = `${rect.top - base + rect.height / 2}px`;
        delete el.dataset.page;
      } else {
        el.style.top = `${6 + stacked * 22}px`;
        el.dataset.page = "1";
        stacked += 1;
      }
    }
  }, [notes]);

  useLayoutEffect(() => {
    if (!doc || !notes.length) return;
    measure();
    const host = hostRef.current;
    if (!host) return;
    // 图片加载完、字体回退都会让块高变化，观察宿主比监听每个块便宜
    const observer = new ResizeObserver(() => measure());
    observer.observe(host);
    window.addEventListener("resize", measure);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, [doc, notes, measure]);

  return (
    <article className={styles.paper} data-print="sheet" data-selectable data-mode={doc?.mode}>
      {error ? (
        <div className={styles.failed}>这一页没能编译出来：{error}</div>
      ) : doc ? (
        <>
          {/* 预设样式是常量字符串，随第一次编译一起到 */}
          <style>{doc.css}</style>
          {/* 空白页给一句话：一张什么都没有的纸，用户分不清是「没内容」还是「坏了」 */}
          {blank && (
            <div className={styles.blank} data-print="hide">
              <b>这一页还没有内容</b>
              <span>在右侧「编辑」里加分区，或直接告诉会话那边你要写什么。</span>
            </div>
          )}
          {/* 批注的定位基准：单独包一层，省得去动纸面本身的 position */}
          <div className={resumeStyles.pageHost} ref={hostRef}>
            <div dangerouslySetInnerHTML={{ __html: doc.html }} />

            {notes.length > 0 && (
              <div className={resumeStyles.noteLayer} data-print="hide">
                {notes.map((note, index) => (
                  <div
                    key={note.id}
                    className={resumeStyles.noteItem}
                    data-note-id={note.id}
                    data-print="hide"
                  >
                    <button
                      type="button"
                      className={cx(
                        resumeStyles.notePin,
                        openId === note.id && resumeStyles.notePinOn,
                      )}
                      aria-label={`批注 ${index + 1}`}
                      aria-expanded={openId === note.id}
                      onClick={() => setOpenId(openId === note.id ? null : note.id)}
                    >
                      {index + 1}
                    </button>

                    {openId === note.id && (
                      <div className={resumeStyles.noteCard} role="dialog" aria-label="批注内容">
                        <div className={resumeStyles.noteHead}>
                          <b>批注 {index + 1}</b>
                          <span>{noteTime(note.at)}</span>
                        </div>
                        <p className={resumeStyles.noteBody}>{note.text}</p>
                        <div className={resumeStyles.noteActs}>
                          <button type="button" onClick={() => dismiss(note.id)}>
                            知道了
                          </button>
                        </div>
                      </div>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
        </>
      ) : (
        <div className={styles.pending} />
      )}
    </article>
  );
}
