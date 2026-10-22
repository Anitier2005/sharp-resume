import { ACCENTS } from "@/types";
import { useWorkspace } from "@/store/workspace";
import { isTauri } from "@/lib/storage";
import { useToast } from "@/components/ui/toast-context";
import { Dropdown, MenuLabel, MenuSep } from "@/components/ui/menu";
import { WinClose, WinMax, WinMin } from "@/components/ui/icons";

async function windowAction(action: "minimize" | "maximize" | "close") {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const win = getCurrentWindow();
  if (action === "minimize") await win.minimize();
  else if (action === "maximize") await win.toggleMaximize();
  else await win.close();
}

export function TitleBar() {
  const { state, resume, dispatch } = useWorkspace();
  const toast = useToast();
  const desktop = isTauri();
  const { accent, mode } = state.ui;

  const run = (action: "minimize" | "maximize" | "close") => {
    if (!desktop) {
      toast("窗口控制在打包后可用");
      return;
    }
    void windowAction(action);
  };

  return (
    <div
      className="titlebar"
      data-tauri-drag-region
      onDoubleClick={() => run("maximize")}
    >
      <span className="glyph" />
      <span className="doc-name">{resume?.title ?? "未选择简历"}</span>
      <span>— Sharp Resume</span>

      <div className="titlebar-right">
        <Dropdown
          width={186}
          trigger={
            <button type="button" className="appearance" aria-label="外观">
              <span
                className="appearance-dot"
                data-accent={accent}
                data-mode={mode}
              />
              外观
            </button>
          }
        >
          <MenuLabel>明暗</MenuLabel>
          <div className="menu-seg">
            <button
              type="button"
              aria-pressed={mode === "light"}
              onClick={() => dispatch({ type: "setMode", mode: "light" })}
            >
              浅色
            </button>
            <button
              type="button"
              aria-pressed={mode === "dark"}
              onClick={() => dispatch({ type: "setMode", mode: "dark" })}
            >
              深色
            </button>
          </div>
          <MenuSep />
          <MenuLabel>强调色</MenuLabel>
          <div className="menu-swatches">
            {ACCENTS.map((item) => (
              <button
                key={item.key}
                type="button"
                className="menu-sw"
                aria-pressed={accent === item.key}
                onClick={() => dispatch({ type: "setAccent", accent: item.key })}
              >
                <i style={{ background: item.color }} />
                {item.label}
              </button>
            ))}
          </div>
        </Dropdown>

        <div className="winbtns">
          <button type="button" aria-label="最小化" onClick={() => run("minimize")}>
            <WinMin />
          </button>
          <button type="button" aria-label="最大化" onClick={() => run("maximize")}>
            <WinMax />
          </button>
          <button type="button" aria-label="关闭" onClick={() => run("close")}>
            <WinClose />
          </button>
        </div>
      </div>
    </div>
  );
}
