import type { ReactNode } from "react";

import { VOCAB } from "../design/tokens";

export interface EmptyStateProps {
  /** 状態を 1 文で（「まだ接続していません。」） */
  message: string;
  /** 次にすることの説明を 1 文で */
  instruction: string;
  /** 行動は 1 つだけ */
  action?: ReactNode;
}

/**
 * 空状態（Imported style）: 左寄せ。見出し 23px・本文 smoke・主アクション 1 つ。
 * 中央揃えの縦積みにせず、余白と罫線だけで組む。アイコンは置かない。
 */
export function EmptyState({
  message,
  instruction,
  action,
}: EmptyStateProps) {
  return (
    <section className="py-6">
      <h2 className="text-heading text-chalk text-pretty">{message}</h2>
      <p className="mt-4 font-mono text-micro tracking-meta text-smoke uppercase">
        {VOCAB.nextAction}
      </p>
      <p className="mt-2 mb-6 max-w-prose text-body text-smoke text-pretty">
        {instruction}
      </p>
      {action}
    </section>
  );
}
