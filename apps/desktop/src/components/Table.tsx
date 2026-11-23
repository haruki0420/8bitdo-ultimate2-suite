import type { ReactNode } from "react";

import { cn } from "../lib/cn";

export interface TableColumn {
  key: string;
  label: string;
  /** 数値・識別子は Hack 12 で組む（§5.5.6） */
  mono?: boolean;
  align?: "start" | "end";
  /** 幅 1100px 未満で隠す列（§5.5.6 の列の優先度） */
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
  /** 読み込み中はスピナーではなく骨組みを 3 行出す（§5.5.9） */
  loading?: boolean;
}

/**
 * 表（§5.5.6）。外枠はカード、ヘッダーは sticky、行は 36px。
 * 選択行は `--accent-soft` の地 + 左 2px のアクセントで表す（枠線の色だけに頼らない）。
 */
export function Table({
  ariaLabel,
  columns,
  rows,
  empty,
  loading = false,
}: TableProps) {
  return (
    <div className="overflow-hidden rounded-lg border border-line">
      <table
        aria-label={ariaLabel}
        aria-busy={loading || undefined}
        className="w-full border-collapse"
      >
        <thead className="bg-sunken">
          <tr>
            {columns.map((column) => (
              <th
                key={column.key}
                scope="col"
                className={cn(
                  "h-row-header px-3 text-left text-meta font-medium whitespace-nowrap text-muted",
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
              <td colSpan={columns.length} className="px-3 py-4">
                <div className="grid gap-3" data-testid="table-skeleton">
                  {[0, 1, 2].map((line) => (
                    <div key={line} className="h-3 rounded-xs bg-sunken" />
                  ))}
                </div>
              </td>
            </tr>
          ) : rows.length === 0 ? (
            <tr>
              <td
                colSpan={columns.length}
                className="px-3 py-8 text-center text-ui text-muted"
              >
                {empty}
              </td>
            </tr>
          ) : (
            rows.map((row) => (
              <tr
                key={row.key}
                className={cn(
                  "border-b border-line last:border-b-0 hover:bg-hover",
                  row.selected && "bg-accent-soft hover:bg-accent-soft",
                )}
              >
                {columns.map((column, index) => (
                  <td
                    key={column.key}
                    className={cn(
                      "h-row px-3 align-middle text-ui text-ink-2",
                      column.mono && "font-mono text-data tracking-data",
                      column.align === "end" && "text-right",
                      column.lowPriority && "hidden cols:table-cell",
                      index === 0 && row.selected && "shadow-row-selected",
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
