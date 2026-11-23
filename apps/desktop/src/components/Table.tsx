import type { ReactNode } from "react";

import { cn } from "../lib/cn";

export interface TableColumn {
  key: string;
  label: string;
  /** 数値・識別子は mono で組む */
  mono?: boolean;
  align?: "start" | "end";
  /** 幅 1100px 未満で隠す列 */
  lowPriority?: boolean;
}

export interface TableRow {
  key: string;
  cells: Record<string, ReactNode>;
  selected?: boolean;
}

export interface TableProps {
  ariaLabel: string;
  columns: readonly TableColumn[];
  rows: readonly TableRow[];
  /** 行が無いときの 1 行（「状態 + 次にすること」） */
  empty: ReactNode;
  /** 読み込み中はスピナーではなく骨組みを 3 行出す */
  loading?: boolean;
}

/**
 * 表（Imported style）: 罫線のみ（1px graphite）。行の塗りは無し、選択行だけ `#1a1a1a`。
 * ヘッダーは mono 12 の大文字 smoke。外枠は 1px graphite + 8px 角丸のカード。
 */
export function Table({
  ariaLabel,
  columns,
  rows,
  empty,
  loading = false,
}: TableProps) {
  return (
    <div className="overflow-hidden rounded-md border border-graphite">
      <table
        aria-label={ariaLabel}
        aria-busy={loading || undefined}
        className="w-full border-collapse"
      >
        <thead>
          <tr className="border-b border-graphite">
            {columns.map((column) => (
              <th
                key={column.key}
                scope="col"
                className={cn(
                  "h-row-header px-4 text-left font-mono text-micro tracking-meta whitespace-nowrap text-smoke uppercase",
                  column.align === "end" && "text-right",
                  column.lowPriority && "hidden cols:table-cell",
                )}
              >
                {column.label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {loading ? (
            <tr>
              <td colSpan={columns.length} className="px-4 py-5">
                <div className="grid gap-3" data-testid="table-skeleton">
                  {[0, 1, 2].map((line) => (
                    <div key={line} className="h-3 rounded-xs bg-graphite" />
                  ))}
                </div>
              </td>
            </tr>
          ) : rows.length === 0 ? (
            <tr>
              <td
                colSpan={columns.length}
                className="px-4 py-8 text-left text-ui text-smoke"
              >
                {empty}
              </td>
            </tr>
          ) : (
            rows.map((row) => (
              <tr
                key={row.key}
                className={cn(
                  "border-b border-graphite last:border-b-0",
                  row.selected && "bg-paper-ink",
                )}
              >
                {columns.map((column) => (
                  <td
                    key={column.key}
                    className={cn(
                      "h-row px-4 align-middle text-ui text-smoke",
                      column.mono && "font-mono text-meta text-ash",
                      column.align === "end" && "text-right",
                      column.lowPriority && "hidden cols:table-cell",
                    )}
                  >
                    {row.cells[column.key]}
                  </td>
                ))}
              </tr>
            ))
          )}
        </tbody>
      </table>
    </div>
  );
}
