import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Attachment, ChatAction, ChatMessage, ResumePage } from "@/types";
import { useWorkspace, uid } from "@/store/workspace";
import { useToast } from "@/components/ui/toast";
import { ChevronDown, ChevronRight } from "@/components/ui/icons";
import { Composer, toAttachment } from "./composer";
import { AssistantTurn, EmptyTurn, TypingTurn, UserTurn } from "./message";
import { keywordCount, rewriteBullet } from "@/lib/rewrite";

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
  const [draft, setDraft] = useState("");
  const [files, setFiles] = useState<Attachment[]>([]);
  const [model, setModel] = useState("Sonnet 4.5");
  const [dragging, setDragging] = useState(false);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const timerRef = useRef<number | null>(null);

  const messages = useMemo(() => resume?.messages ?? [], [resume]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [messages.length, busy, resume?.id]);

  useEffect(() => {
    setBusy(false);
    setFiles([]);
    setDraft("");
    return () => {
      if (timerRef.current) window.clearTimeout(timerRef.current);
    };
  }, [resume?.id]);

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
      setBusy(true);

      const snapshotPerson = person;
      const snapshotResume = resume;
      const snapshotPage = page;

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
    [person, resume, page, dispatch],
  );

  const handleAction = (message: ChatMessage, action: ChatAction) => {
    if (!person || !resume) return;
    if (action.kind === "apply") {
      const change = message.changes?.[0];
      if (!change?.target) {
        toast("这条建议没有可写入的目标");
        return;
      }
      const exp = person.experiences.find((item) => item.id === change.target!.experienceId);
      if (!exp) {
        toast("找不到对应的经历");
        return;
      }
      const bullets = [...exp.bullets];
      bullets[change.target.bulletIndex] = change.after;
      dispatch({
        type: "updateExperience",
        personId: person.id,
        expId: exp.id,
        patch: { bullets },
      });
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
      toast("已写入共享经历，这份人的所有简历都会更新");
      return;
    }
    setDraft("把这段再具体一点，补上当时的判断依据。");
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
        <span className="model-static">{model}</span>
      </header>

      <div className="thread" ref={scrollRef}>
        <div className="thread-in">
          {messages.length === 0 && !busy && <EmptyTurn name={resume.title} onPick={setDraft} />}
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
        model={model}
        onModelChange={setModel}
        onSend={send}
        busy={busy}
        draft={draft}
        onDraftConsumed={() => setDraft("")}
        files={files}
        onFilesChange={setFiles}
      />

      {dragging && <div className="drop-hint">松开即可添加附件</div>}
    </section>
  );
}

export { ChevronDown };
