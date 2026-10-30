import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { useWorkspace } from "@/store/workspace";
import { useToast } from "@/components/ui/toast-context";
import { Dropdown, MenuItem, MenuLabel, MenuSep } from "@/components/ui/menu";
import { Tooltip } from "@/components/ui";
import { ContextMenu } from "@/components/ui/context-menu";
import { Confirm } from "@/components/ui/confirm";
import { ChevronDown, Close, More, Plus } from "@/components/ui/icons";
import { pageIndex } from "@/lib/doc";
import {
  exportResumeFolder,
  pageToMarkdown,
  printPage,
  resumeToMarkdown,
  saveBytes,
  savePdf,
  saveText,
} from "@/lib/export";
import { pageToDocx } from "@/lib/docx";
import { saveAsset } from "@/lib/assets";
import { nextPageLabel } from "@/lib/pages";
import { TOLERANCE_MAX } from "@/lib/measure";
import { A4_H, A4_W, A4_W_PX, PAGE_MARGIN_MM } from "@/lib/paper-size";
import { paginate, resetPagination } from "@/lib/paginate";
import { displayName, type ResumePage } from "@/types";
import { Paper } from "./paper";
import { ResumeForm } from "./form";
import { cx } from "@/lib/cx";
import styles from "./resume.module.css";
import paperStyles from "./paper.module.css";

type ViewMode = "edit" | "preview";

/** 右键菜单是在哪一块点出来的：标签本身就是打开的，折叠项才需要「打开」 */
type MenuFrom = "tab" | "fold";

/** 纸面缩放边界：再小看不清字，再大只是在滚一张糊掉的位图 */
const MIN_ZOOM = 25;
const MAX_ZOOM = 400;
/**
 * 状态栏菜单里的固定档位。
 *
 * 纸现在是 1:1 的真 A4（794px 宽），所以 **100% 就是真实 A4 大小** —— 在
 * 2K 屏上它几乎占满半个窗口。47% 才是「改前那张 374px 小纸」的等价观感，
 * 默认档就是它；需要按打印尺寸核对时再切到 100%。
 */
const ZOOM_PRESETS = [47, 50, 75, 100, 150, 200, 300, 400];
/** `.canvas` 的左右内边距之和：算「可用宽度」时要先扣掉 */
const CANVAS_PAD = 40;
/** 真 A4 在 96dpi 下的 px 宽度（= 210mm）：缩放只走 CSS zoom，不改纸本身 */
const PAPER_W = A4_W_PX;
/**
 * 默认观感倍率：374 / 793.7 ≈ 47%。
 *
 * 纸从缩略改 1:1 之后，100% 会突然变成「一张占满半个屏幕的 A4」，
 * 那不是用户熟悉的预览。默认档把视觉宽度拉回改前的 374px，
 * 尺寸变了、看起来没变。
 */
const DEFAULT_ZOOM = Math.round((374 / A4_W_PX) * 100);

const clampZoom = (value: number) => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, Math.round(value)));

/**
 * 自动适配用的取整：**向下**取整，不四舍五入。
 *
 * 45.9% 舍成 46% 只多 1px，可纸有 794 宽，1px 就够让画布长出横向滚动条
 * （多出来那条把整页往右挤，用户看到的反而是「纸被切了一刀」）。
 * 适配这件事宁可少 1%，不许多 1px。
 */
const autoZoom = (ratio: number) => clampZoom(Math.floor(ratio * 100 + 1e-9));

/** 判「能不能拖」用的容差：1px 的亚像素误差不算可滚 */
const SCROLL_EPS = 1;
/** 拖拽阈值：小于它算点击的手抖，不算要平移 */
const PAN_THRESHOLD = 3;

/** 图片判定与 `saveAsset` 保持同一口径（MIME 或扩展名任一命中） */
const IMAGE_EXT = new Set(["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg"]);
function isImageFile(file: File): boolean {
  if (file.type.startsWith("image/")) return true;
  const ext = file.name.split(".").pop()?.toLowerCase() ?? "";
  return IMAGE_EXT.has(ext);
}

/** 文件名去扩展名当页名：「扫描件.png」→「扫描件」 */
const filenameBase = (name: string) => name.replace(/\.[^.]+$/, "").trim();

