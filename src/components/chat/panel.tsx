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
import { buildSystemPrompt, splitReply, toApiMessages } from "@/lib/prompt";
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

  const configured = aiReady(ai);
  const modelLabel = configured ? ai.model : "未配置模型 · 点此设置";

  const messages = useMemo(() => resume?.messages ?? [], [resume]);

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
      dispatch({ type: "appendMessage", personId: person.id, resumeId: resume.id, message: userMessage });

      const snapshotPerson = person;
      const snapshotResume = resume;
      const snapshotPage = targetPage;
      const patchReply = (messageId: string, patch: Partial<ChatMessage>) =>
        dispatch({
          type: "patchMessage",
          personId: snapshotPerson.id,
          resumeId: snapshotResume.id,
          messageId,
          patch,
        });

      // ---- 流式回答：到这里一定已配置模型，且当前有页面 ----
      setBusy(true);
      const replyId = uid("m");
      dispatch({
        type: "appendMessage",
        personId: snapshotPerson.id,
        resumeId: snapshotResume.id,
        message: {
          id: replyId,
          role: "assistant",
          time: Date.now(),
          content: "",
          tool: `${ai.model} · 正在读简历`,
        },
      });

      const controller = new AbortController();
      abortRef.current?.abort();
      abortRef.current = controller;

      let acc = "";
      let flushedAt = 0;
      const flush = (force: boolean) => {
        const now = Date.now();
        if (!force && now - flushedAt < 70) return;
        flushedAt = now;
        patchReply(replyId, { content: acc });
      };

      const messagesForApi = toApiMessages(
        buildSystemPrompt(snapshotPerson, snapshotPage, snapshotResume.title),
        snapshotResume.messages.map((item) => ({ role: item.role, content: item.content })),
        text,
        attachments.map((file) => ({ name: file.name, text: attachmentText(file.id) })),
      );

      void (async () => {
        try {
          await streamChat({
            settings: ai,
            messages: messagesForApi,
            signal: controller.signal,
            onDelta: (delta) => {
              acc += delta;
              flush(false);
            },
          });
          const { content, changes } = splitReply(acc, snapshotPerson);
          patchReply(replyId, {
            content: content || "（模型没有返回正文）",
            tool: `${ai.model} · 完成`,
            changes: changes.length ? changes : undefined,
            actions: changes.length
              ? [
                  { id: uid("a"), label: `应用 ${changes.length} 处改写`, kind: "apply" },
                  { id: uid("a"), label: "再收一收", kind: "hint" },
                ]
              : undefined,
          });
        } catch (error) {
          if (controller.signal.aborted) return;
          const reason = error instanceof Error ? error.message : String(error);
          patchReply(replyId, {
            content: `调用失败：${reason}`,
            tool: `${ai.model} · 调用失败`,
            actions: [{ id: uid("a"), label: "去检查设置", kind: "hint" }],
          });
          toast("模型调用失败，检查接口地址与密钥");
        } finally {
          setBusy(false);
          abortRef.current = null;
        }
      })();
    },
    [person, resume, page, dispatch, ai, configured, toast],
  );

  const handleAction = (message: ChatMessage, action: ChatAction) => {
    if (!person || !resume) return;
    if (action.kind === "apply") {
      const changes = (message.changes ?? []).filter((change) => change.target);
      if (!changes.length) {
        toast("这条建议没有可写入的目标");
        return;
      }

      // 同一段经历的多处改写要合并成一次写入，否则后一次会覆盖前一次
      const patches = new Map<string, { bullets?: string[]; bulletsEn?: string[] }>();
      for (const change of changes) {
        const target = change.target;
        if (!target) continue;
        const exp = person.experiences.find((item) => item.id === target.experienceId);
        if (!exp) continue;
        const bucket = patches.get(exp.id) ?? {};
        if (target.lang === "en") {
          const list = bucket.bulletsEn ?? [...(exp.bulletsEn ?? [])];
          list[target.bulletIndex] = change.after;
          bucket.bulletsEn = list;
        } else {
          const list = bucket.bullets ?? [...exp.bullets];
          list[target.bulletIndex] = change.after;
          bucket.bullets = list;
        }
        patches.set(exp.id, bucket);
      }

      if (!patches.size) {
        toast("找不到对应的经历，可能在经历库里删掉了");
        return;
      }

      for (const [expId, patch] of patches) {
        dispatch({ type: "updateExperience", personId: person.id, expId, patch });
      }
      dispatch({
        type: "patchMessage",
        personId: person.id,
        resumeId: resume.id,
        messageId: message.id,
        patch: {
          actions: undefined,
          tool: `${message.tool ?? "改写"} · 已写入经历库`,
        },
      });
      toast(`已写入 ${changes.length} 处，名下所有简历同步更新`);
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
      <section className={styles.chat} data-print="hide">
        <div className={styles.empty}>左侧还没有简历，先新建一份。</div>
      </section>
    );
  }

  return (
    <section
      className={styles.chat}
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
          <span className={styles.who}>{person.name}</span>
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
          {busy && <TypingTurn />}
        </div>
      </div>

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
