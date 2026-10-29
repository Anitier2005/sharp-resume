import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Asset, Attachment, ChatAction, ChatMessage } from "@/types";
import { useWorkspace } from "@/store/workspace";
import { uid } from "@/lib/uid";
import { useToast } from "@/components/ui/toast-context";
import { ChevronDown, ChevronRight } from "@/components/ui/icons";
import { Composer } from "./composer";
import { Disclosure } from "@/components/ui";
import { toAttachment, stripPreview, attachmentText, attachmentImage, ensureOcrText, isImage } from "@/lib/attach";
import { attachAtEnd } from "@/lib/inline-tokens";
import { modelById } from "@/lib/model-config";
import { diff, flatten, renderDelta, type Flat } from "@/lib/changes";
import { entriesOf } from "@/lib/blocks";
import { skillsPrompt } from "@/lib/skills";
import { AiSettingsSheet } from "./ai-settings";
import { AssistantTurn, EmptyTurn, SystemTurn, TypingTurn, UserTurn } from "./message";
import { aiReady, loadAi, saveAi, slotAi, streamChat, type AiSettings } from "@/lib/ai";
import { findModel, listProfiles } from "@/lib/model-config";
import { buildContext, buildSystemPrompt, splitReply, toApiMessages } from "@/lib/prompt";
import { isTauri } from "@/lib/storage";
import { applyAgentActions, askSessionFromLayout, registerChatBridge, type AskQuestion } from "@/lib/agent-actions";
import { submitLayout } from "@/lib/layout";
import { AskForm } from "./ask-form";
import { displayName } from "@/types";
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openSettingsWindow, type SettingsChanged } from "@/lib/settings-window";
import styles from "./chat.module.css";

/**
 * 没有配置模型时不产生任何"回答"。
 *
 * 这里曾经用 lib/rewrite.ts 的本地规则拼一段看起来像 AI 的回复
 * （还带"已提取 N 个关键词"这类并未真正计算的工具行），用户会误以为是模型输出。
 * 现在改为：不写消息、不伪造内容，直接把设置面板推到用户面前。
 */

interface SendOptions {
  /**
   * 内部往返：排版 agent 的提问与会话 agent 的答复都进模型上下文，
   * 但**不进对话流** —— 用户要看到的是自己与会话 agent 的对话。
   */
  silent?: boolean;
  /** 这一轮是冲着哪一块问的：答复回投给排版 agent 时要带上它，否则对不上号 */
  blockId?: string;
}

