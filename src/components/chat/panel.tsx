import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Attachment, ChatAction, ChatMessage, ResumePage } from "@/types";
import { useWorkspace } from "@/store/workspace";
import { uid } from "@/lib/uid";
import { useToast } from "@/components/ui/toast-context";
import { ChevronDown, ChevronRight } from "@/components/ui/icons";
import { Composer } from "./composer";
import { toAttachment, attachmentText } from "@/lib/attach";
import { AiSettingsSheet } from "./ai-settings";
import { AssistantTurn, EmptyTurn, TypingTurn, UserTurn } from "./message";
import { keywordCount, rewriteBullet } from "@/lib/rewrite";
import { aiReady, loadAi, saveAi, streamChat, type AiSettings } from "@/lib/ai";
import { buildSystemPrompt, splitReply, toApiMessages } from "@/lib/prompt";

function buildReply(
  personName: string,
  page: ResumePage | null,
  experiences: { id: string; company: string; bullets: string[] }[],
  text: string,
  files: Attachment[],
): ChatMessage {
  const targetId = page?.experienceIds[0];
  const exp = experiences.find((item) => item.id === targetId) ?? experiences[0];
  const bullet = exp?.bullets[0];
  const fileNames = files.map((file) => file.name).join("、");

  if (files.length > 0) {
    return {
      id: uid("m"),
      role: "assistant",
      time: Date.now(),
      tool: `已登记 ${files.length} 个文件 · 提取 ${keywordCount(fileNames + text)} 个关键词`,
      content: exp && bullet
        ? `收到 ${fileNames}。本地模式还没有接入模型读不了文件内容，我先按规则改写了第 1 段的表述——真正的 JD 对齐要等模型接上。`
        : `收到 ${fileNames}。这份简历还没有经历，先在左侧经历库补一段，我再按 JD 对齐。`,
      changes:
        exp && bullet
          ? [
              {
                label: `第 1 段 · ${exp.company}`,
                before: bullet,
                after: rewriteBullet(bullet),
                target: { experienceId: exp.id, bulletIndex: 0 },
              },
            ]
          : undefined,
      actions: exp && bullet
        ? [
            { id: uid("a"), label: `应用到第 1 段`, kind: "apply" },
            { id: uid("a"), label: "换个说法", kind: "hint" },
          ]
        : undefined,
    };
  }

  return {
    id: uid("m"),
    role: "assistant",
    time: Date.now(),
    tool: "本地规则 · 主动语态与结果表述",
    content: exp && bullet
      ? `${personName} 的经历库里有 ${experiences.length} 段经历，这份简历引用了 ${page?.experienceIds.length ?? 0} 段。第 1 段开头是「负责」，改成主动表述会更像主导者。`
      : "这份简历还没有引用任何经历。先在左侧展开候选人补一段经历，再来改写。",
    changes:
      exp && bullet
        ? [
            {
              label: `第 1 段 · ${exp.company}`,
              before: bullet,
              after: rewriteBullet(bullet),
              target: { experienceId: exp.id, bulletIndex: 0 },
            },
          ]
        : undefined,
    actions: exp && bullet
      ? [
          { id: uid("a"), label: "应用到第 1 段", kind: "apply" },
          { id: uid("a"), label: "这段再具体点", kind: "hint" },
        ]
      : undefined,
  };
}

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
  const timerRef = useRef<number | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  const configured = aiReady(ai);
  const modelLabel = configured ? ai.model : "本地规则";

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
    return () => {
      if (timerRef.current) window.clearTimeout(timerRef.current);
    };
  }, [resume?.id]);

  useEffect(() => () => abortRef.current?.abort(), []);

  const send = useCallback(
    (text: string, attachments: Attachment[]) => {
      if (!person || !resume) return;
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
      const snapshotPage = page ?? resume.pages[0] ?? null;
      const patchReply = (messageId: string, patch: Partial<ChatMessage>) =>
        dispatch({
          type: "patchMessage",
          personId: snapshotPerson.id,
          resumeId: snapshotResume.id,
          messageId,
          patch,
        });

      /* ---- 接了模型：流式回答 ---- */
      if (configured && snapshotPage) {
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
        return;
      }

      /* ---- 没接模型：本地规则 ---- */
      setBusy(true);
      timerRef.current = window.setTimeout(() => {
        const reply = buildReply(
          snapshotPerson.name,
          snapshotPage,
          snapshotPerson.experiences,
          text,
          attachments,
        );
        dispatch({
          type: "appendMessage",
          personId: snapshotPerson.id,
          resumeId: snapshotResume.id,
          message: reply,
        });
        setBusy(false);
        timerRef.current = null;
      }, 900);
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
      <section className="chat">
        <div className="chat-empty">左侧还没有简历，先新建一份。</div>
      </section>
    );
  }

  return (
    <section
      className={`chat${dragging ? " dragging" : ""}`}
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
      <header className="chat-head">
        <div className="crumb">
          <span className="who">{person.name}</span>
          <ChevronRight size={9} className="sep" />
          <b>{resume.title}</b>
        </div>
        <button type="button" className="model-static" onClick={() => setAiOpen(true)}>
          {modelLabel}
        </button>
      </header>

      <div className="thread" ref={scrollRef}>
        <div className="thread-in">
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

      {dragging && <div className="drop-hint">松开即可添加附件</div>}
    </section>
  );
}

export { ChevronDown };
