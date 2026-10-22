import { useEffect, useRef, useState } from "react";
import {
  ArrowUp,
  BrainCircuit,
  Check,
  ChevronDown,
  Eraser,
  FileText,
  GitCompare,
  History,
  MoreHorizontal,
  Paperclip,
  Sparkles,
  Undo2,
  Upload,
  X,
} from "lucide-react";
import { toast } from "sonner";

import { FileGlyph, extOf, formatBytes } from "@/components/file-glyph";
import { MarkdownLite } from "@/lib/markdown";
import { cn } from "@/lib/utils";
import type { ChatAttachment, ChatMessage, ResumeItem } from "@/types";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

const SUGGESTIONS = [
  "按 JD 重写工作经历",
  "生成英文版",
  "把成果都量化",
  "帮我写一段自我介绍",
];

const MODELS = [
  { id: "sonnet", name: "Claude Sonnet 4.5", hint: "推荐 · 长上下文" },
  { id: "gpt", name: "GPT-5.2", hint: "结构化改写" },
  { id: "gemini", name: "Gemini 3 Pro", hint: "多模态" },
  { id: "deepseek", name: "DeepSeek V4", hint: "中文友好" },
  { id: "ollama", name: "本地模型 · Ollama", hint: "完全离线" },
];

interface ChatPanelProps {
  resume: ResumeItem;
  pageLabel: string;
  pageCount: number;
}

function uid() {
  return `${Date.now()}-${Math.random().toString(16).slice(2, 8)}`;
}

