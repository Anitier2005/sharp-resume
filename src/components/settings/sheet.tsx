import { useEffect, useRef, useState } from "react";
import { useWorkspace } from "@/store/workspace";
import { Close, Pencil, Plus, Trash } from "@/components/ui/icons";
import { useVeilWheelLock } from "@/components/ui/veil";
import { Modal } from "@/components/ui/modal";
import { Confirm } from "@/components/ui/confirm";
import { useToast } from "@/components/ui/toast-context";
import { ACCENTS, TEXT_SCALES } from "@/types";
import { isHexColor } from "@/lib/color";
import {
  aiFromModel,
  loadAi,
  loadAux,
  saveAi,
  saveAux,
  testAi,
  type AiSettings,
  type AuxModelChoice,
  type AuxSettings,
} from "@/lib/ai";
import {
  findModel,
  isCustomModel,
  listModels,
  removeCustomModel,
  saveCustomModel,
  type ModelConfig,
  type ModelKind,
} from "@/lib/model-config";
import { isTauri } from "@/lib/storage";
import { configureKnowledge } from "@/lib/knowledge";
import { ColorPicker } from "@/components/ui/color-picker";
import { cx } from "@/lib/cx";
import shared from "@/components/ui/shared.module.css";
import {
  Button,
  Checkbox,
  Field,
  Input,
  List,
  ListEmpty,
  ListRow,
  ReadonlyValue,
  Segmented,
  Slider,
  Tooltip,
} from "@/components/ui";
import styles from "./sheet.module.css";

type Tab = "model" | "look";

/**
 * 统一设置：模型与外观收在一个地方。
 *
 * 模型配置写在 localStorage（`sharp-resume:ai:v1`），不进工作区、不随简历同步；
 * 模型清单 = `config/models.json` 里的内置项（打包时已硬编码）+ 用户自建的
 * 自定义项（`sharp-resume:models:v1`，同样只在本机）。
 *
 * 内容块按 open 挂载/卸载 —— 每次打开都拿到最新配置，也省掉一堆「同步草稿」的副作用。
 */
export function SettingsSheet({
  open,
  onClose,
  initialTab = "model",
}: {
  open: boolean;
  onClose: () => void;
  initialTab?: Tab;
}) {
  const veilRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  // 大面板不走 Modal（宽度由内容决定），但「滚轮不许带动底下的纸面」这条要一致
  useVeilWheelLock(veilRef, open);

  if (!open) return null;

  return (
    // 遮罩只挡点击，不关窗 —— 关闭只有两条路：右上角 X 与 ESC
    <div ref={veilRef} className={shared.veil} data-print="hide">
      <SheetBody onClose={onClose} initialTab={initialTab} />
    </div>
  );
}

