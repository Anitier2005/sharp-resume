import {
  File as FileIcon,
  FileArchive,
  FileCode2,
  FileImage,
  FileSpreadsheet,
  FileText,
} from "lucide-react";

import { cn } from "@/lib/utils";

type FileKind = "pdf" | "doc" | "sheet" | "image" | "archive" | "code" | "file";

const EXT_KIND: Record<string, FileKind> = {
  pdf: "pdf",
  doc: "doc",
  docx: "doc",
  rtf: "doc",
  pages: "doc",
  xls: "sheet",
  xlsx: "sheet",
  csv: "sheet",
  numbers: "sheet",
  png: "image",
  jpg: "image",
  jpeg: "image",
  webp: "image",
  gif: "image",
  svg: "image",
  fig: "image",
  zip: "archive",
  rar: "archive",
  "7z": "archive",
  tar: "archive",
  gz: "archive",
  ts: "code",
  tsx: "code",
  js: "code",
  jsx: "code",
  json: "code",
  md: "code",
  py: "code",
};

const KIND_STYLE: Record<FileKind, { wrap: string; icon: typeof FileIcon }> = {
  pdf: {
    wrap: "bg-red-50 text-red-600 ring-red-100 dark:bg-red-500/10 dark:text-red-400 dark:ring-red-500/20",
    icon: FileText,
  },
  doc: {
    wrap: "bg-blue-50 text-blue-600 ring-blue-100 dark:bg-blue-500/10 dark:text-blue-400 dark:ring-blue-500/20",
    icon: FileText,
  },
  sheet: {
    wrap: "bg-emerald-50 text-emerald-600 ring-emerald-100 dark:bg-emerald-500/10 dark:text-emerald-400 dark:ring-emerald-500/20",
    icon: FileSpreadsheet,
  },
  image: {
    wrap: "bg-violet-50 text-violet-600 ring-violet-100 dark:bg-violet-500/10 dark:text-violet-400 dark:ring-violet-500/20",
    icon: FileImage,
  },
  archive: {
    wrap: "bg-amber-50 text-amber-600 ring-amber-100 dark:bg-amber-500/10 dark:text-amber-400 dark:ring-amber-500/20",
    icon: FileArchive,
  },
  code: {
    wrap: "bg-sky-50 text-sky-600 ring-sky-100 dark:bg-sky-500/10 dark:text-sky-400 dark:ring-sky-500/20",
    icon: FileCode2,
  },
  file: {
    wrap: "bg-muted text-muted-foreground ring-border dark:bg-white/5",
    icon: FileIcon,
  },
};

export function extOf(name: string): string {
  const parts = name.split(".");
  return parts.length > 1 ? (parts.pop() as string).toLowerCase() : "";
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function FileGlyph({
  name,
  size = "md",
  className,
}: {
  name: string;
  size?: "sm" | "md" | "lg";
  className?: string;
}) {
  const kind = EXT_KIND[extOf(name)] ?? "file";
  const { wrap, icon: Icon } = KIND_STYLE[kind];
  return (
    <div
      className={cn(
        "grid shrink-0 place-items-center rounded-md ring-1",
        size === "sm" && "size-6 [&_svg]:size-3.5",
        size === "md" && "size-8 [&_svg]:size-4",
        size === "lg" && "size-10 [&_svg]:size-5",
        wrap,
        className,
      )}
    >
      <Icon strokeWidth={1.8} />
    </div>
  );
}
