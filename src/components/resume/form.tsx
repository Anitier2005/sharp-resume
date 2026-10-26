import { useState } from "react";
import type { Field, Person, ResumePage, SectionEntry } from "@/types";
import { useToast } from "@/components/ui/toast-context";
import { Dropdown, MenuItem } from "@/components/ui/menu";
import { ChevronRight, Close, Plus, ArrowUp, ArrowDown } from "@/components/ui/icons";
import { FieldsEditor } from "@/components/ui/fields";
import { AssetsEditor } from "@/components/ui/assets";
import { uid } from "@/lib/uid";
import { displayName, fieldText } from "@/types";
import { cx } from "@/lib/cx";
import styles from "./form.module.css";

/**
 * 编辑态表单：一切皆字段。
 *
 * 页头 = 一组字段；分区 = 用户自己起名 + 一组条目；条目 = 一组字段 + 措辞。
 * 这里不出现任何「姓名 / 公司 / 时间」的预设键。
 */
export interface FormProps {
  person: Person;
  page: ResumePage;
  onHeader: (header: Field[]) => void;
  onLabel: (label: string) => void;
  onAddSection: (title: string) => void;
  onRemoveSection: (sectionId: string) => void;
  onSectionTitle: (sectionId: string, title: string) => void;
  onSectionItems: (sectionId: string, items: SectionEntry[]) => void;
  onEntryPatch: (entryId: string, patch: Partial<SectionEntry>) => void;
}

