import type { ReactNode } from "react";
import { GripVertical, Sparkles } from "lucide-react";
import { toast } from "sonner";

import { Editable } from "@/components/editable";
import { cn } from "@/lib/utils";
import type { PortfolioDoc, ResumeDoc } from "@/types";

const COVERS: Record<string, string> = {
  violet: "from-[#6d5efc] via-[#8b7bff] to-[#c4b5fd]",
  teal: "from-[#0f766e] via-[#14b8a6] to-[#5eead4]",
  amber: "from-[#b45309] via-[#f59e0b] to-[#fcd34d]",
  rose: "from-[#9f1239] via-[#f43f5e] to-[#fda4af]",
};

function PaperBlock({
  children,
  editing,
  className,
}: {
  children: ReactNode;
  editing: boolean;
  className?: string;
}) {
  return (
    <section className={cn("group/block relative", className)}>
      {children}
      {editing && (
        <div className="absolute -right-8 top-0 flex flex-col gap-0.5 opacity-0 transition-opacity group-hover/block:opacity-100 focus-within:opacity-100">
          <button
            type="button"
            aria-label="用 AI 改写这一段"
            onClick={() => toast("AI 改写这一段", { description: "选中文本后可以在会话区直接下指令。" })}
            className="grid size-6 place-items-center rounded-[5px] border border-neutral-200 bg-white text-neutral-400 shadow-sm transition-colors hover:border-[#6d5efc]/40 hover:text-[#6d5efc]"
          >
            <Sparkles className="size-3" />
          </button>
          <button
            type="button"
            aria-label="拖动排序"
            onClick={() => toast("拖动排序", { description: "功能开发阶段支持整段拖拽。" })}
            className="grid size-6 place-items-center rounded-[5px] border border-neutral-200 bg-white text-neutral-400 shadow-sm transition-colors hover:text-neutral-600"
          >
            <GripVertical className="size-3" />
          </button>
        </div>
      )}
    </section>
  );
}

function SectionTitle({ children }: { children: ReactNode }) {
  return (
    <div className="mb-2 flex items-center gap-2.5">
      <h2 className="text-[9.5px] font-semibold uppercase tracking-[0.22em] text-neutral-900">
        {children}
      </h2>
      <span className="h-px flex-1 bg-neutral-200" />
    </div>
  );
}

function PaperFooter({ left, page }: { left: string; page: string }) {
  return (
    <footer className="mt-7 flex items-center justify-between border-t border-neutral-100 pt-2 text-[8px] uppercase tracking-[0.14em] text-neutral-400">
      <span>{left}</span>
      <span className="tabular-nums">{page}</span>
    </footer>
  );
}