export function ChatPanel() {
  const { state, person, resume, page, dispatch, setActivePageId, applyOps, openPerson, openResume } =
    useWorkspace();
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  /**
   * 当前这一轮是不是内部的（排版提问）。
   *
   * busy 只管「同一时刻只能有一轮」，打字体、思考过程、停止按钮是给用户看的，
   * 内部轮一律不渲染；两者揉成一个状态就得在每一处拿出来再猜一遍。
   */
  const [silentBusy, setSilentBusy] = useState(false);
  const [input, setInput] = useState("");
  const [files, setFiles] = useState<Attachment[]>([]);
  const [ai, setAi] = useState<AiSettings>(() => loadAi());
  const [aiOpen, setAiOpen] = useState(false);

  /**
   * 模型设置在独立窗口里。窗口改完会推 `settings:changed` —— 这里重读 localStorage，
   * 否则本面板还拿着旧模型，下一条消息会发给已经换掉的接口。
   * 浏览器调试开不了窗口，退回老的小窗（只服务开发）。
   */
  const openModelSettings = async () => {
    if (!(await openSettingsWindow("model"))) setAiOpen(true);
  };

  useEffect(() => {
    if (!isTauri()) return;
    let off: (() => void) | undefined;
    void listen<SettingsChanged>("settings:changed", (event) => {
      if (event.payload.kind === "ai") setAi(loadAi());
    }).then((fn) => {
      off = fn;
    });
    return () => off?.();
  }, []);
  const [dragging, setDragging] = useState(false);
  /** 模型发来的表单；非空时盖在会话上 */
  const [ask, setAsk] = useState<AskQuestion[] | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  /** send 的自我引用：内部提问是在「收到指令时」再发一轮请求，写在 send 内部就成了递归 */
  const sendRef = useRef<
    ((text: string, attachments: Attachment[], forPage?: string, opts?: SendOptions) => void) | null
  >(null);
  /** 上一轮发出时的页面快照（摊平后），用来算「这一轮之前你手改了什么」 */
  const baseRef = useRef<{ key: string; flat: Flat } | null>(null);
  /** 消息真相源在 Rust（chat_history 拉投影）；浏览器调试模式才退回工作区 */
  const [remote, setRemote] = useState<ChatMessage[]>([]);
  const [reasoning, setReasoning] = useState("");
  const desktop = isTauri();

  const configured = aiReady(ai);
  /**
   * 显示「模型名：配置名」。
   * 老数据没有 profileId：那时每个模型只有一套配置，把第一条当作它。
   */
  const activeSpec = findModel(ai.model);
  const activeProfile = activeSpec
    ? (listProfiles(activeSpec).find((item) => item.id === ai.profileId) ??
      listProfiles(activeSpec)[0])
    : undefined;
  const modelLabel = configured
    ? `${activeSpec?.label ?? ai.model}：${activeProfile?.label ?? "默认"}`
    : "未配置模型 · 点此设置";

  /** 压缩摘要的接入参数（没单独配就是主模型，后端也会兜一层） */
  const compactFor = () => {
    const slot = slotAi("compact");
    return { baseUrl: slot.baseUrl, apiKey: slot.apiKey, model: slot.model };
  };

  /** 底栏直接换「模型 + 配置」：网关、密钥、温度都跟着那套配置走 */
  const pickModel = (packed: string) => {
    const [modelId, profileId] = packed.split("|");
    const spec = findModel(modelId);
    if (!spec) return;
    const list = listProfiles(spec);
    const profile = list.find((item) => item.id === profileId) ?? list[0];
    if (!profile) return;
    const next = {
      ...ai,
      model: modelId,
      profileId: profile.id,
      baseUrl: profile.baseUrl.trim() || spec.baseUrl,
      apiKey: profile.apiKey || ai.apiKey,
      contextWindow: profile.contextWindow ?? spec.contextWindow,
      temperature: profile.temperature ?? spec.defaultTemperature,
      reasoning: profile.reasoning ?? spec.reasoning[0] ?? undefined,
    };
    setAi(next);
    saveAi(next);
    toast(`已切换到 ${spec.label}：${profile.label}`);
  };

  const messages = useMemo(
    () => (desktop ? remote : person?.messages ?? []),
    [desktop, remote, person],
  );

  /**
   * 对话流只放用户与会话 agent 的往返。
   *
   * 内部消息（排版提问与它的答复）仍在 `messages` 里 —— 那是模型上下文的一部分，
   * 下一轮它还看得见；这里只是决定「用户能看到什么」，所以过滤放在渲染前一步。
   */
  const visible = useMemo(() => messages.filter((item) => !item.internal), [messages]);

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
  }, [visible.length, busy, resume?.id]);

  useEffect(() => {
    setBusy(false);
    setSilentBusy(false);
    setFiles([]);
    setInput("");
  }, [resume?.id]);

  useEffect(() => () => abortRef.current?.abort(), []);

  /** 本地投影打补丁（Tauri 模式下消息真相在 Rust，这里只是显示） */
  const patchLocal = useCallback((id: string, patch: Partial<ChatMessage>) => {
    setRemote((prev) => prev.map((item) => (item.id === id ? { ...item, ...patch } : item)));
  }, []);

  /**
   * 会话 agent 说「这页要重排」时，交给页排版 agent。
   *
   * 只投递、不等结果：排版是常驻的后台工作，可能中途被更多输入打断，
   * 也可能连着跑好几轮。结果通过 `layout:applied` 事件回到 provider。
   * 排版本该由页 agent 负责，会话这边只传意图，不碰样式。
   */
  const runLayoutFor = useCallback(
    (pageId: string, brief: string) => {
      if (!person || !resume) return;
      const target = resume.pages.find((item) => item.id === pageId) ?? page;
      if (!target) return;
      void submitLayout({
        person,
        resume,
        page: target,
        state,
        kind: "request",
        text: brief.trim() || "请检查这一页的版式。",
      });
    },
    [person, resume, page, state],
  );

  /**
   * 排版 agent 提问：交给 lib/agent-actions 的统一入口跑一轮内部往返。
   *
   * 提问与答复都不进对话流（silent），答复在那一轮结束后回投排版信箱；
   * 5 秒防重和文案都在 agent-actions 里，面板只补「第几页」需要的页序。
   */
  const askFromLayout = useCallback(
    (pageId: string, question: string, blockId?: string) => {
      askSessionFromLayout({ pages: resume?.pages ?? [], pageId, question, blockId });
    },
    [resume],
  );

  const send = useCallback(
    (text: string, attachments: Attachment[], forPage?: string, opts?: SendOptions) => {
      if (!person || !resume) return;
      const silent = opts?.silent === true;

      // 没有模型就到此为止：不发消息、不伪造回答，直接引导去配置
      if (!configured) {
        // 内部往返是后台发起的，把设置面板顶到用户面前会很突兀
        if (silent) return;
        void openModelSettings();
        toast("先配置模型才能对话");
        return;
      }
      // forPage：排版 agent 问的是「它当时在排的那一页」，可能不是用户此刻看的那页
      const targetPage =
        (forPage ? resume.pages.find((item) => item.id === forPage) : null) ??
        page ??
        resume.pages[0] ??
        null;
      if (!targetPage) {
        if (silent) return;
        toast("这份简历还没有页面，先在右边新增一页");
        return;
      }

      // 变更感知：把右侧手改的地方算成结构化差异，和材料一起交给模型
      const sceneKey = `${person.id}::${targetPage.id}`;
      const current = flatten(person, targetPage);
      const deltas = baseRef.current?.key === sceneKey ? diff(baseRef.current.flat, current) : [];
      baseRef.current = { key: sceneKey, flat: current };
      const context = [buildContext(person, targetPage), renderDelta(deltas)]
        .filter(Boolean)
        .join("\n\n");

      const userMessage: ChatMessage = {
        id: uid("m"),
        role: "user",
        content: text,
        time: Date.now(),
        attachments: attachments.length ? attachments.map(stripPreview) : undefined,
        // 内部消息：进模型上下文，但不进对话流（见 types 里 ChatMessage.internal）
        ...(silent ? { internal: true } : {}),
      };
      const replyId = uid("m");
      const draft: ChatMessage = {
        id: replyId,
        role: "assistant",
        time: Date.now(),
        content: "",
        tool: `${ai.model} · 正在读简历`,
        ...(silent ? { internal: true } : {}),
      };

      /**
       * 答复回投给排版 agent。
       *
       * 提问与答复都不进对话流，用户看不见 —— 但排版 agent 拿不到答复就等于白问，
       * 所以这一轮结束后把正文重新投进那一页的排版信箱。浏览器模式没有信箱，
       * submitLayout 自己会跳过，这里不用分支。
       */
      const answerBack = (content: string) => {
        if (!silent) return;
        const body = content.trim();
        if (!body) return;
        const where = opts?.blockId ? `（块 ${opts.blockId}）` : "";
        void submitLayout({
          person,
          resume,
          page: targetPage,
          state,
          kind: "request",
          text: `会话 agent 针对你上一个问题的答复${where}：\n${body}`,
        });
      };

      setBusy(true);
      setSilentBusy(silent);
      setReasoning("");
      abortRef.current = new AbortController();

      /* ---------- 浏览器调试模式：保持原有工作区流程 ---------- */
      if (!desktop) {
        // 内部消息照旧落库（带 internal），模型下一轮还看得见，只是渲染时被过滤掉
        dispatch({ type: "appendMessage", personId: person.id, message: userMessage });
        dispatch({ type: "appendMessage", personId: person.id, message: draft });
        const controller = abortRef.current;
        let acc = "";
        void (async () => {
          try {
            // 图片的两条通道在这里分流：
            //   模型有视觉 → data URL 直传（走协议的 image_url 段）
            //   没有视觉   → OCR 转文字混进附件正文（慢几秒，只对图片跑）
            const modelHasVision = modelById(ai.model)?.vision === true;
            const imageUrls = modelHasVision
              ? attachments
                  .filter((file) => isImage(file.ext))
                  .map((file) => attachmentImage(file.id))
                  .filter((url): url is string => Boolean(url))
              : [];
            const attachmentBlocks = await Promise.all(
              attachments.map(async (file) => ({
                name: file.name,
                text:
                  attachmentText(file.id) ??
                  (!modelHasVision && isImage(file.ext) ? await ensureOcrText(file.id) : undefined),
              })),
            );
            await streamChat({
              settings: ai,
              messages: toApiMessages(
                buildSystemPrompt(context),
                person.messages.map((item) => ({ role: item.role, content: item.content })),
                text,
                attachmentBlocks,
                imageUrls,
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
            answerBack(content);
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
            setSilentBusy(false);
          }
        })();
        return;
      }

      /* ---------- 桌面：消息交给 Rust，上下文与裁剪也在那边 ---------- */
      // 内部往返不进 remote：用户看到的对话流里不该出现提问与答复这两条
      if (!silent) setRemote((prev) => [...prev, userMessage, draft]);

      void (async () => {
        try {
          await invoke("chat_push", { key: person.id, message: userMessage });

          const channel = new Channel<{
            kind: string;
            delta?: string;
            reasoning?: string;
            message?: string;
            tool?: string;
            detail?: string;
          }>();
          let acc = "";
          let flushed = 0;
          /** 本轮调过的工具，结束后写进这条消息的 tool 行，回放时看得出它读过什么 */
          const used: string[] = [];
          channel.onmessage = (chunk) => {
            if (chunk.kind === "delta" && chunk.delta) {
              acc += chunk.delta;
              // 内部轮没有可见的占位气泡，边流边补投影只是空转
              if (silent) return;
              const now = Date.now();
              if (now - flushed > 70) {
                flushed = now;
                patchLocal(replyId, { content: acc });
              }
            } else if (chunk.kind === "reasoning" && chunk.reasoning) {
              // 思考过程是给用户看的，内部轮不渲染也不必攒
              if (silent) return;
              setReasoning((prev) => prev + chunk.reasoning!);
            } else if (chunk.kind === "tool_start" && chunk.tool) {
              if (!used.includes(chunk.tool)) used.push(chunk.tool);
              if (silent) return;
              patchLocal(replyId, { tool: `${chunk.tool} · ${chunk.detail ?? "执行中"}` });
            }
          };

          const sent = await invoke<{ id: string; actions: unknown[]; assets?: Asset[] }>("chat_send", {
            args: {
              key: person.id,
              context,
              extra: skillsPrompt(),
              baseUrl: ai.baseUrl,
              apiKey: ai.apiKey,
              model: ai.model,
              temperature: ai.temperature,
              reasoning: ai.reasoning ?? null,
              contextWindow: ai.contextWindow ?? null,
              // 压缩摘要用「压缩」用途的模型；没配时后端自动沿用主模型
              compact: compactFor(),
              resumeId: resume.id,
              pageId: targetPage.id,
              view: state.ui.view ?? "edit",
              // 工具层的只读镜像。剥掉 messages：工具不读会话，没必要每次搬全部历史
              workspace: {
                version: state.version,
                ui: state.ui,
                people: state.people.map((item) => ({ ...item, messages: [] })),
              },
            },
            onEvent: channel,
          });
          const id = sent.id;

          const { content, changes } = splitReply(acc, person, targetPage);
          await invoke("chat_patch", {
            key: person.id,
            id,
            patch: {
              content: content || "（模型没有返回正文）",
              tool: used.length ? `${ai.model} · ${used.join(" / ")}` : `${ai.model} · 完成`,
              changes: changes.length ? changes : null,
              actions: changes.length
                ? [
                    { id: uid("a"), label: `应用 ${changes.length} 处改写`, kind: "apply" },
                    { id: uid("a"), label: "再收一收", kind: "hint" },
                  ]
                : null,
              // 内部轮的助手消息由 Rust 建，本地只补过占位气泡；这里补标记，
              // 否则重新拉历史时它会以「一条没有提问的回答」出现在对话流里
              ...(silent ? { internal: true } : {}),
            },
          });

          // 用 Rust 的历史覆盖本地投影：draft 换成正式消息，顺手拿到裁剪后的列表
          setRemote(await invoke<ChatMessage[]>("chat_history", { key: person.id }));

          // 答复必须回到排版 agent 手里，否则这一轮只是白白花掉一次调用
          answerBack(content);

          // 页面控制工具的指令在这里落地：切页、切视图、改快捷栏、调外观
          // 工具新产出的材料先落地，动作里可能引用它
          if (sent.assets?.length) {
            dispatch({ type: "addAssets", personId: person.id, assets: sent.assets });
          }
          const applied = applyAgentActions(sent.actions, {
            openPage: (pageId) => setActivePageId(pageId),
            openPerson,
            openResume,
            patchUi: (patch) => dispatch({ type: "patchUi", patch }),
            ask: (questions) => setAsk(questions),
            pageStyle: (pageId, style) =>
              dispatch({ type: "pageStyle", personId: person.id, resumeId: resume.id, pageId, style }),
            pageLayout: (pageId, layout) =>
              dispatch({ type: "pageLayout", personId: person.id, resumeId: resume.id, pageId, layout }),
            requestLayout: (pageId, brief) => runLayoutFor(pageId, brief),
            // 内容写入走 provider 的确认门：会话 agent 和用户点击必须同一套规则
            applyOps,
            pageNote: (pageId, note) =>
              dispatch({ type: "pageNote", personId: person.id, resumeId: resume.id, pageId, note }),
            askSession: (pageId, question, blockId) =>
              askFromLayout(pageId, question, blockId),
          });
          if (applied > 0) toast(`已按你的要求调整界面（${applied} 处）`);
        } catch (error) {
          const reason = error instanceof Error ? error.message : String(error);
          if (reason.includes("已中断")) {
            // 内部轮没有可见气泡可撤；对话流里的「已停止」也只对用户自己发起的那轮有意义
            if (!silent) {
              setRemote((prev) => prev.filter((item) => item.id !== replyId));
              toast("已停止");
            }
          } else if (silent) {
            // 内部往返失败不在对话流里冒错误，也不回投失败文案（排版读不懂）。
            // 用户看不到这轮，留一条控制台记录便于排查
            console.warn("[chat] 排版提问没有拿到答复", reason);
          } else {
            patchLocal(replyId, {
              content: `调用失败：${reason}`,
              tool: `${ai.model} · 调用失败`,
            });
            toast("模型调用失败，检查接口地址与密钥");
          }
        } finally {
          setBusy(false);
          setSilentBusy(false);
          abortRef.current = null;
        }
      })();
    },
    // state 进依赖：每次发送都要把最新的工作区快照推给 Rust 的工具层
    [
      state,
      person,
      resume,
      page,
      dispatch,
      setActivePageId,
      applyOps,
      openPerson,
      openResume,
      askFromLayout,
      ai,
      configured,
      toast,
      desktop,
      patchLocal,
      runLayoutFor,
    ],
  );

  useEffect(() => {
    sendRef.current = send;
  }, [send]);

  /**
   * 面板是唯一知道「消息存在哪、怎么落库」的地方（桌面端真相在 Rust）。
   * provider 拿到排版 agent 的指令后，通过注册表回到这里执行 —— 比把整条
   * 发送链路搬进 provider 现实得多，也避免两套写消息的代码各自漂移。
   */
  const bridgeApi = useRef<{
    turn: (pageId: string, content: string, silent?: boolean, blockId?: string) => void;
    notice: (content: string) => void;
  }>({ turn: () => {}, notice: () => {} });

  useEffect(() => {
    bridgeApi.current = {
      // 提问一律按内部往返发：它来自排版 agent，不是用户打的字
      turn: (pageId, content, silent, blockId) =>
        sendRef.current?.(content, [], pageId, { silent, blockId }),
      notice: (content) => {
        if (!person) return;
        const message: ChatMessage = {
          id: uid("m"),
          role: "user",
          system: true,
          content,
          time: Date.now(),
        };
        if (!desktop) {
          dispatch({ type: "appendMessage", personId: person.id, message });
          return;
        }
        setRemote((prev) => [...prev, message]);
        void invoke("chat_push", { key: person.id, message });
      },
    };
  });

  useEffect(
    () =>
      registerChatBridge({
        turn: (pageId, content, silent, blockId) =>
          bridgeApi.current.turn(pageId, content, silent, blockId),
        notice: (content) => bridgeApi.current.notice(content),
      }),
    [],
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
      const entries = entriesOf(page);
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
      void openModelSettings();
      return;
    }
    setInput("把这段再具体一点，补上当时的判断依据。");
  };

  const onDrop = (event: React.DragEvent) => {
    event.preventDefault();
    setDragging(false);
    const dropped = Array.from(event.dataTransfer.files ?? []).map(toAttachment);
    if (!dropped.length) return;
    // 拖放没有光标位置，追加到正文末尾（附件要以行内标记的形式存在）
    setFiles((list) => {
      const merged = attachAtEnd(input, list, dropped);
      setInput(merged.text);
      return merged.files;
    });
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
        // 只认文件：拖动页签、拖动字段把手、拖动选中文字都会派发 dragover，
        // 以前不筛，随手一拖就冒出「松开即可添加附件」的遮罩
        if (!event.dataTransfer.types.includes("Files")) return;
        event.preventDefault();
        if (!dragging) setDragging(true);
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
      </header>

      <div className={styles.thread} ref={scrollRef}>
        <div className={styles.threadIn}>
          {messages.length === 0 && !busy && <EmptyTurn name={resume.title} onPick={setInput} />}
          {messages.map((message) =>
            message.system ? (
              <SystemTurn key={message.id} message={message} />
            ) : message.role === "user" ? (
              <UserTurn key={message.id} message={message} />
            ) : (
              <AssistantTurn key={message.id} message={message} onAction={(action) => handleAction(message, action)} />
            ),
          )}
          {/* 内部轮（排版 ↔ 会话）不渲染任何「正在发生」：用户看到的对话里
              根本没有这一轮，冒出一个打字体反而像是卡住了 */}
          {busy && !silentBusy && reasoning && (
            <Disclosure summary={`思考过程 · ${reasoning.length} 字`} className={styles.think}>
              {reasoning}
            </Disclosure>
          )}
          {busy && !silentBusy && <TypingTurn />}
        </div>
      </div>

      {busy && !silentBusy && (
        <div className={styles.stopRow}>
          <button type="button" className={styles.stopBtn} onClick={stop}>
            停止生成
          </button>
        </div>
      )}

      <Composer
        model={modelLabel}
        onPickModel={pickModel}
        onOpenSettings={() => void openModelSettings()}
        onSend={send}
        busy={busy}
        text={input}
        onTextChange={setInput}
        files={files}
        onFilesChange={setFiles}
      />

      {ask && (
        <AskForm
          questions={ask}
          onCancel={() => setAsk(null)}
          onSubmit={(text) => {
            setAsk(null);
            // 表单答案就是一条普通用户消息：后端不用挂起跨请求状态，
            // 模型下一轮读到的是一段结构化的自然语言
            send(text, []);
          }}
        />
      )}

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

      {dragging && (
        <div className={styles.dropHint}>
          <span>松开即可添加附件</span>
        </div>
      )}
    </section>
  );
}

export { ChevronDown };
