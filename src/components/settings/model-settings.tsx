import { useMemo, useState } from "react";
import { aiFromModel, loadAi, loadAux, saveAi, saveAux, type AuxModelChoice } from "@/lib/ai";
import {
  findModel,
  isCustomModel,
  listModels,
  removeCustomModel,
  saveCustomModel,
  type ModelConfig,
  type ModelKind,
} from "@/lib/model-config";
import { configureKnowledge } from "@/lib/knowledge";
import { notifySettingsChanged } from "@/lib/settings-window";
import { Button, Field, Input, Segmented, Slider } from "@/components/ui";
import { Confirm } from "@/components/ui/confirm";
import { ChevronLeft } from "@/components/ui/icons";
import styles from "./settings.module.css";

/**
 * 模型设置（独立窗口）。
 *
 * 结构就是三层：**三个 tab（按用途分）→ 该用途的模型列表 → 单个模型的设置**。
 *
 * 之前是一个长页面把所有东西摊在一起：三组列表、三组接口地址、三组说明文字，
 * 一屏里十几个输入框，改一个温度要在里面找半天。现在同一时刻只显示一件事。
 *
 * 没有「保存」按钮：字段改完即写（localStorage + 通知主窗口）。
 * 设置页里的每一个输入都是「所见即所得」的开关，再要一次确认只是多一步。
 */

const TABS: { key: ModelKind; label: string }[] = [
  { key: "chat", label: "对话" },
  { key: "embedding", label: "嵌入" },
  { key: "ocr", label: "OCR" },
];

/** 新建草稿的默认值：按用途给一套能直接跑的缺省 */
function blank(kind: ModelKind, baseUrl: string): ModelConfig {
  return {
    kind,
    id: "",
    label: "",
    provider: "自定义",
    baseUrl,
    contextWindow: kind === "embedding" ? 8_192 : 64_000,
    maxOutput: 8_192,
    reasoning: [],
    vision: kind === "ocr",
    defaultTemperature: kind === "chat" ? 0.4 : 0,
  };
}