export function ResumeDocument({
  doc,
  editing,
  onChange,
  pageIndex,
  pageTotal,
}: {
  doc: ResumeDoc;
  editing: boolean;
  onChange: (next: ResumeDoc) => void;
  pageIndex: number;
  pageTotal: number;
}) {
  const isEn = doc.kind === "resume-en";

  const update = (patch: Partial<ResumeDoc>) => onChange({ ...doc, ...patch });

  return (
    <article className="resume-paper px-[54px] pb-[36px] pt-[46px] text-neutral-900">
      <header className="flex items-start justify-between gap-7">
        <div className="min-w-0">
          <Editable
            as="h1"
            value={doc.name}
            onChange={(value) => update({ name: value })}
            editing={editing}
            multiline={false}
            ariaLabel="姓名"
            className={cn(
              "font-serif text-[34px] font-semibold leading-[1.12] text-neutral-900",
              isEn ? "tracking-[-0.02em]" : "tracking-[0.06em]",
            )}
          />
          <Editable
            value={doc.title}
            onChange={(value) => update({ title: value })}
            editing={editing}
            multiline={false}
            ariaLabel="求职意向"
            className="mt-2 text-[12px] text-neutral-600"
          />
        </div>

        <div className="shrink-0 space-y-0.5 pt-1.5 text-right">
          {doc.meta.map((item, index) => (
            <Editable
              key={index}
              value={item}
              onChange={(value) =>
                update({ meta: doc.meta.map((meta, i) => (i === index ? value : meta)) })
              }
              editing={editing}
              multiline={false}
              ariaLabel={`联系方式 ${index + 1}`}
              className="text-[10px] leading-[1.75] text-neutral-500"
            />
          ))}
        </div>
      </header>

      <div className="mt-5 h-px bg-neutral-200" />

      <PaperBlock editing={editing} className="mt-4">
        <SectionTitle>{isEn ? "Summary" : "个人简介"}</SectionTitle>
        <Editable
          value={doc.summary}
          onChange={(value) => update({ summary: value })}
          editing={editing}
          ariaLabel="个人简介"
          className="text-[11.5px] leading-[1.95] text-neutral-700"
        />
      </PaperBlock>

      <PaperBlock editing={editing} className="mt-4">
        <SectionTitle>{isEn ? "Experience" : "工作经历"}</SectionTitle>
        <div className="space-y-3.5">
          {doc.experiences.map((exp) => (
            <div key={exp.id}>
              <div className="flex items-baseline justify-between gap-3">
                <div className="flex min-w-0 flex-wrap items-baseline gap-x-2">
                  <Editable
                    value={exp.company}
                    onChange={(value) =>
                      update({
                        experiences: doc.experiences.map((item) =>
                          item.id === exp.id ? { ...item, company: value } : item,
                        ),
                      })
                    }
                    editing={editing}
                    multiline={false}
                    className="text-[12.5px] font-semibold leading-5 text-neutral-900"
                  />
                  <span className="text-[10px] text-neutral-300">/</span>
                  <Editable
                    value={exp.role}
                    onChange={(value) =>
                      update({
                        experiences: doc.experiences.map((item) =>
                          item.id === exp.id ? { ...item, role: value } : item,
                        ),
                      })
                    }
                    editing={editing}
                    multiline={false}
                    className="text-[11px] text-neutral-600"
                  />
                </div>
                <Editable
                  value={exp.period}
                  onChange={(value) =>
                    update({
                      experiences: doc.experiences.map((item) =>
                        item.id === exp.id ? { ...item, period: value } : item,
                      ),
                    })
                  }
                  editing={editing}
                  multiline={false}
                  className="shrink-0 text-[9.5px] tabular-nums text-neutral-500"
                />
              </div>

              {exp.team !== undefined && (
                <Editable
                  value={exp.team}
                  onChange={(value) =>
                    update({
                      experiences: doc.experiences.map((item) =>
                        item.id === exp.id ? { ...item, team: value } : item,
                      ),
                    })
                  }
                  editing={editing}
                  multiline={false}
                  className="mt-0.5 text-[10px] text-neutral-500"
                />
              )}

              <ul className="mt-1.5 space-y-1">
                {exp.bullets.map((bullet, bulletIndex) => (
                  <li key={bulletIndex} className="flex gap-2">
                    <span className="mt-[8px] size-[3px] shrink-0 rounded-full bg-neutral-400" />
                    <Editable
                      value={bullet}
                      onChange={(value) =>
                        update({
                          experiences: doc.experiences.map((item) =>
                            item.id === exp.id
                              ? {
                                  ...item,
                                  bullets: item.bullets.map((text, i) =>
                                    i === bulletIndex ? value : text,
                                  ),
                                }
                              : item,
                          ),
                        })
                      }
                      editing={editing}
                      className="flex-1 text-[11px] leading-[1.85] text-neutral-700"
                    />
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      </PaperBlock>

      {doc.projects.length > 0 && (
        <PaperBlock editing={editing} className="mt-4">
          <SectionTitle>{isEn ? "Projects" : "项目经历"}</SectionTitle>
          <div className="space-y-2.5">
            {doc.projects.map((project) => (
              <div key={project.id}>
                <div className="flex items-baseline justify-between gap-3">
                  <Editable
                    value={project.title}
                    onChange={(value) =>
                      update({
                        projects: doc.projects.map((item) =>
                          item.id === project.id ? { ...item, title: value } : item,
                        ),
                      })
                    }
                    editing={editing}
                    multiline={false}
                    className="text-[11.5px] font-medium text-neutral-900"
                  />
                  <Editable
                    value={project.year}
                    onChange={(value) =>
                      update({
                        projects: doc.projects.map((item) =>
                          item.id === project.id ? { ...item, year: value } : item,
                        ),
                      })
                    }
                    editing={editing}
                    multiline={false}
                    className="shrink-0 text-[9.5px] tabular-nums text-neutral-500"
                  />
                </div>
                <Editable
                  value={project.desc}
                  onChange={(value) =>
                    update({
                      projects: doc.projects.map((item) =>
                        item.id === project.id ? { ...item, desc: value } : item,
                      ),
                    })
                  }
                  editing={editing}
                  className="mt-0.5 text-[10.5px] leading-[1.8] text-neutral-600"
                />
                <div className="mt-1 flex flex-wrap gap-1">
                  {project.tags.map((tag) => (
                    <span
                      key={tag}
                      className="rounded-[3px] bg-neutral-100 px-1.5 py-px text-[8.5px] text-neutral-500"
                    >
                      {tag}
                    </span>
                  ))}
                </div>
              </div>
            ))}
          </div>
        </PaperBlock>
      )}

      <PaperBlock editing={editing} className="mt-4">
        <SectionTitle>{isEn ? "Education" : "教育背景"}</SectionTitle>
        <div className="space-y-1.5">
          {doc.education.map((edu) => (
            <div key={edu.id} className="flex items-baseline justify-between gap-3">
              <div className="flex min-w-0 flex-wrap items-baseline gap-x-2">
                <Editable
                  value={edu.school}
                  onChange={(value) =>
                    update({
                      education: doc.education.map((item) =>
                        item.id === edu.id ? { ...item, school: value } : item,
                      ),
                    })
                  }
                  editing={editing}
                  multiline={false}
                  className="text-[11.5px] font-medium text-neutral-900"
                />
                <span className="text-[10px] text-neutral-300">/</span>
                <Editable
                  value={edu.degree}
                  onChange={(value) =>
                    update({
                      education: doc.education.map((item) =>
                        item.id === edu.id ? { ...item, degree: value } : item,
                      ),
                    })
                  }
                  editing={editing}
                  multiline={false}
                  className="text-[10.5px] text-neutral-600"
                />
              </div>
              <Editable
                value={edu.period}
                onChange={(value) =>
                  update({
                    education: doc.education.map((item) =>
                      item.id === edu.id ? { ...item, period: value } : item,
                    ),
                  })
                }
                editing={editing}
                multiline={false}
                className="shrink-0 text-[9.5px] tabular-nums text-neutral-500"
              />
            </div>
          ))}
        </div>
      </PaperBlock>

      <PaperBlock editing={editing} className="mt-4">
        <SectionTitle>{isEn ? "Skills" : "技能"}</SectionTitle>
        <div className="space-y-1">
          {doc.skills.map((skill, index) => (
            <div key={index} className="flex gap-3">
              <Editable
                value={skill.label}
                onChange={(value) =>
                  update({
                    skills: doc.skills.map((item, i) =>
                      i === index ? { ...item, label: value } : item,
                    ),
                  })
                }
                editing={editing}
                multiline={false}
                className="w-[52px] shrink-0 text-[10px] text-neutral-400"
              />
              <Editable
                value={skill.value}
                onChange={(value) =>
                  update({
                    skills: doc.skills.map((item, i) =>
                      i === index ? { ...item, value } : item,
                    ),
                  })
                }
                editing={editing}
                className="flex-1 text-[10.5px] leading-[1.8] text-neutral-700"
              />
            </div>
          ))}
        </div>
      </PaperBlock>

      <PaperFooter
        left={`${doc.name.replace(/\s/g, "")} · ${isEn ? "Resume" : "中文简历"}`}
        page={`${String(pageIndex + 1).padStart(2, "0")} / ${String(pageTotal).padStart(2, "0")}`}
      />
    </article>
  );
}

export function PortfolioDocument({
  doc,
  editing,
  onChange,
  pageIndex,
  pageTotal,
}: {
  doc: PortfolioDoc;
  editing: boolean;
  onChange: (next: PortfolioDoc) => void;
  pageIndex: number;
  pageTotal: number;
}) {
  const update = (patch: Partial<PortfolioDoc>) => onChange({ ...doc, ...patch });

  return (
    <article className="resume-paper px-[42px] pb-[32px] pt-[40px] text-neutral-900">
      <div className="relative overflow-hidden rounded-lg bg-[#141416] px-7 py-6 text-white">
        <div
          aria-hidden="true"
          className="pointer-events-none absolute -right-12 -top-16 size-48 rounded-full bg-[#6d5efc]/45 blur-2xl"
        />
        <div
          aria-hidden="true"
          className="pointer-events-none absolute -bottom-24 left-1/3 size-56 rounded-full bg-[#f43f5e]/20 blur-3xl"
        />

        <div className="relative">
          <div className="flex items-center justify-between text-[8px] uppercase tracking-[0.28em] text-white/45">
            <Editable
              value={doc.year}
              onChange={(value) => update({ year: value })}
              editing={editing}
              multiline={false}
              className="text-[8px] uppercase tracking-[0.28em] text-white/45"
            />
            <span>{String(doc.projects.length).padStart(2, "0")} projects</span>
          </div>

          <div className="mt-7 flex items-end justify-between gap-6">
            <div className="min-w-0">
              <Editable
                as="h1"
                value={doc.name}
                onChange={(value) => update({ name: value })}
                editing={editing}
                multiline={false}
                className="font-serif text-[36px] font-semibold leading-none tracking-[0.02em] text-white"
              />
              <Editable
                value={doc.role}
                onChange={(value) => update({ role: value })}
                editing={editing}
                multiline={false}
                className="mt-2.5 text-[10px] uppercase tracking-[0.2em] text-white/55"
              />
            </div>
            <Editable
              value={doc.intro}
              onChange={(value) => update({ intro: value })}
              editing={editing}
              className="max-w-[46%] text-right text-[10px] leading-[1.85] text-white/60"
            />
          </div>

          <div className="mt-6 flex flex-wrap items-center gap-x-4 gap-y-1 border-t border-white/10 pt-3">
            {doc.contact.map((item, index) => (
              <Editable
                key={index}
                value={item}
                onChange={(value) =>
                  update({
                    contact: doc.contact.map((contact, i) => (i === index ? value : contact)),
                  })
                }
                editing={editing}
                multiline={false}
                className="text-[8.5px] tracking-[0.08em] text-white/40"
              />
            ))}
          </div>
        </div>
      </div>

      <div className="mt-5 flex items-center gap-2.5">
        <span className="text-[9px] font-semibold uppercase tracking-[0.24em] text-neutral-900">
          Selected Work
        </span>
        <span className="h-px flex-1 bg-neutral-200" />
        <span className="text-[8.5px] tabular-nums text-neutral-400">
          01 — {String(doc.projects.length).padStart(2, "0")}
        </span>
      </div>

      {doc.projects.length === 0 ? (
        <div className="mt-4 flex h-[220px] flex-col items-center justify-center gap-2 rounded-md border border-dashed border-neutral-300 bg-neutral-50 text-center">
          <p className="text-[11px] font-medium text-neutral-500">还没有项目</p>
          <p className="max-w-[220px] text-[10px] leading-relaxed text-neutral-400">
            在会话区上传作品集 PDF 或项目链接，我会按统一版式整理到这一页。
          </p>
        </div>
      ) : (
        <div className="mt-3 grid grid-cols-2 gap-x-3.5 gap-y-4">
          {doc.projects.map((project, index) => (
            <div key={project.id}>
              <div
                className={cn(
                  "relative aspect-[16/10] overflow-hidden rounded-md bg-gradient-to-br saturate-[0.92]",
                  COVERS[project.cover] ?? COVERS.violet,
                )}
              >
                <div
                  aria-hidden="true"
                  className="absolute inset-0 bg-gradient-to-t from-black/30 via-transparent to-white/10"
                />
                <div
                  aria-hidden="true"
                  className="absolute -right-6 -top-8 size-24 rounded-full border border-white/25"
                />
                <div
                  aria-hidden="true"
                  className="absolute -bottom-6 -left-4 size-20 rounded-full bg-white/10 blur-[1px]"
                />
                <span className="absolute left-3 top-2 font-serif text-[26px] leading-none text-white/30">
                  {String(index + 1).padStart(2, "0")}
                </span>
                <span className="absolute bottom-2.5 right-2.5 rounded bg-black/25 px-1.5 py-0.5 text-[8px] text-white/85 backdrop-blur-sm">
                  {project.year}
                </span>
              </div>

              <Editable
                value={project.title}
                onChange={(value) =>
                  update({
                    projects: doc.projects.map((item) =>
                      item.id === project.id ? { ...item, title: value } : item,
                    ),
                  })
                }
                editing={editing}
                multiline={false}
                className="mt-2 text-[11px] font-semibold leading-[1.5] text-neutral-900"
              />
              <Editable
                value={project.desc}
                onChange={(value) =>
                  update({
                    projects: doc.projects.map((item) =>
                      item.id === project.id ? { ...item, desc: value } : item,
                    ),
                  })
                }
                editing={editing}
                className="mt-0.5 text-[9.5px] leading-[1.7] text-neutral-500"
              />
              <div className="mt-1 flex flex-wrap gap-1">
                {project.tags.map((tag) => (
                  <span
                    key={tag}
                    className="rounded-[3px] border border-neutral-200 px-1.5 py-px text-[8px] text-neutral-500"
                  >
                    {tag}
                  </span>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}

      <PaperFooter
        left={`${doc.name} · Portfolio · ${new Date().getFullYear()}`}
        page={`${String(pageIndex + 1).padStart(2, "0")} / ${String(pageTotal).padStart(2, "0")}`}
      />
    </article>
  );
}
