import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Asset, Attachment, ChatAction, ChatMessage, Origin } from "@/types";
import { useWorkspace } from "@/store/workspace";
import { uid } from "@/lib/uid";
import { useToast } from "@/components/ui/toast-context";
import { ChevronDown, ChevronRight } from "@/components/ui/icons";
import { Composer } from "./composer";
import { Disclosure } from "@/components/ui";
import { attachRefs, stripPreview, toAttachment } from "@/lib/attach";
import { attachAtEnd } from "@/lib/inline-tokens";
import { diff, flatten, renderDelta, type Flat } from "@/lib/changes";
import { entriesOf } from "@/lib/blocks";
import { AiSettingsSheet } from "./ai-settings";
import { AssistantTurn, EmptyTurn, SystemTurn, TypingTurn, UserTurn } from "./message";
import { aiReady, loadAi, loadAux, saveAi, slotAi, type AiSettings } from "@/lib/ai";
import { refreshSettings } from "@/lib/settings-store";
import { DEFAULT_RESERVE, findModel, listProfiles } from "@/lib/model-config";
import { buildContext, splitReply } from "@/lib/prompt";
import {
  applyAgentActions,
  askSessionFromLayout,
  registerChatBridge,
  type AgentActionHooks,
  type AskQuestion,
} from "@/lib/agent-actions";
import { submitLayout } from "@/lib/layout";
import { clearOrigin, originOf, putOrigin } from "@/lib/measure";
import { AskForm } from "./ask-form";
import { displayName } from "@/types";
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openSettingsWindow, themeParams, type SettingsChanged } from "@/lib/settings-window";
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

/** 压缩点分隔线的标签：一句话说清「这条之前，模型只看得见摘要」 */
const DIGEST_LABEL = "压缩点 · 此处之前的对话已压成摘要";

/**
 * 发给后端的 OCR 接口配置。
 *
 * 地址/密钥留空表示沿用对话模型那把，与设置页的说明一致；
 * 没配 OCR 模型就返回 null —— 后端这时只有内置 OCR 可用（lite 构建等于没有）。
 */
function ocrConf(ai: AiSettings) {
  const aux = loadAux().ocr;
  if (!aux || !aux.model.trim()) return null;
  return {
    baseUrl: aux.baseUrl.trim() || ai.baseUrl,
    apiKey: aux.apiKey.trim() || ai.apiKey,
    model: aux.model,
  };
}