export function ModelSettings() {
  const [tab, setTab] = useState<ModelKind>("chat");
  const [ai, setAi] = useState(loadAi);
  const [aux, setAux] = useState(loadAux);
  /** 详情页显示哪一条；null = 停在列表 */
  const [openId, setOpenId] = useState<string | null>(null);
  /** 新建草稿：还没落盘的模型 */
  const [draft, setDraft] = useState<ModelConfig | null>(null);
  const [confirming, setConfirming] = useState<ModelConfig | null>(null);
  /** 自定义模型存在 localStorage，写完之后用它触发一次重读 */
  const [tick, setTick] = useState(0);

  const models = useMemo(() => listModels(tab), [tab, tick]);
  const activeId = tab === "chat" ? ai.model : (aux[tab]?.model ?? "");
  const editing = draft ?? (openId ? findModel(openId) ?? null : null);

  /** 对话模型的选择直接影响主窗口，写完通知它重读 */
  const writeAi = (next: typeof ai) => {
    setAi(next);
    saveAi(next);
    void notifySettingsChanged("ai");
  };

  const writeAux = (kind: "embedding" | "ocr", choice: AuxModelChoice | null) => {
    const next = { ...aux, [kind]: choice };
    setAux(next);
    saveAux({ [kind]: choice });
    if (kind === "embedding") configureKnowledge(choice);
    void notifySettingsChanged("aux");
  };

  /** 点列表一行 = 选中它并进设置 */
  const choose = (model: ModelConfig) => {
    setDraft(null);
    setOpenId(model.id);
    if (tab === "chat") {
      if (ai.model !== model.id) writeAi(aiFromModel(model, ai));
    } else if (aux[tab]?.model !== model.id) {
      writeAux(tab, { model: model.id, baseUrl: model.baseUrl, apiKey: "" });
    }
  };

  /** 修改当前模型的自身字段（名称、窗口、维度…）；网关与密钥不在这里 */
  const patchModel = (patch: Partial<ModelConfig>) => {
    if (!editing) return;
    const next = { ...editing, ...patch };
    if (draft) {
      setDraft(next);
      return;
    }
    saveCustomModel(next);
    setTick((value) => value + 1);
  };

  /** 网关与密钥始终属于「这个用途」，不跟着某一条模型走 —— 否则换模型就得重填一次 */
  const patchEndpoint = (patch: { baseUrl?: string; apiKey?: string }) => {
    if (tab === "chat") {
      writeAi({ ...ai, ...patch });
      return;
    }
    const current = aux[tab];
    if (!current) return;
    writeAux(tab, { ...current, ...patch });
  };

  const commitDraft = () => {
    if (!draft) return;
    const id = draft.id.trim();
    if (!id) return;
    const model = { ...draft, label: draft.label.trim() || id };
    saveCustomModel(model);
    setDraft(null);
    setTick((value) => value + 1);
    choose(model);
  };

  const drop = (model: ModelConfig) => {
    setConfirming(null);
    removeCustomModel(model.id);
    setOpenId(null);
    setTick((value) => value + 1);
    if (tab === "chat" && ai.model === model.id) {
      // 删掉的正是当前在用的：退到第一条内置，别留一个查不到的 id
      const fallback = listModels("chat").find((item) => !isCustomModel(item.id));
      if (fallback) writeAi(aiFromModel(fallback, ai));
    } else if (tab !== "chat" && aux[tab]?.model === model.id) {
      writeAux(tab, null);
    }
  };

  const back = () => {
    setDraft(null);
    setOpenId(null);
  };

  const endpoint = tab === "chat"
    ? { baseUrl: ai.baseUrl, apiKey: ai.apiKey }
    : { baseUrl: aux[tab]?.baseUrl ?? "", apiKey: aux[tab]?.apiKey ?? "" };

  /* ---------------- 详情 ---------------- */

  if (editing) {
    const custom = draft !== null || isCustomModel(editing.id);
    const isDraft = draft !== null;
    return (
      <div className={styles.page}>
        <header className={styles.head}>
          <Button variant="quiet" size="sm" icon aria-label="返回列表" onClick={back}>
            <ChevronLeft size={12} />
          </Button>
          <b>{isDraft ? "添加模型" : editing.label}</b>
        </header>

        <div className={styles.fields}>
          <Field label="显示名称">
            <Input
              data-autofocus
              value={editing.label}
              placeholder={isDraft ? "例如：内网 Qwen3" : undefined}
              disabled={!custom}
              onChange={(event) => patchModel({ label: event.target.value })}
            />
          </Field>
          <Field label="模型名">
            <Input
              value={editing.id}
              placeholder="qwen3-32b"
              spellCheck={false}
              // 已保存的自定义模型不允许改 id：改 id 等于另建一条，会留下一个孤儿
              disabled={!isDraft && !custom ? true : !isDraft}
              onChange={(event) => patchModel({ id: event.target.value })}
            />
          </Field>

          <Field label="接口地址" wide>
            <Input
              value={endpoint.baseUrl}
              spellCheck={false}
              placeholder="https://your-gateway/v1"
              onChange={(event) => patchEndpoint({ baseUrl: event.target.value })}
            />
          </Field>
          <Field label="API Key" wide>
            <Input
              type="password"
              value={endpoint.apiKey}
              spellCheck={false}
              placeholder={tab === "chat" ? "sk-…" : "留空沿用对话模型"}
              onChange={(event) => patchEndpoint({ apiKey: event.target.value })}
            />
          </Field>

          {tab === "chat" && (
            <Field label={`温度 · ${ai.temperature.toFixed(2)}`}>
              <Slider
                ariaLabel="温度"
                min={0}
                max={1}
                step={0.05}
                value={ai.temperature}
                onChange={(value) => writeAi({ ...ai, temperature: value })}
              />
            </Field>
          )}

          <Field label="上下文窗口">
            <Input
              value={String(editing.contextWindow)}
              inputMode="numeric"
              disabled={!custom}
              onChange={(event) =>
                patchModel({ contextWindow: Number(event.target.value.replace(/\D/g, "")) || 0 })
              }
            />
          </Field>

          {tab === "chat" && editing.reasoning.length > 0 && (
            <Field label="思维强度" wide>
              <Segmented
                ariaLabel="思维强度"
                value={ai.reasoning ?? editing.reasoning[0] ?? ""}
                options={editing.reasoning.map((level) => ({ value: level, label: level }))}
                onChange={(level) => writeAi({ ...ai, reasoning: level })}
              />
            </Field>
          )}
        </div>

        <div className={styles.actions}>
          {isDraft ? (
            <Button variant="primary" disabled={!draft?.id.trim()} onClick={commitDraft}>
              添加
            </Button>
          ) : (
            custom && (
              <Button variant="quiet" onClick={() => setConfirming(editing)}>
                删除这个模型
              </Button>
            )
          )}
        </div>

        {confirming && (
          <Confirm
            open
            title={`删除「${confirming.label}」？`}
            description="只从这台机器的模型清单里移除，网关与密钥不受影响。"
            onCancel={() => setConfirming(null)}
            onConfirm={() => drop(confirming)}
          />
        )}
      </div>
    );
  }

  /* ---------------- 列表 ---------------- */

  return (
    <div className={styles.page}>
      <nav className={styles.tabs} role="tablist">
        {TABS.map((item) => (
          <button
            key={item.key}
            type="button"
            role="tab"
            aria-selected={tab === item.key}
            onClick={() => {
              setTab(item.key);
              setOpenId(null);
              setDraft(null);
            }}
          >
            {item.label}
          </button>
        ))}
      </nav>

      <div className={styles.list}>
        {tab !== "chat" && (
          <button
            type="button"
            className={cxRow(!activeId)}
            onClick={() => {
              writeAux(tab, null);
              setOpenId(null);
            }}
          >
            <span className={styles.rowName}>不使用</span>
          </button>
        )}

        {models.map((model) => (
          <button
            key={model.id}
            type="button"
            className={cxRow(model.id === activeId)}
            onClick={() => choose(model)}
          >
            <span className={styles.rowName}>{model.label}</span>
            <span className={styles.rowMeta}>{model.provider}</span>
          </button>
        ))}

        {/* 添加是列表的最后一项：这一页就是「模型清单」，加东西当然在清单末尾 */}
        <button
          type="button"
          className={`${styles.row} ${styles.rowAdd}`}
          onClick={() => {
            setOpenId(null);
            setDraft(blank(tab, endpoint.baseUrl || ai.baseUrl));
          }}
        >
          <span className={styles.rowName}>添加模型</span>
        </button>
      </div>
    </div>
  );
}

function cxRow(on: boolean) {
  return on ? `${styles.row} ${styles.rowOn}` : styles.row;
}
