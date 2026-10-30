import { useCallback, useEffect, useRef, useState, type DragEvent as ReactDragEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { BookOpen, Pencil, Search, Sparkles, Trash } from "@/components/ui/icons";
import { Confirm } from "@/components/ui/confirm";
import { Modal } from "@/components/ui/modal";
import { useToast } from "@/components/ui/toast-context";
import {
  configureKnowledge,
  ingestFile,
  knowledgeChunkDelete,
  knowledgeChunkSave,
  knowledgeChunks,
  knowledgeEntryEnrich,
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
  type KnowledgeChunkItem,
  type KnowledgeHit,
  type KnowledgeLibrary,
} from "@/lib/knowledge";
import { capabilityOf, capabilityTest } from "@/lib/capability";
import { aiReady, loadAi, loadAux, slotAi } from "@/lib/ai";
import { extOf, resolveStored, saveAsset } from "@/lib/assets";
import { ImageView } from "@/components/ui/image-view";
import { cx } from "@/lib/cx";
import {
  Button,
  Columns,
  Field,
  Grip,
  Input,
  List,
  ListAdd,
  ListEmpty,
  ListRow,
  Pane,
  PaneFoot,
  PaneHead,
  PaneScroll,
  Textarea,
  Tooltip,
} from "@/components/ui";
import styles from "./panel.module.css";

/**
 * 知识库：两层（库 → chunk）。
 *
 * 「条目」曾经是中间那一列，用户得先建条目、再进去看块。实际用起来那一层
 * 没有任何独立价值：一条长文进来必然被切块，块才是检索与阅读的单位。
 * 所以现在只留库与块 —— 输入一段文字（或导入文件）→ 自动切块 → 逐块改、逐块删。
 *
 * 为什么仍然是三列：第二列是块的列表，第三列是选中的那块的全部内容。
 * 这是「列表 + 详情」，不是三层归属。
 *
 * 模型只读：这里所有写操作都由用户点击发起，模型侧没有写工具。
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

/**
 * 图片说明走哪条链路：能看图的模型 → OCR → 都没有（不写摘要）。
 *
 * 导入与「重写说明」共用这一套判定 —— 各判一套的话，同一个库里会出现
 * 两种口径的描述，用户没法判断哪句更可信。
 */
async function imageAnnotator() {
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

  /**
   * 走「实测」而不是模型清单里那个出厂声明：清单说不能看图而网关其实能看，
   * 结果就是图片永远走 OCR 甚至直接导不进来。
   *
   * 判定顺序有讲究：
   *   1. 实测支持 → 直接让模型描述；
   *   2. 配了 OCR → 用 OCR（确定性更好，也不必为一个导入动作花掉一次探针）；
   *   3. 没测过且没 OCR → 当场测一次，测出来就支持；
   *   4. 其余 → 明确告诉用户「先配一个能看图的模型或 OCR」。
   */
  const conf = chunkAi.model
    ? { baseUrl: chunkAi.baseUrl, model: chunkAi.model, apiKey: chunkAi.apiKey }
    : null;
  let seen = conf ? await capabilityOf(conf).catch(() => null) : null;
  if (seen?.state !== "supported" && !ocr && seen && seen.checkedAt === null && conf) {
    seen = await capabilityTest(conf).catch(() => seen);
  }
  const mode: "model" | "ocr" | "none" =
    seen?.state === "supported" ? "model" : ocr ? "ocr" : "none";
  return { chunkAi, ocr, mode };
}

/** 用户没填标题：取第一行截断。标题只在检索结果里露一面，不值得让他为此多填一栏 */
function deriveTitle(text: string): string {
  const first = text.split("\n").map((line) => line.trim()).find(Boolean) ?? "未命名";
  return first.length > 40 ? `${first.slice(0, 40)}…` : first;
}

/**
 * 这次拖的是文件吗。
 *
 * 必须判：拖选中的文字进面板时也会触发 dragenter/dragover，若一律当成导入，
 * 松手就会对着一个空 FileList 报「没有导入成功」。
 */
function carriesFiles(event: ReactDragEvent): boolean {
  return Array.from(event.dataTransfer.types).includes("Files");
}

/** 列表里那一行叫什么：有标题用标题，没有就取正文开头 */
function chunkTitle(chunk: KnowledgeChunkItem): string {
  if (chunk.kind === "image") return chunk.assetName || "图片";
  if (chunk.heading) return chunk.heading;
  const flat = chunk.text.replace(/\s+/g, " ").trim();
  if (!flat) return "（空块）";
  return flat.length > 26 ? `${flat.slice(0, 26)}…` : flat;
}

const chunkMeta = (chunk: KnowledgeChunkItem) =>
  chunk.kind === "image" ? "图片" : `${chunk.text.length} 字`;

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
  const toast = useToast();

  const [libs, setLibs] = useState<KnowledgeLibrary[]>([]);
  const [libId, setLibId] = useState("");
  const [chunks, setChunks] = useState<KnowledgeChunkItem[]>([]);
  const [chunkId, setChunkId] = useState("");
  /** 正在改正文的块（null = 不在编辑）。改的是块本身，不是整篇 */
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(null);
  /** 「新增文本」小窗的正文 */
  const [textDraft, setTextDraft] = useState<string | null>(null);
  /** 库的新建/重命名小窗 */
  const [libDraft, setLibDraft] = useState<{ id?: string; name: string; note: string } | null>(null);
  /** 待确认的删除：库或某一块 */
  const [removing, setRemoving] = useState<
    { kind: "lib"; id: string; name: string } | { kind: "chunk"; id: string; entryId: string; name: string } | null
  >(null);

  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<KnowledgeHit[] | null>(null);

  const [busy, setBusy] = useState(false);
  /** 正在生成摘要的条目 id（空 = 没在跑） */
  const [enriching, setEnriching] = useState("");
  /** 导入进度文案：非空时提示条显示它 */
  const [importing, setImporting] = useState("");
  /**
   * 正有文件悬在面板上。
   *
   * 拖放是这里唯一的导入入口（面板头不再放按钮），所以「能不能放」必须在拖动
   * 过程中就看得见 —— 松手才发现没反应，等于没有这个功能。
   */
  const [dropping, setDropping] = useState(false);
  /** dragenter / dragleave 成对出现，但子元素会互相触发；用计数抵消抖动 */
  const dragDepth = useRef(0);
  /** 「新增文本」窗里那个拖放区的高亮（它不在面板的 dropHost 里，另算） */
  const [textDrag, setTextDrag] = useState(false);
  const [vectorizing, setVectorizing] = useState(false);
  /** 图片块缩略图地址（按材料池存储名缓存；null = 读不出来） */
  const [thumbs, setThumbs] = useState<Record<string, string | null>>({});
  /** 正在看的大图 */
  const [viewer, setViewer] = useState<{ src: string; name: string } | null>(null);
  /** 正在重写说明的图片块 id（空 = 没有在跑） */
  const [redrawing, setRedrawing] = useState("");
  const [widths, setWidths] = useState<[number, number]>(readWidths);

  /**
   * 从检索结果跳转时要跨库打开一块：切库的那个 effect 会清空选中，
   * 所以先用这个 ref 打一声招呼「下一条别清」。
   */
  const keepChunk = useRef("");

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

  const loadChunks = useCallback(
    async (libraryId: string) => {
      if (!libraryId) {
        setChunks([]);
        return;
      }
      try {
        setChunks(await knowledgeChunks(libraryId));
      } catch (error) {
        fail("读不到分块", error);
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
    void loadChunks(libId);
    setEditing(null);
    // 换库时清空右侧：留着上一块的正文会让人以为它属于这个库。
    // 例外是从检索结果跨库跳进来 —— 那时要保留它指的那一块。
    if (keepChunk.current) {
      setChunkId(keepChunk.current);
      keepChunk.current = "";
      return;
    }
    setChunkId("");
  }, [open, libId, loadChunks]);

  const selected = chunks.find((chunk) => chunk.id === chunkId) ?? null;

  useEffect(() => {
    const images = chunks.filter((chunk) => chunk.kind === "image" && chunk.asset);
    if (images.length === 0) return;
    let alive = true;
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
  }, [chunks]);

  useEffect(() => {
    try {
      localStorage.setItem(WIDTH_KEY, JSON.stringify(widths));
    } catch {
      // 隐私模式写不了：本次会话内仍然生效
    }
  }, [widths]);

  /* ESC 与滚轮锁都交给 Modal：它是弹窗的唯一出口，这里再实现一遍必然走形 */

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
  const enrich = async (entryId: string, force = false) => {
    const ai = loadAi();
    if (!aiReady(ai)) {
      toast("还没有配置对话模型 —— 摘要要用它生成（左下角 设置 → 模型）");
      return;
    }
    setEnriching(entryId);
    try {
      await knowledgeEntryEnrich(entryId, { baseUrl: ai.baseUrl, apiKey: ai.apiKey, model: ai.model }, force);
      await loadChunks(libId);
    } catch (error) {
      fail("生成摘要失败", error);
    } finally {
      setEnriching("");
    }
  };

  /** 新增文本：整段进来，后端按固定规则切块，切完逐块可改 */
  const saveText = async () => {
    if (textDraft === null) return;
    const text = textDraft.trim();
    if (!text) {
      toast("先写点东西再保存");
      return;
    }
    if (!libId || activeLib?.builtin) {
      toast("先选一个自己的库（预置库只读）");
      return;
    }
    setBusy(true);
    try {
      const saved = await knowledgeEntrySave({
        libraryId: libId,
        title: deriveTitle(text),
        tags: [],
        body: text,
      });
      setTextDraft(null);
      await loadChunks(libId);
      await loadLibs();
      setChunkId(saved.chunks[0]?.id ?? "");
      toast(`已存下，切成 ${saved.chunks.length} 块`);
      // 切完自动补摘要：这一步是「让换说法也能检索到」的关键，不能让用户记得点。
      // 只有文字块需要摘要；图片块的描述是另一条链路（多模态 / OCR）
      if (saved.chunks.some((chunk) => chunk.kind === "text" && !chunk.summary)) {
        void enrich(saved.id, false);
      }
    } catch (error) {
      fail("保存失败", error);
    } finally {
      setBusy(false);
    }
  };

  /** 改一块的正文：后端会清掉这块的摘要与向量（它们照旧文字算的） */
  const saveChunkText = async () => {
    if (!editing) return;
    const text = editing.text.trim();
    if (!text) {
      toast("这一块不能是空的 —— 空块既检索不到，也占地方");
      return;
    }
    setBusy(true);
    try {
      setChunks(await knowledgeChunkSave(editing.id, text));
      setEditing(null);
      toast("已保存；这一块的摘要清空了，需要时点「生成摘要」");
    } catch (error) {
      fail("保存失败", error);
    } finally {
      setBusy(false);
    }
  };

  const removeChunk = async (entryId: string, id: string) => {
    setBusy(true);
    try {
      await knowledgeChunkDelete(entryId, id);
      setRemoving(null);
      setChunkId("");
      setEditing(null);
      await loadChunks(libId);
      await loadLibs();
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
      const saved = await knowledgeLibrarySave({ id: libDraft.id, name, note: libDraft.note.trim() });
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
      setChunkId("");
      setEditing(null);
      await loadLibs();
      toast("已删除库（含其中所有块）");
    } catch (error) {
      fail("删除失败", error);
    } finally {
      setBusy(false);
    }
  };

  /**
   * 面板开着时接管整页的拖放。
   *
   * 不接的话 WebView2 会用自己的默认行为：拖一个 pdf 进窗口、松手 —— 整个应用被导航成
   * 那个文件（白屏 + 回不来）。这里只拦默认行为，真正的落点由下面的 dropArea 处理。
   */
  useEffect(() => {
    if (!open) return;
    const stop = (event: DragEvent) => event.preventDefault();
    window.addEventListener("dragover", stop);
    window.addEventListener("drop", stop);
    return () => {
      window.removeEventListener("dragover", stop);
      window.removeEventListener("drop", stop);
    };
  }, [open]);

  /** 导入一批文件：文本走模型分片，图片进材料池再建图块 */
  const importFiles = async (picked: FileList | null) => {
    if (!picked || picked.length === 0) return;
    if (!libId || activeLib?.builtin) {
      toast("先选一个自己的库（预置库只读）");
      return;
    }
    const { chunkAi, ocr: ocrAi, mode } = await imageAnnotator();
    const files = Array.from(picked);
    const failures: string[] = [];
    let done = 0;
    let chunks = 0;
    let lastChunk = "";
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
          chunks += saved.chunks.length;
          lastChunk = saved.chunks[0]?.id ?? lastChunk;
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
                ai:
                  mode === "model"
                    ? { baseUrl: chunkAi.baseUrl, apiKey: chunkAi.apiKey, model: chunkAi.model }
                    : null,
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
          chunks += saved.chunks.length;
          lastChunk = saved.chunks[saved.chunks.length - 1]?.id ?? lastChunk;
        }
        done += 1;
      } catch (error) {
        failures.push(`${file.name}：${error instanceof Error ? error.message : String(error)}`);
      }
    }

    setImporting("");
    await loadChunks(libId);
    await loadLibs();
    if (lastChunk) setChunkId(lastChunk);
    if (failures.length === 0) {
      toast(`已导入 ${done} 个文件 · ${chunks} 块`);
    } else if (done === 0) {
      // 一个都没成：原因必须完整露出来，否则用户只看到「导入 0 个」不知道该换什么格式
      toast(`没有导入成功：${failures[0]}`);
    } else {
      toast(`已导入 ${done} 个（${chunks} 块），${failures.length} 个失败：${failures[0]}`);
    }
  };

  /** 重跑一次图片标注：图还在材料池里，这里只换说明文字 */
  const redrawImage = async (chunk: KnowledgeChunkItem) => {
    const { chunkAi, ocr, mode } = await imageAnnotator();
    if (mode === "none") {
      toast("先配一个能看图的模型或 OCR，才能重写说明（左下角 设置 → 模型）");
      return;
    }
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
      await knowledgeImageUpdate(chunk.id, annotated.summary, annotated.annotation);
      await loadChunks(libId);
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
    setEditing(null);
    if (hit.libraryId !== libId) keepChunk.current = hit.chunkId;
    setLibId(hit.libraryId);
    setChunkId(hit.chunkId);
  };

  if (!open) return null;

  const activeLib = libs.find((item) => item.id === libId);
  const readonly = !activeLib || activeLib.builtin;
  const showHits = hits !== null;
  const chunkOpen = chunkId !== "";

  /* 窗内（embedded）只是一个铺满的容器；弹窗模式走统一的 Modal，
     不再自己画遮罩、标题栏和关闭键 —— 那套东西每改一次都要改两处 */
  const content = (
    /* 拖放挂在整块面板上，不是挂在某一列：用户的动作是「把文件拖进知识库」，
       不会先判断该落在列表上还是详情上。覆盖层只在拖动过程中出现。 */
    <div
      className={styles.dropHost}
      onDragEnter={(event) => {
        if (!carriesFiles(event)) return;
        event.preventDefault();
        dragDepth.current += 1;
        setDropping(true);
      }}
      onDragOver={(event) => {
        if (!carriesFiles(event)) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = "copy";
      }}
      onDragLeave={() => {
        dragDepth.current = Math.max(0, dragDepth.current - 1);
        if (dragDepth.current === 0) setDropping(false);
      }}
      onDrop={(event) => {
        if (!carriesFiles(event)) return;
        event.preventDefault();
        dragDepth.current = 0;
        setDropping(false);
        void importFiles(event.dataTransfer.files);
      }}
    >
      <Columns>
        {/* ---------- 第一列：库 ---------- */}
        <Pane width={widths[0]} bordered>
          <PaneHead title="库" />

          <PaneScroll pad={4}>
            <List>
              <ListAdd label="新建库" onClick={() => setLibDraft({ name: "", note: "" })} />
              {libs.map((library) => (
                <ListRow
                  key={library.id}
                  active={library.id === libId}
                  title={library.name}
                  meta={library.entries}
                  onSelect={() => setLibId(library.id)}
                  actions={
                    !library.builtin && (
                      <>
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
                      </>
                    )
                  }
                />
              ))}
              {libs.length === 0 && <ListEmpty>还没有库</ListEmpty>}
            </List>
          </PaneScroll>

          {/* 向量化是每库一个决策：模型换了就得重算，所以状态与按钮都挂在库上 */}
          <PaneFoot>
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
          </PaneFoot>
        </Pane>

        <Grip
          label="调整库列宽度"
          width={widths[0]}
          min={132}
          max={380}
          hidden={!libId}
          onChange={(next) => setWidths([next, widths[1]])}
        />

        {/* ---------- 第二列：这个库的块（选中库之后才出现） ---------- */}
        <Pane width={chunkOpen ? widths[1] : undefined} grow={!chunkOpen} hidden={!libId}>
          <PaneHead>
            <span className={styles.searchRow}>
              <Search size={11} />
              <input
                value={query}
                placeholder="搜索"
                aria-label="搜索知识库"
                onChange={(event) => void runSearch(event.target.value)}
              />
            </span>
            {/* 面板头不再放导入按钮：整块面板就是落点。导入进度只在这一行
                以文字出现（「导入中 2/5」），不需要一个按钮在旁边守着 */}
            {importing && <span className={styles.importBadge}>导入中 {importing}</span>}
          </PaneHead>

          <PaneScroll pad={4}>
            <List>
              {/* 新建是列表首项。预置库只读，没有「新建」这回事 */}
              {libId && !readonly && (
                <ListAdd label="新增文本" onClick={() => setTextDraft("")} />
              )}
              {showHits ? (
                hits.length === 0 ? (
                  <ListEmpty>没有命中</ListEmpty>
                ) : (
                  hits.map((hit) => (
                    <button
                      key={hit.chunkId}
                      type="button"
                      className={styles.hit}
                      onClick={() => void openHit(hit)}
                    >
                      <span className={styles.hitTitle}>{hit.heading || hit.title}</span>
                      <span className={styles.hitMeta}>{hit.library}</span>
                      <span className={styles.hitSnippet}>{hit.snippet}</span>
                    </button>
                  ))
                )
              ) : (
                <>
                  {chunks.map((chunk) => (
                    <ListRow
                      key={chunk.id}
                      active={chunk.id === chunkId}
                      title={chunkTitle(chunk)}
                      meta={chunkMeta(chunk)}
                      onSelect={() => {
                        setChunkId(chunk.id);
                        setEditing(null);
                      }}
                      actions={
                        !readonly && (
                          <Tooltip label="删除这一块">
                            <button
                              type="button"
                              className={cx(styles.iconBtn, styles.iconDanger)}
                              aria-label={`删除 ${chunkTitle(chunk)}`}
                              onClick={() =>
                                setRemoving({
                                  kind: "chunk",
                                  id: chunk.id,
                                  entryId: chunk.entryId,
                                  name: chunkTitle(chunk),
                                })
                              }
                            >
                              <Trash size={10} />
                            </button>
                          </Tooltip>
                        )
                      }
                    />
                  ))}
                  {chunks.length === 0 && (
                    <ListEmpty>
                      {libId
                        ? "这个库还没有内容 —— 把 PDF / Word / PPT / Excel / 文本 / 图片 拖进来"
                        : "先建一个库，再把文件拖进来"}
                    </ListEmpty>
                  )}
                </>
              )}
            </List>
          </PaneScroll>
        </Pane>

        <Grip
          label="调整块列宽度"
          width={widths[1]}
          min={150}
          max={460}
          hidden={!chunkOpen}
          onChange={(next) => setWidths([widths[0], next])}
        />

        {/* ---------- 第三列：选中的那一块 ---------- */}
        <Pane grow hidden={!chunkOpen}>
          {selected ? (
            <>
              <header className={styles.detailHead}>
                <div className={styles.detailLead}>
                  <b>{chunkTitle(selected)}</b>
                  <span className={styles.detailMeta}>
                    #{selected.ord} · {chunkMeta(selected)}
                    {selected.kind === "image"
                      ? ` · ${annotationLabel(selected)}`
                      : ` · ${selected.summary ? "有摘要" : "无摘要"}`}
                  </span>
                </div>
                {!readonly && !editing && (
                  <div className={styles.detailActs}>
                    {selected.kind !== "image" && (
                      <Tooltip label="改这一块的正文">
                        <Button
                          size="sm"
                          onClick={() => setEditing({ id: selected.id, text: selected.text })}
                        >
                          编辑
                        </Button>
                      </Tooltip>
                    )}
                    <Tooltip label="重新生成摘要与关联问题">
                      <Button
                        size="sm"
                        leading={<Sparkles size={11} />}
                        disabled={enriching !== ""}
                        onClick={() => void enrich(selected.entryId, true)}
                      >
                        {enriching === selected.entryId ? "生成中…" : "生成摘要"}
                      </Button>
                    </Tooltip>
                    <Tooltip label="删除这一块">
                      <Button
                        size="sm"
                        variant="quiet"
                        leading={<Trash size={11} />}
                        aria-label="删除这一块"
                        disabled={busy || redrawing !== ""}
                        onClick={() =>
                          setRemoving({
                            kind: "chunk",
                            id: selected.id,
                            entryId: selected.entryId,
                            name: chunkTitle(selected),
                          })
                        }
                      >
                        删除
                      </Button>
                    </Tooltip>
                  </div>
                )}
              </header>

              <PaneScroll pad={14} className={styles.detailBody}>
                {editing?.id === selected.id ? (
                  <div className={styles.editorBox}>
                    <Textarea
                      data-autofocus
                      rows={14}
                      value={editing.text}
                      onChange={(event) => setEditing({ ...editing, text: event.target.value })}
                    />
                    <p className={styles.hint}>
                      改的是这一块本身。保存后这块的摘要会清空 —— 它原先照旧文字生成，不重算就会骗检索。
                    </p>
                    <div className={styles.actions}>
                      <Button onClick={() => setEditing(null)}>取消</Button>
                      <Button variant="primary" disabled={busy} onClick={() => void saveChunkText()}>
                        保存
                      </Button>
                    </div>
                  </div>
                ) : (
                  <>
                    {selected.kind === "image" ? (
                      <div className={styles.imageBox}>
                        <button
                          type="button"
                          className={styles.imageThumb}
                          aria-label={`查看 ${selected.assetName || "图片"}`}
                          onClick={() => {
                            const src = thumbs[selected.asset];
                            if (src) setViewer({ src, name: selected.assetName || "图片" });
                          }}
                        >
                          {thumbs[selected.asset] ? (
                            <img src={thumbs[selected.asset]!} alt={selected.assetName} />
                          ) : (
                            <span className={styles.imagePending}>
                              {selected.asset in thumbs ? "图片读不出来" : "读取中…"}
                            </span>
                          )}
                        </button>
                        <div className={styles.imageSide}>
                          <span className={styles.annotation}>{annotationLabel(selected)}</span>
                          {!readonly && (
                            <span className={styles.chunkTools}>
                              <Button
                                size="xs"
                                variant="quiet"
                                leading={<Sparkles size={10} />}
                                disabled={redrawing !== "" || busy}
                                onClick={() => void redrawImage(selected)}
                              >
                                {redrawing === selected.id ? "重写中…" : "重写说明"}
                              </Button>
                            </span>
                          )}
                        </div>
                      </div>
                    ) : (
                      <pre className={styles.chunkText} data-selectable>
                        {selected.text}
                      </pre>
                    )}

                    {selected.summary && <p className={styles.summary}>{selected.summary}</p>}
                    {selected.questions.length > 0 && (
                      <div className={styles.questions}>
                        {selected.questions.map((question) => (
                          <span key={question} className={styles.question}>
                            {question}
                          </span>
                        ))}
                      </div>
                    )}
                  </>
                )}
              </PaneScroll>
            </>
          ) : (
            <div className={styles.blank}>
              <BookOpen size={16} />
              <span>选中一块看内容。库与块归你管，模型只读。</span>
            </div>
          )}
        </Pane>
      </Columns>

      {/* 覆盖层不接事件（pointer-events: none）：它只是「可以松手了」的信号，
          接住事件反而会把 drop 从下面的列上抢走 */}
      {dropping && (
        <div className={styles.dropVeil}>
          <span>
            {readonly ? "预置库只读 —— 先建一个自己的库" : "松手即导入"} · PDF / Word / PPT / Excel / 文本 / 图片
          </span>
        </div>
      )}
    </div>
  );

  const surface = embedded ? (
    <div className={cx(styles.box, styles.windowBox)}>{content}</div>
  ) : (
    <Modal title="知识库" onClose={onClose} size="lg" bodyClassName={styles.colsHost}>
      {content}
    </Modal>
  );

  return (
    <>
      {surface}

      {viewer && <ImageView src={viewer.src} name={viewer.name} onClose={() => setViewer(null)} />}

      {textDraft !== null && (
        <Modal
          title="新增文本"
          subtitle="整段文字进来，按空行分段、每段不超过 400 字自动切块；切完可以逐块改"
          onCancel={() => setTextDraft(null)}
          footer={
            <>
              <Button onClick={() => setTextDraft(null)}>取消</Button>
              <Button variant="primary" disabled={busy} onClick={() => void saveText()}>
                保存并切块
              </Button>
            </>
          }
        >
          <div
            className={cx(styles.dropArea, textDrag && styles.dropActive)}
            onDragOver={(event) => {
              event.preventDefault();
              setTextDrag(true);
            }}
            onDragLeave={() => setTextDrag(false)}
            onDrop={(event) => {
              event.preventDefault();
              setTextDrag(false);
              const files = event.dataTransfer.files;
              // 拖进来的文件直接走导入流程，不用先点「保存并切块」
              setTextDraft(null);
              void importFiles(files);
            }}
          >
            <Field label="正文" className={styles.bodyFld}>
              <Textarea
                data-autofocus
                rows={14}
                value={textDraft}
                placeholder="只写客观事实：行业口径、岗位要求、术语解释、常见问法。"
                onChange={(event) => setTextDraft(event.target.value)}
              />
            </Field>
            <p className={styles.hint}>
              也可以把 PDF / Word / PPT / Excel / 文本 / 图片 直接拖进来，松手即导入。
            </p>
          </div>
        </Modal>
      )}

      {libDraft && (
        <Modal
          title={libDraft.id ? "重命名库" : "新建库"}
          subtitle="库是主题边界：一个库管一类知识，删库会连同其中的块一起删掉"
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
          title={`删除${removing.kind === "lib" ? "库" : "这一块"}「${removing.name}」？`}
          description={
            removing.kind === "lib"
              ? "库里的所有块会一起删掉，检索里立刻消失，无法撤销。"
              : "这块从检索里消失，无法撤销；它是这条内容最后一块时，整条也会一起没掉。"
          }
          onCancel={() => setRemoving(null)}
          onConfirm={() =>
            void (removing.kind === "lib"
              ? removeLib(removing.id)
              : removeChunk(removing.entryId, removing.id))
          }
        />
      )}
    </>
  );
}

/** 图片块摘要来源的中文说法：一眼看出「谁给它写的描述」 */
function annotationLabel(chunk: KnowledgeChunkItem): string {
  if (chunk.annotation === "model") return "模型标注";
  if (chunk.annotation === "ocr") return "OCR";
  return "无标注";
}