function SheetBody({ onClose, initialTab }: { onClose: () => void; initialTab: Tab }) {
  const { state, dispatch, textScale, systemTextScale, setTextScale } = useWorkspace();
  const toast = useToast();
  const [tab, setTab] = useState<Tab>(initialTab);
  /**
   * 侧栏的三个二级项都指向这个面板，只是页不同。
   * 面板开着时点另一个页，组件不会重挂（open 一直是 true），
   * 所以必须跟着 initialTab 走一趟，否则点了「外观」还停在「模型」。
   */
  useEffect(() => {
    setTab(initialTab);
  }, [initialTab]);
  const [ai, setAi] = useState<AiSettings>(loadAi);
  const [hex, setHex] = useState(state.ui.accentHex ?? "");
  const [probe, setProbe] = useState<{ tone: "idle" | "ok" | "fail"; text: string }>({
    tone: "idle",
    text: "",
  });
  /**
   * 清单存一份在 state 里：`listModels()` 是普通函数，增删之后 React 不会自己重渲染，
   * 下拉就会停在旧选项上。
   */
  const [models, setModels] = useState<ModelConfig[]>(() => listModels("chat"));
  /**
   * 嵌入 / OCR 是「功能用哪个模型」，不是「当前对话用哪个」——
   * 所以单独一份设置，换对话模型不会顺手把它们换掉。
   */
  const [aux, setAux] = useState<AuxSettings>(loadAux);
  /** 正在编辑的模型：kind + 原始条目（有 model = 编辑，没有 = 新建） */
  const [editing, setEditing] = useState<{ kind: ModelKind; model?: ModelConfig } | null>(null);
  /** 等待确认删除的那一条 */
  const [removing, setRemoving] = useState<{ kind: ModelKind; model: ModelConfig } | null>(null);

  const spec = findModel(ai.model);

  const write = (patch: Partial<AiSettings>) => {
    const next = { ...ai, ...patch };
    setAi(next);
    saveAi(next);
  };

  /** 换模型时把网关/上下文/温度一起换成该模型的默认值，否则会拿旧网关配新模型 */
  const pickModel = (id: string) => {
    const model = findModel(id);
    if (!model) return;
    write(aiFromModel(model, ai));
  };

  /** 改嵌入/OCR 的选择：只动 aux，不碰当前对话模型 */
  const pickAux = (kind: "embedding" | "ocr", id: string) => {
    const model = findModel(id);
    if (!model) return;
    const choice: AuxModelChoice = { model: id, baseUrl: model.baseUrl, apiKey: "" };
    const next = { ...aux, [kind]: choice };
    setAux(next);
    saveAux({ [kind]: choice });
    configureKnowledge(next.embedding);
  };

  const patchAux = (kind: "embedding" | "ocr", patch: Partial<AuxModelChoice>) => {
    const current = aux[kind];
    if (!current) return;
    const next = { ...current, ...patch };
    setAux({ ...aux, [kind]: next });
    saveAux({ [kind]: next });
    configureKnowledge(kind === "embedding" ? next : aux.embedding);
  };

  /** 保存（新建或编辑）：存清单，并把它设为该用途的当前模型 */
  const saveModel = (kind: ModelKind, model: ModelConfig, apiKey: string) => {
    saveCustomModel(model);
    setModels(listModels("chat"));
    if (kind === "chat") {
      const next = aiFromModel(model, ai);
      // 表单里没填 key 就沿用当前那把 —— 同一个网关换型号通常不需要重输
      if (apiKey.trim()) next.apiKey = apiKey.trim();
      setAi(next);
      saveAi(next);
    } else {
      const choice: AuxModelChoice = { model: model.id, baseUrl: model.baseUrl, apiKey: apiKey.trim() };
      setAux({ ...aux, [kind]: choice });
      saveAux({ [kind]: choice });
      if (kind === "embedding") configureKnowledge(choice);
    }
    setEditing(null);
    toast(`已保存 ${model.label}`);
  };

  const dropModel = (kind: ModelKind, model: ModelConfig) => {
    setRemoving(null);
    removeCustomModel(model.id);
    const chatModels = listModels("chat");
    setModels(chatModels);
    if (kind === "chat") {
      // 删掉的正是当前在用的那条：回退到第一条内置，否则设置里会留一个查不到的 id
      if (ai.model === model.id) {
        const fallback = chatModels.find((item) => !isCustomModel(item.id));
        if (fallback) write(aiFromModel(fallback, ai));
      }
    } else if (aux[kind]?.model === model.id) {
      setAux({ ...aux, [kind]: null });
      saveAux({ [kind]: null });
      if (kind === "embedding") configureKnowledge(null);
    }
    toast(`已删除 ${model.label}`);
  };

  const runProbe = async () => {
    setProbe({ tone: "idle", text: "正在连接…" });
    try {
      setProbe({ tone: "ok", text: `连上了：${await testAi(ai)}` });
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      setProbe({
        tone: "fail",
        text: reason.includes("跨域") ? `${reason}（这是浏览器调试模式，桌面窗口里点同一个按钮才准）` : reason,
      });
    }
  };

  const applyHex = (value: string) => {
    setHex(value);
    if (isHexColor(value)) {
      dispatch({ type: "setAccent", accent: state.ui.accent, accentHex: value });
    }
  };

  return (
    <div className={styles.box} role="dialog" aria-modal="true" aria-label="设置">
      <header className={styles.head}>
        <div className={styles.tabs} role="tablist">
          <button
            type="button"
            role="tab"
            aria-selected={tab === "model"}
            onClick={() => setTab("model")}
          >
            模型
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "look"}
            onClick={() => setTab("look")}
          >
            外观
          </button>
        </div>
        <span className={styles.spacer} />
        <button
          type="button"
          className={shared.overlayClose}
          aria-label="关闭"
          onClick={onClose}
        >
          <Close size={11} />
        </button>
      </header>

      <div className={styles.body}>
        {tab === "model" ? (
          <>
            {/* 对话模型 */}
            <div className={styles.group}>
              <div className={styles.groupHead}>
                <div className={styles.groupLead}>
                  <span className={styles.groupTitle}>对话模型</span>
                  <span className={styles.groupNote}>改写、读页、调工具都用它</span>
                </div>
                <Button
                  size="sm"
                  leading={<Plus size={11} />}
                  onClick={() => setEditing({ kind: "chat" })}
                >
                  添加模型
                </Button>
              </div>

              <ModelList
                items={models}
                active={ai.model}
                empty="还没有模型，点右上「添加模型」"
                onPick={(item) => pickModel(item.id)}
                onEdit={(item) => setEditing({ kind: "chat", model: item })}
                onDelete={(item) => setRemoving({ kind: "chat", model: item })}
              />

              <div className={styles.formRow}>
                <Field label="接口地址">
                  <Input
                    value={ai.baseUrl}
                    spellCheck={false}
                    onChange={(event) => write({ baseUrl: event.target.value })}
                  />
                </Field>
                <Field label="API Key · 只存在本机">
                  <Input
                    type="password"
                    value={ai.apiKey}
                    spellCheck={false}
                    placeholder="sk-…"
                    onChange={(event) => write({ apiKey: event.target.value })}
                  />
                </Field>
              </div>

              <div className={styles.formRow}>
                <Field label={`温度 · ${ai.temperature.toFixed(2)}`}>
                  <Slider
                    ariaLabel="温度"
                    min={0}
                    max={1}
                    step={0.05}
                    value={ai.temperature}
                    onChange={(value) => write({ temperature: value })}
                  />
                </Field>
                <Field label="上下文窗口">
                  {/* 只读值用文字展示而不是禁用输入框：禁用输入框看起来像「可以改但坏了」 */}
                  <ReadonlyValue>
                    {(spec?.contextWindow ?? 0).toLocaleString("zh-CN")} tokens
                  </ReadonlyValue>
                </Field>
              </div>

              {spec && spec.reasoning.length > 0 && (
                <Field label="思维强度">
                  <Segmented
                    ariaLabel="思维强度"
                    value={ai.reasoning ?? spec.reasoning[0] ?? ""}
                    options={spec.reasoning.map((level) => ({ value: level, label: level }))}
                    onChange={(level) => write({ reasoning: level })}
                  />
                </Field>
              )}
            </div>

            {/* 嵌入模型 */}
            <div className={styles.group}>
              <div className={styles.groupHead}>
                <div className={styles.groupLead}>
                  <span className={styles.groupTitle}>嵌入模型</span>
                  <span className={styles.groupNote}>知识库的语义召回；不选就只用关键词检索</span>
                </div>
                <Button
                  size="sm"
                  leading={<Plus size={11} />}
                  onClick={() => setEditing({ kind: "embedding" })}
                >
                  添加模型
                </Button>
              </div>

              <ModelList
                items={listModels("embedding")}
                active={aux.embedding?.model ?? ""}
                empty="还没有嵌入模型 —— 不配也能用，检索会退化成关键词匹配"
                allowNone
                onNone={() => {
                  setAux({ ...aux, embedding: null });
                  saveAux({ embedding: null });
                  configureKnowledge(null);
                }}
                onPick={(item) => pickAux("embedding", item.id)}
                onEdit={(item) => setEditing({ kind: "embedding", model: item })}
                onDelete={(item) => setRemoving({ kind: "embedding", model: item })}
              />

              {aux.embedding && (
                <div className={styles.formRow}>
                  <Field label="接口地址 · 留空沿用对话模型">
                    <Input
                      value={aux.embedding.baseUrl}
                      spellCheck={false}
                      onChange={(event) => patchAux("embedding", { baseUrl: event.target.value })}
                    />
                  </Field>
                  <Field label="API Key · 留空沿用对话模型">
                    <Input
                      type="password"
                      value={aux.embedding.apiKey}
                      spellCheck={false}
                      onChange={(event) => patchAux("embedding", { apiKey: event.target.value })}
                    />
                  </Field>
                </div>
              )}
            </div>

            {/* OCR 模型 */}
            <div className={styles.group}>
              <div className={styles.groupHead}>
                <div className={styles.groupLead}>
                  <span className={styles.groupTitle}>OCR 模型</span>
                  <span className={styles.groupNote}>模型看不了图时，图片先交给它转成文字</span>
                </div>
                <Button
                  size="sm"
                  leading={<Plus size={11} />}
                  onClick={() => setEditing({ kind: "ocr" })}
                >
                  添加模型
                </Button>
              </div>

              <ModelList
                items={listModels("ocr")}
                active={aux.ocr?.model ?? ""}
                empty="还没有 OCR 模型 —— 只有「模型不能看图」时才会用到"
                allowNone
                onNone={() => {
                  setAux({ ...aux, ocr: null });
                  saveAux({ ocr: null });
                }}
                onPick={(item) => pickAux("ocr", item.id)}
                onEdit={(item) => setEditing({ kind: "ocr", model: item })}
                onDelete={(item) => setRemoving({ kind: "ocr", model: item })}
              />

              {aux.ocr && (
                <div className={styles.formRow}>
                  <Field label="接口地址 · 留空沿用对话模型">
                    <Input
                      value={aux.ocr.baseUrl}
                      spellCheck={false}
                      onChange={(event) => patchAux("ocr", { baseUrl: event.target.value })}
                    />
                  </Field>
                  <Field label="API Key · 留空沿用对话模型">
                    <Input
                      type="password"
                      value={aux.ocr.apiKey}
                      spellCheck={false}
                      onChange={(event) => patchAux("ocr", { apiKey: event.target.value })}
                    />
                  </Field>
                </div>
              )}
            </div>
          </>
        ) : (
          <>
            <div className={styles.group}>
              <span className={styles.groupTitle}>明暗</span>
              <Segmented
                ariaLabel="明暗"
                value={state.ui.mode}
                options={[
                  { value: "light", label: "浅色" },
                  { value: "dark", label: "深色" },
                ]}
                onChange={(mode) => dispatch({ type: "setMode", mode })}
              />
            </div>

            <div className={styles.group}>
              <span className={styles.groupTitle}>强调色</span>
              <div className={styles.swatches}>
                {ACCENTS.map((item) => (
                  <Tooltip key={item.key} label={item.label}>
                    <button
                      type="button"
                      className={styles.sw}
                      aria-pressed={!state.ui.accentHex && state.ui.accent === item.key}
                      onClick={() => {
                        setHex("");
                        dispatch({ type: "setAccent", accent: item.key });
                      }}
                    >
                      <i style={{ background: item.color }} />
                    </button>
                  </Tooltip>
                ))}
                {/* 自定义色：点色块弹出取色器（色相/明度/饱和 + HEX）。
                    HEX 输入在取色器里，这里不再放第二个 —— 两个框显示同一个值
                    看起来像出了 bug。 */}
                <ColorPicker
                  value={isHexColor(hex) ? hex : state.ui.accentHex ?? "#3b50b4"}
                  onChange={applyHex}
                  label="自定义强调色"
                />
              </div>
              <span className={styles.hint}>
                自定义色会覆盖预设；强调色上的文字会按亮度自动取黑或白。
              </span>
            </div>

            <div className={styles.group}>
              <span className={styles.groupTitle}>
                界面字号 · {Math.round(textScale * 100)}%
              </span>
              <Segmented
                ariaLabel="界面字号"
                value={state.ui.textScale == null ? "auto" : String(state.ui.textScale)}
                options={[
                  {
                    value: "auto",
                    label: "自动",
                    title: `跟随系统（${Math.round(systemTextScale * 100)}%）`,
                  },
                  ...TEXT_SCALES.map((item) => ({
                    value: String(item.value),
                    label: item.label,
                  })),
                ]}
                onChange={(next) =>
                  setTextScale(next === "auto" ? null : Number(next))
                }
              />
              <span className={styles.hint}>只影响界面，不影响纸面与导出。</span>
            </div>
          </>
        )}
      </div>

      {/* 底栏按 tab 分开：外观页没有「通讯层」「测试连接」这些东西 ——
          那是模型的诊断手段，出现在配色页上只会让人以为配色也要联网 */}
      <footer className={styles.foot}>
        {tab === "model" ? (
          <>
            <Tooltip label="桌面端请求走 Rust，浏览器调试走 fetch（会被跨域拦）">
              <span className={styles.route}>
                通讯层：{isTauri() ? "Rust · 不经跨域" : "浏览器 fetch · 会被跨域拦"}
              </span>
            </Tooltip>
            {/* 探针结果可能很长（网关报错原文），一句话放不下时靠提示给全文 */}
            <Tooltip label={probe.text || "配置即时保存"}>
              <span
                className={cx(
                  styles.probe,
                  probe.tone === "ok" && styles.probeOk,
                  probe.tone === "fail" && styles.probeFail,
                )}
                data-selectable
              >
                {probe.text || "配置即时保存"}
              </span>
            </Tooltip>
            <Button
              onClick={() => {
                void runProbe();
              }}
            >
              测试连接
            </Button>
          </>
        ) : (
          // 外观页只有一句说明，用 flex:1 把「完成」顶到右端
          <span className={styles.probe}>主题与字号只影响界面，不改纸面与导出</span>
        )}
        <Button variant="primary" onClick={onClose}>
          完成
        </Button>
      </footer>

      {/* 两个弹层挂在面板根节点里：设置面板本身就是个浮层，
          再 portal 出去反而会丢掉「哪一层在上面」的顺序 */}
      {editing && (
        <CreateModel
          kind={editing.kind}
          initial={editing.model}
          baseUrl={
            editing.kind === "chat" ? ai.baseUrl : aux[editing.kind]?.baseUrl || ai.baseUrl
          }
          taken={listModels(editing.kind).map((item) => item.id)}
          onCancel={() => setEditing(null)}
          onSubmit={(model, apiKey) => saveModel(editing.kind, model, apiKey)}
        />
      )}

      {removing && (
        <Confirm
          open
          title={`删除模型「${removing.model.label}」？`}
          description="只从这台机器的模型清单里移除；服务端配置和密钥都不受影响。删完这个用途会回落到「不使用」。"
          onCancel={() => setRemoving(null)}
          onConfirm={() => dropModel(removing.kind, removing.model)}
        />
      )}
    </div>
  );
}

