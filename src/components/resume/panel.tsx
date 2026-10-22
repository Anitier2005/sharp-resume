import { useEffect, useRef, useState } from "react";
import { useWorkspace } from "@/store/workspace";
import { useToast } from "@/components/ui/toast-context";
import { Dropdown, MenuItem, MenuSep } from "@/components/ui/menu";
import { ChevronDown, Close, Layers, Plus } from "@/components/ui/icons";
import { countChars, pageIndex } from "@/lib/doc";
import { pageToMarkdown, printPage, resumeToMarkdown, saveBytes, saveText } from "@/lib/export";
import { pageToDocx } from "@/lib/docx";
import { PAGE_KIND_LABEL, type PageKind } from "@/types";
import { LibraryDrawer } from "@/components/library/drawer";
import { Paper } from "./paper";
import { Portfolio } from "./portfolio";

const KIND_OPTIONS: PageKind[] = ["zh", "en", "portfolio"];

export function ResumePanel() {
  const { state, person, resume, page, activePageId, setActivePageId, dispatch, saveState } =
    useWorkspace();
  const toast = useToast();
  const [zoom, setZoom] = useState(100);
  const [libOpen, setLibOpen] = useState(false);
  const [sheets, setSheets] = useState(1);
  const canvasRef = useRef<HTMLDivElement | null>(null);
  const wrapRef = useRef<HTMLDivElement | null>(null);

  // 窗口变窄时自动缩小，保证纸面完整
  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const fit = () => {
      const width = el.clientWidth - 40;
      const next = Math.round(Math.min(1, Math.max(0.68, width / 374)) * 100);
      setZoom(next);
    };
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // 按 A4 比例估算这份内容占几页：纸张按宽度自适应，所以用宽高比换算，缩放不影响结果
  useEffect(() => {
    const paper = wrapRef.current?.firstElementChild as HTMLElement | null;
    if (!paper) return;
    const measure = () => {
      const width = paper.clientWidth;
      if (!width) return;
      const height = paper.scrollHeight;
      setSheets(Math.max(1, height / (width * (297 / 210))));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(paper);
    return () => observer.disconnect();
  }, [page?.id]);

  if (!person || !resume || !page) {
    return (
      <section className="doc">
        <div className="doc-empty">还没有简历。左侧点「新建简历」开始。</div>
      </section>
    );
  }

  const index = pageIndex(resume, activePageId);
  const chars = countChars(person, page);
  const meta = { 字: `${chars.toLocaleString("zh-CN")} 字` };

  const patchPage = (patch: Partial<typeof page>) =>
    dispatch({
      type: "updatePage",
      personId: person.id,
      resumeId: resume.id,
      pageId: page.id,
      patch,
    });

  const patchBullet = (expId: string, bulletIndex: number, text: string) => {
    const exp = person.experiences.find((item) => item.id === expId);
    if (!exp) return;
    const key = page.kind === "en" && exp.bulletsEn ? "bulletsEn" : "bullets";
    const list = [...(exp[key] ?? exp.bullets)];
    list[bulletIndex] = text;
    dispatch({
      type: "updateExperience",
      personId: person.id,
      expId,
      patch: { [key]: list },
    });
  };

  const saveLabel = saveState === "saving" ? "保存中…" : "已保存";

  return (
    <section className="doc">
      <div className="tabs">
        {resume.pages.map((item) => (
          <button
            key={item.id}
            type="button"
            className={`tab${item.id === page.id ? " on" : ""}`}
            onClick={() => setActivePageId(item.id)}
          >
            {item.label}
            {resume.pages.length > 1 && (
              <span
                className="tx"
                role="button"
                aria-label={`删除 ${item.label}`}
                tabIndex={-1}
                onClick={(event) => {
                  event.stopPropagation();
                  dispatch({
                    type: "removePage",
                    personId: person.id,
                    resumeId: resume.id,
                    pageId: item.id,
                  });
                  toast(`已删除「${item.label}」`);
                }}
              >
                <Close size={8} />
              </span>
            )}
          </button>
        ))}

        <Dropdown
          width={150}
          trigger={
            <button type="button" className="tab-add" aria-label="新增页面">
              <Plus size={11} />
            </button>
          }
        >
          {KIND_OPTIONS.map((kind) => (
            <MenuItem
              key={kind}
              label={PAGE_KIND_LABEL[kind]}
              onSelect={() =>
                dispatch({ type: "addPage", personId: person.id, resumeId: resume.id, kind })
              }
            />
          ))}
        </Dropdown>

        <button
          type="button"
          className="tabs-btn"
          onClick={() => setLibOpen(true)}
          title="编辑共享经历与作品，勾选本页用哪些"
        >
          <Layers size={11} />
          经历库
        </button>

        <span className="tabs-fill" />

        <Dropdown
          width={196}
          trigger={
            <button type="button" className="export">
              导出
              <ChevronDown size={9} />
            </button>
          }
        >
          <MenuItem label="打印 / 存为 PDF" hint="当前页" onSelect={printPage} />
          <MenuSep />
          <MenuItem
            label="Word · 当前页"
            hint=".docx"
            onSelect={() => {
              const bytes = pageToDocx(person, page, state.ui.accent);
              void saveBytes(
                `${person.name}-${page.label}.docx`,
                bytes,
                "Word 文档",
                ["docx"],
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
              ).then((path) => path && toast("已导出 Word"));
            }}
          />
          <MenuItem
            label="Markdown · 当前页"
            onSelect={() => {
              void saveText(`${person.name}-${page.label}.md`, pageToMarkdown(person, page)).then(
                (path) => path && toast("已导出 Markdown"),
              );
            }}
          />
          <MenuItem
            label="Markdown · 整份"
            onSelect={() => {
              void saveText(`${person.name}-${resume.title}.md`, resumeToMarkdown(person, resume.pages)).then(
                (path) => path && toast("已导出整份 Markdown"),
              );
            }}
          />
          <MenuSep />
          <MenuItem label="PNG 图片" hint="待接入" onSelect={() => toast("图片导出还没接，先用 PDF")} />
        </Dropdown>
      </div>

      <div className="canvas" ref={canvasRef}>
        <div className="paper-wrap" ref={wrapRef} style={{ zoom: zoom / 100 }}>
          {page.kind === "portfolio" ? (
            <Portfolio person={person} page={page} onPatchPage={patchPage} />
          ) : (
            <Paper person={person} page={page} onPatchPage={patchPage} onPatchBullet={patchBullet} />
          )}
        </div>
      </div>

      <div className="doc-foot">
        <span>
          第 {index + 1} 页 / 共 {resume.pages.length} 页
        </span>
        <span>{meta["字"]}</span>
        <span
          className={sheets > 1.02 ? "over" : undefined}
          title="按 A4 比例估算当前页占几页"
        >
          {sheets > 1.02 ? `≈ ${sheets.toFixed(1)} 页 A4` : "1 页 A4"}
        </span>
        <span className="save">
          <i />
          {saveLabel}
        </span>
        <div className="zoom-ctl">
          <button
            type="button"
            aria-label="缩小"
            onClick={() => setZoom((value) => Math.max(60, value - 10))}
          >
            −
          </button>
          <button type="button" className="zoom-val" onClick={() => setZoom(100)} title="回到 100%">
            {zoom}%
          </button>
          <button
            type="button"
            aria-label="放大"
            onClick={() => setZoom((value) => Math.min(150, value + 10))}
          >
            +
          </button>
        </div>
      </div>

      <LibraryDrawer open={libOpen} onClose={() => setLibOpen(false)} />
    </section>
  );
}
