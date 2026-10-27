import type { Person, ResumePage } from "@/types";
import { useEffect, useState } from "react";
import { compilePage, resolveAssetRefs } from "@/lib/compile";
import styles from "./paper.module.css";

/**
 * 纸面：只负责「装上成品」。
 *
 * 这里不再有任何 `if (块类型)` 的分支 —— 版面由 Rust 编译层产出
 * （`src-tauri/src/render`）。以前这里是 React 手写的一棵渲染树，
 * 而 Word 导出是另一套硬编码规则，两边各排各的；现在只有一个真相。
 *
 * 与「打印 / 存为 PDF」共用同一份 DOM：`window.print()` 打的就是这个节点。
 */
interface PaperProps {
  person: Person;
  page: ResumePage;
}

interface Loaded {
  html: string;
  css: string;
  mode: "structured" | "manual";
}

export function Paper({ person, page }: PaperProps) {
  const [doc, setDoc] = useState<Loaded | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    // 编译要走一次 IPC：打字时每个键都发一次太浪费，等停下来再算
    const timer = window.setTimeout(() => {
      void (async () => {
        try {
          const compiled = await compilePage(page, person);
          if (!alive || !compiled) return;
          const html = await resolveAssetRefs(compiled.html, person);
          if (!alive) return;
          setDoc({ html, css: compiled.css, mode: compiled.mode });
          setError(null);
        } catch (cause) {
          // 编译失败绝不能静默变成一片空白：用户会以为是自己的内容没了
          if (!alive) return;
          setError(cause instanceof Error ? cause.message : String(cause));
        }
      })();
    }, 120);
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [page, person]);

  return (
    <article className={styles.paper} data-print="sheet" data-selectable data-mode={doc?.mode}>
      {error ? (
        <div className={styles.failed}>这一页没能编译出来：{error}</div>
      ) : doc ? (
        <>
          {/* 预设样式是常量字符串，随第一次编译一起到 */}
          <style>{doc.css}</style>
          <div dangerouslySetInnerHTML={{ __html: doc.html }} />
        </>
      ) : (
        <div className={styles.pending} />
      )}
    </article>
  );
}