/* ==========================================================================
   模型列表
   ========================================================================== */

/**
 * 一行一个模型：点名字选中，右侧是编辑 / 删除。
 *
 * 为什么不用原生 <select>：系统下拉在深浅两套主题里都是另一套外观（灰底、系统字体、
 * 高亮条），和应用的发丝线、令牌色完全对不上 —— 而且它没法放「编辑」「删除」。
 */
function ModelList({
  items,
  active,
  empty,
  allowNone,
  onNone,
  onPick,
  onEdit,
  onDelete,
}: {
  items: ModelConfig[];
  active: string;
  empty: string;
  /** 嵌入 / OCR 允许「不使用」：这个用途本来就是可选的 */
  allowNone?: boolean;
  onNone?: () => void;
  onPick: (model: ModelConfig) => void;
  onEdit: (model: ModelConfig) => void;
  onDelete: (model: ModelConfig) => void;
}) {
  return (
    <List>
      {allowNone && (
        <ListRow title="不使用" meta="关掉这项能力" active={!active} onSelect={onNone} />
      )}

      {items.map((item) => {
        const on = item.id === active;
        const custom = isCustomModel(item.id);
        return (
          <ListRow
            key={item.id}
            title={item.label}
            meta={`${item.provider}${custom ? " · 自定义" : ""}`}
            active={on}
            onSelect={() => onPick(item)}
            // 内置项来自 config/models.json，改不了也删不掉 —— 只有自定义项带动作
            actions={
              custom ? (
                <>
                  <Tooltip label="编辑">
                    <Button
                      variant="quiet"
                      size="sm"
                      icon
                      aria-label={`编辑 ${item.label}`}
                      onClick={() => onEdit(item)}
                    >
                      <Pencil size={11} />
                    </Button>
                  </Tooltip>
                  <Tooltip label="删除">
                    <Button
                      variant="quiet"
                      size="sm"
                      icon
                      aria-label={`删除 ${item.label}`}
                      onClick={() => onDelete(item)}
                    >
                      <Trash size={11} />
                    </Button>
                  </Tooltip>
                </>
              ) : undefined
            }
          />
        );
      })}

      {items.length === 0 && <ListEmpty>{empty}</ListEmpty>}
    </List>
  );
}

