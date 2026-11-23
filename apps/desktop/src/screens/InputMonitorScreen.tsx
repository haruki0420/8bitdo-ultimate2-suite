import { Banner } from "../components";
import {
  formatSigned,
  formatTrigger,
  normalizeStick,
  normalizeTrigger,
  screenOffset,
  type StickVector,
} from "../input/input-report";

/** SVG 内でスティックの点が動く半径（スティック外周 16px より内側）。 */
const STICK_DOT_RADIUS = 12;

/**
 * 画面の見本データ。実機の入力は次のフェーズで接続する。
 * 左スティックは右上、右スティックは左下、L2 は 62 % の想定。
 */
const SAMPLE = {
  left: normalizeStick(180, 140),
  right: normalizeStick(117, 88),
  leftTrigger: normalizeTrigger(158),
  rightTrigger: normalizeTrigger(31),
  pressed: "A / L1",
} as const;

interface PadGraphicProps {
  left: StickVector;
  right: StickVector;
}

/**
 * コントローラの線画。押されたボタンだけを LED（--stage-led）で塗る。
 * 非押下は輪郭のみ（--stage-line-strong）。色だけに頼らないよう、
 * 押下ボタン名は下の数値一覧にもテキストで出す。
 */
function PadGraphic({ left, right }: PadGraphicProps) {
  const leftDot = screenOffset(left, STICK_DOT_RADIUS);
  const rightDot = screenOffset(right, STICK_DOT_RADIUS);

  return (
    <svg
      viewBox="0 0 300 180"
      role="img"
      aria-label="コントローラの見本（左スティックは右上、右スティックは左下、A と L1 を押下）"
      className="block w-full"
    >
      <g
        className="fill-none stroke-stage-line-strong"
        strokeWidth={1.5}
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        {/* グリップ */}
        <rect
          x={44}
          y={96}
          width={70}
          height={66}
          rx={30}
          transform="rotate(14 79 129)"
        />
        <rect
          x={186}
          y={96}
          width={70}
          height={66}
          rx={30}
          transform="rotate(-14 221 129)"
        />
        {/* 本体 */}
        <rect x={62} y={52} width={176} height={66} rx={33} />
        {/* R1 は未押下 */}
        <rect x={156} y={44} width={52} height={12} rx={6} />
        {/* 背面パドル */}
        <rect
          x={48}
          y={112}
          width={16}
          height={26}
          rx={6}
          transform="rotate(14 56 125)"
        />
        <rect
          x={236}
          y={112}
          width={16}
          height={26}
          rx={6}
          transform="rotate(-14 244 125)"
        />
      </g>

      {/* L1 押下 */}
      <rect
        x={92}
        y={44}
        width={52}
        height={12}
        rx={6}
        strokeWidth={1.5}
        className="stage-lit fill-stage-led stroke-stage-led"
      />

      {/* 十字キー */}
      <path
        d="M84 76h20M94 66v20"
        strokeWidth={1}
        className="fill-none stroke-stage-line"
      />
      <path
        d="M82 74h24v4H82zM92 64h4v24h-4z"
        strokeWidth={1.5}
        className="fill-none stroke-stage-line-strong"
      />

      {/* フェイスボタン（A だけ押下） */}
      <circle
        cx={216}
        cy={74}
        r={6}
        strokeWidth={1.5}
        className="stage-lit fill-stage-led stroke-stage-led"
      />
      <g
        className="fill-none stroke-stage-line-strong"
        strokeWidth={1.5}
      >
        <circle cx={230} cy={86} r={5} />
        <circle cx={216} cy={98} r={5} />
        <circle cx={202} cy={86} r={5} />
      </g>

      {/* 左スティック */}
      <g
        className="fill-none stroke-stage-line-strong"
        strokeWidth={1.5}
      >
        <circle cx={116} cy={92} r={16} />
        <circle
          cx={116}
          cy={92}
          r={10}
          strokeWidth={1}
          className="stroke-stage-line"
        />
      </g>
      <path
        d="M116 78v6M116 100v6M102 92h6M124 92h6"
        strokeWidth={1}
        className="stroke-stage-line"
      />
      <circle
        cx={116 + leftDot.x}
        cy={92 + leftDot.y}
        r={4.5}
        className="stage-lit fill-stage-led"
      />

      {/* 右スティック */}
      <g
        className="fill-none stroke-stage-line-strong"
        strokeWidth={1.5}
      >
        <circle cx={184} cy={92} r={16} />
        <circle
          cx={184}
          cy={92}
          r={10}
          strokeWidth={1}
          className="stroke-stage-line"
        />
      </g>
      <path
        d="M184 78v6M184 100v6M170 92h6M192 92h6"
        strokeWidth={1}
        className="stroke-stage-line"
      />
      <circle
        cx={184 + rightDot.x}
        cy={92 + rightDot.y}
        r={4}
        className="stage-lit fill-stage-led"
      />
    </svg>
  );
}

