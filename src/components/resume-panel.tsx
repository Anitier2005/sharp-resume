import { useEffect, useRef, useState } from "react";
import {
  CheckCircle2,
  ChevronDown,
  Copy,
  Download,
  Eye,
  FileDown,
  FileText,
  Image as ImageIcon,
  Languages,
  LayoutGrid,
  Maximize2,
  MoreHorizontal,
  Pencil,
  Plus,
  Printer,
  Trash2,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { toast } from "sonner";

import { PortfolioDocument, ResumeDocument } from "@/components/resume-documents";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { countChars } from "@/lib/doc";
import { cn } from "@/lib/utils";
import type { Doc, PageKind, ResumeItem, ResumePage } from "@/types";

interface ResumePanelProps {
  resume: ResumeItem;
  activePageId: string;
  editing: boolean;
  onEditingChange: (editing: boolean) => void;
  onSelectPage: (pageId: string) => void;
  onAddPage: (kind: PageKind) => void;
  onRemovePage: (pageId: string) => void;
  onDocChange: (pageId: string, doc: Doc) => void;
}

function PageGlyph({ kind, className }: { kind: PageKind; className?: string }) {
  if (kind === "resume-en") return <Languages className={className} />;
  if (kind === "portfolio") return <LayoutGrid className={className} />;
  return <FileText className={className} />;
}

const EXPORT_ITEMS = [
  { key: "pdf", label: "PDF 文档", hint: "保留排版", icon: FileText },
  { key: "docx", label: "Word (.docx)", hint: "可继续编辑", icon: FileDown },
  { key: "md", label: "Markdown", hint: "纯文本", icon: FileText },
  { key: "png", label: "PNG 长图", hint: "适合分享", icon: ImageIcon },
  { key: "print", label: "打印", hint: "调用系统打印", icon: Printer },
];

export function ResumePanel({
  resume,
  activePageId,
  editing,
  onEditingChange,
  onSelectPage,
  onAddPage,
  onRemovePage,
  onDocChange,
}: ResumePanelProps) {
  const [zoom, setZoom] = useState(90);
  const [autoFit, setAutoFit] = useState(true);
  const canvasRef = useRef<HTMLDivElement>(null);
  const activeTabRef = useRef<HTMLDivElement>(null);

  const pageIndex = Math.max(
    0,
    resume.pages.findIndex((page) => page.id === activePageId),
  );
  const page: ResumePage = resume.pages[pageIndex] ?? resume.pages[0];
  const chars = countChars(page.doc);

  useEffect(() => {
    const element = canvasRef.current;
    if (!element) return;

    const applyFit = () => {
      const width = element.clientWidth;
      if (!width) return;
      const next = Math.round(((width - 64) / 780) * 100);
      setZoom(Math.min(105, Math.max(50, next)));
    };

    const observer = new ResizeObserver(() => {
      if (autoFit) applyFit();
    });
    observer.observe(element);
    applyFit();

    return () => observer.disconnect();
  }, [autoFit, resume.id]);

  useEffect(() => {
    activeTabRef.current?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [page.id, resume.pages.length]);

  const exportPage = (label: string) => {
    toast(`正在导出「${page.label}」`, {
      description: `${label} · 功能开发阶段接入真实导出（原型占位）`,
    });
  };

  return (
    <section className="flex h-full min-w-0 flex-col bg-canvas">
      <div className="resume-toolbar flex h-12 shrink-0 items-center gap-2 border-b bg-background px-2.5">
        <div className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          {resume.pages.map((item) => {
            const active = item.id === page.id;
            return (
              <div
                key={item.id}
                ref={active ? activeTabRef : undefined}
                role="tab"
                aria-selected={active}
                tabIndex={0}
                onClick={() => onSelectPage(item.id)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    onSelectPage(item.id);
                  }
                }}
                className={cn(
                  "group/tab flex h-8 shrink-0 cursor-pointer items-center gap-1.5 rounded-md border px-2.5 text-[11.5px] transition-colors",
                  active
                    ? "border-border bg-background text-foreground shadow-xs"
                    : "border-transparent text-muted-foreground hover:bg-muted hover:text-foreground",
                )}
              >
                <PageGlyph
                  kind={item.kind}
                  className={cn("size-3.5", active ? "text-brand" : "text-muted-foreground/70")}
                />
                <span className="max-w-[132px] truncate">{item.label}</span>
                {resume.pages.length > 1 && (
                  <button
                    type="button"
                    aria-label={`删除 ${item.label}`}
                    onClick={(event) => {
                      event.stopPropagation();
                      onRemovePage(item.id);
                    }}
                    className="grid size-3.5 place-items-center rounded text-muted-foreground/60 opacity-0 transition hover:bg-accent hover:text-foreground group-hover/tab:opacity-100"
                  >
                    <X className="size-3" />
                  </button>
                )}
              </div>
            );
          })}

          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                aria-label="添加页面"
                className="grid size-7 shrink-0 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              >
                <Plus className="size-3.5" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="w-48">
              <DropdownMenuLabel className="text-[10px] font-normal uppercase tracking-wide text-muted-foreground">
                添加页面
              </DropdownMenuLabel>
              <DropdownMenuItem className="text-[12px]" onSelect={() => onAddPage("resume-zh")}>
                <FileText className="size-3.5" />
                中文简历
              </DropdownMenuItem>
              <DropdownMenuItem className="text-[12px]" onSelect={() => onAddPage("resume-en")}>
                <Languages className="size-3.5" />
                English Resume
              </DropdownMenuItem>
              <DropdownMenuItem className="text-[12px]" onSelect={() => onAddPage("portfolio")}>
                <LayoutGrid className="size-3.5" />
                作品集
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                className="text-[12px]"
                onSelect={() => toast("模板库", { description: "功能阶段提供 6 套排版模板。" })}
              >
                <Copy className="size-3.5" />
                从模板新建…
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        <div className="flex shrink-0 items-center gap-1.5 pl-1">
          <div className="flex items-center rounded-md bg-muted p-0.5">
            <button
              type="button"
              onClick={() => onEditingChange(true)}
              aria-pressed={editing}
              aria-label="编辑模式"
              className={cn(
                "toolbar-btn flex h-7 items-center gap-1 rounded-[5px] px-2 text-[11px] transition-colors",
                editing
                  ? "bg-background text-foreground shadow-xs"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              <Pencil className="size-3" />
              <span className="toolbar-text">编辑</span>
            </button>
            <button
              type="button"
              onClick={() => onEditingChange(false)}
              aria-pressed={!editing}
              aria-label="预览模式"
              className={cn(
                "toolbar-btn flex h-7 items-center gap-1 rounded-[5px] px-2 text-[11px] transition-colors",
                !editing
                  ? "bg-background text-foreground shadow-xs"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              <Eye className="size-3" />
              <span className="toolbar-text">预览</span>
            </button>
          </div>

          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                className="flex h-8 items-center gap-1.5 rounded-md bg-primary px-2.5 text-[11.5px] font-medium text-primary-foreground shadow-sm transition-opacity hover:opacity-90"
              >
                <Download className="size-3.5" />
                导出
                <ChevronDown className="size-3 opacity-60" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-56">
              <DropdownMenuLabel className="text-[10px] font-normal text-muted-foreground">
                导出当前页面 · {page.label}
              </DropdownMenuLabel>
              {EXPORT_ITEMS.map((item) => (
                <DropdownMenuItem
                  key={item.key}
                  className="text-[12px]"
                  onSelect={() => exportPage(item.label)}
                >
                  <item.icon className="size-3.5" />
                  <span className="flex-1">{item.label}</span>
                  <span className="text-[9.5px] text-muted-foreground">{item.hint}</span>
                </DropdownMenuItem>
              ))}
              <DropdownMenuSeparator />
              <DropdownMenuItem
                className="text-[12px]"
                onSelect={() => toast("导出整份简历", { description: "会把全部页面合并为一个 PDF。" })}
              >
                <FileDown className="size-3.5" />
                导出整份简历（{resume.pages.length} 页）
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>

          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                aria-label="更多页面操作"
                className="grid size-7 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              >
                <MoreHorizontal className="size-3.5" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-48">
              <DropdownMenuItem
                className="text-[12px]"
                onSelect={() => toast("重命名页面", { description: "直接在页签上双击也能改。" })}
              >
                <Pencil className="size-3.5" />
                重命名页面
              </DropdownMenuItem>
              <DropdownMenuItem
                className="text-[12px]"
                onSelect={() => onAddPage(page.kind)}
              >
                <Copy className="size-3.5" />
                复制这一页
              </DropdownMenuItem>
              <DropdownMenuItem
                className="text-[12px]"
                onSelect={() => toast("页面设置", { description: "页边距、字号、字体与主题色。" })}
              >
                <LayoutGrid className="size-3.5" />
                版式设置
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                className="text-[12px] text-destructive focus:text-destructive"
                onSelect={() => onRemovePage(page.id)}
              >
                <Trash2 className="size-3.5" />
                删除这一页
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>

      <div ref={canvasRef} className="dot-grid relative flex-1 overflow-auto bg-canvas">
        <div className="flex min-h-full w-max min-w-full justify-center px-8 py-7">
          <div style={{ zoom: zoom / 100 }} className="w-[780px] shrink-0">
            <div className="paper-shadow relative w-full overflow-hidden rounded-[3px] bg-white ring-1 ring-black/[0.06]">
              <div key={page.id} className="min-h-[1103px]">
                {page.doc.kind === "portfolio" ? (
                  <PortfolioDocument
                    doc={page.doc}
                    editing={editing}
                    onChange={(next) => onDocChange(page.id, next)}
                    pageIndex={pageIndex}
                    pageTotal={resume.pages.length}
                  />
                ) : (
                  <ResumeDocument
                    doc={page.doc}
                    editing={editing}
                    onChange={(next) => onDocChange(page.id, next)}
                    pageIndex={pageIndex}
                    pageTotal={resume.pages.length}
                  />
                )}
              </div>
            </div>
            <p className="py-3 text-center text-[9px] uppercase tracking-[0.18em] text-muted-foreground/70">
              A4 · 210 × 297 mm · {page.label}
            </p>
          </div>
        </div>
      </div>

      <div className="flex h-8 shrink-0 items-center gap-3 border-t bg-background px-3 text-[10px] text-muted-foreground">
        <span className="tabular-nums">
          第 {pageIndex + 1} / {resume.pages.length} 页
        </span>
        <span className="h-3 w-px bg-border" />
        <span className="tabular-nums">约 {chars} 字</span>
        <span className="h-3 w-px bg-border" />
        <span className="flex items-center gap-1">
          <CheckCircle2 className="size-3 text-emerald-500" />
          已保存 · {page.updatedAt}
        </span>

        <div className="ml-auto flex items-center gap-0.5">
          <button
            type="button"
            aria-label="缩小"
            onClick={() => {
              setAutoFit(false);
              setZoom((value) => Math.max(50, value - 10));
            }}
            className="grid size-6 place-items-center rounded text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <ZoomOut className="size-3.5" />
          </button>
          <button
            type="button"
            onClick={() => {
              setAutoFit(false);
              setZoom(100);
            }}
            className="w-9 rounded py-0.5 text-center text-[10px] tabular-nums text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            {zoom}%
          </button>
          <button
            type="button"
            aria-label="放大"
            onClick={() => {
              setAutoFit(false);
              setZoom((value) => Math.min(130, value + 10));
            }}
            className="grid size-6 place-items-center rounded text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <ZoomIn className="size-3.5" />
          </button>
          <span className="mx-1 h-3 w-px bg-border" />
          <button
            type="button"
            aria-label="适应宽度"
            onClick={() => {
              setAutoFit(true);
              const width = canvasRef.current?.clientWidth;
              if (!width) return;
              setZoom(Math.min(105, Math.max(50, Math.round(((width - 64) / 780) * 100))));
            }}
            className="grid size-6 place-items-center rounded text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <Maximize2 className="size-3.5" />
          </button>
        </div>
      </div>
    </section>
  );
}
