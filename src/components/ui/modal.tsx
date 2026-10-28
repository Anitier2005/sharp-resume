import {
  useEffect,
  useRef,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { Close } from "./icons";
import { useVeilWheelLock } from "./veil";
import { cx } from "@/lib/cx";
import shared from "./shared.module.css";
import styles from "./modal.module.css";

/**
 * 统一模态外壳（小窗档）。
 *
 * 抽出来不是为了少写几行，而是把四件容易各写各的事收在一处：
 *
 *   1. **令牌作用域** —— 必须 portal 进 `.win`。`--bg-elev` / `--danger` 都定义在那儿，
 *      挂在 provider（`.win` 外层）时 `var()` 全部解析成空串，卡片透明、按钮没底，
 *      用户会看到「弹了个透明框、找不到按钮」。这个坑踩过一次，别再踩第二次。
 *   2. **关闭路径** —— 取消按钮和右上角 X 是**互斥**的两个出口。同屏同时出现，
 *      用户就得判断「点哪个才是否定」，这是设计出来的歧义。
 *   3. **ESC** —— 按一次只关一层，且不泄漏到下面正在编辑的纸面。
 *   4. **焦点与滚动** —— Tab 不许跑到纸上（回车就改到了没看见的东西），
 *      滚轮不许带动底下的简历。
 *
 * 尺寸只此一档 420。大面板（设置 560、资料 764×668）不走这里：它们的宽度由
 * 内容决定，硬塞进同一号会没法用 —— 分档统一，不是全应用一个尺寸。
 */
export interface ModalProps {
  title: string;
  /** 头部第二行小字：一句说明它为什么出现 */
  subtitle?: ReactNode;
  /**
   * 「取消」。给了就渲染取消按钮，且 ESC 走它。
   * 与 `onClose` 同时给时，以 `onCancel` 为准，X 不渲染。
   */
  onCancel?: () => void;
  /** 只有「知道了」这类单向窗才用它 —— 那类窗没有取消，需要一个纯关闭出口 */
  onClose?: () => void;
  /** 底部按钮区。不传则整条底栏不渲染 */
  footer?: ReactNode;
  /**
   * 按钮区不带分界：确认类小窗用它 —— 一条横线加一块深色底会把 420 的小窗
   * 切成「正文 + 工具条」两截，而这窗里只有一句话和两个按钮。
   */
  footerBare?: boolean;
  /** 中间段的额外类：调用方自己决定内部是 flex 列表还是普通段落 */
  bodyClassName?: string;
  children?: ReactNode;
}

/** 可聚焦元素。不用 `:focusable`（不存在），逐个列出来更可靠 */
const FOCUSABLE = [
  "button:not([disabled])",
  "input:not([disabled])",
  "textarea:not([disabled])",
  "select:not([disabled])",
  "a[href]",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

/**
 * 遮罩上的滚轮拦截见 `./veil`。放在那个文件里，是因为大面板（设置 560、
 * 资料 764）不走本组件，但这条行为必须一致。
 */

export function Modal({
  title,
  subtitle,
  onCancel,
  onClose,
  footer,
  footerBare,
  bodyClassName,
  children,
}: ModalProps) {
  const veilRef = useRef<HTMLDivElement | null>(null);

  // 有取消走取消，否则走关闭 —— 这一句就是「两个出口互斥」的全部实现
  const dismiss = onCancel ?? onClose;
  const dismissRef = useRef(dismiss);
  /**
   * 回调存 ref，不直接进依赖：调用方每次渲染都传新函数，直接依赖会让 ESC 监听
   * 反复解绑重挂，打字中途按 ESC 有可能正好落在重挂的空档里。
   *
   * 在 effect 里同步而不是渲染期赋值 —— 渲染期写 ref 是副作用，
   * 并发渲染下可能把「没提交的那一版」写进去。
   */
  useEffect(() => {
    dismissRef.current = dismiss;
  }, [dismiss]);

  /**
   * ESC。用 ref 存回调：调用方每次渲染都传新函数，直接进依赖会让监听反复解绑重挂，
   * 打字中途按 ESC 有可能正好落在重挂的空档里。
   * 用捕获阶段 + stopPropagation：这一层开着的时候，ESC 不该再触发下面编辑器的退出。
   */
  useEffect(() => {
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      dismissRef.current?.();
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, []);

  /**
   * 滚轮要拦住。见 `useVeilWheelLock` 的注释 —— 大面板也用它，行为统一。
   */
  useVeilWheelLock(veilRef);

  /**
   * 打开时把焦点送进来。
   *
   * 优先 `[data-autofocus]`（设置面板给输入框，确认窗给「取消」—— 危险操作默认
   * 停在安全的那一侧），都没有就取第一个可聚焦元素，免得焦点留在下面的纸面上。
   */
  useEffect(() => {
    const veil = veilRef.current;
    if (!veil) return;
    const target = veil.querySelector<HTMLElement>("[data-autofocus]") ?? veil.querySelector<HTMLElement>(FOCUSABLE);
    target?.focus();
  }, []);

  /** Tab 关在窗里。不拦的话 Tab 会把焦点带到纸上，用户回车就改到了没看见的东西 */
  const keepFocus = (event: ReactKeyboardEvent) => {
    if (event.key !== "Tab") return;
    const nodes = veilRef.current?.querySelectorAll<HTMLElement>(FOCUSABLE);
    if (!nodes?.length) return;
    const first = nodes[0];
    const last = nodes[nodes.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  // 只在「没有取消按钮」时补 X。两个出口不能同时出现在一个窗里
  const showX = !onCancel && Boolean(onClose);

  const dialog = (
    <div
      ref={veilRef}
      className={cx(shared.veil, styles.veil)}
      data-print="hide"
      onKeyDown={keepFocus}
    >
      <div className={styles.card} role="dialog" aria-modal="true" aria-label={title} data-selectable>
        <header className={styles.head}>
          <div className={shared.overlayTitle}>
            <b>{title}</b>
            {subtitle ? <span>{subtitle}</span> : null}
          </div>
          {showX && (
            <button type="button" className={shared.overlayClose} aria-label="关闭" onClick={onClose}>
              <Close size={11} />
            </button>
          )}
        </header>

        {children ? (
          <div className={cx(styles.body, bodyClassName)} data-scroll>
            {children}
          </div>
        ) : null}

        {footer ? (
          <footer className={cx(styles.foot, footerBare && styles.footBare)}>{footer}</footer>
        ) : null}
      </div>
    </div>
  );

  /**
   * 挂到 `.win` 里面，不是 body —— 令牌跟着 DOM 走。
   * 只在挂载时取一次：Modal 本身是条件渲染的（open 才挂），此刻 `.win` 一定在。
   */
  const host = typeof document === "undefined" ? null : document.querySelector<HTMLElement>(".win");
  return host ? createPortal(dialog, host) : dialog;
}
