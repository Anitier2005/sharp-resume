import { useEffect, useState } from "react";
import type { AccentKey } from "@/types";
import { readableOn } from "@/lib/color";
import { notifyUiPatch } from "@/lib/settings-window";
import { LookSettings } from "@/components/settings/look-settings";
import { ModelSettings } from "@/components/settings/model-settings";

/**
 * 设置窗口的根。
 *
 * 同一个入口页面被三种情况复用：主窗口（`/`）、模型窗（`?view=model`）、
 * 外观窗（`?view=look`）。靠查询参数分流，而不是再建一个 HTML 入口 ——
 * 多入口意味着多一份 vite/tauri 配置要维护，而这里只差一个分支。
 *
 * 初值全部从查询参数带进来：窗口不需要读工作区，也就不必为它挂一整套 store。
 */
export function SettingsRoot() {
  const params = new URLSearchParams(window.location.search);
  const view = params.get("view") === "look" ? "look" : "model";


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
      ) : (
        <ModelSettings />
      )}
    </div>
  );
}
