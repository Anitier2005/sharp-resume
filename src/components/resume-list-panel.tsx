import { useMemo, useState } from "react";
import {
  ArrowUpDown,
  Check,
  Copy,
  FileDown,
  FolderOpen,
  HardDrive,
  MoreHorizontal,
  Pencil,
  Plus,
  Search,
  SearchX,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";

import { Input } from "@/components/ui/input";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import type { ResumeItem } from "@/types";

interface ResumeListPanelProps {
  resumes: ResumeItem[];
  activeId: string;
  onSelect: (id: string) => void;
  onCreate: () => void;
}

type SortKey = "recent" | "name";

export function ResumeListPanel({ resumes, activeId, onSelect, onCreate }: ResumeListPanelProps) {
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<SortKey>("recent");

  const filtered = useMemo(() => {
    const keyword = query.trim().toLowerCase();
    const list = resumes.filter((item) =>
      keyword.length === 0
        ? true
        : `${item.name}${item.role}${item.company}`.toLowerCase().includes(keyword),
    );
    if (sort === "name") {
      return [...list].sort((a, b) => a.name.localeCompare(b.name, "zh-CN"));
    }
    return list;
  }, [resumes, query, sort]);

  return (
    <aside className="flex h-full min-w-0 flex-col border-r bg-muted/40">
      <div className="px-2.5 pb-2 pt-3">
        <div className="flex items-center justify-between px-1">
          <div className="flex items-center gap-1.5">
            <span className="text-[10px] font-semibold uppercase tracking-[0.16em] text-muted-foreground">
              简历库
            </span>
            <span className="rounded-full bg-background px-1.5 py-px text-[9.5px] tabular-nums text-muted-foreground ring-1 ring-border/70">
              {resumes.length}
            </span>
          </div>

          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                aria-label="排序方式"
                className="grid size-6 place-items-center rounded text-muted-foreground transition-colors hover:bg-background hover:text-foreground"
              >
                <ArrowUpDown className="size-3" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-36">
              <DropdownMenuLabel className="text-[10px] font-normal uppercase tracking-wide text-muted-foreground">
                排序
              </DropdownMenuLabel>
              <DropdownMenuItem onSelect={() => setSort("recent")} className="text-[12px]">
                最近编辑
                {sort === "recent" && <Check className="ml-auto size-3.5" />}
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={() => setSort("name")} className="text-[12px]">
                按名称
                {sort === "name" && <Check className="ml-auto size-3.5" />}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        <div className="relative mt-2">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="搜索简历"
            aria-label="搜索简历"
            className="h-8 rounded-md border-transparent bg-background/70 pl-8 pr-9 text-[12px] shadow-none placeholder:text-muted-foreground/70 focus-visible:border-border focus-visible:ring-2 focus-visible:ring-brand/10"
          />
          <kbd className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 rounded border bg-muted/60 px-1 py-px font-sans text-[9px] text-muted-foreground">
            /
          </kbd>
        </div>

        <button
          type="button"
          onClick={onCreate}
          className="mt-2 flex h-8 w-full items-center gap-1.5 rounded-md border border-dashed bg-background/60 px-2.5 text-[12px] text-foreground/80 transition-colors hover:border-brand/40 hover:bg-background hover:text-foreground"
        >
          <Plus className="size-3.5 text-brand" />
          新建简历
          <span className="ml-auto text-[9.5px] text-muted-foreground">Ctrl N</span>
        </button>
      </div>

      <div className="flex-1 overflow-y-auto px-2.5 pb-2">
        <div className="flex items-center justify-between px-1 py-1.5">
          <span className="text-[9.5px] uppercase tracking-[0.14em] text-muted-foreground/80">
            {query ? "搜索结果" : "最近编辑"}
          </span>
          <span className="text-[9.5px] tabular-nums text-muted-foreground/70">
            {filtered.length} 份
          </span>
        </div>

        <div className="space-y-1">
          {filtered.map((resume) => {
            const active = resume.id === activeId;
            return (
              <div key={resume.id} className="group relative">
                <button
                  type="button"
                  onClick={() => onSelect(resume.id)}
                  aria-current={active ? "true" : undefined}
                  className={cn(
                    "relative flex w-full items-start gap-2.5 rounded-lg px-2 py-2.5 text-left transition-all",
                    active
                      ? "bg-background shadow-[0_1px_2px_rgba(0,0,0,0.04)] ring-1 ring-border"
                      : "hover:bg-background/60",
                  )}
                >
                  {active && (
                    <span className="absolute -left-1.5 top-1/2 h-5 w-[2px] -translate-y-1/2 rounded-full bg-brand" />
                  )}
                  <div
                    className={cn(
                      "grid size-7 shrink-0 place-items-center rounded-md bg-gradient-to-br text-[11px] font-semibold text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.25)]",
                      resume.accent,
                    )}
                  >
                    {resume.name.slice(0, 1)}
                  </div>

                  <div className="min-w-0 flex-1 pt-px">
                    <div className="flex items-center gap-1.5 pr-5">
                      <span className="truncate text-[12.5px] font-medium leading-4">
                        {resume.name}
                      </span>
                      {active && <span className="size-1 shrink-0 rounded-full bg-brand" />}
                    </div>
                    <p className="mt-0.5 truncate text-[10.5px] text-muted-foreground">
                      {resume.company}
                    </p>
                    <div className="mt-1.5 flex items-center gap-1">
                      {resume.pages.map((page) => (
                        <span
                          key={page.id}
                          className="rounded-[3px] bg-muted px-1 py-px text-[9px] font-medium leading-3 text-muted-foreground ring-1 ring-border/50"
                          title={page.label}
                        >
                          {page.badge}
                        </span>
                      ))}
                      <span className="ml-auto shrink-0 text-[9.5px] text-muted-foreground/80">
                        {resume.updatedAt}
                      </span>
                    </div>
                  </div>
                </button>

                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <button
                      type="button"
                      aria-label={`${resume.name} 更多操作`}
                      className="absolute right-1 top-1 grid size-6 place-items-center rounded text-muted-foreground opacity-0 transition-opacity hover:bg-accent hover:text-foreground focus-visible:opacity-100 group-hover:opacity-100 data-[state=open]:opacity-100"
                    >
                      <MoreHorizontal className="size-3.5" />
                    </button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="start" className="w-40">
                    <DropdownMenuItem className="text-[12px]" onSelect={() => onSelect(resume.id)}>
                      <Pencil className="size-3.5" />
                      打开
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      className="text-[12px]"
                      onSelect={() => toast("重命名简历", { description: "功能开发阶段接入。" })}
                    >
                      <Pencil className="size-3.5" />
                      重命名
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      className="text-[12px]"
                      onSelect={() => toast("已复制一份简历（演示）")}
                    >
                      <Copy className="size-3.5" />
                      复制一份
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      className="text-[12px]"
                      onSelect={() =>
                        toast("导出整份简历", { description: "会打包该简历的所有页面。" })
                      }
                    >
                      <FileDown className="size-3.5" />
                      导出全部页面
                    </DropdownMenuItem>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem
                      className="text-[12px] text-destructive focus:text-destructive"
                      onSelect={() => toast("删除简历", { description: "功能阶段会移入回收站。" })}
                    >
                      <Trash2 className="size-3.5" />
                      删除
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
            );
          })}

          {filtered.length === 0 && (
            <div className="mt-10 flex flex-col items-center gap-2 px-4 text-center">
              <SearchX className="size-6 text-muted-foreground/50" />
              <p className="text-[11.5px] text-muted-foreground">没有找到匹配的简历</p>
              <button
                type="button"
                onClick={() => setQuery("")}
                className="text-[11px] text-brand hover:underline"
              >
                清空搜索
              </button>
            </div>
          )}
        </div>
      </div>

      <div className="border-t bg-background/50 px-3 py-2.5">
        <div className="flex items-center gap-1.5 text-[11px] font-medium">
          <HardDrive className="size-3.5 text-emerald-500" />
          本地存储
        </div>
        <p className="mt-1 text-[10px] leading-relaxed text-muted-foreground">
          数据保存在本机，不上传云端
        </p>
        <div className="mt-1.5 flex items-center justify-between">
          <span className="text-[10px] tabular-nums text-muted-foreground">
            {resumes.length} 份 · 共 {resumes.reduce((sum, item) => sum + item.pages.length, 0)} 页
          </span>
          <button
            type="button"
            onClick={() => toast("打开数据目录", { description: "打包后调用系统文件管理器。" })}
            className="flex items-center gap-1 text-[10px] text-brand transition-opacity hover:opacity-80"
          >
            <FolderOpen className="size-3" />
            打开目录
          </button>
        </div>
      </div>
    </aside>
  );
}
