import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Attachment, ChatAction, ChatMessage } from "@/types";
import { useWorkspace } from "@/store/workspace";
import { uid } from "@/lib/uid";
import { useToast } from "@/components/ui/toast-context";
import { ChevronDown, ChevronRight } from "@/components/ui/icons";
import { Composer } from "./composer";
import { toAttachment, attachmentText } from "@/lib/attach";
import { AiSettingsSheet } from "./ai-settings";
import { AssistantTurn, EmptyTurn, TypingTurn, UserTurn } from "./message";
import { aiReady, loadAi, saveAi, streamChat, type AiSettings } from "@/lib/ai";
import { buildContext, buildSystemPrompt, splitReply, toApiMessages } from "@/lib/prompt";
import { isTauri } from "@/lib/storage";
import { displayName } from "@/types";
import { Channel, invoke } from "@tauri-apps/api/core";
import styles from "./chat.module.css";

/**
 * 没有配置模型时不产生任何"回答"。
 *
 * 这里曾经用 lib/rewrite.ts 的本地规则拼一段看起来像 AI 的回复
 * （还带"已提取 N 个关键词"这类并未真正计算的工具行），用户会误以为是模型输出。
 * 现在改为：不写消息、不伪造内容，直接把设置面板推到用户面前。
 */

export function ChatPanel() {
  const { person, resume, page, dispatch } = useWorkspace();
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const [input, setInput] = useState("");
  const [files, setFiles] = useState<Attachment[]>([]);
  const [ai, setAi] = useState<AiSettings>(() => loadAi());
  const [aiOpen, setAiOpen] = useState(false);
  const [dragging, setDragging] = useState(false);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  /** 消息真相源在 Rust（chat_history 拉投影）；浏览器调试模式才退回工作区 */
  const [remote, setRemote] = useState<ChatMessage[]>([]);
  const [reasoning, setReasoning] = useState("");
  const desktop = isTauri();

  const configured = aiReady(ai);
  const modelLabel = configured ? ai.model : "未配置模型 · 点此设置";

  const messages = useMemo(
    () => (desktop ? remote : person?.messages ?? []),
    [desktop, remote, person],
  );

  const personId = person?.id ?? null;

  // 切人/启动时从 Rust 拉一次（只给要显示的那一段，Rust 已按 token 预算裁剪过）
  useEffect(() => {
    if (!desktop || !personId) {
      setRemote([]);
      return;
    }
    let alive = true;
    void invoke<ChatMessage[]>("chat_history", { key: personId })
      .then((list) => alive && setRemote(list))
      .catch(() => alive && setRemote([]));
    return () => {
      alive = false;
    };
  }, [desktop, personId]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [messages.length, busy, resume?.id]);

  useEffect(() => {
    setBusy(false);
    setFiles([]);
    setInput("");
  }, [resume?.id]);

  useEffect(() => () => abortRef.current?.abort(), []);

  /** 本地投影打补丁（Tauri 模式下消息真相在 Rust，这里只是显示） */
  const patchLocal = useCallback((id: string, patch: Partial<ChatMessage>) => {
    setRemote((prev) => prev.map((item) => (item.id === id ? { ...item, ...patch } : item)));
  }, []);

  const send = useCallback(
    (text: string, attachments: Attachment[]) => {
      if (!person || !resume) return;

      // 没有模型就到此为止：不发消息、不伪造回答，直接引导去配置
      if (!configured) {
        setAiOpen(true);
        toast("先配置模型才能对话");
        return;
      }

      const targetPage = page ?? resume.pages[0] ?? null;
      if (!targetPage) {
        toast("这份简历还没有页面，先在右边新增一页");
        return;
      }

      const userMessage: ChatMessage = {
        id: uid("m"),
        role: "user",
        content: text,
        time: Date.now(),
        attachments: attachments.length ? attachments : undefined,
      };
      const replyId = uid("m");
      const draft: ChatMessage = {
        id: replyId,
        role: "assistant",
        time: Date.now(),
        content: "",
        tool: `${ai.model} · 正在读简历`,
      };

      setBusy(true);
      setReasoning("");
      abortRef.current = new AbortController();

      /* ---------- 浏览器调试模式：保持原有工作区流程 ---------- */
      if (!desktop) {
        dispatch({ type: "appendMessage", personId: person.id, message: userMessage });
        dispatch({ type: "appendMessage", personId: person.id, message: draft });
        const controller = abortRef.current;
        let acc = "";
        void (async () => {
          try {
            await streamChat({
              settings: ai,
              messages: toApiMessages(
                buildSystemPrompt(person),
                person.messages.map((item) => ({ role: item.role, content: item.content })),
                text,
                attachments.map((file) => ({ name: file.name, text: attachmentText(file.id) })),
              ),
              signal: controller.signal,
              onDelta: (delta) => {
                acc += delta;
                dispatch({
                  type: "patchMessage",
                  personId: person.id,
                  messageId: replyId,
                  patch: { content: acc },
                });
              },
            });
            const { content, changes } = splitReply(acc, person, targetPage);
            dispatch({
              type: "patchMessage",
              personId: person.id,
              messageId: replyId,
              patch: {
                content: content || "（模型没有返回正文）",
                tool: `${ai.model} · 完成`,
                changes: changes.length ? changes : undefined,
              },
            });
          } catch (error) {
            if (controller.signal.aborted) return;
            const reason = error instanceof Error ? error.message : String(error);
            dispatch({
              type: "patchMessage",
              personId: person.id,
              messageId: replyId,
              patch: { content: `调用失败：${reason}`, tool: `${ai.model} · 调用失败` },
            });
          } finally {
            setBusy(false);
          }
        })();
        return;
      }

      /* ---------- 桌面：消息交给 Rust，上下文与裁剪也在那边 ---------- */
      setRemote((prev) => [...prev, userMessage, draft]);

      void (async () => {
        try {
          await invoke("chat_push", { key: person.id, message: userMessage });

          const channel = new Channel<{
            kind: string;
            delta?: string;
            reasoning?: string;
            message?: string;
          }>();
          let acc = "";
          let flushed = 0;
          channel.onmessage = (chunk) => {
            if (chunk.kind === "delta" && chunk.delta) {
              acc += chunk.delta;
              const now = Date.now();
              if (now - flushed > 70) {
                flushed = now;
                patchLocal(replyId, { content: acc });
              }
            } else if (chunk.kind === "reasoning" && chunk.reasoning) {
              setReasoning((prev) => prev + chunk.reasoning);
            }
          };

          const id = await invoke<string>("chat_send", {
            args: {
              key: person.id,
              context: buildContext(person, targetPage),
              baseUrl: ai.baseUrl,
              apiKey: ai.apiKey,
              model: ai.model,
              temperature: ai.temperature,
              reasoning: ai.reasoning ?? null,
              contextWindow: ai.contextWindow ?? null,
            },
            onEvent: channel,
          });

          const { content, changes } = splitReply(acc, person, targetPage);
          await invoke("chat_patch", {
            key: person.id,
            id,
            patch: {
              content: content || "（模型没有返回正文）",
              tool: `${ai.model} · 完成`,
              changes: changes.length ? changes : null,
              actions: changes.length
                ? [
                    { id: uid("a"), label: `应用 ${changes.length} 处改写`, kind: "apply" },
                    { id: uid("a"), label: "再收一收", kind: "hint" },
                  ]
                : null,
            },
          });

          // 用 Rust 的历史覆盖本地投影：draft 换成正式消息，顺手拿到裁剪后的列表
          setRemote(await invoke<ChatMessage[]>("chat_history", { key: person.id }));
        } catch (error) {
          const reason = error instanceof Error ? error.message : String(error);
          if (reason.includes("已中断")) {
            setRemote((prev) => prev.filter((item) => item.id !== replyId));
            toast("已停止");
          } else {
            patchLocal(replyId, {
              content: `调用失败：${reason}`,
              tool: `${ai.model} · 调用失败`,
            });
            toast("模型调用失败，检查接口地址与密钥");
          }
        } finally {
          setBusy(false);
          abortRef.current = null;
        }
      })();
    },
    [person, resume, page, dispatch, ai, configured, toast, desktop, patchLocal],
  );

  /** 统一补丁入口：桌面端消息真相在 Rust，浏览器端在工作区 */
  const patchMessage = useCallback(
    (id: string, patch: Partial<ChatMessage>) => {
      if (!person) return;
      if (!desktop) {
        dispatch({ type: "patchMessage", personId: person.id, messageId: id, patch });
        return;
      }
      patchLocal(id, patch);
      // undefined 序列化会丢字段，Rust 侧用 null 表示"清空"
      const wire: Record<string, unknown> = {};
      for (const [field, value] of Object.entries(patch)) {
        wire[field] = value === undefined ? null : value;
      }
      void invoke("chat_patch", { key: person.id, id, patch: wire });
    },
    [person, desktop, dispatch, patchLocal],
  );

  /** 中断当前请求（只有桌面模式有效：命令层能置取消标记） */
  const stop = useCallback(() => {
    abortRef.current?.abort();
    if (desktop && person) {
      void invoke("chat_cancel", { key: person.id });
    }
  }, [desktop, person]);

  const handleAction = (message: ChatMessage, action: ChatAction) => {
    if (!person || !resume) return;
    if (action.kind === "apply") {
      const changes = (message.changes ?? []).filter((change) => change.target);
      if (!changes.length) {
        toast("这条建议没有可写入的目标");
        return;
      }

      // 改写只写回当前这一页的措辞（entry），不碰经历库里的记录。
      // 同一条 entry 的多处改写合并成一次写入，否则后一次会覆盖前一次。
      if (!page) return;
      // v4：措辞只有一套，直接按 entryId 改本页那一条
      const entries = page.sections.flatMap((section) => section.items);
      const patches = new Map<string, string[]>();
      for (const change of changes) {
        const target = change.target;
        if (!target) continue;
        const entry = entries.find((item) => item.id === target.entryId);
        if (!entry) continue;
        const list = patches.get(entry.id) ?? [...entry.bullets];
        list[target.bulletIndex] = change.after;
        patches.set(entry.id, list);
      }

      if (!patches.size) {
        toast("找不到对应的句子，可能这条已经从本页移除了");
        return;
      }

      for (const [entryId, bullets] of patches) {
        dispatch({
          type: "entryPatch",
          personId: person.id,
          resumeId: resume.id,
          pageId: page.id,
          entryId,
          patch: { bullets },
        });
      }
      patchMessage(message.id, {
        actions: undefined,
        tool: `${message.tool ?? "改写"} · 已写入本页`,
      });
      toast(`已写入 ${changes.length} 处`);
      return;
    }

    if (action.label === "去检查设置") {
      setAiOpen(true);
      return;
    }
    setInput("把这段再具体一点，补上当时的判断依据。");
  };

  const onDrop = (event: React.DragEvent) => {
    event.preventDefault();
    setDragging(false);
    const dropped = Array.from(event.dataTransfer.files ?? []).map(toAttachment);
    if (dropped.length) setFiles((list) => [...list, ...dropped]);
  };

  if (!person || !resume) {
    return (
      <section className={styles.chat} data-col="chat" data-print="hide">
        <div className={styles.empty}>左侧还没有简历，先新建一份。</div>
      </section>
    );
  }

  return (
    <section
      className={styles.chat}
      data-col="chat"
      data-print="hide"
      onDragOver={(event) => {
        event.preventDefault();
        setDragging(true);
      }}
      onDragLeave={(event) => {
        if (event.currentTarget.contains(event.relatedTarget as Node)) return;
        setDragging(false);
      }}
      onDrop={onDrop}
    >
      <header className={styles.head}>
        <div className={styles.crumb}>
          <span className={styles.who}>{displayName(person.fields)}</span>
          <ChevronRight size={9} className={styles.sep} />
          <b>{resume.title}</b>
        </div>
        <button type="button" className={styles.modelStatic} onClick={() => setAiOpen(true)}>
          {modelLabel}
        </button>
      </header>

      <div className={styles.thread} ref={scrollRef}>
        <div className={styles.threadIn}>
          {messages.length === 0 && !busy && <EmptyTurn name={resume.title} onPick={setInput} />}
          {messages.map((message) =>
            message.role === "user" ? (
              <UserTurn key={message.id} message={message} />
            ) : (
              <AssistantTurn key={message.id} message={message} onAction={(action) => handleAction(message, action)} />
            ),
          )}
          {busy && reasoning && (
            <details className={styles.think}>
              <summary>思考过程 · {reasoning.length} 字</summary>
              <pre>{reasoning}</pre>
            </details>
          )}
          {busy && <TypingTurn />}
        </div>
      </div>

      {busy && (
        <div className={styles.stopRow}>
          <button type="button" className={styles.stopBtn} onClick={stop}>
            停止生成
          </button>
        </div>
      )}

      <Composer
        model={modelLabel}
        onOpenSettings={() => setAiOpen(true)}
        onSend={send}
        busy={busy}
        text={input}
        onTextChange={setInput}
        files={files}
        onFilesChange={setFiles}
      />

      {aiOpen && (
        <AiSettingsSheet
          settings={ai}
          onClose={() => setAiOpen(false)}
          onSave={(next) => {
            saveAi(next);
            setAi(next);
            setAiOpen(false);
            toast(`已切到 ${next.model}`);
          }}
        />
      )}

      {dragging && <div className={styles.dropHint}>松开即可添加附件</div>}
    </section>
  );
}

export { ChevronDown };