export function ResumePanel() {
  const {
    state,
    person,
    resume,
    page,
    measure: pageMeasure,
    openPages,
    openPage,
    closePage,
    dispatch,
  } = useWorkspace();
  const toast = useToast();
  /** 视图进 store：会话要据此告诉模型「用户现在在预览」 */
  const mode = state.ui.view ?? "edit";
  const setMode = (next: ViewMode) => dispatch({ type: "patchUi", patch: { view: next } });

  // 排版不再由「切到预览」触发：排版已经并进会话工具集，只由会话 agent 在
  // 自己那一轮里动手。预览只是一个看法，顺手烧一次模型调用既费钱又不是用户要的时机。
  const [zoom, setZoom] = useState(DEFAULT_ZOOM);
  const canvasRef = useRef<HTMLDivElement | null>(null);
  const fileRef = useRef<HTMLInputElement | null>(null);
  /**
   * 当前倍率的一份即时副本。
   *
   * 滚轮回调是原生监听器、按钮回调也常常连点，闭包里的 `zoom` 可能已经过期；
   * 算「下一档」和「要不要动」都以它为基准，避免连点两次只放大一次。
   */
  const zoomRef = useRef(zoom);
  /**
   * 用户手动调过倍率（按钮、Ctrl+滚轮、菜单里的固定档）。
   *
   * 「放大到 130% 又跳回 100%」的根因就在这里：纸面变大 → 画布出滚动条、
   * 尺寸变化 → ResizeObserver 又按「铺满」算了一遍，把刚调好的倍率冲掉。
   * 手动档一旦成立，观察器只负责通报，不再写 zoom。
   */
  const manual = useRef(false);
  /**
   * 自动档的两副面孔，和 manual 是两个概念，别揉成一个：
   * page = 整页装进面板（照旧不超过 100%，保持原来的观感）；
   * width = 铺满宽度（宽面板就该铺满，允许超过 100%，上限 400%）。
   */
  const fitMode = useRef<"page" | "width">("page");
  /**
   * 「再适配一次」的入口。
   *
   * 适配不只在画布尺寸变的时候要做：**分页完成**换掉的是内容层（一张 210mm
   * 的占位变成 N 张真纸），画布自己的盒子可能一点没变，ResizeObserver 不会响。
   * 分页那一支要能主动叫一次，所以把适配函数挂到 ref 上。
   */
  const refit = useRef<() => void>(() => {});

  /** 只改倍率本身：所有写入都经过它，zoomRef 才不会和 state 走散 */
  const applyZoom = useCallback((next: number) => {
    zoomRef.current = next;
    setZoom(next);
  }, []);

  /**
   * 纸的**排版宽度**（未经缩放的 px）。
   *
   * 分页之后纸是 Paged.js 造的 `.pagedjs_page`，它的 `offsetWidth` 是 794
   * （它自己按 mm 取整算出来的），和常量 `A4_W_PX` 的 793.7 差 0.3px ——
   * 在 47% 档上这点差被四舍五入放大成 1px 的横向溢出。所以有真纸就量真纸，
   * 没有（回退档的单张纸，宽度就是 `.wrap` 的 210mm）才退回常量。
   *
   * `offsetWidth` 读的是**布局**宽度，不跟着祖先的 `zoom` 变
   * （zoom 只改绘制和 `getBoundingClientRect`），所以这里读到的永远是
   * 794 而不是「794×当前倍率」—— 正好是算比例该用的基准。
   */
  const paperWidth = useCallback(() => {
    const page = pagedRef.current?.querySelector<HTMLElement>(".pagedjs_page");
    const measured = page?.offsetWidth ?? 0;
    return measured > 0 ? measured : PAPER_W;
  }, []);

  /** 按画布可用宽度算一档倍率；量不到画布（不在预览）返回 null */
  const fitZoom = useCallback(
    (kind: "page" | "width") => {
      const el = canvasRef.current;
      if (!el) return null;
      const ratio = (el.clientWidth - CANVAS_PAD) / paperWidth();
      // 整页档：上限是「和改前那张小纸等宽」的 47%，面板再宽也不继续放大 ——
      // 1:1 的 A4 铺满宽面板只会变成一屏糊字，反而看不出整页结构。
      // 面板太窄时按上限的 68% 收缩，和改前「最小 68%」的观感对齐。
      if (kind === "page") {
        const cap = DEFAULT_ZOOM / 100;
        return autoZoom(Math.max(cap * 0.68, Math.min(cap, ratio)));
      }
      return autoZoom(ratio);
    },
    [paperWidth],
  );

  /**
   * 改倍率并把画布滚动位置按比例校正。
   *
   * 倍率一变内容尺寸立刻不同，不动 scrollLeft/scrollTop 的话可视区会跳到左上角那块，
   * 用户正看着的地方凭空消失。锚点取「可视中心在内容里的比例」而不是像素值：
   * 内容尺寸本身就在变，比例才是缩放前后都成立的那个量。
   */
  const zoomTo = useCallback(
    (next: number) => {
      const value = clampZoom(next);
      const el = canvasRef.current;
      if (!el || value === zoomRef.current) return;
      const ratioX = el.scrollWidth ? (el.scrollLeft + el.clientWidth / 2) / el.scrollWidth : 0.5;
      const ratioY = el.scrollHeight ? (el.scrollTop + el.clientHeight / 2) / el.scrollHeight : 0.5;
      applyZoom(value);
      // 等这一帧把新 zoom 画上去再改滚动量，否则读到的还是旧尺寸
      requestAnimationFrame(() => {
        el.scrollLeft = ratioX * el.scrollWidth - el.clientWidth / 2;
        el.scrollTop = ratioY * el.scrollHeight - el.clientHeight / 2;
      });
    },
    [applyZoom],
  );

  /** 手动缩放：按钮、滚轮、菜单里的固定档都走这里，从此不再自动适配 */
  const zoomManual = useCallback(
    (next: number) => {
      manual.current = true;
      zoomTo(next);
    },
    [zoomTo],
  );

  /** 适应宽度：宽面板就该把纸铺满，允许超过 100%；容器再变也跟着重算 */
  const zoomWidth = useCallback(() => {
    manual.current = false;
    fitMode.current = "width";
    const next = fitZoom("width");
    if (next !== null) zoomTo(next);
  }, [fitZoom, zoomTo]);

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
  /**
   * 等确认框点头才真删的块 / 分区。
   *
   * 为什么放在面板而不是表单里：删除只是「改 store 的两条指令」，
   * 确认必须和 agent 那条通道用同一个组件、同一套文案口径 ——
   * 否则会出现「agent 删分区要问、用户自己删不问」的荒唐局面。
   */
  const [pendingCut, setPendingCut] = useState<{ kind: "section" | "block"; blockId: string } | null>(
    null,
  );
  /** 等用户点头「仍然导出」的那一次导出；放面板不放 provider：它是导出的局部状态 */
  const [pendingExport, setPendingExport] = useState<{
    run: () => void;
    title: string;
    description: string;
  } | null>(null);
  /** 刚点了「+」，等 state 落地后把新页选中 */
  const wantNewPage = useRef(false);
  const preview = mode === "preview";
  /**
   * 这一页实测几页；量不到就是 null。
   * 分页虚线、导出前置门都用它 —— 只有一份数，别再在这里自己量一遍纸面。
   */
  const pages =
    pageMeasure?.status === "ok" && pageMeasure.onePage && pageMeasure.contentPx
      ? pageMeasure.contentPx / pageMeasure.onePage
      : null;
  /**
   * 分页结果的挂载点：Paged.js 切好的那几张纸。
   * 同时它也是一个「是否真分页成功」的开关 —— `paged` 为 false 时退回单张纸。
   */
  const pagedRef = useRef<HTMLDivElement | null>(null);
  /**
   * 单张纸那一份。它有两个身份：分页失败时的回退档，以及**分页的内容来源**
   * （编辑态根本没有纸面 DOM，只有预览这一份能克隆）。
   */
  const flatRef = useRef<HTMLDivElement | null>(null);
  const [paged, setPaged] = useState(false);
  /**
   * 分页切出来的**纸张数**（≠「第 N 页 / 共 M 页」里的页数）。
   *
   * 这两个数说的是两件事：页是简历的段数，纸是这一页排出来占了几张 A4。
   * 一页切成两张纸时，状态栏仍然写着「第 1 页 / 共 3 页」—— 用户于是以为
   * 「没分页」。所以纸张数必须在预览区自己说出来。
   */
  const [sheets, setSheets] = useState(0);
  /** 分页序号：切页之后上一轮迟到的结果直接丢掉，不许它盖掉新页 */
  const pagedSeq = useRef(0);

  /**
   * 内容比画布大才挂 `data-pannable`（CSS 靠它给抓手手势）。
   *
   * 直接写 DOM 而不是进 state：这是「把量到的结果标到外部对象上」，
   * 进 state 只会多一轮渲染，滚动条一出现/消失还会跟着抖。
   */
  const syncPannable = useCallback(() => {
    const el = canvasRef.current;
    if (!el) return;
    const scrollable =
      el.scrollWidth > el.clientWidth + SCROLL_EPS ||
      el.scrollHeight > el.clientHeight + SCROLL_EPS;
    if (scrollable) el.dataset.pannable = "1";
    else delete el.dataset.pannable;
  }, []);

  // 只在预览态有纸面，所以跟着 mode 重跑；切页也要重判（见 fit 里的分支）
  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const fit = () => {
      // 手动档：容器这点变化是「纸面放大后出滚动条」造成的，不是窗口变了
      if (manual.current) return;
      // 整页原件（扫描的 PDF 转图）是位图：自动缩放只会把它压糊，
      // 而且「原件」的意义就是按原样看 —— 自动档不动它，
      // 看不全就横向滚动，或者自己点「适应宽度」把面板吃满。
      if (page?.kind === "file" && fitMode.current === "page") return;
      const next = fitZoom(fitMode.current);
      if (next !== null) zoomTo(next);
    };
    // 切页 / 进预览：回到自动档 —— 上一页手动调的倍率不该跟过来
    manual.current = false;
    fitMode.current = "page";
    refit.current = fit;
    fit();
    syncPannable();
    const observer = new ResizeObserver(fit);
    observer.observe(el);
    return () => {
      observer.disconnect();
      refit.current = () => {};
    };
  }, [mode, page?.id, page?.kind, fitZoom, zoomTo, syncPannable]);

  /**
   * 分页结果换了内容层，就地再适配一次。
   *
   * 必须放在 layout 阶段：分页那一步 `setPaged(true)` 之后，画布里的东西从
   * 「一张 210mm 的占位」变成「N 张真纸」，而画布自己的盒子常常一点没变
   * （滚动条早就在了）—— ResizeObserver 不会响，于是倍率停在按**旧内容**
   * 算出来的那一档上。47% 的档位量给 794 的纸正好，量给别的宽度就会整张溢出。
   * 放到 layout 里重算，用户看不到中间那一帧。
   */
  useLayoutEffect(() => {
    if (!preview) return;
    refit.current();
    syncPannable();
  }, [preview, paged, sheets, syncPannable]);

  // 倍率一变，能不能拖也跟着变（放大到 100% 才需要拖）
  useEffect(() => {
    syncPannable();
  }, [zoom, paged, syncPannable]);

  /**
   * 按住拖动 = 平移画布。
   *
   * 为什么要有：纸是 1:1 的真 A4，47% 时整张放得下，可一旦放大到 100% 以上，
   * 一屏只看得到纸的一角 —— 只靠滚动条拖，鼠标得先跑到底下那条细线上，
   * 手感和「这张纸能不能动」完全不是一回事。预览里没有需要拖选的正文
   * （那是只读成品），所以左键整块让给平移，不会抢走别的交互。
   *
   * 三条纪律：
   *   1. 只在真能滚的时候接管，否则「拖了没反应」比不能拖更让人困惑；
   *   2. 从按钮上起手不算拖拽 —— 回退档纸面上还有批注针要点；
   *   3. 拖动期间临时 `user-select: none`，松手就摘掉：常驻会把预览里
   *      「选一段文字复制」也一起废掉。
   */
  const panState = useRef<{
    x: number;
    y: number;
    left: number;
    top: number;
    id: number;
    moved: boolean;
  } | null>(null);
  const [panning, setPanning] = useState(false);

  const panStart = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    const el = event.currentTarget;
    if ((event.target as HTMLElement).closest("button, a, input, textarea, select")) return;
    if (
      el.scrollWidth <= el.clientWidth + SCROLL_EPS &&
      el.scrollHeight <= el.clientHeight + SCROLL_EPS
    ) {
      return;
    }
    panState.current = {
      x: event.clientX,
      y: event.clientY,
      left: el.scrollLeft,
      top: el.scrollTop,
      id: event.pointerId,
      moved: false,
    };
    // 捕获指针：鼠标滑出画布（甚至滑出窗口）也还在拖，不会半路丢掉 move
    el.setPointerCapture(event.pointerId);
  }, []);

  const panMove = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    const pan = panState.current;
    if (!pan || pan.id !== event.pointerId) return;
    const dx = event.clientX - pan.x;
    const dy = event.clientY - pan.y;
    if (!pan.moved) {
      // 手抖 1px 不算拖：过阈值才认，免得点一下就把画面挪走
      if (Math.hypot(dx, dy) < PAN_THRESHOLD) return;
      pan.moved = true;
      setPanning(true);
      // 认了拖拽就把已经选起来的文字清掉，松手不会留下一片反白
      window.getSelection()?.removeAllRanges();
    }
    const el = event.currentTarget;
    el.scrollLeft = pan.left - dx;
    el.scrollTop = pan.top - dy;
  }, []);

  const panEnd = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    const pan = panState.current;
    if (!pan) return;
    panState.current = null;
    const el = event.currentTarget;
    // 捕获一定要还回去：留着的话后续的 pointer 事件会全被这个节点截走
    if (el.hasPointerCapture(pan.id)) el.releasePointerCapture(pan.id);
    if (pan.moved) setPanning(false);
  }, []);

  // Ctrl+滚轮缩放。必须用原生监听器：React 把 onWheel 挂在根节点上是 passive 的，
  // 里面调 preventDefault 只会得到一条控制台警告，画布照旧滚。
  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const onWheel = (event: WheelEvent) => {
      if (!event.ctrlKey) return; // 普通滚轮还是滚画布，别拦
      event.preventDefault();
      // 触控板捏合也带 ctrlKey，deltaY 只是很小；统一按 ×1.1 / ÷1.1 走一档，
      // 连续滚就是连续缩放，不去猜设备
      manual.current = true;
      zoomTo(zoomRef.current * (event.deltaY < 0 ? 1.1 : 1 / 1.1));
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [preview, zoomTo]);

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

  /**
   * 真分页：把这一页的成品切成若干张 A4。
   *
   * 为什么不能只画一条虚线：虚线只说得出「大约在这儿被切断」，说不出切断之后
   * 什么样 —— 段落会不会被劈成两半、小标题会不会孤零零留在上一页，都得真排一遍
   * 才看得见。预览的意义就是「所见即所印」，所以这里用 Paged.js 真的切一次。
   *
   * 三条纪律：
   *   1. 失败必退档。抛错、0 张纸，都退回单张纸 + 虚线，只留一条 console.warn。
   *      预览是只读的展示位，它白屏等于用户的内容丢了。
   *   2. 分页结果是 cloneNode 的副本，事件不会跟过来 —— 那一块不许挂任何交互。
   *   3. 只认预览这棵树。编辑态的分页没有意义，多跑一次只是白烧 CPU。
   *
   * 成品是**异步**编译出来的（Paper 内部 debounce + 一次 IPC），所以不能只在
   * 进预览时跑一次：盯住回退档那棵树，DOM 一变就重排，否则模型改完内容，
   * 分页还停在上一版。
   */
  useEffect(() => {
    if (!preview) {
      setPaged(false);
      setSheets(0);
      return;
    }
    const target = pagedRef.current;
    const flat = flatRef.current;
    if (!target || !flat) return;

    let alive = true;
    let timer = 0;

    const run = async () => {
      const source = flat.querySelector<HTMLElement>(".doc");
      if (!source) {
        setPaged(false);
        setSheets(0);
        return;
      }
      const seq = ++pagedSeq.current;
      try {
        await paginate(source, target, A4_W, A4_H, () => !alive || seq !== pagedSeq.current);
        if (!alive || seq !== pagedSeq.current) return;
        const count = target.querySelectorAll(".pagedjs_page").length;
        if (!count) {
          // 空白页切不出纸。退回单张纸 —— 它会显示「这一页还没有内容」，
          // 比一个空白的预览区可信。
          console.warn("[preview] 分页得到 0 张纸，退回单张纸");
          resetPagination();
        }
        setSheets(count);
        setPaged(count > 0);
      } catch (error) {
        if (!alive || seq !== pagedSeq.current) return;
        console.warn("[preview] Paged.js 分页失败，退回单张纸 + 虚线", error);
        resetPagination();
        setSheets(0);
        setPaged(false);
      }
    };
    /** 一次编译会连着改好几处 DOM，攒一下再跑，别每处都重排一遍 */
    const schedule = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => void run(), 180);
    };

    setPaged(false);
    void run();
    const observer = new MutationObserver(schedule);
    observer.observe(flat, { childList: true, subtree: true, characterData: true });
    return () => {
      alive = false;
      observer.disconnect();
      window.clearTimeout(timer);
      // 旧纸和它的样式一起收掉：Paged.js 插进 <head> 的样式不会自己走，
      // 留着会把 `.doc` 的 padding 也一起抹掉（那里有一条从 @media print
      // 展开出来的规则），回退档的单张纸就贴着纸边排了。
      target.replaceChildren();
      resetPagination();
    };
  }, [preview, page?.id]);

  // 纸面高度不再在这里量：测量统一归 provider 的离屏宿主（两套公式必然漂移），
  // 这里只读 pageMeasure（provider 的离屏实测），见下面的分页虚线与导出门。

  if (!person || !resume) {
    return (
      <section className={styles.doc}>
        <div className={styles.empty}>还没有简历。左侧点「新增候选人」开始。</div>
      </section>
    );
  }

  /**
   * 有简历、但一页都没有。
   *
   * agent 能把所有页删光（`delete_page` 不做「最后一页」保护），删完必须留一条
   * 能自己走出来的路 —— 早先这里复用「还没有简历」那句，也没有新建入口，
   * 用户会卡在一个既没内容也没按钮的面板里。
   */
  if (!page) {
    return (
      <section className={styles.doc}>
        <div className={styles.emptyStack}>
          <div>这份简历还没有页面。</div>
          <button
            type="button"
            className={styles.emptyAction}
            onClick={() => {
              wantNewPage.current = true;
              dispatch({
                type: "addPage",
                personId: person.id,
                resumeId: resume.id,
                label: nextPageLabel(resume.pages),
              });
            }}
          >
            新建一页
          </button>
        </div>
      </section>
    );
  }

  /**
   * 「打出来的那一份」必须与屏幕上那一份是同一个 DOM。
   * 编辑态下纸面是表单，导出前先切预览、等两帧（样式与分页都落定）再动手。
   */
  const whenPreview = (run: () => void) => {
    if (preview) {
      run();
      return;
    }
    setMode("preview");
    requestAnimationFrame(() => requestAnimationFrame(run));
  };

  /** 打印前先切预览：保证打出来的就是屏幕上这份 DOM */
  const handlePrint = () => whenPreview(printPage);

  const base = { personId: person.id, resumeId: resume.id, pageId: page.id };

  // 文件名与「导出」下拉保持一致：显示名-页名.扩展名
  const exportDocx = (target: ResumePage) => {
    const bytes = pageToDocx(person, target, state.ui.accent);
    void saveBytes(
      `${displayName(person.fields)}-${target.label || "未命名页"}.docx`,
      bytes,
      "Word 文档",
      ["docx"],
    ).then((path) => path && toast("已导出 Word"));
  };
  /**
   * 导出 PDF。边距按「纸面归谁管」分两档：
   * 分页后的真 A4 已经把页边距画在纸里，再给一次就是双重边距（内容整体缩小一圈）；
   * 单张预览没有印在纸上的边距，那一份必须由打印参数补上。
   */
  const exportPdf = (target: ResumePage) => {
    whenPreview(() => {
      void savePdf(
        `${displayName(person.fields)}-${target.label || "未命名页"}.pdf`,
        paged ? 0 : PAGE_MARGIN_MM,
      )
        .then((path) => path && toast("已导出 PDF"))
        .catch((error) => toast(`导出 PDF 失败：${String(error)}`));
    });
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

  /**
   * 这一页实测溢出多少页；没量到、或还在 2% 容忍内就给 null。
   * 量不到的页不拦（F-011）：没有数就说没有数，不拿「1 页」替用户决定。
   */
  const overflowBy = () => (pages !== null && pages > TOLERANCE_MAX ? pages : null);

  /** 单页导出的门：打印 / Word 当前页。已设目标页数且实测溢出才问一句 */
  const gatePage = (run: () => void) => {
    const by = overflowBy();
    const target = resume.targetPages;
    if (by === null || !target) {
      run();
      return;
    }
    setPendingExport({
      run,
      title: target > 1 ? `这一页还没排进 ${target} 页` : "这一页还没排进 1 页",
      description: `实测 ${by.toFixed(2)} 页，多出来的部分会排到第二页。`,
    });
  };

  /**
   * 整份导出的门：离屏宿主只渲染当前页，所以这里最多列出一条。
   * 只要有一条溢出就说「还有 N 页」—— 整份导出会把每一页都写出去。
   */
  const gateAll = (run: () => void) => {
    const by = overflowBy();
    const target = resume.targetPages;
    if (by === null || !target) {
      run();
      return;
    }
    const rows = [
      `第 ${pageIndex(resume, page.id) + 1} 页 实测 ${by.toFixed(2)} 页 / 目标 ${target} 页`,
    ];
    setPendingExport({
      run,
      title: `还有 ${rows.length} 页没排进目标页数`,
      description: `${rows.join("；")}；导出的文件里，多出来的部分会排到下一页。`,
    });
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

  /**
   * 用户点「+」新建空白页：页名当场去重。
   *
   * 连点两次都叫「未命名页」时，页签栏上两页长得一模一样，改名前只能靠位置猜；
   * agent 建页仍然留空 —— 起名是用户的活，不该由程序替他决定这一页叫什么。
   */
  const addBlankPage = () => {
    wantNewPage.current = true;
    dispatch({
      type: "addPage",
      personId: person.id,
      resumeId: resume.id,
      label: nextPageLabel(resume.pages),
    });
    toast("已新增一页（空白，什么也没预置）");
  };

  /**
   * 上传一整页原件。
   *
   * 渲染层对 file 页只会画一张 <img>（Rust 的 render_file_page），所以这里只收图片：
   * 把 PDF 塞进去只会得到一张裂图，比当场说「不行」更让人摸不着头脑。
   */
  const addFilePage = async (file: File) => {
    if (!isImageFile(file)) {
      toast(
        "PDF / Word 不能直接作为页 —— 先导出成图片，或者把它当附件发给会话，让 agent 读文字",
      );
      return;
    }
    try {
      const asset = await saveAsset(file);
      const label = nextPageLabel(resume.pages, filenameBase(file.name));
      // 先入材料池再建页：页上的 assetId 得指得到东西，顺序反了会有一帧裂图
      dispatch({ type: "addAssets", personId: person.id, assets: [asset] });
      wantNewPage.current = true;
      dispatch({
        type: "addPage",
        personId: person.id,
        resumeId: resume.id,
        kind: "file",
        assetId: asset.id,
        label,
      });
      toast(`已把「${file.name}」作为第 ${resume.pages.length + 1} 页`);
    } catch (error) {
      toast(error instanceof Error ? error.message : String(error));
    }
  };

  const menuPage = menu ? pageMap.get(menu.pageId) ?? null : null;
  const pendingPage = pendingDelete ? pageMap.get(pendingDelete) ?? null : null;
  const cutBlock = pendingCut ? (page.blocks ?? []).find((item) => item.id === pendingCut.blockId) ?? null : null;

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
          {/* 建页有两种：自己写一页，或者把扫好的整页原件放进来当一页。
              以前点「+」只给空白页，想放原件得先去材料区绕一圈 */}
          <Dropdown
            width={188}
            trigger={
              <button type="button" ref={addRef} className={styles.tabAdd} aria-label="新增页面">
                <Plus size={12} />
              </button>
            }
          >
            <MenuItem label="空白页" hint="自己写" onSelect={addBlankPage} />
            <MenuItem
              label="添加文件"
              hint="整页原件"
              onSelect={() => fileRef.current?.click()}
            />
          </Dropdown>
          {/* 文件选择框放在 Dropdown 外面：菜单一点就收起，input 在里面会跟着卸载 */}
          <input
            ref={fileRef}
            type="file"
            accept="image/*"
            hidden
            onChange={(event) => {
              const file = event.target.files?.[0];
              // 清空 value：同一个文件再选一次也要能触发 change
              event.target.value = "";
              if (file) void addFilePage(file);
            }}
          />

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
          <MenuItem
            label="PDF · 当前页"
            hint=".pdf"
            onSelect={() => gatePage(() => exportPdf(page))}
          />
          <MenuItem label="打印…" hint="系统对话框" onSelect={() => gatePage(handlePrint)} />
          <MenuSep />
          <MenuItem
            label="Word · 当前页"
            hint=".docx"
            onSelect={() => gatePage(() => exportDocx(page))}
          />
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
            onSelect={() => gateAll(exportFolder)}
          />
        </Dropdown>
      </div>

      {preview ? (
        <div
          className={styles.canvas}
          ref={canvasRef}
          data-print="flow"
          // data-pannable 由 syncPannable 直接写（量到才挂），这里只管拖拽态
          data-panning={panning ? "1" : undefined}
          onPointerDown={panStart}
          onPointerMove={panMove}
          onPointerUp={panEnd}
          onPointerCancel={panEnd}
          onLostPointerCapture={panEnd}
        >
          {/* 真分页打出来的纸已经自带 16mm 页边距，@page 再给一次就等于打了两次边距
              （内容会被整体缩小一圈）。@page 没法按选择器分支，只能换一条规则覆盖 ——
              后注入的同名规则生效。 */}
          {paged && (
            <style>{`@media print { @page { size: ${A4_W} ${A4_H}; margin: 0; } }`}</style>
          )}
          <div className={paperStyles.wrap} data-print="page" style={{ zoom: zoom / 100 }}>
            {/* 真分页的结果：几张真 A4 竖着排。这里是 cloneNode 的副本，
                事件不会跟过来，所以一个交互也不许挂（点击、Hover 都没有意义）。 */}
            <div
              ref={pagedRef}
              className={paperStyles.paged}
              data-paged
              // 用 style 而不是 hidden 属性：`.paged { display: flex }` 会盖掉
              // [hidden] 的 UA 规则（那是 display:none !important 之外的一条普通规则）
              style={{ display: paged ? undefined : "none" }}
            />
            {/* 回退档：单张纸 + A4 虚线，只在分页不可用时露面。
                它必须一直挂着 —— 分页的内容来源就是这里。 */}
            <div ref={flatRef} style={{ display: paged ? "none" : undefined }}>
              <Paper person={person} page={page} />
              {/* 虚线本身就在被缩放的坐标系里，top 直接用未缩放的 px：
                  再乘一次 zoom 会缩两遍（47% 时会画到半页高的地方）。 */}
              {!paged && pages !== null && pages > TOLERANCE_MAX && (
                <div
                  className={paperStyles.break}
                  style={{ top: pageMeasure?.onePage ?? 0 }}
                  aria-hidden
                >
                  <span>A4</span>
                </div>
              )}
            </div>
          </div>
        </div>
      ) : (
        <ResumeForm
          person={person}
          page={page}
          onHeader={(header) => dispatch({ type: "pageHeader", ...base, header })}
          onLabel={(label) => dispatch({ type: "pageLabel", ...base, label })}
          onAddSection={(title) => dispatch({ type: "addSection", ...base, title })}
          onRemoveSection={(sectionId) => setPendingCut({ kind: "section", blockId: sectionId })}
          onRemoveBlock={(blockId) => setPendingCut({ kind: "block", blockId })}
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

      {/* 只留缩放与纸张数：页码、字数、保存状态都在窗口底部的统一状态栏里 */}
      {preview && (
        <div className={styles.foot} data-print="hide">
          {/* 实际纸张数：分页成功才有。
              它说的是「这一页排出来占了几张 A4」，和状态栏那个「第 N 页 / 共 M 页」
              （简历的段数）不是一回事 —— 一页切成两张纸时，后者仍然写着「共 1 页」，
              用户就会以为分页没生效。放在底栏而不是浮在纸上：纸面右上角常常是
              证件照，压上去就是「挡内容」。 */}
          {paged && sheets > 0 && (
            <div className={styles.sheets} aria-live="polite">
              共 {sheets} 张纸
            </div>
          )}
          <div className={styles.zoomCtl}>
            <button
              type="button"
              aria-label="缩小"
              onClick={() => zoomManual(zoomRef.current - 10)}
            >
              −
            </button>
            {/* 百分比本身就是菜单入口：点它既能看到「现在多少」，也能直接跳到固定档 */}
            <Dropdown
              width={132}
              trigger={
                <Tooltip label="缩放">
                  <button type="button" className={styles.zoomVal}>
                    {zoom}%
                  </button>
                </Tooltip>
              }
            >
              <MenuItem label="适应宽度" onSelect={zoomWidth} />
              <MenuSep />
              {ZOOM_PRESETS.map((preset) => (
                <MenuItem
                  key={preset}
                  label={`${preset}%`}
                  hint={preset === zoom ? "当前" : undefined}
                  onSelect={() => zoomManual(preset)}
                />
              ))}
            </Dropdown>
            <button
              type="button"
              aria-label="放大"
              onClick={() => zoomManual(zoomRef.current + 10)}
            >
              +
            </button>
          </div>
        </div>
      )}

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

      {/* 导出门：溢出 + 已设目标页数时先问一句。不硬拦 —— 用户可能就是要一份
          两页的稿子；拦死等于替他决定「必须塞进一页」。 */}
      {pendingExport && (
        <Confirm
          open
          danger={false}
          title={pendingExport.title}
          description={pendingExport.description}
          confirmLabel="仍然导出"
          cancelLabel="取消"
          onConfirm={() => {
            const job = pendingExport;
            setPendingExport(null);
            job.run();
          }}
          onCancel={() => setPendingExport(null)}
        />
      )}

      {pendingPage && (
        <Confirm
          open
          title={`删除「${pendingPage.label || "未命名页"}」？`}
          description="这一页的内容会被移除，无法撤销。如果只是想让它从页签栏消失，用「关闭标签」就好。"
          onCancel={() => setPendingDelete(null)}
          onConfirm={() => handleDelete(pendingPage)}
        />
      )}

      {/* 删分区 / 删块：和 agent 那条通道同一个确认窗、同一套措辞，
          区别只是这里的标题指得出具体是哪一块 */}
      {pendingCut && (
        <Confirm
          open
          title={
            pendingCut.kind === "section"
              ? `删除「${cutBlock?.title?.trim() || "未命名分区"}」这个分区？`
              : "删除这一块？"
          }
          description={
            pendingCut.kind === "section"
              ? `分区里的 ${cutBlock?.items?.length ?? 0} 条内容会一起删掉，无法撤销。`
              : cutBlock?.kind === "media"
                ? "只从这一页移走，图片还在材料里，随时能再插回来。"
                : "这一块的内容会被移除，无法撤销。"
          }
          onCancel={() => setPendingCut(null)}
          onConfirm={() => {
            if (pendingCut.kind === "section") {
              dispatch({ type: "removeSection", ...base, sectionId: pendingCut.blockId });
              toast(`已删除「${cutBlock?.title?.trim() || "未命名分区"}」`);
            } else {
              dispatch({ type: "removeBlock", ...base, blockId: pendingCut.blockId });
            }
            setPendingCut(null);
          }}
        />
      )}
    </section>
  );
}