function StickReadout({
  label,
  vector,
}: {
  label: string;
  vector: StickVector;
}) {
  return (
    <div className="flex items-baseline justify-between gap-2">
      <dt className="text-caption whitespace-nowrap text-stage-text-muted">
        {label}
      </dt>
      <dd className="font-mono text-data tracking-data whitespace-nowrap text-stage-text">
        X {formatSigned(vector.x)}
        <span className="px-1 text-stage-line-strong">/</span>
        Y {formatSigned(vector.y)}
      </dd>
    </div>
  );
}

function TriggerReadout({ label, value }: { label: string; value: number }) {
  return (
    <div className="flex items-center gap-2">
      <dt className="w-6 text-caption text-stage-text-muted">{label}</dt>
      <dd className="flex flex-1 items-center gap-2">
        <span className="h-1.5 flex-1 overflow-hidden rounded-full bg-stage-line">
          <span
            className="block h-full rounded-full bg-stage-led"
            style={{ width: `${Math.round(value * 100)}%` }}
          />
        </span>
        <span className="w-9 text-right font-mono text-data tracking-data text-stage-text">
          {formatTrigger(value)}
        </span>
      </dd>
    </div>
  );
}

/**
 * 入力モニター画面。ダークステージの見本（スティック 2 つ・トリガー 2 本）。
 * 静止プレビューなので、描画と数値は同じ正規化値から作る（§5.6.1）。
 */
export function InputMonitorScreen() {
  return (
    <div>
      <h1 className="text-display font-bold text-balance text-ink">
        入力モニター
      </h1>
      <p className="mt-2 mb-5 max-w-prose text-body text-ink-2 text-pretty">
        スティックとトリガーの状態を表示します。
      </p>

      <div className="mb-5">
        <Banner tone="warning" title="見本の値です">
          ステージの表示はダミーデータです。実機の入力は次のフェーズで表示します。
        </Banner>
      </div>

      <section aria-labelledby="input-stage-title">
        <h2 id="input-stage-title" className="sr-only">
          入力モニターのステージ
        </h2>
        <div className="mb-2 flex justify-end">
          <span className="font-mono text-data tracking-data whitespace-nowrap text-muted">
            12 B / 0x0003
          </span>
        </div>

        <div
          data-surface="stage"
          className="rounded-lg border border-stage-line bg-stage-bg p-5 text-stage-text"
        >
          <div className="mb-2 flex items-center justify-between gap-2">
            <span className="inline-flex items-center gap-2 text-caption font-medium text-stage-text-2">
              <span
                aria-hidden="true"
                className="size-2 rounded-full bg-stage-led shadow-led"
              />
              見本の値
            </span>
            <span className="font-mono text-caption tracking-caption text-stage-text-muted">
              受信していません
            </span>
          </div>

          <div className="mx-auto w-full max-w-md">
            <PadGraphic left={SAMPLE.left} right={SAMPLE.right} />

            <dl className="mt-2 grid gap-1.5">
              <StickReadout label="左スティック" vector={SAMPLE.left} />
              <StickReadout label="右スティック" vector={SAMPLE.right} />
              <TriggerReadout label="L2" value={SAMPLE.leftTrigger} />
              <TriggerReadout label="R2" value={SAMPLE.rightTrigger} />
              <div className="flex items-baseline justify-between gap-2">
                <dt className="text-caption whitespace-nowrap text-stage-text-muted">
                  押下
                </dt>
                <dd className="font-mono text-data tracking-data whitespace-nowrap text-stage-text">
                  {SAMPLE.pressed}
                </dd>
              </div>
            </dl>
          </div>
        </div>

        <div className="mt-2 flex justify-between gap-2 text-meta text-muted">
          <span>Y 軸は上が正</span>
          <span>実機の入力は未接続</span>
        </div>
      </section>
    </div>
  );
}
