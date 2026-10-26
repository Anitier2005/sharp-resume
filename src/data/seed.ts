import { EMPTY_WORKSPACE } from "./empty";
import type { Workspace } from "@/types";

/**
 * 开发期种子数据：**空**。
 *
 * 之前这里塞了 8 位虚构候选人和一段假的 AI 对话，被当成用户数据落盘，
 * 让用户以为自己的数据被污染了（2026-10-25 事故）。现在示例数据彻底不要，
 * 开发与生产都从零开始 —— 需要样例就在界面里自己建。
 */
export const SEED: Workspace = EMPTY_WORKSPACE;
