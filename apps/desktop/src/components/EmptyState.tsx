import type { ReactNode } from "react";

import { VOCAB } from "../design/tokens";

export interface EmptyStateProps {
  /** 状態を 1 文で（「まだ接続していません。」） */
  message: string;
  /** 次にすることの説明を 1 文で */
  instruction: string;
  /** 行動は 1 つだけ（§5.5.9）。 */
  action?: ReactNode;
}

/**
 * 空状態（§5.5.9）。中央揃えの縦積みにせず、余白と罫線だけで組む。
 * アイコンは飾りなので置かない。
 */
export function EmptyState({
  message,
  instruction,
  action,
}: EmptyStateProps) {
  return (
    <section className="py-8">
      <p className="text-body text-ink-2 text-pretty">{message}</p>
      <p className="mt-4 text-meta font-medium text-muted">
        {VOCAB.nextAction}
      </p>
      <p className="mt-1 mb-4 max-w-prose text-body text-ink-2 text-pretty">
        {instruction}
      </p>
      {action}
    </section>
  );
}
