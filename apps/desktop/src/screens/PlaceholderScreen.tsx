import { Button, EmptyState } from "../components";

export interface PlaceholderScreenProps {
  title: string;
  description: string;
  onOpenDevices: () => void;
}

/** 後続フェーズの画面。骨格だけを先に置き、行き止まりにしない。 */
export function PlaceholderScreen({
  title,
  description,
  onOpenDevices,
}: PlaceholderScreenProps) {
  return (
    <div>
      <h1 className="text-display font-bold text-balance text-ink">{title}</h1>
      <p className="mt-2 mb-5 max-w-prose text-body text-ink-2 text-pretty">
        {description}
      </p>
      <EmptyState
        message="この画面はまだ準備中です。"
        instruction="次のフェーズで追加します。いまは「デバイス」で接続状態を確認できます。"
        action={
          <Button variant="secondary" onClick={onOpenDevices}>
            デバイスを開く
          </Button>
        }
      />
    </div>
  );
}
