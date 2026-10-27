import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { useWorkspace } from "@/store/workspace";
import { useToast } from "@/components/ui/toast-context";
import { Dropdown, MenuItem, MenuLabel, MenuSep } from "@/components/ui/menu";
import { ContextMenu } from "@/components/ui/context-menu";
import { Confirm } from "@/components/ui/confirm";
import { ChevronDown, Close, More, Plus } from "@/components/ui/icons";
import { countChars, pageIndex } from "@/lib/doc";
import {
  exportResumeFolder,
  pageToMarkdown,
  printPage,
  resumeToMarkdown,
  saveBytes,
  saveText,
} from "@/lib/export";
import { pageToDocx } from "@/lib/docx";
import { displayName, type ResumePage } from "@/types";
import { Paper } from "./paper";
import { ResumeForm } from "./form";
import { cx } from "@/lib/cx";
import styles from "./resume.module.css";
import paperStyles from "./paper.module.css";

type ViewMode = "edit" | "preview";

/** 右键菜单是在哪一块点出来的：标签本身就是打开的，折叠项才需要「打开」 */
type MenuFrom = "tab" | "fold";

export function ResumePanel() {
  const {
    state,
    person,
    resume,
    page,
    activePageId,
    openPages,
    openPage,
    closePage,
    dispatch,
    saveState,
    layoutStatus,
  } = useWorkspace();
  const toast = useToast();
  /** 视图进 store：会话要据此告诉模型「用户现在在预览」 */
  const mode = state.ui.view ?? "edit";
  const setMode = (next: ViewMode) => dispatch({ type: "patchUi", patch: { view: next } });

  // 排版不再由「切到预览」触发：内容一变就自动通知排版 agent，
  // 归 provider 统一发起（那里是 state 的唯一汇聚点）。
  // 预览只是一个看法，顺手烧一次模型调用既费钱又不是用户要的时机。
  const [zoom, setZoom] = useState(100);
  const [sheets, setSheets] = useState(1);
  const [a4Height, setA4Height] = useState(529);
  const canvasRef = useRef<HTMLDivElement | null>(null);
  /** 页签栏外层：量可用宽度用 */
  const tabsRef = useRef<HTMLDivElement | null>(null);
  /** 页签实体层：页签与「+」都在里面，装不下的收进折叠菜单 */
  const stripRef = useRef<HTMLDivElement | null>(null);
  const addRef = useRef<HTMLButtonElement | null>(null);
  /** 页签实测宽度缓存：被折叠的页签已不在 DOM 里，只能靠它估算 */
  const widths = useRef(new Map<string, number>());
  const [visible, setVisible] = useState(99);
  const visibleRef = useRef(99);
  const [showMore, setShowMore] = useState(false);
  /** 「⋯」的开关放到这里：右键折叠项时要把它收起来，让位给右键菜单 */
  const [moreOpen, setMoreOpen] = useState(false);
  /** 右键菜单指向的那一页 */
  const [menu, setMenu] = useState<{ x: number; y: number; pageId: string; from: MenuFrom } | null>(
    null,
  );
  /** 等确认框点头才真删的页 */
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);
  /** 刚点了「+」，等 state 落地后把新页选中 */
  const wantNewPage = useRef(false);
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

  // 刚新增的那一页：直接选中。折叠逻辑会把当前页顶进可见区，不用再滚
  useEffect(() => {
    if (!wantNewPage.current || !resume?.pages.length) return;
    wantNewPage.current = false;
    openPage(resume.pages[resume.pages.length - 1].id);
  }, [resume?.pages.length, resume, openPage]);

  /**
   * 页签装不下就折叠成「⋯」，不再横向滚 —— 横滚的页签条会把右边的
   * 「编辑 | 预览」和「导出」压掉一截，看起来像被盖住。
   *
   * 宽度靠「实测 + 缓存」：可见的每次实测覆盖缓存，被折叠的沿用上次的值；
   * 出现没量过的新页签时先全展开量一轮，下一轮再收敛。
   *
   * avail 只由列宽和右侧两块决定，不看页签层自己有多宽 —— 结果确定，
   * 所以不会出现「收起一个 → 变宽 → 又放回去」的抖动。
   *
   * 关掉页签（页还在）之后也必须留着「⋯」：那是这些页唯一的重新打开入口。
   */
  const measure = useCallback(() => {
    const tabs = tabsRef.current;
    const strip = stripRef.current;
    if (!tabs || !strip || !resume) return;

    for (const node of strip.querySelectorAll<HTMLElement>("[data-tab-id]")) {
      const id = node.dataset.tabId;
      if (id) widths.current.set(id, node.offsetWidth);
    }

    const ids = openPages;
    if (ids.some((id) => !widths.current.has(id))) {
      if (visibleRef.current !== ids.length) {
        visibleRef.current = ids.length;
        setVisible(ids.length);
      }
      return;
    }

    const GAP = 1;
    const MORE_W = 24;
    const widthOf = (id: string) => widths.current.get(id) ?? 72;
    const seg = tabs.querySelector<HTMLElement>("[data-seg]")?.offsetWidth ?? 0;
    const exp = tabs.querySelector<HTMLElement>("[data-export]")?.offsetWidth ?? 0;
    const addW = addRef.current?.offsetWidth ?? 22;
    // 12 是左右内边距；「+」和「⋯」现在钉在页签区外面，也要先扣掉；6 是各处 gap 的余量
    const base = tabs.clientWidth - 12 - seg - exp - addW - 6;
    const sum = ids.reduce((total, id) => total + widthOf(id) + GAP, 0);

    const closed = resume.pages.length - ids.length;
    const withMore = closed > 0 || sum > base;
    const avail = withMore ? base - MORE_W - GAP : base;

    let used = 0;
    let fit = 0;
    for (const id of ids) {
      const w = widthOf(id);
      if (used + w + GAP > avail) break;
      used += w + GAP;
      fit += 1;
    }

    setShowMore(withMore);
    // 「⋯」收起时顺手把它的菜单也关掉，否则它下次出现会带着上次的展开态
    if (!withMore) setMoreOpen(false);
    if (visibleRef.current !== fit) {
      visibleRef.current = fit;
      setVisible(fit);
    }
  }, [openPages, resume]);

  /**
   * 首测必须发生在 paint 之前，否则会先闪一帧「页签全挤在一起」。
   * 所以用 useLayoutEffect 只建观察器：ResizeObserver 的初始观测回调
   * 就在 layout 之后、paint 之前，把测量写进回调等于免费拿到这个时机。
   * 连每个页签一起观察，页签增减或改名都会重新量。
   */
  useLayoutEffect(() => {
    const tabs = tabsRef.current;
    const strip = stripRef.current;
    if (!tabs || !strip) return;
    const observer = new ResizeObserver(() => measure());
    observer.observe(tabs);
    for (const node of Array.from(strip.children)) observer.observe(node);
    return () => observer.disconnect();
  }, [measure, visible, showMore]);

  useEffect(() => {
    const paper = wrapRef.current?.firstElementChild as HTMLElement | null;
    if (!paper) return;
    const read = () => {
      const width = paper.offsetWidth;
      if (!width) return;
      const onePage = width * (297 / 210);
      setA4Height(onePage);
      setSheets(Math.max(1, paper.offsetHeight / onePage));
    };
    read();
    const observer = new ResizeObserver(read);
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

  // 文件名与「导出」下拉保持一致：显示名-页名.扩展名
  const exportDocx = (target: ResumePage) => {
    const bytes = pageToDocx(person, target, state.ui.accent);
    void saveBytes(
      `${displayName(person.fields)}-${target.label || "未命名页"}.docx`,
      bytes,
      "Word 文档",
      ["docx"],
      "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ).then((path) => path && toast("已导出 Word"));
  };
  const exportMd = (target: ResumePage) => {
    void saveText(`${displayName(person.fields)}-${target.label || "未命名页"}.md`, pageToMarkdown(person, target)).then(
      (path) => path && toast("已导出 Markdown"),
    );
  };

  /** 整份导出：后端按页写文件到文件夹，并留一份 manifest 供下次增量更新 */
  const exportFolder = () => {
    void exportResumeFolder(state, person.id, resume.id)
      .then((out) => {
        const notes = [
          out.removed.length ? `清掉 ${out.removed.length} 个旧文件` : "",
          out.conflicts.length ? `${out.conflicts.length} 个同名文件另起了名字` : "",
        ].filter(Boolean);
        toast(
          `已导出 ${out.files.length} 个文件到「${out.dirName}」${notes.length ? `（${notes.join("，")}）` : ""}`,
        );
      })
      .catch((error) => toast(String(error)));
  };

  const pageMap = new Map(resume.pages.map((item) => [item.id, item]));
  /** 只剩一页时不能删：页是简历的骨架，删空了整份简历就没地方写东西了 */
  const sole = resume.pages.length <= 1;

  /**
   * 到底显示哪几个页签：快速检索列表的前 visible 个；当前页若落在折叠区，
   * 就让它顶掉最后一个，否则会出现「点了却在菜单里」，用户找不到自己刚打开的页。
   */
  const at = openPages.indexOf(page.id);
  const shownCount = Math.min(Math.max(visible, 0), openPages.length);
  const shownIds = openPages.slice(0, shownCount);
  if (showMore && at >= shownCount) {
    if (shownIds.length === 0) shownIds.push(page.id);
    else shownIds[shownIds.length - 1] = page.id;
  }
  const shownSet = new Set(shownIds);
  const foldedIds = openPages.filter((id) => !shownSet.has(id));
  const openSet = new Set(openPages);
  /** 被关掉、但页还在的那些：没有它们，「关闭标签」就是一条单向路 */
  const closedIds = resume.pages.map((item) => item.id).filter((id) => !openSet.has(id));

  const handleClose = (item: ResumePage) => {
    closePage(item.id);
    toast(`已关闭「${item.label || "未命名页"}」的标签，页还在`);
  };

  const handleDelete = (item: ResumePage) => {
    dispatch({ type: "removePage", ...base, pageId: item.id });
    // 顺手把死 id 从快速检索里清掉；删的是当前页时，closePage 会挑下一个落点
    closePage(item.id);
    setPendingDelete(null);
    toast(`已删除「${item.label || "未命名页"}」`);
  };

  const menuPage = menu ? pageMap.get(menu.pageId) ?? null : null;
  const pendingPage = pendingDelete ? pageMap.get(pendingDelete) ?? null : null;

  const openMenu = (event: ReactMouseEvent, pageId: string, from: MenuFrom) => {
    event.preventDefault();
    if (from === "fold") setMoreOpen(false);
    setMenu({ x: event.clientX, y: event.clientY, pageId, from });
  };

  /** 折叠项与「未在检索栏」共用一行：点一下就是打开并顶到最前 */
  const foldItem = (id: string) => {
    const item = pageMap.get(id);
    if (!item) return null;
    return (
      <MenuItem
        key={id}
        label={item.label || "未命名页"}
        onSelect={() => openPage(id)}
        onContextMenu={(event) => openMenu(event, id, "fold")}
      />
    );
  };

  return (
    <section className={styles.doc} data-col="doc">
      <div className={styles.tabs} data-print="hide" ref={tabsRef}>
        {/* 页签栏只是「快速检索」：放的是最近打开过的页，装不下的收进「⋯」。
            关掉某个标签只是把它从这里拿掉，页还在简历里。 */}
        <div className={styles.tabStrip} ref={stripRef}>
          {shownIds.map((id) => {
            const item = pageMap.get(id);
            if (!item) return null;
            return (
              <button
                key={id}
                type="button"
                data-tab-id={id}
                className={cx(styles.tab, id === page.id && styles.tabOn)}
                aria-current={id === page.id ? "page" : undefined}
                title={item.label || "未命名页"}
                onClick={() => openPage(id)}
                onContextMenu={(event) => openMenu(event, id, "tab")}
              >
                <span className={styles.tabName}>{item.label || "未命名页"}</span>
                {/* 最后一个标签不给关：关了也还得立刻放回来（当前页必须在检索栏里） */}
                {openPages.length > 1 && (
                  <span
                    className={styles.tx}
                    role="button"
                    aria-label={`关闭 ${item.label} 的标签`}
                    tabIndex={-1}
                    onClick={(event) => {
                      event.stopPropagation();
                      handleClose(item);
                    }}
                  >
                    <Close size={8} />
                  </span>
                )}
              </button>
            );
          })}
        </div>

        {/* 「+」和「⋯」是一组工具，钉在页签区右边。
            以前「+」在 .tabStrip 里，页签一多就和「⋯」分居两处，看着是散的。 */}
        <div className={styles.tabTools}>
          <button
            type="button"
            ref={addRef}
            className={styles.tabAdd}
            aria-label="新增页面"
            onClick={() => {
              wantNewPage.current = true;
              dispatch({ type: "addPage", personId: person.id, resumeId: resume.id });
              toast("已新增一页（空白，什么也没预置）");
            }}
          >
            <Plus size={12} />
          </button>

          {/* 折叠菜单挂在 .tabStrip 外面，否则会被它的 overflow:hidden 裁掉 */}
          {showMore && (
            <Dropdown
              open={moreOpen}
              onOpenChange={setMoreOpen}
              width={186}
              maxHeight={264}
              trigger={
                <button type="button" className={styles.more} aria-label="更多页面">
                  <More size={12} />
                </button>
              }
            >
              {foldedIds.length > 0 && (
                <>
                  <MenuLabel>已折叠 {foldedIds.length} 页</MenuLabel>
                  {foldedIds.map(foldItem)}
                </>
              )}
              {closedIds.length > 0 && (
                <>
                  {foldedIds.length > 0 && <MenuSep />}
                  <MenuLabel>未在检索栏</MenuLabel>
                  {closedIds.map(foldItem)}
                </>
              )}
            </Dropdown>
          )}
        </div>

        <div className={styles.seg} data-seg role="tablist" aria-label="编辑与预览">
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
            <button type="button" className={styles.export} data-export>
              导出
              <ChevronDown size={9} />
            </button>
          }
        >
          <MenuItem label="打印 / 存为 PDF" hint="当前页" onSelect={handlePrint} />
          <MenuSep />
          <MenuItem label="Word · 当前页" hint=".docx" onSelect={() => exportDocx(page)} />
          <MenuItem label="Markdown · 当前页" onSelect={() => exportMd(page)} />
          <MenuItem
            label="Markdown · 整份"
            onSelect={() => {
              void saveText(
                `${displayName(person.fields)}-${resume.title}.md`,
                resumeToMarkdown(person, resume.pages),
              ).then((path) => path && toast("已导出整份 Markdown"));
            }}
          />
          <MenuSep />
          <MenuItem
            label="Word · 整份"
            hint="导出文件夹"
            onSelect={exportFolder}
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
          onRemoveBlock={(blockId) => dispatch({ type: "removeBlock", ...base, blockId })}
          onBlockFields={(blockId, fields) =>
            dispatch({ type: "blockFields", ...base, blockId, fields })
          }
          onSectionTitle={(sectionId, title) =>
            dispatch({ type: "sectionTitle", ...base, sectionId, title })
          }
          onSectionItems={(sectionId, items) =>
            dispatch({ type: "sectionItems", ...base, sectionId, items })
          }
          onEntryPatch={(entryId, patch) => dispatch({ type: "entryPatch", ...base, entryId, patch })}
          onAddAssets={(assets) =>
            dispatch({ type: "addAssets", personId: person.id, assets })
          }
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
        {/* 排版 agent 在干活：只看「这一页」的，别页在忙不该在这显示 */}
        {layoutStatus?.pageId === page?.id && (
          <span className={styles.layout} title="页排版 agent 正在处理这一页">
            <i />
            {layoutStatus.text}
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

      {/* 页签和折叠项走同一套菜单：同一页不该因为藏在「⋯」里就少几项能力。
          区别只有「打开」和「关闭标签」——标签本身就是打开的，不需要再打开一次。 */}
      {menu && menuPage && (
        <ContextMenu x={menu.x} y={menu.y} onClose={() => setMenu(null)}>
          {menu.from === "fold" && (
            <MenuItem
              label="打开"
              hint={`第 ${pageIndex(resume, menuPage.id) + 1} 页`}
              onSelect={() => openPage(menuPage.id)}
            />
          )}
          {menu.from === "tab" && openPages.length > 1 && (
            <MenuItem label="关闭标签" hint="页保留" onSelect={() => handleClose(menuPage)} />
          )}
          <MenuItem label="导出 Word" hint=".docx" onSelect={() => exportDocx(menuPage)} />
          <MenuItem label="导出 Markdown" hint=".md" onSelect={() => exportMd(menuPage)} />
          <MenuSep />
          <MenuItem
            label="删除这一页"
            danger
            disabled={sole}
            hint={sole ? "至少要留一页" : undefined}
            onSelect={() => setPendingDelete(menuPage.id)}
          />
        </ContextMenu>
      )}

      {pendingPage && (
        <Confirm
          title={`删除「${pendingPage.label}」？`}
          desc="这一页的内容会被移除，无法撤销。如果只是想让它从页签栏消失，用「关闭标签」就好。"
          onCancel={() => setPendingDelete(null)}
          onConfirm={() => handleDelete(pendingPage)}
        />
      )}
    </section>
  );
}

