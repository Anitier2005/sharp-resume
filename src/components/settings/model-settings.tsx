import { useMemo, useState, type ChangeEvent } from "react";
import {
  aiFromModel,
  loadAi,
  loadAux,
  saveAi,
  saveAux,
  type AiSettings,
  type AuxModelChoice,
} from "@/lib/ai";
import {
  CONTEXT_PRESETS,
  defaultProfile,
  findModel,
  isCustomModel,
  listModels,
  kindOf,
  listProfiles,
  removeCustomModel,
  removeProfile,
  saveCustomModel,
  saveProfile,
  type ModelConfig,
  type ModelKind,
  type ModelProfile,
} from "@/lib/model-config";
import { configureKnowledge } from "@/lib/knowledge";
import { notifySettingsChanged } from "@/lib/settings-window";
import { Button, Field, Input, ListEmpty, PasswordInput, Select, Tooltip } from "@/components/ui";
import { Confirm } from "@/components/ui/confirm";
import { Check, ChevronLeft, Gear, Plus, Trash } from "@/components/ui/icons";
import { cx } from "@/lib/cx";
import styles from "./settings.module.css";

/**
 * 模型设置（独立窗口）。
 *
 * 三层：**三个 tab（按用途）→ 该用途的模型列表 → 单个模型的设置**。
 *
 * 列表里「点行 = 选中这个模型」，「行尾齿轮 = 进它的设置」——
 * 这两件事以前挤在同一个点击上：想看看某条配置，一点就把当前模型换掉了。
 *
 * 每个模型下可以有若干**配置**（官方 / 公司代理 / 临时 key）。配置是一组
 * 「网关 + 密钥 + 参数」，切换只换指针，不用重填。字段改完即写，没有保存按钮。
 */

