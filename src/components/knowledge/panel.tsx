import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import {
  BookOpen,
  Close,
  Pencil,
  Plus,
  Search,
  Sparkles,
  Trash,
  Upload,
} from "@/components/ui/icons";
import { Confirm } from "@/components/ui/confirm";
import { Modal } from "@/components/ui/modal";
import { useToast } from "@/components/ui/toast-context";
import { useVeilWheelLock } from "@/components/ui/veil";
import {
  configureKnowledge,
  ingestFile,
  knowledgeChunkDelete,
  knowledgeEntries,
  knowledgeEntryDelete,
  knowledgeEntryEnrich,
  knowledgeEntryGet,
  knowledgeEntrySave,
  knowledgeImageAnnotate,
  knowledgeImageUpdate,
  knowledgeImportImage,
  knowledgeImportText,
  knowledgeLibraries,
  knowledgeLibraryDelete,
  knowledgeLibrarySave,
  knowledgeSearch,
  knowledgeVectorize,
  type KnowledgeChunk,
  type KnowledgeDoc,
  type KnowledgeEntry,
  type KnowledgeHit,
  type KnowledgeLibrary,
} from "@/lib/knowledge";
import { findModel } from "@/lib/model-config";
import { aiReady, loadAi, loadAux, slotAi } from "@/lib/ai";
import { extOf, resolveStored, saveAsset } from "@/lib/assets";
import { ImageView } from "@/components/ui/image-view";
import { cx } from "@/lib/cx";
import shared from "@/components/ui/shared.module.css";
import { Button, Field, Input, Textarea, Tooltip } from "@/components/ui";
import styles from "./panel.module.css";

/**
 * 知识库：三层（库 → 条目 → chunk）。
 *
 * 为什么是三列：这三层是**归属关系**而不是「列表 + 详情」——
 * 库决定条目范围，条目决定看哪些块。三列并排时，切换库、切换条目、
 * 看块内容都在原位完成，不用来回返回。
 *
 * 两列之间可以拖动。宽度存 localStorage：这是用户的阅读习惯，
 * 不该每次打开都重来一遍。
 *
 * 模型只读：这里的所有写操作都由用户点击发起，模型侧没有写工具。
 */

const WIDTH_KEY = "sharp-resume:kb:cols:v1";

function readWidths(): [number, number] {
  try {
    const raw = localStorage.getItem(WIDTH_KEY);
    if (raw) {
      const value: unknown = JSON.parse(raw);
      if (Array.isArray(value) && value.length === 2) {
        const [a, b] = value as [number, number];
        if (a >= 120 && b >= 140) return [a, b];
      }
    }
  } catch {
    // 坏数据就回到缺省，不值得为它报错
  }
  return [190, 232];
}

const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), max);

const parseTags = (text: string) =>
  text
    .split(/[、,，\s]+/)
    .map((item) => item.trim())
    .filter(Boolean);

/**
 * 图片说明走哪条链路：能看图的模型 → OCR → 都没有（不写摘要）。
 *
 * 导入与「重写说明」共用这一套判定 —— 各判一套的话，同一个库里会出现
 * 两种口径的描述，用户没法判断哪句更可信。
 */
function imageAnnotator() {
  const chatAi = loadAi();
  const chunkAi = slotAi("chunk");
  const ocrChoice = loadAux().ocr;
  const ocr =
    ocrChoice && ocrChoice.model.trim()
      ? {
          baseUrl: ocrChoice.baseUrl.trim() || chatAi.baseUrl,
          apiKey: ocrChoice.apiKey.trim() || chatAi.apiKey,
          model: ocrChoice.model,
        }
      : null;
  const mode: "model" | "ocr" | "none" = findModel(chunkAi.model)?.vision
    ? "model"
    : ocr
      ? "ocr"
      : "none";
  return { chunkAi, ocr, mode };
}