export function ChatPanel() {
  const {
    state,
    person,
    resume,
    page,
    measure,
    measureOf,
    dispatch,
    setActivePageId,
    applyOps,
    openPerson,
    openResume,
  } = useWorkspace();
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
   * 建窗失败时退回浮层（例如窗口管理出问题），不让入口变成死链。
   */
  const openModelSettings = async () => {
    // 带上主题：模型窗不读工作区，空参数会开出默认浅色的窗
    if (!(await openSettingsWindow("model", themeParams(state.ui)))) setAiOpen(true);
  };

  useEffect(() => {
    let off: (() => void) | undefined;
    void listen<SettingsChanged>("settings:changed", (event) => {
      // 设置窗写的是它自己那份缓存 + 数据库，这里必须重读，否则还拿着旧模型
      if (event.payload.kind === "ai") void refreshSettings().then(() => setAi(loadAi()));
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
  /** 消息真相源在 Rust：组件只持有 chat_history 拉回来的投影 */
  const [remote, setRemote] = useState<ChatMessage[]>([]);
  /**
   * 压缩点：`upto` 是最后一条被压进摘要的消息 id。
   * 模型侧照旧按 token 预算裁上下文，这里只负责让用户看见「从哪一条开始模型只看到摘要」。
   */
  const [digest, setDigest] = useState<{ upto: string; text: string } | null>(null);
  const [reasoning, setReasoning] = useState("");

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
      reserve: profile.reserve ?? DEFAULT_RESERVE,
      temperature: profile.temperature ?? spec.defaultTemperature,
      reasoning: profile.reasoning ?? spec.reasoning[0] ?? undefined,
    };
    setAi(next);
    saveAi(next);
    toast(`已切换到 ${spec.label}：${profile.label}`);
  };

  const messages = useMemo(() => remote, [remote]);

  /**
   * 对话流只放用户与会话 agent 的往返。
   *
   * 内部消息（排版提问与它的答复）仍在 `messages` 里 —— 那是模型上下文的一部分，
   * 下一轮它还看得见；这里只是决定「用户能看到什么」，所以过滤放在渲染前一步。
   */
  const visible = useMemo(() => messages.filter((item) => !item.internal), [messages]);

  const personId = person?.id ?? null;

  // 切人/启动时从 Rust 拉一次：现在拿的是全部消息，不再随模型窗口变
  useEffect(() => {
    if (!personId) {
      setRemote([]);
      setDigest(null);
      return;
    }
    let alive = true;
    void invoke<{ messages: ChatMessage[]; digest: { upto: string; text: string } | null }>(
      "chat_history",
      { key: personId },
    )
      .then((slice) => {
        if (!alive) return;
        setRemote(slice.messages);
        setDigest(slice.digest);
      })
      .catch(() => {
        if (!alive) return;
        setRemote([]);
        setDigest(null);
      });
    return () => {
      alive = false;
    };
  }, [personId]);

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

  /**
   * 换人 / 换简历：上一轮那份原件的页数不再代表这一轮。
   * 不清的话排版侧会拿别人那份 PDF 的页数去判「沿用原件页数」，
   * 这正是「跨请求残留」那类错。
   */
  useEffect(() => {
    if (resume?.id) clearOrigin(resume.id);
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
        // 实测必须跟着走：不带的话排版 agent 看到的是「这一页还没量到」，
        // 它只能在看不见页数的情况下凭字数猜，压版就成了闭眼开方子。
        // 每一页都有自己的离屏实测，所以这里按目标页取，不是按当前页。
        measure: measureOf(target.id),
        origin: originOf(resume.id),
        text: brief.trim() || "请检查这一页的版式。",
      });
    },
    [person, resume, page, state, measureOf],
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

  /**
   * 这一页的实测值；量的不是这一页就给 null。
   * 不拿「1 页」去凑一个像样的对象 —— 假数比没有数更糟（F-007）。
   */
  const measureFor = useCallback(
    (pageId: string) => (measure && measure.pageId === pageId ? measure : null),
    [measure],
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
       * 所以这一轮结束后把正文重新投进那一页的排版信箱。
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
          measure: measureOf(targetPage.id),
          origin: originOf(resume.id),
          text: `会话 agent 针对你上一个问题的答复${where}：\n${body}`,
        });
      };

      setBusy(true);
      setSilentBusy(silent);
      setReasoning("");
      abortRef.current = new AbortController();

      // 内部往返不进 remote：用户看到的对话流里不该出现提问与答复这两条
      if (!silent) setRemote((prev) => [...prev, userMessage, draft]);

      void (async () => {
        try {
          await invoke("chat_push", { key: person.id, message: userMessage });

          /**
           * 一轮的界面指令都走这一套 hooks：途中的 `mutate` 信封与轮末的
           * `sent.actions` 必须是同一份，两处各写一份迟早会漂移。
           *
           * 内容写入一律进 provider 的确认门（`applyOps`），中途也不绕过：
           * 删除类整批挂起是预期行为，等用户点确认。
           */
          const agentHooks: AgentActionHooks = {
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
          };

          /**
           * 附件只报名字：正文、OCR、PDF 页面都由后端在组上下文时现取。
           * 前端不再读字节，也就不存在「刷新一次缓存就没了」这回事。
           */
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
            } else if (chunk.kind === "mutate" && chunk.detail) {
              // 中途写入：模型改一条就落一条。坏信封直接丢——轮末还会再结算一次，
              // 这里抛出去只会让整轮报错
              try {
                applyAgentActions([JSON.parse(chunk.detail)], agentHooks);
              } catch {
                /* 忽略：信封坏了不该影响正在跑的这轮 */
              }
            }
          };

          const sent = await invoke<{
            id: string;
            actions: unknown[];
            assets?: Asset[];
            origin?: Origin | null;
            originDoc?: { stored: string; name: string; pages?: number } | null;
          }>("chat_send", {
            args: {
              key: person.id,
              context,
              // 附件引用（inbox 存储名）：后端按它取字、OCR、渲染页面
              attachments: attachRefs(attachments),
              // 设置里配的 OCR 接口；没配就只有内置 OCR（lite 构建等于没有）
              ocr: ocrConf(ai),
              baseUrl: ai.baseUrl,
              apiKey: ai.apiKey,
              model: ai.model,
              temperature: ai.temperature,
              reasoning: ai.reasoning ?? null,
              contextWindow: ai.contextWindow ?? null,
              // 压缩预留：窗口减掉它才是历史能占的部分（模型配置里可调）
              reserve: ai.reserve ?? null,
              // 压缩摘要用「压缩」用途的模型；没配时后端自动沿用主模型
              compact: compactFor(),
              resumeId: resume.id,
              pageId: targetPage.id,
              // 实测值只对「量的那一页」成立：问的是别的页就传 null，
              // 不能让模型拿甲页的页数去说乙页
              measure: measureFor(targetPage.id),
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
          // 本轮原件的页数只活到下一次排版提交：没有原件就清掉，
          // 免得上一轮那份 PDF 的页数被当成本轮的
          if (sent.origin) putOrigin(resume.id, sent.origin);
          else clearOrigin(resume.id);
          // 原件指针挂在简历上：排版 agent 每轮靠它调 render_doc_page 对照版式。
          // 不进人的材料列表，也不进任何 blocks —— 它不是这一页的内容
          if (sent.originDoc) {
            dispatch({
              type: "originDoc",
              personId: person.id,
              resumeId: resume.id,
              originDoc: sent.originDoc,
            });
          }

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

          // 用 Rust 的历史覆盖本地投影：draft 换成正式消息，顺手把压缩点一起刷新
          {
            const slice = await invoke<{
              messages: ChatMessage[];
              digest: { upto: string; text: string } | null;
            }>("chat_history", { key: person.id });
            setRemote(slice.messages);
            setDigest(slice.digest);
          }

          // 答复必须回到排版 agent 手里，否则这一轮只是白白花掉一次调用
          answerBack(content);

          // 页面控制工具的指令在这里落地：切页、切视图、改快捷栏、调外观
          // 工具新产出的材料先落地，动作里可能引用它
          if (sent.assets?.length) {
            dispatch({ type: "addAssets", personId: person.id, assets: sent.assets });
          }
          const applied = applyAgentActions(sent.actions, agentHooks);
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
      measureFor,
      dispatch,
      setActivePageId,
      applyOps,
      openPerson,
      openResume,
      askFromLayout,
      ai,
      configured,
      toast,
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

  /** 统一补丁入口：消息真相在 Rust，这里先改本地投影，再把补丁写回去 */
  const patchMessage = useCallback(
    (id: string, patch: Partial<ChatMessage>) => {
      if (!person) return;
      patchLocal(id, patch);
      // undefined 序列化会丢字段，Rust 侧用 null 表示"清空"
      const wire: Record<string, unknown> = {};
      for (const [field, value] of Object.entries(patch)) {
        wire[field] = value === undefined ? null : value;
      }
      void invoke("chat_patch", { key: person.id, id, patch: wire });
    },
    [person, patchLocal],
  );

  /** 中断当前请求：命令层置取消标记，模型那边这一轮会被丢掉 */
  const stop = useCallback(() => {
    abortRef.current?.abort();
    if (person) void invoke("chat_cancel", { key: person.id });
  }, [person]);

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
    const files = Array.from(event.dataTransfer.files ?? []);
    if (!files.length) return;
    void Promise.all(files.map(toAttachment)).then((dropped) => {
      // 拖放没有光标位置，追加到正文末尾（附件要以行内标记的形式存在）
      setFiles((list) => {
        const merged = attachAtEnd(input, list, dropped);
        setInput(merged.text);
        return merged.files;
      });
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
          {messages.map((message) => (
            <Fragment key={message.id}>
              {message.system ? (
                <SystemTurn message={message} />
              ) : message.role === "user" ? (
                <UserTurn message={message} />
              ) : (
                <AssistantTurn message={message} onAction={(action) => handleAction(message, action)} />
              )}
              {/* 压缩点：这条之后的历史模型只看到摘要。upto 不在当前列表里（会话被清过）
                  就什么都不插 —— 找不到锚点时分隔线只会莫名其妙地贴在末尾 */}
              {digest && message.id === digest.upto && (
                <div className={styles.digest}>
                  <Disclosure summary={DIGEST_LABEL} className={styles.digestMark}>
                    <span className={styles.digestText}>{digest.text}</span>
                  </Disclosure>
                </div>
              )}
            </Fragment>
          ))}
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