const TABS: { key: ModelKind; label: string }[] = [
  { key: "chat", label: "对话" },
  { key: "embedding", label: "嵌入" },
  { key: "ocr", label: "OCR" },
];

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
  /**
   * 详情页显示哪个模型；null = 停在列表。
   * 支持 `?open=<modelId>` 直接落到某个模型的详情（窗口可以被外部指到具体一条）。
   */
  const [openId, setOpenId] = useState<string | null>(() =>
    new URLSearchParams(window.location.search).get("open"),
  );
  /** 新建草稿：还没落盘的模型 */
  const [draft, setDraft] = useState<ModelConfig | null>(null);
  const [dropModel, setDropModel] = useState<ModelConfig | null>(null);
  const [dropProfile, setDropProfile] = useState<ModelProfile | null>(null);
  /** 模型与配置都存在 localStorage，写完之后用它触发一次重读 */
  const [tick, setTick] = useState(0);

  const models = useMemo(() => listModels(tab), [tab, tick]);
  const activeId = tab === "chat" ? ai.model : (aux[tab]?.model ?? "");
  const activeProfileId = tab === "chat" ? ai.profileId : aux[tab]?.profileId;

  const editing = draft ?? (openId ? (findModel(openId) ?? null) : null);
  const isDraft = draft !== null;
  const custom = editing !== null && (isDraft || isCustomModel(editing.id));
  const profiles = useMemo(
    () => (editing && !isDraft ? listProfiles(editing) : []),
    // tick 变了说明磁盘上的配置变了，必须重算
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [editing, tick, isDraft],
  );
  const current = profiles.find((item) => item.id === activeProfileId) ?? profiles[0] ?? null;

  /* ---------------- 写入 ---------------- */

  const writeAi = (next: AiSettings) => {
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

  /** 把某套配置落到运行时设置上（对话写 ai，嵌入/OCR 写 aux） */
  const applyToRuntime = (model: ModelConfig, profile: ModelProfile) => {
    if (tab === "chat") {
      writeAi({
        ...ai,
        model: model.id,
        profileId: profile.id,
        baseUrl: profile.baseUrl || model.baseUrl,
        apiKey: profile.apiKey,
        temperature: profile.temperature ?? model.defaultTemperature,
        reasoning: profile.reasoning ?? model.reasoning[0],
        contextWindow: profile.contextWindow ?? model.contextWindow,
      });
    } else {
      writeAux(tab, {
        model: model.id,
        profileId: profile.id,
        baseUrl: profile.baseUrl,
        apiKey: profile.apiKey,
      });
    }
  };

  /** 点列表一行 = 选中这个模型（用它当前生效的那套配置） */
  const choose = (model: ModelConfig) => {
    const list = listProfiles(model);
    let profile = list.find((item) => item.id === activeProfileId) ?? list[0];
    if (!profile) {
      // 配置被删光了的模型：选中时补一套默认的，否则选中了也没有可用网关
      profile = defaultProfile(model);
      saveProfile(model.id, profile);
      setTick((value) => value + 1);
    }
    applyToRuntime(model, profile);
  };

  /**
   * 正在看配置是否就是当前生效的那套。
   *
   * 老数据里 `ai` 没有 profileId（那时还没有「配置」这个概念），
   * 这时把第一条当作生效的那条 —— 否则第一次打开时没有任何一条打勾，
   * 用户会以为「当前配置」是空的。
   */
  const liveProfileId =
    editing && activeId === editing.id ? (activeProfileId ?? profiles[0]?.id) : null;
  const isLive = (profile: ModelProfile) =>
    editing !== null && activeId === editing.id && liveProfileId === profile.id;

  /** 改配置字段：写盘；如果它正在生效，同时同步到运行时 */
  const patchProfile = (profile: ModelProfile, patch: Partial<ModelProfile>) => {
    if (!editing) return;
    const next = { ...profile, ...patch };
    saveProfile(editing.id, next);
    setTick((value) => value + 1);
    if (isLive(profile)) applyToRuntime(editing, next);
  };

  /** 选中某套配置 */
  const activate = (profile: ModelProfile) => {
    if (!editing) return;
    applyToRuntime(editing, profile);
  };

  const addProfile = () => {
    if (!editing) return;
    const profile: ModelProfile = {
      id: `p-${Date.now().toString(36)}`,
      label: `配置 ${profiles.length + 1}`,
      baseUrl: editing.baseUrl,
      apiKey: "",
      temperature: editing.defaultTemperature,
      reasoning: editing.reasoning[0],
      contextWindow: editing.contextWindow,
    };
    saveProfile(editing.id, profile);
    setTick((value) => value + 1);
    // 新加的这套直接生效：用户点「添加配置」就是想马上用它
    applyToRuntime(editing, profile);
  };

  /* ---------------- 模型本身 ---------------- */

  const patchModel = (patch: Partial<ModelConfig>) => {
    if (!editing) return;
    const next = { ...editing, ...patch };
    if (isDraft) {
      setDraft(next);
      return;
    }
    saveCustomModel(next);
    setTick((value) => value + 1);
  };

  const commitDraft = () => {
    if (!draft) return;
    const id = draft.id.trim();
    if (!id) return;
    const model = { ...draft, label: draft.label.trim() || id };
    saveCustomModel(model);
    const profile = defaultProfile(model);
    saveProfile(model.id, profile);
    setDraft(null);
    setTick((value) => value + 1);
    setOpenId(model.id);
    applyToRuntime(model, profile);
  };

  const drop = (model: ModelConfig) => {
    setDropModel(null);
    removeCustomModel(model.id);
    setOpenId(null);
    setTick((value) => value + 1);
    if (tab === "chat" && ai.model === model.id) {
      const fallback = listModels("chat").find((item) => !isCustomModel(item.id));
      if (fallback) {
        const first = listProfiles(fallback)[0];
        if (first) writeAi(aiFromModel(fallback, ai));
      }
    } else if (tab !== "chat" && aux[tab]?.model === model.id) {
      writeAux(tab, null);
    }
  };

  const back = () => {
    setDraft(null);
    setOpenId(null);
  };

  /* ---------------- 详情 ---------------- */

  if (editing) {
    const kindLabel = editing.kind === "embedding" ? "嵌入" : editing.kind === "ocr" ? "OCR" : "对话";
    return (
      <div className={styles.page}>
        <header className={styles.head}>
          <Button variant="quiet" size="sm" icon aria-label="返回列表" onClick={back}>
            <ChevronLeft size={12} />
          </Button>
          <b>{isDraft ? "添加模型" : editing.label}</b>
          {!isDraft && custom && (
            <Button variant="quiet" size="sm" onClick={() => setDropModel(editing)}>
              删除模型
            </Button>
          )}
        </header>

        <section className={styles.group}>
          <span className={styles.groupTitle}>模型 · {kindLabel}</span>
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
                // 保存后的自定义模型不允许改 id：改 id 等于另建一条，会留下孤儿配置
                disabled={!isDraft}
                onChange={(event) => patchModel({ id: event.target.value })}
              />
            </Field>
            {editing.kind !== "ocr" && custom && (
              <Field label="思维强度档位 · 逗号分隔" wide>
                <Input
                  value={editing.reasoning.join(", ")}
                  placeholder="low, medium, high"
                  spellCheck={false}
                  onChange={(event) =>
                    patchModel({
                      reasoning: event.target.value
                        .split(/[,，\s]+/)
                        .map((item) => item.trim())
                        .filter(Boolean),
                    })
                  }
                />
              </Field>
            )}
          </div>
        </section>

        {isDraft ? (
          <DraftProfile draft={draft!} onCommit={commitDraft} />
        ) : (
          <>
            <section className={styles.group}>
              <div className={styles.groupHead}>
                <span className={styles.groupTitle}>配置</span>
                <Button size="sm" leading={<Plus size={10} />} onClick={addProfile}>
                  添加配置
                </Button>
              </div>
              <div className={styles.list}>
                {profiles.map((profile) => {
                  const live = isLive(profile);
                  return (
                    <div
                      key={profile.id}
                      className={cx(styles.row, styles.rowStatic, live && styles.rowOn)}
                    >
                      <button
                        type="button"
                        className={styles.rowMain}
                        onClick={() => activate(profile)}
                      >
                        <span className={styles.rowName}>{profile.label}</span>
                        {live && <Check size={10} className={styles.rowCheck} />}
                      </button>
                      <Tooltip label="删除这组配置">
                        <Button
                          variant="quiet"
                          size="xs"
                          icon
                          aria-label={`删除配置 ${profile.label}`}
                          onClick={() => setDropProfile(profile)}
                        >
                          <Trash size={10} />
                        </Button>
                      </Tooltip>
                    </div>
                  );
                })}
                {profiles.length === 0 && <ListEmpty>还没有配置，点右上「添加配置」</ListEmpty>}
              </div>
            </section>

            {current && (
              <section className={styles.group}>
                <span className={styles.groupTitle}>
                  配置内容{isLive(current) ? " · 正在使用" : ""}
                </span>
                <div className={styles.fields}>
                  <Field label="配置名称">
                    <Input
                      value={current.label}
                      onChange={(event: ChangeEvent<HTMLInputElement>) =>
                        patchProfile(current, { label: event.target.value })
                      }
                    />
                  </Field>
                  <Field label="上下文长度">
                    <Select
                      ariaLabel="上下文长度"
                      value={String(current.contextWindow ?? editing.contextWindow)}
                      options={contextOptions(current.contextWindow ?? editing.contextWindow)}
                      onChange={(value) =>
                        patchProfile(current, { contextWindow: Number(value) })
                      }
                    />
                  </Field>
                  <Field label="接口地址" wide>
                    <Input
                      value={current.baseUrl}
                      spellCheck={false}
                      placeholder="https://your-gateway/v1"
                      onChange={(event) => patchProfile(current, { baseUrl: event.target.value })}
                    />
                  </Field>
                  <Field label="API Key" wide>
                    <PasswordInput
                      value={current.apiKey}
                      spellCheck={false}
                      placeholder={tab === "chat" ? "sk-…" : "留空沿用对话模型"}
                      onChange={(event) => patchProfile(current, { apiKey: event.target.value })}
                    />
                  </Field>
                  {kindOf(editing) === "chat" && (
                    <>
                      <Field label="温度">
                        <Input
                          value={String(current.temperature ?? editing.defaultTemperature)}
                          inputMode="decimal"
                          onChange={(event) => {
                            const next = Number(event.target.value.replace(/[^\d.]/g, ""));
                            // 空输入时先不写：写 0 会把用户正在输入的「0.7」变成 0
                            if (event.target.value.trim() === "") return;
                            patchProfile(current, {
                              temperature: Number.isFinite(next)
                                ? Math.min(2, Math.max(0, next))
                                : undefined,
                            });
                          }}
                        />
                      </Field>
                      {editing.reasoning.length > 0 && (
                        <Field label="思维强度">
                          <Select
                            ariaLabel="思维强度"
                            value={current.reasoning ?? editing.reasoning[0] ?? ""}
                            options={editing.reasoning.map((level) => ({
                              value: level,
                              label: level,
                            }))}
                            onChange={(value) => patchProfile(current, { reasoning: value })}
                          />
                        </Field>
                      )}
                    </>
                  )}
                </div>
              </section>
            )}
          </>
        )}

        {dropModel && (
          <Confirm
            open
            title={`删除模型「${dropModel.label}」？`}
            description="模型和它下面的所有配置都会从这台机器上移除。"
            onCancel={() => setDropModel(null)}
            onConfirm={() => drop(dropModel)}
          />
        )}

        {dropProfile && editing && (
          <Confirm
            open
            title={`删除配置「${dropProfile.label}」？`}
            description="只删这一组网关与密钥，模型本身保留。"
            onCancel={() => setDropProfile(null)}
            onConfirm={() => {
              removeProfile(editing.id, dropProfile.id);
              setDropProfile(null);
              setTick((value) => value + 1);
            }}
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
            className={cx(styles.row, !activeId && styles.rowOn)}
            onClick={() => {
              writeAux(tab, null);
              setOpenId(null);
            }}
          >
            <span className={styles.rowName}>不使用</span>
          </button>
        )}

        {models.map((model) => {
          const on = model.id === activeId;
          const count = listProfiles(model).length;
          return (
            <div key={model.id} className={cx(styles.row, styles.rowStatic, on && styles.rowOn)}>
              {/* 点主体 = 选中；右侧齿轮 = 进设置。两件事不能挤在同一个点击上 */}
              <button type="button" className={styles.rowMain} onClick={() => choose(model)}>
                <span className={styles.rowName}>{model.label}</span>
                <span className={styles.rowMeta}>{model.provider}</span>
                {count > 1 && <span className={styles.rowMeta}>{count} 组配置</span>}
                {on && <Check size={10} className={styles.rowCheck} />}
              </button>
              <Tooltip label="设置">
                <Button
                  variant="quiet"
                  size="xs"
                  icon
                  aria-label={`打开 ${model.label} 的设置`}
                  onClick={() => {
                    setDraft(null);
                    setOpenId(model.id);
                  }}
                >
                  <Gear size={11} />
                </Button>
              </Tooltip>
            </div>
          );
        })}

        {/* 添加是列表的最后一项：这一页就是「模型清单」，加东西当然在清单末尾 */}
        <button
          type="button"
          className={`${styles.row} ${styles.rowAdd}`}
          onClick={() => {
            setOpenId(null);
            setDraft(blank(tab, tab === "chat" ? ai.baseUrl : (aux[tab]?.baseUrl ?? ai.baseUrl)));
          }}
        >
          <span className={styles.rowName}>添加模型</span>
        </button>
      </div>
    </div>
  );
}

/* ==========================================================================
   新建模型时的第一组配置
   ========================================================================== */

function DraftProfile({ draft, onCommit }: { draft: ModelConfig; onCommit: () => void }) {
  const [profile, setProfile] = useState<ModelProfile>(() => ({
    ...defaultProfile(draft),
    label: "默认",
  }));

  return (
    <>
      <section className={styles.group}>
        <span className={styles.groupTitle}>第一组配置</span>
        <div className={styles.fields}>
          <Field label="配置名称">
            <Input
              value={profile.label}
              onChange={(event: ChangeEvent<HTMLInputElement>) =>
                setProfile({ ...profile, label: event.target.value })
              }
            />
          </Field>
          <Field label="上下文长度">
            <Select
              ariaLabel="上下文长度"
              value={String(profile.contextWindow ?? draft.contextWindow)}
              options={contextOptions(profile.contextWindow ?? draft.contextWindow)}
              onChange={(value) => setProfile({ ...profile, contextWindow: Number(value) })}
            />
          </Field>
          <Field label="接口地址" wide>
            <Input
              value={profile.baseUrl}
              spellCheck={false}
              placeholder="https://your-gateway/v1"
              onChange={(event) => setProfile({ ...profile, baseUrl: event.target.value })}
            />
          </Field>
          <Field label="API Key" wide>
            <PasswordInput
              value={profile.apiKey}
              spellCheck={false}
              placeholder="sk-…"
              onChange={(event) => setProfile({ ...profile, apiKey: event.target.value })}
            />
          </Field>
        </div>
      </section>

      <div className={styles.actions}>
        <Button variant="primary" disabled={!draft.id.trim()} onClick={onCommit}>
          添加
        </Button>
      </div>
    </>
  );
}

/** 下拉档位：模型自带的值如果不在预设里，补一条进去，免得显示成空 */
function contextOptions(value: number) {
  const options: { value: string; label: string }[] = CONTEXT_PRESETS.map((item) => ({
    value: String(item.value),
    label: String(item.label),
  }));
  if (!options.some((item) => item.value === String(value))) {
    options.push({ value: String(value), label: `${Math.round(value / 1000)}K` });
  }
  return options;
}
