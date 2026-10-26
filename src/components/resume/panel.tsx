import { useEffect, useRef, useState } from "react";
import { useWorkspace } from "@/store/workspace";
import { useToast } from "@/components/ui/toast-context";
import { Dropdown, MenuItem, MenuSep } from "@/components/ui/menu";
import { ChevronDown, Close, Plus } from "@/components/ui/icons";
import { countChars, pageIndex } from "@/lib/doc";
import { pageToMarkdown, printPage, resumeToMarkdown, saveBytes, saveText } from "@/lib/export";
import { pageToDocx } from "@/lib/docx";
import { displayName } from "@/types";
import { Paper } from "./paper";
import { ResumeForm } from "./form";
import { cx } from "@/lib/cx";
import styles from "./resume.module.css";
import paperStyles from "./paper.module.css";

type ViewMode = "edit" | "preview";

export function ResumePanel() {
  const { state, person, resume, page, activePageId, setActivePageId, dispatch, saveState } =
    useWorkspace();
  const toast = useToast();
  const [mode, setMode] = useState<ViewMode>("edit");
  const [zoom, setZoom] = useState(100);
  const [sheets, setSheets] = useState(1);
  const [a4Height, setA4Height] = useState(529);
  const canvasRef = useRef<HTMLDivElement | null>(null);
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const preview = mode === "preview";

  // 只在预览态有纸面，所以跟着 mode 重跑
  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const fit = () => {
      const width = el.clientWidth - 40;
      setZoom(Math.round(Math.min(1, Math.max(0.68, width / 374)) * 100));
    };
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(el);
    return () => observer.disconnect();
  }, [mode]);

  useEffect(() => {
    const paper = wrapRef.current?.firstElementChild as HTMLElement | null;
    if (!paper) return;
    const measure = () => {
      const width = paper.offsetWidth;
      if (!width) return;
      const onePage = width * (297 / 210);
      setA4Height(onePage);
      setSheets(Math.max(1, paper.offsetHeight / onePage));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(paper);
    return () => observer.disconnect();
  }, [mode, page?.id]);

  if (!person || !resume || !page) {
    return (
      <section className={styles.doc}>
        <div className={styles.empty}>还没有简历。左侧点「新增候选人」开始。</div>
      </section>
    );
  }

  const index = pageIndex(resume, activePageId);
  const chars = countChars(person, page);

  /** 打印前先切预览：保证打出来的就是屏幕上这份 DOM */
  const handlePrint = () => {
    if (preview) {
      printPage();
      return;
    }
    setMode("preview");
    requestAnimationFrame(() => requestAnimationFrame(printPage));
  };

  const base = { personId: person.id, resumeId: resume.id, pageId: page.id };
  const saveLabel = saveState === "saving" ? "保存中…" : "已保存";

  return (
    <section className={styles.doc} data-col="doc">
      <div className={styles.tabs} data-print="hide">
        {resume.pages.map((item) => (
          <button
            key={item.id}
            type="button"
            className={cx(styles.tab, item.id === page.id && styles.tabOn)}
            onClick={() => setActivePageId(item.id)}
          >
            {item.label}
            {resume.pages.length > 1 && (
              <span
                className={styles.tx}
                role="button"
                aria-label={`删除 ${item.label}`}
                tabIndex={-1}
                onClick={(event) => {
                  event.stopPropagation();
                  dispatch({ type: "removePage", ...base, pageId: item.id });
                  toast(`已删除「${item.label}」`);
                }}
              >
                <Close size={8} />
              </span>
            )}
          </button>
        ))}

        <button
          type="button"
          className={styles.tabAdd}
          aria-label="新增页面"
          onClick={() => {
            dispatch({ type: "addPage", personId: person.id, resumeId: resume.id });
            toast("已新增一页（空白，什么也没预置）");
          }}
        >
          <Plus size={11} />
        </button>

        <span className={styles.fill} />

        <div className={styles.seg} role="tablist" aria-label="编辑与预览">
          <button
            type="button"
            role="tab"
            aria-selected={mode === "edit"}
            className={cx(styles.segBtn, mode === "edit" && styles.segOn)}
            onClick={() => setMode("edit")}
          >
            编辑
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={mode === "preview"}
            className={cx(styles.segBtn, mode === "preview" && styles.segOn)}
            onClick={() => setMode("preview")}
          >
            预览
          </button>
        </div>

        <Dropdown
          width={196}
          trigger={
            <button type="button" className={styles.export}>
              导出
              <ChevronDown size={9} />
            </button>
          }
        >
          <MenuItem label="打印 / 存为 PDF" hint="当前页" onSelect={handlePrint} />
          <MenuSep />
          <MenuItem
            label="Word · 当前页"
            hint=".docx"
            onSelect={() => {
              const bytes = pageToDocx(person, page, state.ui.accent);
              void saveBytes(
                `${displayName(person.fields)}-${page.label}.docx`,
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
              void saveText(`${displayName(person.fields)}-${page.label}.md`, pageToMarkdown(person, page)).then(
                (path) => path && toast("已导出 Markdown"),
              );
            }}
          />
          <MenuItem
            label="Markdown · 整份"
            onSelect={() => {
              void saveText(
                `${displayName(person.fields)}-${resume.title}.md`,
                resumeToMarkdown(person, resume.pages),
              ).then((path) => path && toast("已导出整份 Markdown"));
            }}
          />
        </Dropdown>
      </div>

      {preview ? (
        <div className={styles.canvas} ref={canvasRef} data-print="flow">
          <div className={paperStyles.wrap} data-print="page" ref={wrapRef} style={{ zoom: zoom / 100 }}>
            <Paper person={person} page={page} />
            {sheets > 1.02 && (
              <div className={paperStyles.break} style={{ top: a4Height }} aria-hidden>
                <span>A4</span>
              </div>
            )}
          </div>
        </div>
      ) : (
        <ResumeForm
          person={person}
          page={page}
          onHeader={(header) => dispatch({ type: "pageHeader", ...base, header })}
          onLabel={(label) => dispatch({ type: "pageLabel", ...base, label })}
          onAddSection={(title) => dispatch({ type: "addSection", ...base, title })}
          onRemoveSection={(sectionId) => dispatch({ type: "removeSection", ...base, sectionId })}
          onSectionTitle={(sectionId, title) =>
            dispatch({ type: "sectionTitle", ...base, sectionId, title })
          }
          onSectionItems={(sectionId, items) =>
            dispatch({ type: "sectionItems", ...base, sectionId, items })
          }
          onEntryPatch={(entryId, patch) => dispatch({ type: "entryPatch", ...base, entryId, patch })}
        />
      )}

      <div className={styles.foot} data-print="hide">
        <span>
          第 {index + 1} 页 / 共 {resume.pages.length} 页
        </span>
        <span>{chars.toLocaleString("zh-CN")} 字</span>
        {preview && (
          <span className={sheets > 1.02 ? styles.over : undefined} title="按 A4 比例估算">
            {sheets > 1.02 ? `≈ ${sheets.toFixed(1)} 页 A4` : "1 页 A4"}
          </span>
        )}
        <span className={styles.save}>
          <i />
          {saveLabel}
        </span>
        {preview && (
          <div className={styles.zoomCtl}>
            <button type="button" aria-label="缩小" onClick={() => setZoom((v) => Math.max(60, v - 10))}>
              −
            </button>
            <button type="button" className={styles.zoomVal} onClick={() => setZoom(100)} title="回到 100%">
              {zoom}%
            </button>
            <button type="button" aria-label="放大" onClick={() => setZoom((v) => Math.min(150, v + 10))}>
              +
            </button>
          </div>
        )}
      </div>

    </section>
  );
}

