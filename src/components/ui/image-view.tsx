import {
  useEffect,
  useRef,
  useState,
  type MouseEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { Button } from "./button";
import { Modal } from "./modal";
import { Dropdown, MenuItem } from "./menu";
import { useToast } from "./toast-context";
import { Download } from "./icons";
import { cx } from "@/lib/cx";
import styles from "./image-view.module.css";

/** 缩放档位：低于 50% 的图已经看不清内容，再小只是让自己找不到图 */
const MIN_SCALE = 0.5;
const MAX_SCALE = 4;
/** 按钮与滚轮各用一档步长：按钮要「按一下有明显变化」，滚轮要「连续可微调」 */
const STEP = 1.25;
const WHEEL_STEP = 1.1;

const clamp = (value: number, min: number, max: number) =>
  Math.min(max, Math.max(min, value));

type Format = "png" | "jpeg" | "webp";

interface Spec {
  type: string;
  /** 落盘用的扩展名与菜单上显示的名字分开写：`jpeg` 的扩展名惯例是 `.jpg` */
  ext: string;
  label: string;
  hint: string;
}

const FORMATS: Record<Format, Spec> = {
  png: { type: "image/png", ext: "png", label: "PNG", hint: "无损" },
  jpeg: { type: "image/jpeg", ext: "jpg", label: "JPEG", hint: "白底" },
  webp: { type: "image/webp", ext: "webp", label: "WebP", hint: "体积小" },
};

/** `图.png` + `jpg` → `图.jpg`。没有扩展名（或者点号在开头）就整名当中干 */
function renameTo(name: string, ext: string) {
  const dot = name.lastIndexOf(".");
  const stem = dot > 0 ? name.slice(0, dot) : name;
  return `${stem}.${ext}`;
}

/**
 * 把已显示的这张图画进 canvas 再编码。
 *
 * 用画面上这张 `<img>` 而不是重新 `new Image()`：它已经加载完，不必再等一次网络；
 * 代价是如果图片来自跨源地址，canvas 会被污染、`toBlob` 抛 SecurityError ——
 * 这条由调用方捕获后 toast，不能静默失败。
 */
function encode(img: HTMLImageElement, spec: Spec): Promise<Blob> {
  const canvas = document.createElement("canvas");
  canvas.width = img.naturalWidth;
  canvas.height = img.naturalHeight;
  const ctx = canvas.getContext("2d");
  if (!ctx) return Promise.reject(new Error("no 2d context"));

  if (spec.type === "image/jpeg") {
    // JPEG 没有 alpha 通道，不铺底时透明区会被编码成纯黑，
    // 截图带圆角的地方就会出现一圈黑角。这里必须是不透明白，不能用 --paper：
    // 深色模式下那个令牌不是白，导出的文件也就不该跟着界面走。
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
  }
  ctx.drawImage(img, 0, 0);

  return new Promise<Blob>((resolve, reject) => {
    canvas.toBlob(
      (blob) => (blob ? resolve(blob) : reject(new Error("encode failed"))),
      spec.type,
      0.92,
    );
  });
}

/**
 * 大图查看器：缩放、平移、格式转换下载。
 *
 * 复用 `ui/modal` 的外壳：遮罩、ESC、焦点圈闭、滚轮拦截都在那儿，图片窗不该各写一份。
 * 只传 `onClose` 不传 `onCancel` —— 看图没有「取消」这一说，X 与取消按钮互斥是仓库规则。
 *
 * 与旧版的关键差别：**点图片不再关闭**。看图时手的第一反应确实常是点图，但这里点下去
 * 是「拖动」，同一个手势不能既是平移又是退出，否则一拖就把窗拖没了。退出只留 X 和 ESC。
 */
export function ImageView({
  src,
  name,
  onClose,
}: {
  src: string;
  name: string;
  onClose: () => void;
}) {
  const toast = useToast();
  const stageRef = useRef<HTMLDivElement | null>(null);
  const imgRef = useRef<HTMLImageElement | null>(null);

  const [natural, setNatural] = useState<{ w: number; h: number } | null>(null);
  const [failed, setFailed] = useState(false);
  /** 画布可用尺寸：适应窗口要按它算，改窗口/改字号都得跟着变 */
  const [box, setBox] = useState({ w: 0, h: 0 });

  /** fit 是一个「模式」而不是一个数：窗口变化时它要一直重算，用户手动缩放后就不该再动 */
  const [mode, setMode] = useState<"fit" | "manual">("fit");
  const [manual, setManual] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    const el = stageRef.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => {
      // contentRect 已经扣掉内边距，正是图片可用区域
      const { width, height } = entry.contentRect;
      setBox((prev) =>
        prev.w === width && prev.h === height ? prev : { w: width, h: height },
      );
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // 小图不放大：「适应窗口」若把 32×32 的图标拉到满屏，看到的是四个色块
  const fit =
    natural && natural.w > 0 && natural.h > 0 && box.w > 0 && box.h > 0
      ? Math.min(box.w / natural.w, box.h / natural.h, 1)
      : 1;
  const scale = mode === "fit" ? fit : manual;

  const scaledW = natural ? natural.w * scale : 0;
  const scaledH = natural ? natural.h * scale : 0;
  // 只有图比画布大才有「拖」的余地，两边都装得下时指针应该是默认箭头
  const maxX = Math.max(0, (scaledW - box.w) / 2);
  const maxY = Math.max(0, (scaledH - box.h) / 2);
  const canPan = maxX > 0 || maxY > 0;

  const resetFit = () => {
    setMode("fit");
    setOffset({ x: 0, y: 0 });
  };

  /** 位移上限就是「图比画布多出来的那一半」—— 再多就拖出空白了 */
  const clampOffset = (x: number, y: number, at: number) => {
    const mx = Math.max(0, (natural ? natural.w * at - box.w : 0) / 2);
    const my = Math.max(0, (natural ? natural.h * at - box.h : 0) / 2);
    return { x: clamp(x, -mx, mx), y: clamp(y, -my, my) };
  };

  /**
   * 落到一个具体比例。
   *
   * 缩放后原来贴边的位移可能已经越界（缩小时图比画布还小，位移必须归零），
   * 这里顺手按新比例夹一次，否则松手后图会停在视野外的空白里。
   */
  const apply = (next: number) => {
    setMode("manual");
    setManual(next);
    setOffset((prev) => clampOffset(prev.x, prev.y, next));
  };

  const zoomBy = (factor: number) => {
    // 从「适应窗口」按第一下时，以当前看到的比例当基准，图不会突然跳一档
    const base = mode === "fit" ? fit : manual;
    // 下界让到「适应比例」：一张 8000px 长图在窗口里可能只有 18%，
    // 若按 50% 兜底，按「−」反而会放大一截，方向都反了
    apply(clamp(base * factor, Math.min(MIN_SCALE, fit), MAX_SCALE));
  };

  /**
   * 滚轮监听挂在原生事件上：React 给 wheel 挂的是 passive，preventDefault 无效。
   * 模态里滚动没有别的含义（遮罩的滚动锁也放行了 `[data-scroll]`），
   * 所以这里按住滚轮就缩放，并且必须 preventDefault —— 否则弹窗中间段会跟着滚。
   */
  const zoomRef = useRef(zoomBy);
  useEffect(() => {
    zoomRef.current = zoomBy;
  });
  useEffect(() => {
    const el = stageRef.current;
    if (!el) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      // 触摸板横向滑动的 deltaY 是 0，按式子会算成「缩小」；那不是一个缩放意图
      if (event.deltaY === 0) return;
      zoomRef.current(event.deltaY < 0 ? WHEEL_STEP : 1 / WHEEL_STEP);
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  // 拖动起点记在 ref 里：pointermove 每次都要拿「按下那一刻」的位移算总账，
  // 用 state 会多一层异步，快速拖动时会出现跟手迟滞
  const drag = useRef<{ id: number; x: number; y: number; ox: number; oy: number } | null>(
    null,
  );

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!canPan || event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = {
      id: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      ox: offset.x,
      oy: offset.y,
    };
    setDragging(true);
  };

  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    const start = drag.current;
    if (!start || start.id !== event.pointerId) return;
    setOffset({
      x: clamp(start.ox + (event.clientX - start.x), -maxX, maxX),
      y: clamp(start.oy + (event.clientY - start.y), -maxY, maxY),
    });
  };

  const endDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (drag.current?.id !== event.pointerId) return;
    drag.current = null;
    setDragging(false);
  };

  const toggleFit = () => {
    if (mode === "fit") apply(1);
    else resetFit();
  };

  const save = async (kind: Format) => {
    const img = imgRef.current;
    const spec = FORMATS[kind];
    if (!img || !natural) {
      toast("图还没读完，稍等一下再存");
      return;
    }
    try {
      const blob = await encode(img, spec);
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = renameTo(name, spec.ext);
      link.click();
      // 立刻 revoke 会把还在读 blob 的下载掐断，等这一轮事件循环过去再清
      window.setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch {
      // 跨源图片会污染 canvas，编码直接抛错；说清楚是哪一步失败，别让按钮看起来坏了
      toast("这张图转不了格式，可能是地址不允许读取");
    }
  };

  return (
    <Modal title={name} onClose={onClose} bodyClassName={styles.body}>
      <div className={styles.viewer} data-image-view data-print="hide">
        <div
          ref={stageRef}
          className={cx(styles.stage, canPan && styles.pannable, dragging && styles.dragging)}
          // Modal 打开时会把焦点送给第一个可聚焦元素，那样就是「缩小」按钮：
          // 上一处交互残留的一次回车就会把图缩掉。让焦点落在画布上，按坏也没有副作用。
          data-autofocus
          tabIndex={-1}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={endDrag}
          onPointerCancel={endDrag}
          onLostPointerCapture={endDrag}
          onDoubleClick={toggleFit}
        >
          {failed ? (
            <p className={styles.hint}>这张图读不出来</p>
          ) : (
            <>
              {natural ? null : <p className={styles.hint}>正在读取…</p>}
              <img
                ref={imgRef}
                className={styles.image}
                src={src}
                alt={name}
                draggable={false}
                style={{
                  transform: `translate(-50%, -50%) translate(${offset.x}px, ${offset.y}px) scale(${scale})`,
                  // 尺寸未知时先藏着：按 100% 顶出来一帧再缩回去，比等一会儿更晃眼
                  visibility: natural ? "visible" : "hidden",
                }}
                onLoad={(event) =>
                  setNatural({
                    w: event.currentTarget.naturalWidth,
                    h: event.currentTarget.naturalHeight,
                  })
                }
                onError={() => setFailed(true)}
              />
            </>
          )}
        </div>

        <div className={styles.bar}>
          <span className={styles.info}>
            {natural
              ? `${natural.w} × ${natural.h}`
              : failed
                ? "—"
                : "读取中"}
          </span>

          <div className={styles.zoom}>
            <Button
              variant="quiet"
              size="sm"
              icon
              disabled={!natural}
              aria-label="缩小"
              onClick={() => zoomBy(1 / STEP)}
            >
              −
            </Button>
            <Button
              variant="quiet"
              size="sm"
              disabled={!natural}
              aria-label="适应窗口"
              onClick={resetFit}
            >
              {Math.round(scale * 100)}%
            </Button>
            <Button
              variant="quiet"
              size="sm"
              icon
              disabled={!natural}
              aria-label="放大"
              onClick={() => zoomBy(STEP)}
            >
              +
            </Button>
          </div>

          <div className={styles.save}>
            <Dropdown
              align="end"
              trigger={
                <Button variant="ghost" size="sm" disabled={!natural} leading={<Download size={11} />}>
                  下载
                </Button>
              }
            >
              {(Object.keys(FORMATS) as Format[]).map((kind) => (
                <MenuItem
                  key={kind}
                  label={FORMATS[kind].label}
                  hint={FORMATS[kind].hint}
                  onSelect={() => void save(kind)}
                />
              ))}
            </Dropdown>
          </div>
        </div>
      </div>
    </Modal>
  );
}

/** 浮层最大尺寸，和 module.css 里的 220×160 必须是同一组数 */
const PEEK_W = 220;
const PEEK_H = 160;
/** 标签与浮层之间的缝：贴太紧会盖住标签自己的描边 */
const PEEK_GAP = 6;
/** 水平夹取时给视口留的边 */
const PEEK_EDGE = 8;
/** 离开后的关闭延迟：快速从标签上划过不该闪一下 */
const HIDE_DELAY = 120;

interface Spot {
  left: number;
  top?: number;
  bottom?: number;
}

export interface ImagePeekProps {
  /** 标签自己的样式（输入框标记 / 消息里的行内附件） */
  className?: string;
  /** 文件名，同时作为浮层 alt 与大图标题 */
  title: string;
  /** 首次悬停/点击才解析：历史消息拿不到图时就不该有浮层 */
  resolve: () => string | null | Promise<string | null>;
  children: ReactNode;
}

function place(el: HTMLElement): Spot {
  const rect = el.getBoundingClientRect();
  const center = rect.left + rect.width / 2;
  const half = PEEK_W / 2 + PEEK_EDGE;
  const left = Math.min(Math.max(center, half), window.innerWidth - half);
  // 上方放不下就翻到标签下面：会话流顶端的附件贴着窗口上沿，浮层会被顶出去
  const above = rect.top > PEEK_H + PEEK_GAP + PEEK_EDGE;
  return above
    ? { left, bottom: window.innerHeight - rect.top + PEEK_GAP }
    : { left, top: rect.bottom + PEEK_GAP };
}

/**
 * 带悬停缩略图的图片标签。
 *
 * 输入框的高亮层与消息里的行内附件是两套 DOM，但「悬停看缩略图、点击看大图」
 * 是同一个交互；两处各写一遍必然漂移（延迟、位置、关闭时机都会不一样）。
 *
 * 已解析的地址存在组件内部：调用方换了一份附件时**必须换 key**，
 * 否则会沿用上一张图（列表 key 用下标的场合尤其容易踩）。
 */
export function ImagePeek({ className, title, resolve, children }: ImagePeekProps) {
  const [src, setSrc] = useState<string | null>(null);
  const [spot, setSpot] = useState<Spot | null>(null);
  const [zoom, setZoom] = useState(false);
  const pending = useRef<Promise<string | null> | null>(null);
  const timer = useRef<number | null>(null);
  const hovering = useRef(false);

  useEffect(
    () => () => {
      if (timer.current) window.clearTimeout(timer.current);
    },
    [],
  );

  // 滚动/改窗口尺寸后浮层还钉在原位就成了「悬空的图」，直接收掉
  useEffect(() => {
    if (!spot) return;
    const hide = () => setSpot(null);
    window.addEventListener("scroll", hide, true);
    window.addEventListener("resize", hide);
    return () => {
      window.removeEventListener("scroll", hide, true);
      window.removeEventListener("resize", hide);
    };
  }, [spot]);

  const cancelClose = () => {
    if (timer.current) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
  };

  /** 解析结果按组件实例记；失败（null）不记住，下一轮悬停还能再试一次 */
  const load = (): Promise<string | null> => {
    if (src) return Promise.resolve(src);
    if (!pending.current) {
      pending.current = Promise.resolve(resolve())
        .catch(() => null)
        .then((value) => {
          setSrc(value);
          if (!value) pending.current = null;
          return value;
        });
    }
    return pending.current;
  };

  const enter = async (event: MouseEvent<HTMLSpanElement>) => {
    hovering.current = true;
    cancelClose();
    const el = event.currentTarget;
    const value = await load();
    // 解析是异步的（消息里的图要从材料池读），等回来时鼠标可能已经走了
    if (!value || !hovering.current) return;
    setSpot(place(el));
  };

  const leave = () => {
    hovering.current = false;
    cancelClose();
    // 浮层本身 pointer-events:none，鼠标不可能停上去；延迟只为压住快速划过的抖动
    timer.current = window.setTimeout(() => setSpot(null), HIDE_DELAY);
  };

  const open = async () => {
    const value = src ?? (await load());
    if (!value) return;
    setSpot(null);
    setZoom(true);
  };

  // portal 进 .win：缩略图要用 --bg-elev / --shadow-paper，令牌只在那棵树里
  const host = typeof document === "undefined" ? null : document.querySelector<HTMLElement>(".win");
  const peek =
    spot && src && host
      ? createPortal(
          <div
            className={styles.peek}
            style={spot}
            data-print="hide"
            aria-hidden
          >
            <img src={src} alt="" draggable={false} />
          </div>,
          host,
        )
      : null;

  return (
    <>
      <span
        className={className}
        onMouseEnter={(event) => void enter(event)}
        onMouseLeave={leave}
        onClick={() => void open()}
      >
        {children}
      </span>
      {peek}
      {zoom && src ? (
        <ImageView name={title} src={src} onClose={() => setZoom(false)} />
      ) : null}
    </>
  );
}
