import { Minus, Moon, Search, Settings, Square, Sun, X } from "lucide-react";
import { toast } from "sonner";

import { cn } from "@/lib/utils";

export function Logo({ className }: { className?: string }) {
  return (
    <div
      className={cn(
        "grid size-6 shrink-0 place-items-center rounded-[7px] bg-gradient-to-br from-[#6d5efc] to-[#a78bfa] text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.3),0_1px_2px_rgba(80,60,220,0.35)]",
        className,
      )}
    >
      <svg viewBox="0 0 24 24" className="size-4" fill="none" aria-hidden="true">
        <path
          d="M15.5 5H8a3.2 3.2 0 0 0 0 6.4h4.2a3.2 3.2 0 0 1 0 6.4H5"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
        />
        <path
          d="M18 4.5 15 19.5"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          opacity="0.45"
        />
      </svg>
    </div>
  );
}

export function AppTitleBar({
  theme,
  onToggleTheme,
}: {
  theme: "light" | "dark";
  onToggleTheme: () => void;
}) {
  const runWindowAction = async (action: "minimize" | "maximize" | "close") => {
    const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
    if (!inTauri) {
      toast("桌面端能力", { description: "打包为 exe 后，这里会调用原生窗口控制。" });
      return;
    }
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const win = getCurrentWindow();
    if (action === "minimize") await win.minimize();
    if (action === "maximize") await win.toggleMaximize();
    if (action === "close") await win.close();
  };

  return (
    <header
      data-tauri-drag-region
      className="drag-region relative z-30 flex h-11 shrink-0 items-center gap-2 border-b bg-background/85 px-2 pl-3 backdrop-blur-md"
    >
      <div data-tauri-drag-region className="flex items-center gap-2">
        <Logo />
        <div data-tauri-drag-region className="flex items-baseline gap-1.5">
          <span data-tauri-drag-region className="text-[12.5px] font-semibold tracking-[-0.01em]">
            Sharp Resume
          </span>
          <span className="rounded border px-1 py-px text-[9px] font-medium uppercase tracking-[0.08em] text-muted-foreground">
            alpha
          </span>
        </div>
      </div>

      <div className="mx-2 hidden h-4 w-px bg-border sm:block" />

      <div data-tauri-drag-region className="hidden items-center gap-1.5 text-[11.5px] text-muted-foreground sm:flex">
        <span className="size-1.5 rounded-full bg-emerald-500 shadow-[0_0_0_3px_rgba(16,185,129,0.12)]" />
        本地工作区
      </div>

      <div className="no-drag ml-auto flex items-center gap-1">
        <button
          type="button"
          onClick={() => toast("全局搜索", { description: "命令面板会在功能阶段接入，支持简历与对话检索。" })}
          className="hidden h-7 items-center gap-2 rounded-md border bg-muted/40 px-2 text-[11px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground xl:flex"
        >
          <Search className="size-3.5" />
          搜索简历与对话
          <kbd className="ml-2 rounded border bg-background px-1 py-px font-sans text-[9px] text-muted-foreground">
            Ctrl K
          </kbd>
        </button>

        <button
          type="button"
          onClick={onToggleTheme}
          aria-label={theme === "light" ? "切换到深色模式" : "切换到浅色模式"}
          className="grid size-7 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        >
          {theme === "light" ? <Moon className="size-3.5" /> : <Sun className="size-3.5" />}
        </button>

        <button
          type="button"
          onClick={() => toast("偏好设置", { description: "模型、存储路径、快捷键等设置项在功能阶段补齐。" })}
          aria-label="设置"
          className="grid size-7 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        >
          <Settings className="size-3.5" />
        </button>
      </div>

      <div className="ml-1 flex items-center border-l pl-1">
        <button
          type="button"
          onClick={() => void runWindowAction("minimize")}
          aria-label="最小化"
          className="grid h-8 w-9 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        >
          <Minus className="size-3.5" />
        </button>
        <button
          type="button"
          onClick={() => void runWindowAction("maximize")}
          aria-label="最大化"
          className="grid h-8 w-9 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        >
          <Square className="size-3" />
        </button>
        <button
          type="button"
          onClick={() => void runWindowAction("close")}
          aria-label="关闭"
          className="grid h-8 w-9 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-red-500 hover:text-white"
        >
          <X className="size-3.5" />
        </button>
      </div>
    </header>
  );
}
