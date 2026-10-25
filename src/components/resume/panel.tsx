import { useEffect, useRef, useState } from "react";
import { useWorkspace } from "@/store/workspace";
import { useToast } from "@/components/ui/toast-context";
import { Dropdown, MenuItem, MenuSep } from "@/components/ui/menu";
import { ChevronDown, Close, Plus } from "@/components/ui/icons";
import { countChars, pageIndex } from "@/lib/doc";
import { pageToMarkdown, printPage, resumeToMarkdown, saveBytes, saveText } from "@/lib/export";
import { pageToDocx } from "@/lib/docx";
import { PAGE_KIND_LABEL, type PageKind } from "@/types";
import { Paper } from "./paper";
import { Portfolio } from "./portfolio";
import { ResumeForm } from "./form";
import { cx } from "@/lib/cx";
import styles from "./resume.module.css";
import paperStyles from "./paper.module.css";

const KIND_OPTIONS: PageKind[] = ["zh", "en", "portfolio"];

type ViewMode = "edit" | "preview";

export function ResumePanel() {
  const { state, person, resume, page, activePageId, setActivePageId, dispatch, saveState } =
    useWorkspace();
  const toast = useToast();
  const [mode, setMode] = useState<ViewMode>("edit");
  const [zoom, setZoom] = useState(100);
  const [sheets, setSheets] = useState(1);
  /** 当前纸宽下 A4 一页的高度（px），用来画分页线 */
  const [a4Height, setA4Height] = useState(529);
  const canvasRef = useRef<HTMLDivElement | null>(null);
  const wrapRef = useRef<HTMLDivElement | null>(null);

  const preview = mode === "preview";

  // 窗口变窄时自动缩小，保证纸面完整（只在预览态有纸面，所以要跟着 mode 重跑）
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
  }, [mode]);

  // 按 A4 比例估算这份内容占几页：纸张按宽度自适应，所以用宽高比换算，缩放不影响结果
  useEffect(() => {
    const paper = wrapRef.current?.firstElementChild as HTMLElement | null;
    if (!paper) return;
    const measure = () => {
      const width = paper.offsetWidth;
      if (!width) return;
      const height = paper.offsetHeight;
      const onePage = width * (297 / 210);
      setA4Height(onePage);
      setSheets(Math.max(1, height / onePage));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(paper);
    return () => observer.disconnect();
  }, [mode, page?.id]);

  if (!person || !resume || !page) {
    return (
      <section className={styles.doc} data-col="doc">
        <div className={styles.empty}>还没有简历。左侧点「新建简历」开始。</div>
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

  /** 预览与导出必须是同一份 DOM：打印前先切到预览，避免用户在编辑态按 Ctrl+P 打出空白 */
  const handlePrint = () => {
    if (mode === "preview") {
      printPage();
      return;
    }
    setMode("preview");
    requestAnimationFrame(() => requestAnimationFrame(printPage));
  };

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
            <button type="button" className={styles.tabAdd} aria-label="新增页面">
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

      {preview ? (
        <div className={styles.canvas} ref={canvasRef} data-print="flow">
          <div
            className={paperStyles.wrap}
            data-print="page"
            ref={wrapRef}
            style={{ zoom: zoom / 100 }}
          >
            {page.kind === "portfolio" ? (
              <Portfolio person={person} page={page} />
            ) : (
              <Paper person={person} page={page} />
            )}
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
          onPatchPage={patchPage}
          onAddEntry={(experienceId) =>
            dispatch({
              type: "addEntry",
              personId: person.id,
              resumeId: resume.id,
              pageId: page.id,
              experienceId,
            })
          }
          onRemoveEntry={(entryId) =>
            dispatch({
              type: "removeEntry",
              personId: person.id,
              resumeId: resume.id,
              pageId: page.id,
              entryId,
            })
          }
          onMoveEntry={(entryId, dir) =>
            dispatch({
              type: "moveEntry",
              personId: person.id,
              resumeId: resume.id,
              pageId: page.id,
              entryId,
              dir,
            })
          }
          onUpdateEntry={(entryId, patch) =>
            dispatch({
              type: "updateEntry",
              personId: person.id,
              resumeId: resume.id,
              pageId: page.id,
              entryId,
              patch,
            })
          }
          onToggleProject={(itemId) =>
            dispatch({
              type: "togglePageItem",
              personId: person.id,
              resumeId: resume.id,
              pageId: page.id,
              field: "projectIds",
              itemId,
            })
          }
          onMoveProject={(itemId, dir) =>
            dispatch({
              type: "movePageItem",
              personId: person.id,
              resumeId: resume.id,
              pageId: page.id,
              field: "projectIds",
              itemId,
              dir,
            })
          }
        />
      )}

      <div className={styles.foot} data-print="hide">
        <span>
          第 {index + 1} 页 / 共 {resume.pages.length} 页
        </span>
        <span>{meta["字"]}</span>
        {/* 页数与缩放只对预览有意义：编辑态没有纸面，估算出来的数字会过期 */}
        {preview && (
          <span
            className={sheets > 1.02 ? styles.over : undefined}
            title="按 A4 比例估算当前页占几页"
          >
            {sheets > 1.02 ? `≈ ${sheets.toFixed(1)} 页 A4` : "1 页 A4"}
          </span>
        )}
        <span className={styles.save}>
          <i />
          {saveLabel}
        </span>
        {preview && (
        <div className={styles.zoomCtl}>
          <button
            type="button"
            aria-label="缩小"
            onClick={() => setZoom((value) => Math.max(60, value - 10))}
          >
            −
          </button>
          <button type="button" className={styles.zoomVal} onClick={() => setZoom(100)} title="回到 100%">
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
        )}
      </div>
    </section>
  );
}