export function KnowledgePanel({
  open,
  onClose,
  embedded = false,
}: {
  open: boolean;
  onClose: () => void;
  /**
   * 跑在独立窗口里：不要遮罩、不要圆角与外框、不要自己的关闭键 ——
   * 窗口标题栏已经有一个了，两个 X 会让人不知道该点哪个。
   */
  embedded?: boolean;
}) {
  const veilRef = useRef<HTMLDivElement | null>(null);
  const toast = useToast();

  const [libs, setLibs] = useState<KnowledgeLibrary[]>([]);
  const [libId, setLibId] = useState("");
  const [entries, setEntries] = useState<KnowledgeEntry[]>([]);
  const [entryId, setEntryId] = useState("");
  const [doc, setDoc] = useState<KnowledgeDoc | null>(null);

  /** 条目的编辑态（null = 不在编辑）。新建时 id 为空 */
  const [draft, setDraft] = useState<{ id?: string; title: string; tags: string; body: string } | null>(
    null,
  );
  /** 库的新建/重命名小窗 */
  const [libDraft, setLibDraft] = useState<{ id?: string; name: string; note: string } | null>(null);
  /** 待确认的删除：库、条目或某个图片块 */
  const [removing, setRemoving] = useState<{
    kind: "lib" | "entry" | "chunk";
    id: string;
    name: string;
  } | null>(null);

  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<KnowledgeHit[] | null>(null);
  const [focusChunk, setFocusChunk] = useState("");

  const [busy, setBusy] = useState(false);
  const [enriching, setEnriching] = useState(false);
  /** 导入进度文案：非空时按钮显示它 */
  const [importing, setImporting] = useState("");
  const [vectorizing, setVectorizing] = useState(false);
  /** 图片块缩略图地址（按材料池存储名缓存；null = 读不出来） */
  const [thumbs, setThumbs] = useState<Record<string, string | null>>({});
  /** 正在看的大图 */
  const [viewer, setViewer] = useState<{ src: string; name: string } | null>(null);
  /** 正在重写说明的图片块 id（空 = 没有在跑） */
  const [redrawing, setRedrawing] = useState("");
  const [widths, setWidths] = useState<[number, number]>(readWidths);

  const chunkRefs = useRef<Record<string, HTMLElement | null>>({});
  /** 隐藏的文件选择框：导入按钮只负责触发它 */
  const importRef = useRef<HTMLInputElement | null>(null);
  /**
   * 从检索结果跳转时要跨库打开一条条目：切库的那个 effect 会清空右侧，
   * 所以先用这个 ref 打一声招呼「下一条别清」。
   */
  const keepEntry = useRef("");

  const fail = (label: string, error: unknown) =>
    toast(`${label}：${error instanceof Error ? error.message : String(error)}`);

  const loadLibs = useCallback(async () => {
    try {
      const list = await knowledgeLibraries();
      setLibs(list);
      setLibId((current) => {
        if (current && list.some((item) => item.id === current)) return current;
        return list[0]?.id ?? "";
      });
    } catch (error) {
      fail("读不到知识库", error);
    }
  }, [toast]);

  const loadEntries = useCallback(
    async (libraryId: string) => {
      if (!libraryId) {
        setEntries([]);
        return;
      }
      try {
        setEntries(await knowledgeEntries(libraryId));
      } catch (error) {
        fail("读不到条目", error);
      }
    },
    [toast],
  );

  useEffect(() => {
    if (!open) return;
    configureKnowledge();
    void loadLibs();
  }, [open, loadLibs]);

  useEffect(() => {
    if (!open) return;
    void loadEntries(libId);
    // 换库时清空右侧：留着上一条会把「这个库有什么」搞混。
    // 例外是从检索结果跨库跳进来 —— 那时要保留它指的那一条。
    if (keepEntry.current && keepEntry.current === entryId) {
      keepEntry.current = "";
      setDraft(null);
      return;
    }
    setEntryId("");
    setDoc(null);
    setDraft(null);
  }, [open, libId, entryId, loadEntries]);

  useEffect(() => {
    if (!entryId) {
      setDoc(null);
      return;
    }
    let alive = true;
    void knowledgeEntryGet(entryId)
      .then((found) => {
        if (alive) setDoc(found);
      })
      .catch((error) => fail("读不到这一条", error));
    return () => {
      alive = false;
    };
  }, [entryId, toast]);

  useEffect(() => {
    if (!doc) return;
    let alive = true;
    const images = doc.chunks.filter((chunk) => chunk.kind === "image" && chunk.asset);
    void Promise.all(
      images.map(async (chunk) => {
        if (chunk.asset in thumbs) return;
        const url = await resolveStored(chunk.asset);
        if (alive) setThumbs((prev) => ({ ...prev, [chunk.asset]: url }));
      }),
    );
    return () => {
      alive = false;
    };
    // thumbs 只作为「已解析过」的缓存用，变化不该触发重新解析
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [doc]);

  useEffect(() => {
    try {
      localStorage.setItem(WIDTH_KEY, JSON.stringify(widths));
    } catch {
      // 隐私模式写不了：本次会话内仍然生效
    }
  }, [widths]);

  // 命中的块滚到眼前：检索结果点进来时，用户要立刻看到「是这一段」
  useEffect(() => {
    if (!focusChunk || !doc) return;
    const node = chunkRefs.current[focusChunk];
    node?.scrollIntoView({ block: "center" });
    const timer = window.setTimeout(() => setFocusChunk(""), 1800);
    return () => window.clearTimeout(timer);
  }, [focusChunk, doc]);

  // ESC 关闭：捕获阶段拦下，别让底下的编辑器也收到（窗口模式不需要，关窗即可）
  useEffect(() => {
    if (!open || embedded) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      onClose();
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [open, onClose, embedded]);

  useVeilWheelLock(veilRef, open);

  const dragColumn = (which: "lib" | "entry") => (event: React.PointerEvent) => {
    event.preventDefault();
    const startX = event.clientX;
    const [libWidth, entryWidth] = widths;
    const onMove = (move: PointerEvent) => {
      const delta = move.clientX - startX;
      if (which === "lib") setWidths([clamp(libWidth + delta, 132, 380), entryWidth]);
      else setWidths([libWidth, clamp(entryWidth + delta, 150, 460)]);
    };
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      document.body.style.cursor = "";
    };
    document.body.style.cursor = "col-resize";
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  const runSearch = async (text: string) => {
    setQuery(text);
    if (!text.trim()) {
      setHits(null);
      return;
    }
    try {
      setHits(await knowledgeSearch(text.trim()));
    } catch {
      setHits([]);
    }
  };

  /** 摘要与关联问题：用设置里的对话模型生成，没配就给一句明确指引 */
  const enrich = async (target: KnowledgeDoc, force = false) => {
    const ai = loadAi();
    if (!aiReady(ai)) {
      toast("还没有配置对话模型 —— 摘要要用它生成（左下角 设置 → 模型）");
      return;
    }
    setEnriching(true);
    try {
      const next = await knowledgeEntryEnrich(
        target.id,
        { baseUrl: ai.baseUrl, apiKey: ai.apiKey, model: ai.model },
        force,
      );
      setDoc(next);
      await loadEntries(libId);
    } catch (error) {
      fail("生成摘要失败", error);
    } finally {
      setEnriching(false);
    }
  };

  const saveEntry = async () => {
    if (!draft) return;
    if (!draft.title.trim() || !draft.body.trim()) {
      toast("标题和正文都要填");
      return;
    }
    if (!libId) {
      toast("先建一个库（左列右上角的 +）");
      return;
    }
    setBusy(true);
    try {
      const saved = await knowledgeEntrySave({
        id: draft.id,
        libraryId: libId,
        title: draft.title,
        tags: parseTags(draft.tags),
        body: draft.body,
      });
      setDraft(null);
      setDoc(saved);
      setEntryId(saved.id);
      await loadEntries(libId);
      toast(`已保存，切成 ${saved.chunks.length} 块`);
      // 切完自动补摘要：这一步是「让换说法也能检索到」的关键，不能让用户记得点
      // 只有文字块需要摘要；图片块的描述是另一条链路（多模态 / OCR）
      if (saved.chunks.some((chunk) => chunk.kind === "text" && !chunk.summary)) {
        void enrich(saved, false);
      }
    } catch (error) {
      fail("保存失败", error);
    } finally {
      setBusy(false);
    }
  };

  const removeEntry = async (id: string) => {
    setBusy(true);
    try {
      await knowledgeEntryDelete(id);
      setRemoving(null);
      setEntryId("");
      setDoc(null);
      await loadEntries(libId);
      await loadLibs();
      toast("已删除条目");
    } catch (error) {
      fail("删除失败", error);
    } finally {
      setBusy(false);
    }
  };

  const removeChunk = async (chunkId: string) => {
    if (!doc) return;
    setBusy(true);
    try {
      // 后端会把正文里对应的 ![名](asset:…) 行一起摘掉，所以直接用返回的 doc 覆盖
      setDoc(await knowledgeChunkDelete(doc.id, chunkId));
      setRemoving(null);
      await loadEntries(libId);
      toast("已删除这块，立即生效");
    } catch (error) {
      fail("删除失败", error);
    } finally {
      setBusy(false);
    }
  };

  const saveLib = async () => {
    if (!libDraft) return;
    const name = libDraft.name.trim();
    if (!name) {
      toast("库名不能为空");
      return;
    }
    setBusy(true);
    try {
      const saved = await knowledgeLibrarySave({
        id: libDraft.id,
        name,
        note: libDraft.note.trim(),
      });
      setLibDraft(null);
      await loadLibs();
      setLibId(saved.id);
      toast(libDraft.id ? "已重命名" : "已新建库");
    } catch (error) {
      fail("保存失败", error);
    } finally {
      setBusy(false);
    }
  };

  const removeLib = async (id: string) => {
    setBusy(true);
    try {
      await knowledgeLibraryDelete(id);
      setRemoving(null);
      setEntryId("");
      setDoc(null);
      await loadLibs();
      toast("已删除库（含其中条目）");
    } catch (error) {
      fail("删除失败", error);
    } finally {
      setBusy(false);
    }
  };

  /** 导入一批文件：文本走模型分片，图片进材料池再建图块 */
  const importFiles = async (picked: FileList | null) => {
    if (!picked || picked.length === 0) return;
    if (!libId || activeLib?.builtin) {
      toast("先选一个自己的库（预置库只读）");
      return;
    }
    const { chunkAi, ocr: ocrAi, mode } = imageAnnotator();
    const files = Array.from(picked);
    const failures: string[] = [];
    let done = 0;
    setImporting(`0/${files.length}`);

    for (const [index, file] of files.entries()) {
      setImporting(`${index + 1}/${files.length}`);
      try {
        const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
        const parsed = await ingestFile(file.name, bytes);
        if (parsed.text) {
          const saved = await knowledgeImportText(
            { libraryId: libId, title: parsed.title, tags: [], text: parsed.text },
            { baseUrl: chunkAi.baseUrl, apiKey: chunkAi.apiKey, model: chunkAi.model },
          );
          setDoc(saved);
          setEntryId(saved.id);
        }
        for (const image of parsed.images) {
          const blob = new File([new Uint8Array(image.bytes)], image.name, {
            type: `image/${image.ext === "jpg" ? "jpeg" : image.ext}`,
          });
          const asset = await saveAsset(blob);
          // 有能看图的模型就让它描述；否则用 OCR 抄字；两者都没有就只存图、不写摘要
          let annotated = { summary: "", annotation: "" };
          if (mode !== "none") {
            try {
              annotated = await knowledgeImageAnnotate({
                bytes: image.bytes,
                ext: image.ext,
                mode,
                ai: mode === "model" ? { baseUrl: chunkAi.baseUrl, apiKey: chunkAi.apiKey, model: chunkAi.model } : null,
                ocr: mode === "ocr" ? ocrAi : null,
              });
            } catch {
              // 标注失败不该让导入失败：图还是要存进去，只是没有摘要
            }
          }
          const saved = await knowledgeImportImage({
            libraryId: libId,
            title: parsed.title,
            asset: asset.stored,
            assetName: image.name,
            summary: annotated.summary,
            annotation: annotated.annotation,
          });
          setDoc(saved);
          setEntryId(saved.id);
        }
        done += 1;
      } catch (error) {
        failures.push(`${file.name}：${error instanceof Error ? error.message : String(error)}`);
      }
    }

    setImporting("");
    await loadEntries(libId);
    await loadLibs();
    if (failures.length === 0) {
      toast(`已导入 ${done} 个文件`);
    } else {
      toast(`导入 ${done} 个，失败 ${failures.length} 个：${failures[0]}`);
    }
  };

  /** 重跑一次图片标注：图还在材料池里，这里只换说明文字 */
  const redrawImage = async (chunk: KnowledgeChunk) => {
    const { chunkAi, ocr, mode } = imageAnnotator();
    if (mode === "none") {
      toast("先配一个能看图的模型或 OCR，才能重写说明（左下角 设置 → 模型）");
      return;
    }
    if (!doc) return;
    setRedrawing(chunk.id);
    try {
      const bytes = await invoke<number[]>("pool_get", { stored: chunk.asset });
      // 用原始文件名而不是 stored（sha256 前 16 位.扩展名）：原文件没扩展名时能如实得到空串
      const ext = extOf(chunk.assetName || chunk.asset);
      const annotated = await knowledgeImageAnnotate({
        bytes,
        ext,
        mode,
        ai:
          mode === "model"
            ? { baseUrl: chunkAi.baseUrl, apiKey: chunkAi.apiKey, model: chunkAi.model }
            : null,
        ocr: mode === "ocr" ? ocr : null,
      });
      setDoc(await knowledgeImageUpdate(chunk.id, annotated.summary, annotated.annotation));
      toast("已重写这块的说明");
    } catch (error) {
      fail("重写说明失败", error);
    } finally {
      setRedrawing("");
    }
  };

  /** 手动向量化当前库：模型不一致时检索会自动退回词频，所以这一步必须显式 */
  const vectorizeLib = async () => {
    const choice = loadAux().embedding;
    if (!choice || !choice.model.trim()) {
      toast("先在 设置 → 模型 → 嵌入 里配一个嵌入模型");
      return;
    }
    const chatAi = loadAi();
    setVectorizing(true);
    try {
      const saved = await knowledgeVectorize(libId, {
        baseUrl: choice.baseUrl.trim() || chatAi.baseUrl,
        apiKey: choice.apiKey.trim() || chatAi.apiKey,
        model: choice.model,
      });
      await loadLibs();
      toast(
        `已向量化 ${saved.vectorized} 块 · ${saved.embeddingModel}${
          saved.embeddingDim ? `（${saved.embeddingDim} 维）` : ""
        }`,
      );
    } catch (error) {
      fail("向量化失败", error);
    } finally {
      setVectorizing(false);
    }
  };

  const openHit = async (hit: KnowledgeHit) => {
    setQuery("");
    setHits(null);
    keepEntry.current = hit.entryId;
    if (hit.libraryId !== libId) setLibId(hit.libraryId);
    setEntryId(hit.entryId);
    setFocusChunk(hit.chunkId);
    setDraft(null);
  };

  if (!open) return null;

  const activeLib = libs.find((item) => item.id === libId);
  const showHits = hits !== null;
  /** 第三列（块）在选中条目或正在编辑时才有内容可放 */
  const entryOpen = entryId !== "" || draft !== null;

  const box = (
    <div
      className={embedded ? `${styles.box} ${styles.windowBox}` : styles.box}
      role={embedded ? undefined : "dialog"}
      aria-modal={embedded ? undefined : true}
      aria-label="知识库"
    >
      {!embedded && (
        <div className={styles.head}>
          <div className={styles.title}>
            <BookOpen size={13} />
            <b>知识库</b>
          </div>
          <button type="button" className={shared.overlayClose} aria-label="关闭" onClick={onClose}>
            <Close size={11} />
          </button>
        </div>
      )}

      {/* 隐藏的文件选择框：accept 与后端 ingest_file 支持的格式一一对应 */}
      <input
        ref={importRef}
        type="file"
        multiple
        accept=".pdf,.md,.markdown,.txt,image/*"
        className={styles.hidden}
        onChange={(event) => {
          void importFiles(event.target.files);
          event.target.value = "";
        }}
      />

      <div className={styles.cols}>
        {/* ---------- 第一列：库 ---------- */}
        {/* 没选库时只该有这一列，铺满整块面板：留一条空荡荡的条目列只会让人以为数据没加载 */}
        <section className={styles.pane} style={libId ? { width: widths[0] } : { flex: 1 }}>
          <header className={styles.paneHead}>
            <span className={styles.paneTitle}>库</span>
          </header>

          <div className={styles.list}>
            {/* 新建就是列表的第一项：列头右上角那个 + 太容易被当成「展开更多」 */}
            <button
              type="button"
              className={styles.rowNew}
              onClick={() => setLibDraft({ name: "", note: "" })}
            >
              <Plus size={10} />
              <span>新建库</span>
            </button>
            {libs.map((library) => (
              <div
                key={library.id}
                className={cx(styles.row, library.id === libId && styles.rowOn)}
              >
                <button
                  type="button"
                  className={styles.rowMain}
                  onClick={() => setLibId(library.id)}
                >
                  <span className={styles.rowName}>{library.name}</span>
                  <span className={styles.rowMeta}>{library.entries}</span>
                </button>
                {!library.builtin && (
                  <span className={styles.rowActions}>
                    <Tooltip label="重命名">
                      <button
                        type="button"
                        className={styles.iconBtn}
                        aria-label={`重命名 ${library.name}`}
                        onClick={() =>
                          setLibDraft({ id: library.id, name: library.name, note: library.note })
                        }
                      >
                        <Pencil size={10} />
                      </button>
                    </Tooltip>
                    <Tooltip label="删除这个库">
                      <button
                        type="button"
                        className={cx(styles.iconBtn, styles.iconDanger)}
                        aria-label={`删除 ${library.name}`}
                        onClick={() =>
                          setRemoving({ kind: "lib", id: library.id, name: library.name })
                        }
                      >
                        <Trash size={10} />
                      </button>
                    </Tooltip>
                  </span>
                )}
              </div>
            ))}
            {libs.length === 0 && <p className={styles.empty}>还没有库</p>}
          </div>

          {/* 向量化是每库一个决策：模型换了就得重算，所以状态与按钮都挂在库上 */}
          <div className={styles.paneFoot}>
            <span className={styles.footMeta}>
              {activeLib?.builtin
                ? "只读"
                : activeLib?.embeddingModel
                  ? `${activeLib.vectorized} 块 · ${activeLib.embeddingModel}`
                  : "未向量化"}
            </span>
            <Button
              size="xs"
              leading={<Sparkles size={10} />}
              disabled={!libId || activeLib?.builtin || vectorizing || busy}
              onClick={() => void vectorizeLib()}
            >
              {vectorizing ? "向量化…" : "向量化"}
            </Button>
          </div>
        </section>

        <div
          className={styles.grip}
          role="separator"
          aria-orientation="vertical"
          aria-label="调整库列宽度"
          onPointerDown={dragColumn("lib")}
          style={libId ? undefined : { display: "none" }}
        />

        {/* ---------- 第二列：条目（选中库之后才出现） ---------- */}
        <section
          className={styles.pane}
          style={{
            width: widths[1],
            display: libId ? undefined : "none",
            // 还没选条目时没有第三列，条目列自己把剩下的宽度吃掉
            flex: entryOpen ? "none" : "1",
          }}
        >
          <header className={styles.paneHead}>
            <span className={styles.searchRow}>
              <Search size={11} />
              <input
                value={query}
                placeholder="搜索"
                aria-label="搜索知识库"
                onChange={(event) => void runSearch(event.target.value)}
              />
            </span>
            <Tooltip label="导入文件（PDF / MD / TXT / 图片）">
              <button
                type="button"
                className={styles.iconBtn}
                aria-label="导入文件"
                disabled={!libId || activeLib?.builtin || importing !== ""}
                onClick={() => importRef.current?.click()}
              >
                <Upload size={11} />
              </button>
            </Tooltip>
          </header>

          <div className={styles.list}>
            {/* 同上：新建是列表首项。预置库只读，没有「新建」这回事 */}
            {libId && !activeLib?.builtin && (
              <button
                type="button"
                className={styles.rowNew}
                onClick={() => {
                  setEntryId("");
                  setDoc(null);
                  setDraft({ title: "", tags: "", body: "" });
                }}
              >
                <Plus size={10} />
                <span>新建条目</span>
              </button>
            )}
            {showHits ? (
              hits.length === 0 ? (
                <p className={styles.empty}>没有命中</p>
              ) : (
                hits.map((hit) => (
                  <button
                    key={hit.chunkId}
                    type="button"
                    className={styles.hit}
                    onClick={() => void openHit(hit)}
                  >
                    <span className={styles.hitTitle}>{hit.title}</span>
                    <span className={styles.hitMeta}>
                      {hit.library}
                      {hit.heading ? ` · ${hit.heading}` : ""}
                    </span>
                    <span className={styles.hitSnippet}>{hit.snippet}</span>
                  </button>
                ))
              )
            ) : (
              <>
                {entries.map((entry) => (
                  <div
                    key={entry.id}
                    className={cx(styles.row, entry.id === entryId && styles.rowOn)}
                  >
                    <button
                      type="button"
                      className={styles.rowMain}
                      onClick={() => {
                        setEntryId(entry.id);
                        setDraft(null);
                      }}
                    >
                      <span className={styles.rowName}>{entry.title}</span>
                      <span className={styles.rowMeta}>
                        {entry.chunks}
                        {entry.enriched ? "" : " ·"}
                      </span>
                    </button>
                    {!entry.preset && (
                      <span className={styles.rowActions}>
                        <Tooltip label="删除这一条">
                          <button
                            type="button"
                            className={cx(styles.iconBtn, styles.iconDanger)}
                            aria-label={`删除 ${entry.title}`}
                            onClick={() =>
                              setRemoving({ kind: "entry", id: entry.id, name: entry.title })
                            }
                          >
                            <Trash size={10} />
                          </button>
                        </Tooltip>
                      </span>
                    )}
                  </div>
                ))}
                {entries.length === 0 && (
                  <p className={styles.empty}>{libId ? "这个库还没有条目" : "先建一个库"}</p>
                )}
              </>
            )}
          </div>
        </section>

        <div
          className={styles.grip}
          role="separator"
          aria-orientation="vertical"
          aria-label="调整条目列宽度"
          onPointerDown={dragColumn("entry")}
          style={entryOpen ? undefined : { display: "none" }}
        />

        {/* ---------- 第三列：这一条的块（选中条目后才出现） ---------- */}
        <section className={styles.detail} style={entryOpen ? undefined : { display: "none" }}>
          {draft ? (
            <div className={styles.editor}>
              <Field label="标题">
                <Input
                  data-autofocus
                  value={draft.title}
                  placeholder="一个主题一句话"
                  onChange={(event) => setDraft({ ...draft, title: event.target.value })}
                />
              </Field>
              <Field label="标签 · 顿号或逗号分隔">
                <Input
                  value={draft.tags}
                  placeholder="行业、岗位"
                  onChange={(event) => setDraft({ ...draft, tags: event.target.value })}
                />
              </Field>
              <Field label="正文 · 按空行分段，每段不超过 400 字会自动再切" className={styles.bodyFld}>
                <Textarea
                  rows={14}
                  value={draft.body}
                  placeholder="只写客观事实：行业口径、岗位要求、术语解释、常见问法。"
                  onChange={(event) => setDraft({ ...draft, body: event.target.value })}
                />
              </Field>
              {/* 图片条目也走这个表单：图片那几行 Markdown 就在正文里，得说清楚删了意味着什么 */}
              {doc?.chunks.some((chunk) => chunk.kind === "image") && (
                <p className={styles.hint}>
                  正文里 `![名字](asset:…)` 那几行就是图片本身，删掉哪一行就等于删掉那张图；
                  图片块会排在文字块之后。
                </p>
              )}
              <div className={styles.actions}>
                <Button
                  onClick={() => {
                    setDraft(null);
                    if (!doc) setEntryId("");
                  }}
                >
                  取消
                </Button>
                <Button variant="primary" disabled={busy} onClick={() => void saveEntry()}>
                  保存并切块
                </Button>
              </div>
            </div>
          ) : doc ? (
            <>
              <header className={styles.detailHead}>
                <div className={styles.detailLead}>
                  <b>{doc.title}</b>
                  <span className={styles.detailMeta}>
                    {doc.chunks.length} 块
                    {enriching
                      ? " · 正在生成摘要…"
                      : doc.chunks
                            .filter((chunk) => chunk.kind === "text")
                            .every((chunk) => chunk.summary)
                        ? " · 摘要已生成"
                        : " · 摘要待生成"}
                    {doc.tags.length > 0 ? ` · ${doc.tags.join(" · ")}` : ""}
                  </span>
                </div>
                {!doc.preset && (
                  <div className={styles.detailActs}>
                    <Tooltip label="重新生成摘要与关联问题">
                      <Button
                        size="sm"
                        leading={<Sparkles size={11} />}
                        disabled={enriching}
                        onClick={() => void enrich(doc, true)}
                      >
                        生成摘要
                      </Button>
                    </Tooltip>
                    <Button
                      size="sm"
                      onClick={() => {
                        setDraft({
                          id: doc.id,
                          title: doc.title,
                          tags: doc.tags.join("、"),
                          body: doc.body,
                        });
                      }}
                    >
                      编辑
                    </Button>
                  </div>
                )}
              </header>

              <div className={styles.chunkList}>
                {doc.chunks.map((chunk) => (
                  <article
                    key={chunk.id}
                    ref={(node) => {
                      chunkRefs.current[chunk.id] = node;
                    }}
                    className={cx(styles.chunk, focusChunk === chunk.id && styles.chunkFocus)}
                  >
                    <div className={styles.chunkHead}>
                      <span className={styles.chunkNo}>#{chunk.ord}</span>
                      {chunk.heading && <b>{chunk.heading}</b>}
                      <span className={styles.chunkLen}>{chunk.text.length} 字</span>
                    </div>
                    {chunk.kind === "image" ? (
                      <div className={styles.imageBox}>
                        <button
                          type="button"
                          className={styles.imageThumb}
                          aria-label={`查看 ${chunk.assetName || "图片"}`}
                          onClick={() => {
                            const src = thumbs[chunk.asset];
                            if (src) setViewer({ src, name: chunk.assetName || "图片" });
                          }}
                        >
                          {thumbs[chunk.asset] ? (
                            <img src={thumbs[chunk.asset]!} alt={chunk.assetName} />
                          ) : (
                            <span className={styles.imagePending}>
                              {chunk.asset in thumbs ? "图片读不出来" : "读取中…"}
                            </span>
                          )}
                        </button>
                        <div className={styles.imageSide}>
                          <span className={styles.annotation}>{annotationLabel(chunk)}</span>
                          {/* 图片块能单独重写/删除：图在材料池里是独立的一份。
                              文字块不给删除 —— 它的真相在正文里，删了下次保存又切回来 */}
                          {!doc.preset && (
                            <span className={styles.chunkTools}>
                              <Button
                                size="xs"
                                variant="quiet"
                                leading={<Sparkles size={10} />}
                                disabled={redrawing !== "" || busy}
                                onClick={() => void redrawImage(chunk)}
                              >
                                {redrawing === chunk.id ? "重写中…" : "重写说明"}
                              </Button>
                              <Button
                                size="xs"
                                variant="quiet"
                                leading={<Trash size={10} />}
                                aria-label={`删除图片块 ${chunk.assetName || `#${chunk.ord}`}`}
                                disabled={redrawing !== "" || busy}
                                onClick={() =>
                                  setRemoving({
                                    kind: "chunk",
                                    id: chunk.id,
                                    name: chunk.assetName || `#${chunk.ord}`,
                                  })
                                }
                              >
                                删除
                              </Button>
                            </span>
                          )}
                        </div>
                      </div>
                    ) : (
                      <pre className={styles.chunkText} data-selectable>
                        {chunk.text}
                      </pre>
                    )}
                    {chunk.summary && <p className={styles.summary}>{chunk.summary}</p>}
                    {chunk.questions.length > 0 && (
                      <div className={styles.questions}>
                        {chunk.questions.map((question) => (
                          <span key={question} className={styles.question}>
                            {question}
                          </span>
                        ))}
                      </div>
                    )}
                  </article>
                ))}
                {doc.chunks.length === 0 && <p className={styles.empty}>这一条还没有切块</p>}
              </div>
            </>
          ) : (
            <div className={styles.blank}>
              <BookOpen size={16} />
              <span>选中条目查看分块。库与条目归你管，模型只读。</span>
            </div>
          )}
        </section>
      </div>
    </div>
  );

  const surface = embedded ? (
    box
  ) : (
    <div ref={veilRef} className={shared.veil} data-print="hide">
      {box}
    </div>
  );

  const host =
    typeof document === "undefined" || embedded
      ? null
      : document.querySelector<HTMLElement>(".win");

  return (
    <>
      {host ? createPortal(surface, host) : surface}

      {viewer && (
        <ImageView src={viewer.src} name={viewer.name} onClose={() => setViewer(null)} />
      )}

      {libDraft && (
        <Modal
          title={libDraft.id ? "重命名库" : "新建库"}
          subtitle="库是主题边界：一个库管一类知识，删库会连同条目一起删掉"
          onCancel={() => setLibDraft(null)}
          footer={
            <>
              <Button onClick={() => setLibDraft(null)}>取消</Button>
              <Button variant="primary" disabled={busy} onClick={() => void saveLib()}>
                保存
              </Button>
            </>
          }
        >
          <Field label="库名">
            <Input
              data-autofocus
              value={libDraft.name}
              placeholder="例如：电商行业口径"
              onChange={(event) => setLibDraft({ ...libDraft, name: event.target.value })}
            />
          </Field>
          <Field label="说明 · 选填">
            <Input
              value={libDraft.note}
              placeholder="这个库收什么、不收什么"
              onChange={(event) => setLibDraft({ ...libDraft, note: event.target.value })}
            />
          </Field>
        </Modal>
      )}

      {/* 确认窗必须挂在 portal 之外单独渲染：它自己也 portal 进 .win，
          嵌在对话框树里会因为层级关系被盖住 */}
      {removing && (
        <Confirm
          open
          title={`删除${{ lib: "库", entry: "条目", chunk: "图片块" }[removing.kind]}「${removing.name}」？`}
          description={
            removing.kind === "lib"
              ? "库里的条目和分块会一起删掉，检索里立刻消失，无法撤销。"
              : removing.kind === "entry"
                ? "这一条会从检索里消失，无法撤销。"
                : "这块从检索里消失，正文里对应的图片行也会一起删掉，无法撤销。"
          }
          onCancel={() => setRemoving(null)}
          onConfirm={() =>
            void (removing.kind === "lib"
              ? removeLib(removing.id)
              : removing.kind === "entry"
                ? removeEntry(removing.id)
                : removeChunk(removing.id))
          }
        />
      )}
    </>
  );
}

/** 图片块摘要来源的中文说法：一眼看出「谁给它写的描述」 */
function annotationLabel(chunk: KnowledgeChunk): string {
  if (chunk.annotation === "model") return "模型标注";
  if (chunk.annotation === "ocr") return "OCR";
  return "无标注";
}