export function ResumeForm({
  person,
  page,
  onHeader,
  onLabel,
  onAddSection,
  onRemoveSection,
  onSectionTitle,
  onSectionItems,
  onEntryPatch,
}: FormProps) {
  const toast = useToast();
  const [open, setOpen] = useState<Record<string, boolean>>({ header: true });

  const toggle = (key: string) => setOpen((prev) => ({ ...prev, [key]: !prev[key] }));
  const isOpen = (key: string, fallback = false) => open[key] ?? fallback;

  const addFromLibrary = (sectionId: string, refId: string) => {
    const section = page.sections.find((item) => item.id === sectionId);
    if (!section) return;
    if (section.items.some((entry) => entry.refId === refId)) {
      toast("这条已经在本页了");
      return;
    }
    onSectionItems(sectionId, [
      ...section.items,
      { id: uid("se"), refId, fields: [], bullets: [] },
    ]);
  };

  return (
    <div className={styles.form} data-print="hide">
      <div className={styles.scroll}>
        {/* 页签名字：用户自己起 */}
        <section className={styles.labelRow}>
          <span className={styles.labelKey}>页名</span>
          <input
            value={page.label}
            spellCheck={false}
            placeholder="给这一页起个名字"
            onChange={(event) => onLabel(event.target.value)}
          />
        </section>

        <div className={styles.sec}>
          <button type="button" className={styles.head} onClick={() => toggle("header")}>
            <ChevronRight size={10} className={cx(styles.chev, isOpen("header", true) && styles.chevOn)} />
            <b>页头</b>
            <span className={styles.hint}>第一行作标题，其余依次排开</span>
            <span className={styles.cnt}>{page.header.length} 项</span>
          </button>
          {isOpen("header", true) && (
            <div className={styles.body}>
              <FieldsEditor fields={page.header} onChange={onHeader} />
            </div>
          )}
        </div>

        {page.sections.map((section) => {
          const opened = isOpen(section.id, true);
          return (
            <div className={styles.sec} key={section.id}>
              <div className={styles.sectionHead}>
                <button type="button" className={styles.head} onClick={() => toggle(section.id)}>
                  <ChevronRight size={10} className={cx(styles.chev, opened && styles.chevOn)} />
                  <input
                    className={styles.sectionTitle}
                    value={section.title}
                    spellCheck={false}
                    placeholder="分区标题，例如 工作经历 / 获奖 / 开源"
                    onClick={(event) => event.stopPropagation()}
                    onChange={(event) => onSectionTitle(section.id, event.target.value)}
                  />
                  <span className={styles.cnt}>{section.items.length} 条</span>
                </button>
                <button
                  type="button"
                  className={styles.remove}
                  aria-label="删除分区"
                  onClick={() => onRemoveSection(section.id)}
                >
                  <Close size={9} />
                </button>
              </div>

              {opened && (
                <div className={styles.body}>
                  {section.items.map((entry, index) => {
                    const source = person.items.find((item) => item.id === entry.refId);
                    return (
                      <div className={styles.entry} key={entry.id}>
                        <div className={styles.entryHead}>
                          <span className={styles.idx}>{String(index + 1).padStart(2, "0")}</span>
                          <span className={styles.who}>
                            {source ? displayName(source.fields) : "本页自有条目"}
                          </span>
                          <span className={styles.acts}>
                            <button
                              type="button"
                              aria-label="上移"
                              disabled={index === 0}
                              onClick={() => {
                                const next = [...section.items];
                                [next[index - 1], next[index]] = [next[index], next[index - 1]];
                                onSectionItems(section.id, next);
                              }}
                            >
                              <ArrowUp size={9} />
                            </button>
                            <button
                              type="button"
                              aria-label="下移"
                              disabled={index === section.items.length - 1}
                              onClick={() => {
                                const next = [...section.items];
                                [next[index + 1], next[index]] = [next[index], next[index + 1]];
                                onSectionItems(section.id, next);
                              }}
                            >
                              <ArrowDown size={9} />
                            </button>
                            <button
                              type="button"
                              aria-label="从本页移除"
                              onClick={() =>
                                onSectionItems(
                                  section.id,
                                  section.items.filter((item) => item.id !== entry.id),
                                )
                              }
                            >
                              <Close size={9} />
                            </button>
                          </span>
                        </div>

                        {/* 引用库条目时，字段默认继承库里的那份；
                            一旦在本页加了字段，这一条就有自己的字段，不再跟着库走。 */}
                        {entry.fields.length === 0 && source && source.fields.length > 0 ? (
                          <div className={styles.inherited}>
                            <span className={styles.inheritedK}>
                              字段来自库里那一条（在左侧资料里改）
                            </span>
                            {source.fields.map((field) => (
                              <div className={styles.inheritedRow} key={field.id}>
                                {fieldText(field)}
                              </div>
                            ))}
                            <button
                              type="button"
                              className={styles.addRow}
                              onClick={() => onEntryPatch(entry.id, { fields: source.fields.map((field) => ({ ...field, id: uid("f") })) })}
                            >
                              在本页另写一份
                            </button>
                          </div>
                        ) : (
                          <FieldsEditor
                            fields={entry.fields}
                            onChange={(fields) => onEntryPatch(entry.id, { fields })}
                            addLabel="加一个字段"
                          />
                        )}

                        <div className={styles.bullets}>
                          {entry.bullets.map((bullet, at) => (
                            <div className={styles.bulletRow} key={at}>
                              <span className={styles.dot} />
                              <textarea
                                rows={2}
                                value={bullet}
                                spellCheck={false}
                                placeholder="这个岗位要强调什么"
                                onChange={(event) => {
                                  const next = entry.bullets.map((line, i) =>
                                    i === at ? event.target.value : line,
                                  );
                                  onEntryPatch(entry.id, { bullets: next });
                                }}
                              />
                              <button
                                type="button"
                                aria-label="删掉这一句"
                                onClick={() =>
                                  onEntryPatch(entry.id, {
                                    bullets: entry.bullets.filter((_, i) => i !== at),
                                  })
                                }
                              >
                                <Close size={9} />
                              </button>
                            </div>
                          ))}
                          <button
                            type="button"
                            className={styles.addRow}
                            onClick={() => onEntryPatch(entry.id, { bullets: [...entry.bullets, ""] })}
                          >
                            <Plus size={9} /> 加一句
                          </button>
                        </div>

                        {/* 这一条自己带的材料：证书扫描件、作品截图、外链 */}
                        <AssetsEditor
                          assets={entry.assets ?? []}
                          onChange={(assets) => onEntryPatch(entry.id, { assets })}
                          addLabel="加材料"
                        />
                      </div>
                    );
                  })}

                  <div className={styles.btnRow}>
                    <button
                      type="button"
                      className={styles.addRow}
                      onClick={() =>
                        onSectionItems(section.id, [
                          ...section.items,
                          { id: uid("se"), fields: [], bullets: [] },
                        ])
                      }
                    >
                      <Plus size={9} /> 新建条目
                    </button>
                    <Dropdown
                      width={200}
                      trigger={
                        <button type="button" className={styles.addRow}>
                          <Plus size={9} /> 从资料里引用
                        </button>
                      }
                    >
                      {person.items.length === 0 ? (
                        <MenuItem
                          label="资料里还没有条目"
                          onSelect={() => toast("先在左侧「资料」里加一条")}
                        />
                      ) : (
                        person.items.map((item) => (
                          <MenuItem
                            key={item.id}
                            label={displayName(item.fields)}
                            onSelect={() => addFromLibrary(section.id, item.id)}
                          />
                        ))
                      )}
                    </Dropdown>
                  </div>
                </div>
              )}
            </div>
          );
        })}

        <button type="button" className={styles.addSection} onClick={() => onAddSection("")}>
          <Plus size={10} /> 加一个分区
        </button>
      </div>
    </div>
  );
}
