import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { BookOpen, Close, Plus, Search } from "@/components/ui/icons";
import { Confirm } from "@/components/ui/confirm";
import { useToast } from "@/components/ui/toast-context";
import { useVeilWheelLock } from "@/components/ui/veil";
import {
  configureKnowledge,
  knowledgeDelete,
  knowledgeGet,
  knowledgeList,
  knowledgeSearch,
  knowledgeUpsert,
  type KnowledgeDoc,
  type KnowledgeEntry,
  type KnowledgeHit,
} from "@/lib/knowledge";
import { cx } from "@/lib/cx";
import shared from "@/components/ui/shared.module.css";
import styles from "./panel.module.css";

/**
 * 知识库面板：预置（只读）+ 我的（可改）在同一个抽屉里。
 *
 * 为什么不做成一个「列表 + 详情」的独立页面：知识库是给模型用的，
 * 用户打开它多半是为了「加一条事实」或「看看里面有什么」—— 两步之内要能完成，
 * 所以左侧检索列表、右侧直接编辑，不加第二层跳转。
 *
 * 预置条目来自编译期嵌入的清单，改不了；想改就「复制为我的条目」。
 */
export function KnowledgePanel({ open, onClose }: { open: boolean; onClose: () => void }) {
  const veilRef = useRef<HTMLDivElement | null>(null);
  const toast = useToast();

  const [entries, setEntries] = useState<KnowledgeEntry[]>([]);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<KnowledgeHit[] | null>(null);
  const [doc, setDoc] = useState<KnowledgeDoc | null>(null);
  /** 编辑态：新建或复制时打开，正文与标签都可改 */
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState({ title: "", tags: "", body: "" });
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);

  const reload = useCallback(async () => {
    try {
      setEntries(await knowledgeList());
    } catch (error) {
      toast(`读不到知识库：${error instanceof Error ? error.message : String(error)}`);
    }
  }, [toast]);

  // 打开时拉一次清单，并把嵌入模型配置推给后端（向量通道的开关）
  useEffect(() => {
    if (!open) return;
    configureKnowledge();
    void reload();
  }, [open, reload]);

  // ESC 关闭：捕获阶段拦下，别让底下的编辑器也收到
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      onClose();
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [open, onClose]);

  useVeilWheelLock(veilRef, open);

  const pick = async (id: string) => {
    try {
      const found = await knowledgeGet(id);
      setDoc(found);
      setEditing(false);
      if (found) setDraft({ title: found.title, tags: found.tags.join("、"), body: found.body });
    } catch (error) {
      toast(`读不到这一条：${error instanceof Error ? error.message : String(error)}`);
    }
  };

  const runSearch = async (text: string) => {
    setQuery(text);
    if (!text.trim()) {
      setHits(null);
      return;
    }
    try {
      setHits(await knowledgeSearch(text.trim()));
    } catch {
      setHits([]);
    }
  };

  const startNew = () => {
    setDoc(null);
    setEditing(true);
    setDraft({ title: "", tags: "", body: "" });
  };

  const copyAsMine = () => {
    if (!doc) return;
    setEditing(true);
    setDraft({
      title: `${doc.title}（副本）`,
      tags: doc.tags.join("、"),
      body: doc.body,
    });
  };

  const save = async () => {
    if (!draft.title.trim() || !draft.body.trim()) {
      toast("标题和正文都要填");
      return;
    }
    setBusy(true);
    try {
      const saved = await knowledgeUpsert({
        // 预置条目永远不带 id 保存：复制出来就是一条新条目
        id: doc && !doc.preset ? doc.id : "",
        title: draft.title,
        tags: draft.tags
          .split(/[、,，\s]+/)
          .map((item) => item.trim())
          .filter(Boolean),
        body: draft.body,
      });
      await reload();
      setDoc(saved);
      setEditing(false);
      toast("已保存");
    } catch (error) {
      toast(`保存失败：${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!doc) return;
    setBusy(true);
    try {
      await knowledgeDelete(doc.id);
      await reload();
      setDoc(null);
      setEditing(false);
      setConfirming(false);
      toast("已删除");
    } catch (error) {
      toast(`删除失败：${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setBusy(false);
    }
  };

  const preset = entries.filter((item) => item.preset);
  const mine = entries.filter((item) => !item.preset);

  if (!open) return null;

  const dialog = (
    <div ref={veilRef} className={shared.veil} data-print="hide">
      <div className={styles.box} role="dialog" aria-modal="true" aria-label="知识库">
        <header className={styles.head}>
          <div className={styles.title}>
            <BookOpen size={13} />
            <b>知识库</b>
            <span>{entries.length} 条 · 预置 {preset.length} 条</span>
          </div>
          <button type="button" className={shared.btnGhost} onClick={startNew}>
            <Plus size={11} />
            新建
          </button>
          <button type="button" className={shared.overlayClose} aria-label="关闭" onClick={onClose}>
            <Close size={11} />
          </button>
        </header>

        <div className={styles.body}>
          <div className={styles.side}>
            <div className={styles.searchRow}>
              <Search size={11} />
              <input
                value={query}
                placeholder="搜索知识库"
                aria-label="搜索知识库"
                onChange={(event) => void runSearch(event.target.value)}
              />
            </div>

            <div className={styles.list}>
              {hits ? (
                hits.length === 0 ? (
                  <p className={styles.empty}>没有命中。换个说法试试，或者直接新建一条。</p>
                ) : (
                  hits.map((hit) => (
                    <button
                      key={`${hit.id}-${hit.heading}`}
                      type="button"
                      className={styles.hit}
                      onClick={() => void pick(hit.id)}
                    >
                      <span className={styles.hitTitle}>
                        {hit.title}
                        {hit.preset && <i>预置</i>}
                      </span>
                      {hit.heading && <span className={styles.hitHead}>{hit.heading}</span>}
                      <span className={styles.hitText}>{hit.snippet}</span>
                    </button>
                  ))
                )
              ) : (
                <>
                  <div className={styles.groupLabel}>预置 · 只读</div>
                  {preset.length === 0 && (
                    <p className={styles.empty}>还没有预置条目（编译期清单为空）。</p>
                  )}
                  {preset.map((item) => (
                    <button
                      key={item.id}
                      type="button"
                      className={cx(styles.row, doc?.id === item.id && styles.rowOn)}
                      onClick={() => void pick(item.id)}
                    >
                      <span>{item.title}</span>
                      <i>预置</i>
                    </button>
                  ))}
                  <div className={styles.groupLabel}>我的</div>
                  {mine.length === 0 && <p className={styles.empty}>还没有自建条目。</p>}
                  {mine.map((item) => (
                    <button
                      key={item.id}
                      type="button"
                      className={cx(styles.row, doc?.id === item.id && styles.rowOn)}
                      onClick={() => void pick(item.id)}
                    >
                      <span>{item.title}</span>
                    </button>
                  ))}
                </>
              )}
            </div>
          </div>

          <div className={styles.detail}>
            {editing ? (
              <>
                <label className={shared.fld}>
                  <span className={shared.fldK}>标题</span>
                  <input
                    data-autofocus
                    value={draft.title}
                    placeholder="一个主题一句话"
                    onChange={(event) => setDraft({ ...draft, title: event.target.value })}
                  />
                </label>
                <label className={shared.fld}>
                  <span className={shared.fldK}>标签 · 顿号或逗号分隔</span>
                  <input
                    value={draft.tags}
                    placeholder="项目、电商"
                    onChange={(event) => setDraft({ ...draft, tags: event.target.value })}
                  />
                </label>
                <label className={cx(shared.fld, styles.bodyFld)}>
                  <span className={shared.fldK}>正文 · 超过 400 字会按段落切块</span>
                  <textarea
                    value={draft.body}
                    placeholder="写可核对的事实：数字、时间、角色、结果。"
                    onChange={(event) => setDraft({ ...draft, body: event.target.value })}
                  />
                </label>
                <div className={styles.actions}>
                  <button
                    type="button"
                    className={shared.btnGhost}
                    onClick={() => {
                      setEditing(false);
                      if (!doc) setDraft({ title: "", tags: "", body: "" });
                    }}
                  >
                    取消
                  </button>
                  <button
                    type="button"
                    className={shared.btnPrimary}
                    disabled={busy}
                    onClick={() => void save()}
                  >
                    保存
                  </button>
                </div>
              </>
            ) : doc ? (
              <>
                <div className={styles.docHead}>
                  <b>{doc.title}</b>
                  {doc.preset && <i>预置 · 只读</i>}
                  {doc.tags.length > 0 && <span className={styles.tags}>{doc.tags.join(" · ")}</span>}
                </div>
                <pre className={styles.docBody} data-selectable>
                  {doc.body}
                </pre>
                <div className={styles.actions}>
                  {doc.preset ? (
                    <button type="button" className={shared.btnGhost} onClick={copyAsMine}>
                      复制为我的条目
                    </button>
                  ) : (
                    <>
                      <button
                        type="button"
                        className={shared.btnGhost}
                        onClick={() => setEditing(true)}
                      >
                        编辑
                      </button>
                      <button
                        type="button"
                        className={cx(shared.btnGhost, styles.danger)}
                        onClick={() => setConfirming(true)}
                      >
                        删除
                      </button>
                    </>
                  )}
                </div>
              </>
            ) : (
              <div className={styles.blank}>
                <BookOpen size={18} />
                <b>知识库给模型查事实用</b>
                <span>
                  左侧选一条看内容；「新建」写你自己的。检索不看的条目也不会被用上 ——
                  写清楚、单一主题最容易命中。
                </span>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );

  const host = typeof document === "undefined" ? null : document.querySelector<HTMLElement>(".win");
  return (
    <>
      {host ? createPortal(dialog, host) : dialog}
      {/* 确认窗必须挂在 portal 之外单独渲染：它自己也 portal 进 .win，
          嵌在对话框树里会因为层级关系被盖住 */}
      {confirming && doc && (
        <Confirm
          open
          title={`删除「${doc.title}」？`}
          description="这条知识会从检索里消失，无法撤销。"
          onCancel={() => setConfirming(false)}
          onConfirm={() => void remove()}
        />
      )}
    </>
  );
}
