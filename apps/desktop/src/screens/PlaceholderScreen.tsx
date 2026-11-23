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
      <h1 className="text-display tracking-display text-balance text-chalk">
        {title}
      </h1>
      <p className="mt-3 mb-6 max-w-prose text-body text-smoke text-pretty">
        {description}
      </p>
      <EmptyState
        message="この画面はまだ準備中です。"
        instruction="次のフェーズで追加します。いまは「デバイス」で接続状態を確認できます。"
        action={
          <Button onClick={onOpenDevices}>デバイスを開く</Button>
        }
      />
    </div>
  );
}
