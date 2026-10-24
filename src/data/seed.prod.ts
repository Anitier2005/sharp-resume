import { EMPTY_WORKSPACE } from "./empty";
import type { Workspace } from "@/types";

/**
 * 生产构建用的 seed 替身。
 *
 * `vite.config.ts` 在 production 模式下把 `@/data/seed` 重定向到这里，
 * 真正的 `seed.ts`（506 行示例简历）根本不会进入打包产物。
 *
 * 为什么不在 store 里用 `import.meta.env.DEV ? SEED : EMPTY` 做运行时分支：
 * seed.ts 顶层有 `const now = Date.now()` 这类调用，rollup 视其为副作用，
 * 不敢摇掉整个模块，示例数据仍然会留在 bundle 里（实测确认过）。
 * 构建期换模块没有这个问题，也不依赖摇树策略。
 */
export const SEED: Workspace = EMPTY_WORKSPACE;
