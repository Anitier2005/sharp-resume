import { useEffect, useRef, useState } from "react";
import { useWorkspace } from "@/store/workspace";
import { useToast } from "@/components/ui/toast-context";
import { Dropdown, MenuItem, MenuSep } from "@/components/ui/menu";
import { ChevronRight, Plus, Search, User } from "@/components/ui/icons";
import { pageShort, type Person, type Resume } from "@/types";

function pageBadges(resume: Resume): string {
  const kinds = resume.pages.slice(0, 2).map((page) => pageShort(page.kind));
  return kinds.join(" · ");
}

export function Sidebar() {
  const { state, person, resume, dispatch, expanded, toggleExpanded } = useWorkspace();
  const toast = useToast();
  const [query, setQuery] = useState("");
  const [renaming, setRenaming] = useState<string | null>(null);
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
    if (item.name.toLowerCase().includes(q)) return true;
    return item.resumes.some((r) => r.title.toLowerCase().includes(q));
  });

  const selectResume = (owner: Person, target: Resume) => {
    dispatch({ type: "selectResume", personId: owner.id, resumeId: target.id });
    if (!expanded.has(owner.id)) toggleExpanded(owner.id);
  };

  return (
    <aside className="side">
      <button
        type="button"
        className="row em"
        onClick={() => {
          const target = person ?? state.people[0];
          if (!target) return;
          dispatch({ type: "createResume", personId: target.id });
          toast("已新建简历");
        }}
      >
        <Plus size={12} />
        新建简历
      </button>

      <div className="row search">
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
        <span className="kbd">Ctrl K</span>
      </div>

      <div className="sec">
        候选人
        <button
          type="button"
          aria-label="新建候选人"
          onClick={() => {
            dispatch({ type: "createPerson" });
            toast("已新建候选人");
          }}
        >
          <Plus size={11} />
        </button>
      </div>

      <div className="people">
        {people.map((item) => {
          const open = expanded.has(item.id);
          return (
            <div key={item.id}>
              <button
                type="button"
                className={`row person${open ? " open" : ""}`}
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
                <span className="pname">{item.name}</span>
                <span className="n">{item.resumes.length}</span>
                <ChevronRight size={9} className="chev" />
              </button>

              {open && (
                <ul className="subs">
                  {item.resumes.map((target) => {
                    const active = resume?.id === target.id;
                    if (renaming === target.id) {
                      return (
                        <li key={target.id} className="item editing">
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
                        className={`item${active ? " on" : ""}`}
                        onClick={() => selectResume(item, target)}
                      >
                        <span className="it">{target.title}</span>
                        <span className="pg">{pageBadges(target)}</span>
                        <span
                          className="more"
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
                      className="new-res"
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

        {people.length === 0 && <div className="side-empty">没有匹配的结果</div>}
      </div>

      <div className="side-foot">
        <span className="live" />
        本地工作区 · 自动保存
      </div>
    </aside>
  );
}
