import { Channel, invoke } from "@tauri-apps/api/core";
import type { Person, ResumePage, Workspace } from "@/types";
import { aiReady, loadAi } from "@/lib/ai";
import { isTauri } from "@/lib/storage";

/**
 * 叫一次「页排版 agent」。
 *
 * 为什么放在前端发起而不是后端自动跑：
 *   触发点是「用户切到预览」这种界面事件，后端感知不到；
 *   而且排版要花一次模型调用，不该在用户只是改个字的时候也跑。
 *
 * 一个页面一条会话（key 由 Rust 侧拼 `layout:<resumeId>:<pageId>`），
 * 所以多次切换会累积上下文——它记得上一轮为什么这么排。
 */
export async function requestLayout(input: {
  person: Person;
  resume: { id: string };
  page: ResumePage;
  state: Workspace;
  brief?: string;
}): Promise<unknown[] | null> {
  if (!isTauri()) return null;
  const ai = loadAi();
  if (!aiReady(ai)) return null;

  // 命令签名里有 `onEvent: Channel<StreamChunk>`（它内部走的是和会话同一套流式循环），
  // 不传就是 "missing required key onEvent"。排版结果是动作而不是正文，
  // 所以这里不往对话里写，只收着；将来要显示「正在排版…」的进度可以从这里进。
  const channel = new Channel<{ kind?: string; delta?: string; message?: string }>();
  channel.onmessage = () => {};

  const result = await invoke<{ id: string; actions: unknown[] }>("layout_send", {
    args: {
      personId: input.person.id,
      resumeId: input.resume.id,
      pageId: input.page.id,
      brief: input.brief ?? "",
      // 工具层只读镜像：剥掉会话历史，排版 agent 不需要
      workspace: {
        version: input.state.version,
        ui: input.state.ui,
        people: input.state.people.map((item) => ({ ...item, messages: [] })),
      },
      baseUrl: ai.baseUrl,
      apiKey: ai.apiKey,
      model: ai.model,
      temperature: ai.temperature,
      reasoning: ai.reasoning ?? null,
      contextWindow: ai.contextWindow ?? null,
    },
    onEvent: channel,
  });

  return result.actions ?? [];
}