/* ==========================================================================
   新建 / 编辑自定义模型
   ========================================================================== */

/**
 * 能填的字段刻意只留「配置网关需要的」，其余走缺省：
 * 用户要的是「把公司内网的 qwen 接进来」，不是填一份模型能力调研表。
 * 视觉能力单独给一个勾选 —— 它决定图片走视觉直传还是 OCR（见 lib/attach）。
 */
function CreateModel({
  kind,
  initial,
  baseUrl,
  taken,
  onCancel,
  onSubmit,
}: {
  kind: ModelKind;
  /** 有值 = 编辑已有模型：预填并锁定 id（改 id 等于新建一条，容易产生重复） */
  initial?: ModelConfig;
  baseUrl: string;
  taken: string[];
  onCancel: () => void;
  onSubmit: (model: ModelConfig, apiKey: string) => void;
}) {
  const [label, setLabel] = useState(initial?.label ?? "");
  const [id, setId] = useState(initial?.id ?? "");
  const [url, setUrl] = useState(initial?.baseUrl || baseUrl);
  const [key, setKey] = useState("");
  const [window_, setWindow] = useState(
    String(initial?.contextWindow ?? (kind === "embedding" ? 8192 : 64000)),
  );
  const [temp, setTemp] = useState(String(initial?.defaultTemperature ?? (kind === "chat" ? 0.4 : 0)));
  const [levels, setLevels] = useState((initial?.reasoning ?? []).join(", "));
  const [vision, setVision] = useState(initial?.vision ?? kind === "ocr");
  const [dim, setDim] = useState(initial?.dim ? String(initial.dim) : "");

  const modelId = id.trim() || label.trim();
  /** 同名会覆盖：说清楚而不是拦下来 —— 改错重填一次比「先删再建」顺手 */
  const override = modelId.length > 0 && taken.includes(modelId);

  const submit = () => {
    if (!modelId) return;
    onSubmit(
      {
        id: modelId,
        label: label.trim() || modelId,
        kind,
        provider: "自定义",
        baseUrl: url.trim(),
        contextWindow: Number(window_) > 0 ? Math.round(Number(window_)) : 64_000,
        maxOutput: 8_192,
        reasoning: levels
          .split(/[,，\s]+/)
          .map((item) => item.trim())
          .filter(Boolean),
        vision,
        defaultTemperature: Math.min(2, Math.max(0, Number(temp) || 0)),
        ...(kind === "embedding" && Number(dim) > 0 ? { dim: Math.round(Number(dim)) } : {}),
      },
      key,
    );
  };

  return (
    <Modal
      title={`${initial ? "编辑" : "新建"}${
        kind === "chat" ? "对话模型" : kind === "embedding" ? "嵌入模型" : "OCR 模型"
      }`}
      subtitle="任何 OpenAI 兼容接口都可以"
      onCancel={onCancel}
      bodyClassName={styles.form}
      footer={
        <>
          <Button onClick={onCancel}>取消</Button>
          <Button variant="primary" disabled={!modelId} onClick={submit}>
            保存并使用
          </Button>
        </>
      }
    >
      <Field label="显示名称">
        <Input
          data-autofocus
          value={label}
          placeholder="例如：内网 Qwen3-32B"
          spellCheck={false}
          onChange={(event) => setLabel(event.target.value)}
        />
      </Field>
      <Field label="模型名 · 接口里的 model 字段">
        <Input
          value={id}
          placeholder="qwen3-32b"
          spellCheck={false}
          disabled={Boolean(initial)}
          onChange={(event) => setId(event.target.value)}
        />
      </Field>
      <Field label="Base URL">
        <Input
          value={url}
          placeholder="https://your-gateway/v1"
          spellCheck={false}
          onChange={(event) => setUrl(event.target.value)}
        />
      </Field>
      <Field label="API Key · 留空沿用当前密钥">
        <Input
          type="password"
          value={key}
          placeholder="sk-…"
          spellCheck={false}
          onChange={(event) => setKey(event.target.value)}
        />
      </Field>

      <div className={styles.formRow}>
        <Field label="上下文窗口">
          <Input
            value={window_}
            inputMode="numeric"
            spellCheck={false}
            onChange={(event) => setWindow(event.target.value.replace(/[^\d]/g, ""))}
          />
        </Field>
        {kind !== "ocr" && (
          <Field label="温度">
            <Input
              value={temp}
              inputMode="decimal"
              spellCheck={false}
              onChange={(event) => setTemp(event.target.value.replace(/[^\d.]/g, ""))}
            />
          </Field>
        )}
        {kind === "embedding" && (
          <Field label="向量维度 · 选填">
            <Input
              value={dim}
              inputMode="numeric"
              placeholder="1024"
              spellCheck={false}
              onChange={(event) => setDim(event.target.value.replace(/[^\d]/g, ""))}
            />
          </Field>
        )}
      </div>

      {kind === "chat" && (
        <Field label="思维强度档位 · 逗号分隔，留空表示没有">
          <Input
            value={levels}
            placeholder="low, medium, high"
            spellCheck={false}
            onChange={(event) => setLevels(event.target.value)}
          />
        </Field>
      )}

      {kind === "chat" && (
        <Checkbox checked={vision} onChange={setVision}>
          这个模型能看图（图片直接给模型，不走 OCR）
        </Checkbox>
      )}

      <p className={styles.formHint}>
        {override
          ? "这个模型名已经存在：保存会覆盖你自己建的那一条。"
          : "自定义模型只存在这台机器上，不随工作区同步；模型名与内置项重名时永远选不到内置的那条，所以会拦住。"}
      </p>
    </Modal>
  );
}
