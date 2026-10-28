import { useEffect, useRef, useState } from "react";
import { useWorkspace } from "@/store/workspace";
import { useToast } from "@/components/ui/toast-context";
import { Dropdown, MenuItem, MenuSep } from "@/components/ui/menu";
import {
  BookOpen,
  ChevronDown,
  ChevronRight,
  Gear,
  Layers,
  Palette,
  Plus,
  Search,
  Sparkles,
  User,
} from "@/components/ui/icons";
import { LibraryDrawer } from "@/components/library/drawer";
import { KnowledgePanel } from "@/components/knowledge/panel";
import { SettingsSheet } from "@/components/settings/sheet";
import { ContextMenu } from "@/components/ui/context-menu";
import { Confirm } from "@/components/ui/confirm";
import { MenuItem as CtxItem, MenuSep as CtxSep } from "@/components/ui/menu";
import { displayName, type Person, type Resume } from "@/types";
import { cx } from "@/lib/cx";
import styles from "./sidebar.module.css";

function pageBadges(resume: Resume): string {
  return resume.pages.length ? `${resume.pages.length} 页` : "空";
}

export function Sidebar() {
  const { state, person, resume, dispatch, expanded, toggleExpanded } = useWorkspace();
  const toast = useToast();
  const [query, setQuery] = useState("");
  const [renaming, setRenaming] = useState<string | null>(null);
  /** 经历库属于「人」，抽屉也开在这里；面板自己从 store 取当前人 */
  const [libOpen, setLibOpen] = useState(false);
  /** 知识库是全局的（跨候选人）；设置同理，两者都收在左下角的「设置」项下面 */
  const [kbOpen, setKbOpen] = useState(false);
  /** 设置面板打开在哪一页；null = 关闭 */
  const [sheetTab, setSheetTab] = useState<"model" | "look" | null>(null);
  /** 「设置」下挂三个二级项，默认展开：默认收起的话这三个入口等于藏起来了 */
  const [navOpen, setNavOpen] = useState(true);
  /** 右键菜单：记下坐标与目标，null 表示关闭 */
  const [ctx, setCtx] = useState<
    | { x: number; y: number; kind: "person"; personId: string }
    | { x: number; y: number; kind: "resume"; personId: string; resumeId: string }
    | null
  >(null);
  /** 删候选人要二次确认（不可撤销） */
  const [pendingDelete, setPendingDelete] = useState<{ personId: string; name: string } | null>(null);
  const searchRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        searchRef.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const q = query.trim().toLowerCase();
  const people = state.people.filter((item) => {
    if (!q) return true;
    if (displayName(item.fields).toLowerCase().includes(q)) return true;
    return item.resumes.some((r) => r.title.toLowerCase().includes(q));
  });

  const openSheet = (tab: "model" | "look") => {
    setKbOpen(false);
    setSheetTab(tab);
  };

  const openKnowledge = () => {
    setSheetTab(null);
    setKbOpen(true);
  };

  const selectResume = (owner: Person, target: Resume) => {
    dispatch({ type: "selectResume", personId: owner.id, resumeId: target.id });
    if (!expanded.has(owner.id)) toggleExpanded(owner.id);
  };

  return (
    <aside className={styles.side} data-col="side" data-print="hide">
      {/* 搜索置顶：找人是这个侧栏最高频的动作 */}
      <div className={cx(styles.row, styles.search)}>
        <Search size={12} />
        <input
          ref={searchRef}
          value={query}
          placeholder="搜索"
          aria-label="搜索候选人或简历"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              setQuery("");
              event.currentTarget.blur();
            }
          }}
        />
        <span className={styles.kbd}>Ctrl K</span>
      </div>

      {/* 新增候选人：与「候选人」同级的一行，紧跟搜索。
          createPerson 会连带建「通用版」简历和首页，空库点它也不会没反应。 */}
      <button
        type="button"
        className={cx(styles.row, styles.em)}
        onClick={() => {
          dispatch({ type: "createPerson" });
          toast(state.people.length === 0 ? "已新建第一位候选人与简历" : "已新建候选人");
        }}
      >
        <Plus size={12} />
        新增候选人
      </button>


      <div className={styles.sec}>候选人</div>

      <div className={styles.people}>
        {people.map((item) => {
          const open = expanded.has(item.id);
          return (
            <div key={item.id}>
              <button
                type="button"
                className={cx(styles.row, styles.person, open && styles.personOpen)}
                onContextMenu={(event) => {
                  event.preventDefault();
                  if (person?.id !== item.id) {
                    dispatch({ type: "selectPerson", personId: item.id });
                  }
                  setCtx({ x: event.clientX, y: event.clientY, kind: "person", personId: item.id });
                }}
                onClick={() => {
                  if (!open) {
                    dispatch({ type: "selectPerson", personId: item.id });
                    toggleExpanded(item.id);
                  } else {
                    toggleExpanded(item.id);
                  }
                }}
              >
                <User size={12} />
                <span className={styles.pname}>
                  {displayName(item.fields) === "未命名"
                    ? `候选人 ${state.people.indexOf(item) + 1}`
                    : displayName(item.fields)}
                </span>
                <span className={styles.count}>{item.resumes.length}</span>
                <ChevronRight size={9} className={styles.chev} />
              </button>

              {open && (
                <ul className={styles.subs}>
                  {/* 经历与作品挂在「人」这一级，入口就放在人下面，不放简历工具条 */}
                  <li>
                    <button
                      type="button"
                      className={styles.libEntry}
                      title={`编辑${displayName(item.fields)}的字段与资料`}
                      onClick={() => {
                        if (person?.id !== item.id) {
                          dispatch({ type: "selectPerson", personId: item.id });
                        }
                        setLibOpen(true);
                      }}
                    >
                      <Layers size={10} />
                      <span className={styles.libName}>资料</span>
                      <span className={styles.libCount}>
                        {item.items.length} 条
                      </span>
                    </button>
                  </li>

                  {item.resumes.map((target) => {
                    const active = resume?.id === target.id;
                    if (renaming === target.id) {
                      return (
                        <li key={target.id} className={cx(styles.item, styles.editing)}>
                          <input
                            autoFocus
                            defaultValue={target.title}
                            aria-label="简历名称"
                            onBlur={(event) => {
                              const next = event.target.value.trim();
                              if (next && next !== target.title) {
                                dispatch({
                                  type: "renameResume",
                                  personId: item.id,
                                  resumeId: target.id,
                                  title: next,
                                });
                              }
                              setRenaming(null);
                            }}
                            onKeyDown={(event) => {
                              if (event.key === "Enter") event.currentTarget.blur();
                              if (event.key === "Escape") setRenaming(null);
                            }}
                          />
                        </li>
                      );
                    }
                    return (
                      <li
                        key={target.id}
                        className={cx(styles.item, active && styles.itemOn)}
                        onContextMenu={(event) => {
                          event.preventDefault();
                          selectResume(item, target);
                          setCtx({
                            x: event.clientX,
                            y: event.clientY,
                            kind: "resume",
                            personId: item.id,
                            resumeId: target.id,
                          });
                        }}
                        onClick={() => selectResume(item, target)}
                      >
                        <span className={styles.it}>{target.title}</span>
                        <span className={styles.pg}>{pageBadges(target)}</span>
                        <span
                          className={styles.more}
                          onClick={(event) => event.stopPropagation()}
                          role="presentation"
                        >
                          <Dropdown
                            width={140}
                            trigger={
                              <button type="button" aria-label="更多操作">
                                ⋯
                              </button>
                            }
                          >
                            <MenuItem label="重命名" onSelect={() => setRenaming(target.id)} />
                            <MenuItem label="复制一份" onSelect={() => dispatch({ type: "createResume", personId: item.id })} />
                            <MenuSep />
                            <MenuItem
                              label="删除简历"
                              danger
                              onSelect={() => {
                                if (item.resumes.length <= 1) {
                                  toast("至少保留一份简历");
                                  return;
                                }
                                dispatch({ type: "deleteResume", personId: item.id, resumeId: target.id });
                                toast("已删除");
                              }}
                            />
                          </Dropdown>
                        </span>
                      </li>
                    );
                  })}

                  <li>
                    <button
                      type="button"
                      className={styles.newRes}
                      onClick={() => dispatch({ type: "createResume", personId: item.id })}
                    >
                      + 新建简历
                    </button>
                  </li>
                </ul>
              )}
            </div>
          );
        })}

        {people.length === 0 && (
          <div className={styles.empty}>
            {state.people.length === 0
              ? "还没有候选人。点上方「新增候选人」开始。"
              : "没有匹配的结果"}
          </div>
        )}
      </div>

      {/* 底部：设置是一级项，下挂三个二级项。
          这三件都是「全局功能」（跟哪个候选人无关），放进候选人树会让人
          以为它属于某个人；塞进顶栏又会被当成窗口动作。 */}
      <nav className={styles.nav}>
        <button
          type="button"
          className={cx(styles.row, styles.navTop)}
          aria-expanded={navOpen}
          onClick={() => setNavOpen((value) => !value)}
        >
          <Gear size={12} />
          设置
          <ChevronDown
            size={10}
            className={cx(styles.navChevron, navOpen && styles.navChevronOn)}
          />
        </button>

        {navOpen && (
          <div className={styles.navSub}>
            <button
              type="button"
              className={cx(styles.subItem, kbOpen && styles.subItemOn)}
              onClick={openKnowledge}
            >
              <BookOpen size={12} />
              知识库
            </button>
            <button
              type="button"
              className={cx(styles.subItem, sheetTab === "model" && styles.subItemOn)}
              onClick={() => openSheet("model")}
            >
              <Sparkles size={12} />
              模型
            </button>
            <button
              type="button"
              className={cx(styles.subItem, sheetTab === "look" && styles.subItemOn)}
              onClick={() => openSheet("look")}
            >
              <Palette size={12} />
              外观
            </button>
          </div>
        )}
      </nav>

      <div className={styles.foot}>
        <span className={styles.live} />
        <span className={styles.footText}>本地工作区 · 自动保存</span>
      </div>

      {ctx && (
        <ContextMenu x={ctx.x} y={ctx.y} onClose={() => setCtx(null)}>
          {ctx.kind === "person" ? (
            <>
              <CtxItem
                label="新建简历"
                onSelect={() => {
                  dispatch({ type: "createResume", personId: ctx.personId });
                  toast("已新建简历");
                }}
              />
              <CtxItem label="编辑资料" onSelect={() => setLibOpen(true)} />
              <CtxSep />
              <CtxItem
                label="删除候选人"
                danger
                onSelect={() => {
                  const owner = state.people.find((row) => row.id === ctx.personId);
                  setPendingDelete({
                    personId: ctx.personId,
                    name: owner ? displayName(owner.fields) : "这位候选人",
                  });
                }}
              />
            </>
          ) : (
            <>
              <CtxItem label="重命名" onSelect={() => setRenaming(ctx.resumeId)} />
              <CtxItem
                label="复制一份"
                onSelect={() => {
                  dispatch({ type: "createResume", personId: ctx.personId });
                  toast("已新建简历");
                }}
              />
              <CtxSep />
              <CtxItem
                label="删除简历"
                danger
                onSelect={() => {
                  const owner = state.people.find((row) => row.id === ctx.personId);
                  if ((owner?.resumes.length ?? 0) <= 1) {
                    toast("至少保留一份简历");
                    return;
                  }
                  dispatch({
                    type: "deleteResume",
                    personId: ctx.personId,
                    resumeId: ctx.resumeId,
                  });
                  toast("已删除简历");
                }}
              />
            </>
          )}
        </ContextMenu>
      )}

      {pendingDelete && (
        <Confirm
          open
          title={`删除「${pendingDelete.name}」？`}
          description="这个人的简历、经历、作品和会话会一起删掉，无法撤销。"
          onCancel={() => setPendingDelete(null)}
          onConfirm={() => {
            dispatch({ type: "deletePerson", personId: pendingDelete.personId });
            setPendingDelete(null);
            toast("已删除候选人");
          }}
        />
      )}

      <LibraryDrawer open={libOpen} onClose={() => setLibOpen(false)} />
      <KnowledgePanel open={kbOpen} onClose={() => setKbOpen(false)} />
      <SettingsSheet
        open={sheetTab !== null}
        initialTab={sheetTab ?? "model"}
        onClose={() => setSheetTab(null)}
      />
    </aside>
  );
}
