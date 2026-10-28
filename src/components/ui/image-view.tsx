import {
  useEffect,
  useRef,
  useState,
  type MouseEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { Modal } from "./modal";
import styles from "./image-view.module.css";

/**
 * 大图查看。
 *
 * 复用 `ui/modal` 的外壳：遮罩、ESC、焦点圈闭、滚轮拦截都在那儿，图片窗不该各写一份。
 * 只传 `onClose` 不传 `onCancel` —— 看图没有「取消」这一说，X 与取消按钮互斥是仓库规则。
 * 点图片本身也关闭：看图时手的第一反应是点图/点空白，不是找右上角。
 */
export function ImageView({
  title,
  src,
  onClose,
}: {
  title: string;
  src: string;
  onClose: () => void;
}) {
  return (
    <Modal title={title} onClose={onClose}>
      <button
        type="button"
        className={styles.stage}
        onClick={onClose}
        aria-label={`关闭「${title}」`}
        // 宽度规则挂在「装了本组件的 dialog」上，用它当锚点，见 module.css
        data-image-view
      >
        <img className={styles.image} src={src} alt={title} draggable={false} />
      </button>
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
        <ImageView title={title} src={src} onClose={() => setZoom(false)} />
      ) : null}
    </>
  );
}