function nowTime() {
  return new Date().toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

export function ChatPanel({ resume, pageLabel, pageCount }: ChatPanelProps) {
  const [threads, setThreads] = useState<Record<string, ChatMessage[]>>({});
  const [text, setText] = useState("");
  const [attachments, setAttachments] = useState<ChatAttachment[]>([]);
  const [pending, setPending] = useState(false);
  const [dragActive, setDragActive] = useState(false);
  const [model, setModel] = useState(MODELS[0].name);

  const scrollRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const dragDepth = useRef(0);
  const timerRef = useRef<number | null>(null);

  const messages = threads[resume.id] ?? resume.messages;

  useEffect(() => {
    setText("");
    setAttachments([]);
    setPending(false);
    if (timerRef.current) window.clearTimeout(timerRef.current);
  }, [resume.id]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
  }, [messages.length, pending]);

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
  }, [text]);

  useEffect(() => {
    return () => {
      if (timerRef.current) window.clearTimeout(timerRef.current);
    };
  }, []);

  const addFiles = (files: FileList | File[]) => {
    const incoming = Array.from(files).map<ChatAttachment>((file) => ({
      id: uid(),
      name: file.name,
      size: file.size,
      ext: extOf(file.name),
    }));
    if (incoming.length === 0) return;
    setAttachments((prev) => [...prev, ...incoming]);
    toast(`已添加 ${incoming.length} 个文件`, {
      description: incoming.map((item) => item.name).join("、"),
    });
  };

  const send = () => {
    const content = text.trim();
    if (!content && attachments.length === 0) return;

    const userMessage: ChatMessage = {
      id: uid(),
      role: "user",
      time: nowTime(),
      content: content || "先看一下这些文件，再告诉我可以在简历里怎么用。",
      attachments: attachments.length > 0 ? attachments : undefined,
    };

    const nextThread = [...messages, userMessage];
    setThreads((prev) => ({ ...prev, [resume.id]: nextThread }));
    setText("");
    setAttachments([]);
    setPending(true);

    const fileNames = userMessage.attachments?.map((item) => item.name) ?? [];
    timerRef.current = window.setTimeout(() => {
      const reply: ChatMessage = {
        id: uid(),
        role: "assistant",
        time: nowTime(),
        content:
          fileNames.length > 0
            ? `收到，已把 **${fileNames[0]}**${fileNames.length > 1 ? ` 等 ${fileNames.length} 个文件` : ""} 加入上下文。\n\n我会先提取关键信息，再对照右侧「${pageLabel}」逐段调整，改动会以小块的形式同步过去，方便你确认。`
            : `收到。我先按这个方向调整右侧「${pageLabel}」，改动控制在 3 处以内，完成后你可以在简历上直接改。`,
      };
      setPending(false);
      setThreads((prev) => ({ ...prev, [resume.id]: [...(prev[resume.id] ?? []), reply] }));
    }, 900);
  };

  const clearThread = () => {
    setThreads((prev) => ({ ...prev, [resume.id]: [] }));
    toast("已清空当前会话");
  };

  const handleDrop = (event: React.DragEvent) => {
    event.preventDefault();
    dragDepth.current = 0;
    setDragActive(false);
    if (event.dataTransfer.files.length > 0) {
      addFiles(event.dataTransfer.files);
    }
  };

  return (
    <section
      className="relative flex h-full min-w-0 flex-col bg-background"
      onDragEnter={(event) => {
        event.preventDefault();
        dragDepth.current += 1;
        setDragActive(true);
      }}
      onDragOver={(event) => {
        event.preventDefault();
        event.dataTransfer.dropEffect = "copy";
      }}
      onDragLeave={(event) => {
        event.preventDefault();
        dragDepth.current -= 1;
        if (dragDepth.current <= 0) {
          dragDepth.current = 0;
          setDragActive(false);
        }
      }}
      onDrop={handleDrop}
    >
      <div className="flex h-[52px] shrink-0 items-center gap-1 border-b px-3.5">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <h2 className="truncate text-[12.5px] font-semibold">{resume.name}</h2>
            <span className="rounded bg-brand/10 px-1 py-px text-[8.5px] font-bold uppercase tracking-[0.1em] text-brand">
              AI
            </span>
          </div>
          <p className="mt-0.5 truncate text-[10px] text-muted-foreground">
            上下文 {pageCount} 页 · {messages.length} 条消息
          </p>
        </div>

        <button
          type="button"
          onClick={() => toast("会话历史", { description: "每个简历只保留一条对话线，历史版本会在功能阶段支持回溯。" })}
          aria-label="会话历史"
          className="grid size-7 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        >
          <History className="size-3.5" />
        </button>

        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              aria-label="会话更多操作"
              className="grid size-7 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
            >
              <MoreHorizontal className="size-3.5" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-44">
            <DropdownMenuItem className="text-[12px]" onSelect={clearThread}>
              <Eraser className="size-3.5" />
              清空当前会话
            </DropdownMenuItem>
            <DropdownMenuItem
              className="text-[12px]"
              onSelect={() => toast("导出对话", { description: "会导出为 Markdown 文件。" })}
            >
              <FileText className="size-3.5" />
              导出对话记录
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              className="text-[12px]"
              onSelect={() => toast("会话设置", { description: "模型、温度、上下文范围。" })}
            >
              <BrainCircuit className="size-3.5" />
              会话设置
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <div className="relative min-h-0 flex-1">
        <div ref={scrollRef} className="h-full overflow-y-auto">
          {messages.length === 0 ? (
            <div className="flex h-full flex-col items-center justify-center gap-3 px-8 pb-12 text-center">
              <div className="grid size-11 place-items-center rounded-xl bg-brand/10 text-brand">
                <Sparkles className="size-5" />
              </div>
              <div>
                <p className="text-[13px] font-medium">开始一段新对话</p>
                <p className="mx-auto mt-1 max-w-[300px] text-[11.5px] leading-relaxed text-muted-foreground">
                  描述你想改的地方，或上传 JD、旧简历、作品集 —— 我会读完再动手。
                </p>
              </div>
              <div className="mt-1 flex max-w-[380px] flex-wrap justify-center gap-1.5">
                {["上传旧简历，先帮我搭结构", "按 JD 生成一版", "把经历都量化"].map((item) => (
                  <button
                    key={item}
                    type="button"
                    onClick={() => {
                      setText(item);
                      textareaRef.current?.focus();
                    }}
                    className="rounded-full border bg-background px-2.5 py-1 text-[10.5px] text-muted-foreground transition-colors hover:border-brand/40 hover:text-brand"
                  >
                    {item}
                  </button>
                ))}
              </div>
            </div>
          ) : (
            <div className="mx-auto flex w-full max-w-[620px] flex-col gap-5 px-4 py-5">
              <div className="flex items-center gap-2">
                <span className="h-px flex-1 bg-border" />
                <span className="text-[9.5px] uppercase tracking-[0.16em] text-muted-foreground/70">
                  今天
                </span>
                <span className="h-px flex-1 bg-border" />
              </div>

              {messages.map((message, index) =>
                message.role === "assistant" ? (
                  <AssistantMessage
                    key={message.id}
                    message={message}
                    showSuggestions={index === 0}
                    onSuggestion={(value) => {
                      setText(value);
                      textareaRef.current?.focus();
                    }}
                  />
                ) : (
                  <UserMessage key={message.id} message={message} />
                ),
              )}

              {pending && (
                <div className="flex animate-in-soft gap-2.5">
                  <AiAvatar />
                  <div className="flex h-8 items-center gap-1 rounded-lg border bg-muted/40 px-3">
                    {[0, 1, 2].map((i) => (
                      <span
                        key={i}
                        className="size-1.5 animate-bounce rounded-full bg-muted-foreground/50"
                        style={{ animationDelay: `${i * 120}ms`, animationDuration: "900ms" }}
                      />
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}
        </div>
        <div className="pointer-events-none absolute inset-x-0 top-0 h-5 bg-gradient-to-b from-background to-transparent" />
      </div>

      <div className="shrink-0 border-t bg-background px-3 pb-3 pt-2.5">
        <div className="mx-auto w-full max-w-[620px]">
          <div
            className={cn(
              "rounded-xl border bg-background shadow-[0_1px_2px_rgba(0,0,0,0.03)] transition-shadow",
              "focus-within:border-brand/40 focus-within:ring-2 focus-within:ring-brand/10",
            )}
          >
            {attachments.length > 0 && (
              <div className="flex flex-wrap gap-1.5 border-b px-2.5 py-2">
                {attachments.map((file) => (
                  <div
                    key={file.id}
                    className="flex max-w-[220px] items-center gap-1.5 rounded-md border bg-muted/30 py-1 pl-1 pr-1.5"
                  >
                    <FileGlyph name={file.name} size="sm" />
                    <div className="min-w-0">
                      <p className="truncate text-[11px] font-medium leading-4">{file.name}</p>
                      <p className="text-[9px] leading-3 text-muted-foreground">
                        {formatBytes(file.size)}
                      </p>
                    </div>
                    <button
                      type="button"
                      aria-label={`移除 ${file.name}`}
                      onClick={() =>
                        setAttachments((prev) => prev.filter((item) => item.id !== file.id))
                      }
                      className="ml-0.5 grid size-4 shrink-0 place-items-center rounded text-muted-foreground hover:bg-accent hover:text-foreground"
                    >
                      <X className="size-3" />
                    </button>
                  </div>
                ))}
              </div>
            )}

            <textarea
              ref={textareaRef}
              value={text}
              onChange={(event) => setText(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  send();
                }
              }}
              rows={1}
              placeholder="描述你想怎么改，或把 JD / 旧简历 / 作品集拖进来…"
              className="min-h-[52px] w-full resize-none bg-transparent px-3 py-2.5 text-[12.5px] leading-relaxed outline-none placeholder:text-muted-foreground/70"
            />

            <div className="flex items-center gap-1 px-2 pb-2">
              <input
                ref={fileInputRef}
                type="file"
                multiple
                className="hidden"
                onChange={(event) => {
                  if (event.target.files) addFiles(event.target.files);
                  event.target.value = "";
                }}
              />
              <button
                type="button"
                onClick={() => fileInputRef.current?.click()}
                className="flex h-7 items-center gap-1.5 rounded-md px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              >
                <Paperclip className="size-3.5" />
                附件
              </button>
              <div className="mx-1 h-4 w-px bg-border" />
              <span className="flex h-6 items-center gap-1 rounded border border-dashed px-1.5 text-[10px] text-muted-foreground">
                <FileText className="size-3" />
                当前：{pageLabel}
              </span>

              <div className="ml-auto flex items-center gap-1.5">
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <button
                      type="button"
                      className="flex h-7 items-center gap-1 rounded-md px-1.5 text-[10.5px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                    >
                      <BrainCircuit className="size-3.5 text-brand" />
                      <span className="max-w-[110px] truncate">
                        {model.replace("Claude ", "")}
                      </span>
                      <ChevronDown className="size-3 opacity-60" />
                    </button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="w-56">
                    <DropdownMenuLabel className="text-[10px] font-normal uppercase tracking-wide text-muted-foreground">
                      选择模型
                    </DropdownMenuLabel>
                    {MODELS.map((item) => (
                      <DropdownMenuItem
                        key={item.id}
                        className="text-[12px]"
                        onSelect={() => setModel(item.name)}
                      >
                        <div className="min-w-0 flex-1">
                          <p className="truncate text-[12px]">{item.name}</p>
                          <p className="text-[10px] text-muted-foreground">{item.hint}</p>
                        </div>
                        {model === item.name && <Check className="size-3.5 text-brand" />}
                      </DropdownMenuItem>
                    ))}
                    <DropdownMenuSeparator />
                    <p className="px-2 py-1.5 text-[10px] leading-relaxed text-muted-foreground">
                      密钥与本地模型地址在设置里配置，仅保存在本机。
                    </p>
                  </DropdownMenuContent>
                </DropdownMenu>

                <button
                  type="button"
                  onClick={send}
                  disabled={!text.trim() && attachments.length === 0}
                  aria-label="发送"
                  className="grid size-8 place-items-center rounded-lg bg-primary text-primary-foreground shadow-sm transition-all hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-30"
                >
                  <ArrowUp className="size-4" />
                </button>
              </div>
            </div>
          </div>

          <div className="mt-2 flex flex-wrap items-center justify-center gap-x-2 gap-y-1 px-1 text-center text-[9.5px] text-muted-foreground/80">
            <span>Enter 发送 · Shift + Enter 换行</span>
            <span className="size-0.5 rounded-full bg-muted-foreground/40" />
            <span>支持任意格式文件</span>
          </div>
        </div>
      </div>

      {dragActive && (
        <div className="pointer-events-none absolute inset-2 z-20 flex flex-col items-center justify-center gap-2 rounded-xl border-2 border-dashed border-brand/50 bg-background/85 backdrop-blur-sm">
          <div className="grid size-12 place-items-center rounded-xl bg-brand/10 text-brand">
            <Upload className="size-5" />
          </div>
          <p className="text-[13px] font-medium">松手上传</p>
          <p className="text-[11px] text-muted-foreground">任意格式文件都会挂到这个会话</p>
        </div>
      )}
    </section>
  );
}

function AiAvatar() {
  return (
    <div className="mt-0.5 grid size-6 shrink-0 place-items-center rounded-md bg-gradient-to-br from-[#6d5efc] to-[#a78bfa] text-white shadow-sm">
      <Sparkles className="size-3.5" />
    </div>
  );
}

function AssistantMessage({
  message,
  showSuggestions,
  onSuggestion,
}: {
  message: ChatMessage;
  showSuggestions: boolean;
  onSuggestion: (value: string) => void;
}) {
  return (
    <div className="flex animate-rise gap-2.5">
      <AiAvatar />
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-1.5">
          <span className="text-[11.5px] font-semibold">Sharp AI</span>
          <span className="text-[9.5px] tabular-nums text-muted-foreground">{message.time}</span>
        </div>

        <div className="mt-1.5 text-[12.5px] text-foreground/90">
          <MarkdownLite text={message.content} />
        </div>

        {message.fileMeta && (
          <div className="mt-2.5 rounded-lg border bg-muted/30 p-2.5">
            <div className="flex items-start gap-2.5">
              <FileGlyph name={message.fileMeta.name} size="md" />
              <div className="min-w-0 flex-1">
                <p className="truncate text-[11.5px] font-medium">{message.fileMeta.name}</p>
                <p className="mt-0.5 text-[9.5px] text-muted-foreground">
                  已解析 {message.fileMeta.pages} 页 · 提取 {message.fileMeta.keywords.length} 个关键词
                </p>
                <div className="mt-1.5 flex flex-wrap gap-1">
                  {message.fileMeta.keywords.map((keyword) => (
                    <span
                      key={keyword}
                      className="rounded bg-background px-1.5 py-0.5 text-[9.5px] text-muted-foreground ring-1 ring-border"
                    >
                      {keyword}
                    </span>
                  ))}
                </div>
              </div>
            </div>
          </div>
        )}

        {message.changes && message.changes.length > 0 && (
          <div className="mt-2.5 overflow-hidden rounded-lg border bg-background/60">
            <div className="flex items-center gap-1.5 border-b bg-muted/40 px-2.5 py-2">
              <GitCompare className="size-3.5 text-brand" />
              <span className="min-w-0 flex-1 truncate text-[11px] font-medium">
                {message.changesTitle ?? "已更新简历"}
              </span>
              <span className="rounded bg-brand/10 px-1.5 py-px text-[9px] font-medium text-brand">
                {message.changes.length} 处修改
              </span>
            </div>
            <ul className="divide-y">
              {message.changes.map((change) => (
                <li key={change.label} className="flex gap-2 px-2.5 py-2">
                  <Check className="mt-[3px] size-3 shrink-0 text-emerald-500" />
                  <div className="min-w-0">
                    <p className="text-[11.5px] font-medium leading-4">{change.label}</p>
                    <p className="mt-0.5 text-[10.5px] leading-relaxed text-muted-foreground">
                      {change.detail}
                    </p>
                  </div>
                </li>
              ))}
            </ul>
            <div className="flex items-center gap-1 border-t px-1.5 py-1">
              <button
                type="button"
                onClick={() => toast("查看差异", { description: "会在右侧打开逐句 diff 视图。" })}
                className="rounded px-1.5 py-1 text-[10.5px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              >
                查看差异
              </button>
              <button
                type="button"
                onClick={() => toast("已撤销本次修改（演示）")}
                className="flex items-center gap-1 rounded px-1.5 py-1 text-[10.5px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              >
                <Undo2 className="size-3" />
                撤销
              </button>
            </div>
          </div>
        )}

        {showSuggestions && (
          <div className="mt-2.5 flex flex-wrap gap-1.5">
            {SUGGESTIONS.map((suggestion) => (
              <button
                key={suggestion}
                type="button"
                onClick={() => onSuggestion(suggestion)}
                className="rounded-full border bg-background px-2.5 py-1 text-[10.5px] text-muted-foreground transition-colors hover:border-brand/40 hover:text-brand"
              >
                {suggestion}
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function UserMessage({ message }: { message: ChatMessage }) {
  return (
    <div className="flex animate-rise flex-col items-end gap-1.5">
      {message.attachments && message.attachments.length > 0 && (
        <div className="flex max-w-[88%] flex-wrap justify-end gap-1.5">
          {message.attachments.map((file) => (
            <div
              key={file.id}
              className="flex w-[216px] items-center gap-2 rounded-lg border bg-background px-2 py-1.5 shadow-xs"
            >
              <FileGlyph name={file.name} size="md" />
              <div className="min-w-0 flex-1">
                <p className="truncate text-[11px] font-medium leading-4">{file.name}</p>
                <p className="mt-0.5 text-[9px] text-muted-foreground">
                  {file.ext.toUpperCase() || "文件"} · {formatBytes(file.size)}
                </p>
              </div>
            </div>
          ))}
        </div>
      )}
      <div className="max-w-[88%] rounded-2xl rounded-tr-md bg-muted px-3.5 py-2.5 text-[12.5px] leading-relaxed text-foreground/90">
        {message.content}
      </div>
      <span className="pr-1 text-[9.5px] tabular-nums text-muted-foreground/80">
        {message.time}
      </span>
    </div>
  );
}
