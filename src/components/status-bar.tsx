import { useEffect, useState } from "react";
import { useWorkspace } from "@/store/workspace";
import { countChars, pageIndex } from "@/lib/doc";
import { measureTip, TOLERANCE_MAX } from "@/lib/measure";
import styles from "./status-bar.module.css";

/**
 * 秒表文案：一分钟以内 `42s`，过了就换成 `1:05`。
 * 从 `since` 现算而不是累加，标签页被浏览器节流后回到前台也不会落后。
 */
function elapsedText(since: number): string {
  const secs = Math.max(0, Math.floor((Date.now() - since) / 1000));
  if (secs < 60) return `${secs}s`;
  return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
}

/**
 * 窗口底部状态栏（唯一一条，贯通整个宽度）。
 *
 * 之前三栏各有一条底栏：侧栏写「本地工作区 · 自动保存」、会话写键盘提示、
 * 简历写页码与字数。三条各自带上边框，看起来像被切成三段 —— 而这三件事本来
 * 就是同一个窗口的状态。收成一条之后：左边是工作区、中间是操作提示、右边是
 * 当前页与保存状态，边框只有一条、贯通到底。
 *
 * 布局用 grid 而不是 flex：中间那段要真正居中（相对整个窗口），
 * 用 flex + auto 边距会因为左右两段宽度不同而偏。
 */
export function StatusBar() {
  const { person, resume, page, saveState, layoutStatus, measure } = useWorkspace();

  /**
   * 秒表的节拍器：一秒一次，只推动本组件重渲染，时间本身不落进 store。
   *
   * 依赖只看「当前显示的排版页」：文案每变一次对象就换，拿对象当依赖会把
   * interval 拆了重建；别页在跑而这里根本不显示时也不该空转。
   * 没有排版可显示时直接 return，卸载时 clearInterval + alive 双保险。
   */
  const busyPageId = layoutStatus && page && layoutStatus.pageId === page.id ? page.id : null;
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!busyPageId) return;
    let alive = true;
    const timer = window.setInterval(() => {
      if (alive) setTick((n) => n + 1);
    }, 1000);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [busyPageId]);

  const index = resume ? pageIndex(resume, page?.id ?? null) : -1;
  const chars = person ? countChars(person, page) : 0;

  // 只有真量到才算得出页数：拿 unknown 当 1 页会让用户以为「已经一页了」
  const pages =
    measure?.status === "ok" && measure.onePage && measure.contentPx
      ? measure.contentPx / measure.onePage
      : null;
  const pagesOk = pages !== null && pages <= TOLERANCE_MAX;
  const pagesText =
    pages === null
      ? ""
      : pages <= 1
        ? "1.00 页内"
        : pages <= TOLERANCE_MAX
          ? `${pages.toFixed(2)} 页`
          : `${pages.toFixed(2)} 页 · 超 ${(pages - 1).toFixed(2)}`;

  return (
    <footer className={styles.bar} data-print="hide">
      <span className={styles.left}>
        <i className={styles.live} />
        本地工作区 · 自动保存
      </span>

      <span className={styles.mid}>
        <kbd>Enter</kbd> 发送 · <kbd>Shift</kbd>
        <kbd>Enter</kbd> 换行
      </span>

      <span className={styles.right}>
        {resume && page ? (
          <>
            {/* 排版 agent 在干活：只看当前页的，别的页在忙不该在这显示 */}
            {layoutStatus?.pageId === page.id && (
              <span className={styles.busy}>
                <i className={styles.dotBusy} />
                <span className={styles.busyText}>{layoutStatus.text}</span>
                <span className={styles.tick}>{elapsedText(layoutStatus.since)}</span>
                {layoutStatus.step > 0 && (
                  <span className={styles.steps}>· 第 {layoutStatus.step} 步</span>
                )}
              </span>
            )}
            <span>
              第 {index + 1} 页 / 共 {resume.pages.length} 页
            </span>
            {/* 实测徽标：口径文案全在 measureTip 里，这里只负责画点和数字 */}
            {measure && (
              <span className={styles.measure} title={measureTip(measure, resume.targetPages)}>
                {pages !== null ? (
                  <>
                    <i className={pagesOk ? styles.dotIdle : styles.dotWarn} />
                    {pagesText}
                  </>
                ) : (
                  <>
                    <i className={styles.dotIdleDash} />
                    页数未知
                  </>
                )}
              </span>
            )}
            <span>{chars.toLocaleString("zh-CN")} 字</span>
            <span className={styles.save}>
              <i className={saveState === "saving" ? styles.dotBusy : styles.dotIdle} />
              {saveState === "saving" ? "保存中…" : "已保存"}
            </span>
          </>
        ) : (
          <span className={styles.idle}>还没有打开简历</span>
        )}
      </span>
    </footer>
  );
}
