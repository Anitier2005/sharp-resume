import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { AccentKey } from "@/types";
import { readableOn } from "@/lib/color";
import { notifyUiPatch } from "@/lib/settings-window";
import { ToastProvider } from "@/components/ui/toast";
import { Close } from "@/components/ui/icons";
import { LookSettings } from "@/components/settings/look-settings";
import { ModelSettings } from "@/components/settings/model-settings";
import { KnowledgePanel } from "@/components/knowledge/panel";
import styles from "@/components/settings/settings.module.css";

/**
 * 设置窗口的根。
 *
 * 同一个入口页面被四种情况复用：主窗口（`/`）、模型窗、外观窗、知识库窗
 * （`?view=model|look|knowledge`）。靠查询参数分流，而不是再建一个 HTML 入口 ——
 * 多入口意味着多一份 vite/tauri 配置要维护，而这里只差一个分支。
 *
 * 窗口是**无边框**的（`decorations: false`）：主窗口就是自绘标题栏，
 * 设置窗露出一条系统白边会立刻显得不是同一个应用。代价是要自己提供
 * 拖动区（`data-tauri-drag-region`）和关闭键。
 *
 * 初值全部从查询参数带进来：窗口不读工作区，也就不必为它挂一整套 store。
 */
export function SettingsRoot() {
  const params = new URLSearchParams(window.location.search);
  const raw = params.get("view");
  const view = raw === "look" || raw === "knowledge" ? raw : "model";

  const [accent, setAccent] = useState<AccentKey>((params.get("accent") as AccentKey) ?? "white");
  const [hex, setHex] = useState(params.get("hex") ?? "");
  const [mode, setMode] = useState<"light" | "dark">(
    params.get("mode") === "dark" ? "dark" : "light",
  );
  const [scale, setScale] = useState(params.get("scale") ?? "");
  const systemScale = Number(params.get("sys") ?? "1.2") || 1.2;

  // 令牌照主窗口那套写：accent 挂 html 属性，字号写 :root 变量，明暗挂 .win
  useEffect(() => {
    document.documentElement.dataset.accent = accent;
    const effective = scale ? Number(scale) : systemScale;
    document.documentElement.style.setProperty("--text-scale", String(effective));
  }, [accent, scale, systemScale]);

  const pickAccent = (key: AccentKey) => {
    setAccent(key);
    setHex("");
    void notifyUiPatch({ accent: key, accentHex: "" });
  };

  const pickHex = (value: string) => {
    setHex(value);
    // 输入到一半的 HEX 不推送：主窗口会拿它当颜色用，界面会闪一下灰
    if (/^#[0-9a-fA-F]{6}$/.test(value)) void notifyUiPatch({ accentHex: value });
  };

  const pickMode = (next: "light" | "dark") => {
    setMode(next);
    void notifyUiPatch({ mode: next });
  };

  const pickScale = (next: string) => {
    setScale(next);
    void notifyUiPatch({ textScale: next === "" ? null : Number(next) });
  };

  const close = () => {
    void getCurrentWindow().close();
  };

  const title = view === "model" ? "模型" : view === "look" ? "外观" : "知识库";

  return (
    <div
      className="win"
      data-mode={mode}
      style={
        hex
          ? ({ "--acc": hex, "--acc-fg": readableOn(hex) } as React.CSSProperties)
          : undefined
      }
    >
      <header className={styles.bar} data-tauri-drag-region>
        <span className={styles.barTitle} data-tauri-drag-region>
          {title}
        </span>
        <button type="button" className={styles.barClose} aria-label="关闭" onClick={close}>
          <Close size={10} />
        </button>
      </header>

      <div className={styles.host}>
        {view === "look" ? (
          <LookSettings
            accent={accent}
            hex={hex}
            mode={mode}
            scale={scale}
            systemScale={systemScale}
            onAccent={pickAccent}
            onHex={pickHex}
            onMode={pickMode}
            onScale={pickScale}
          />
        ) : view === "knowledge" ? (
          // 知识库面板自己会弹 toast（保存/删除的结果），窗口里也要有 provider
          <ToastProvider>
            <KnowledgePanel open embedded onClose={close} />
          </ToastProvider>
        ) : (
          <ModelSettings />
        )}
      </div>
    </div>
  );
}
