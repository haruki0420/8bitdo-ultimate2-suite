import { VOCAB } from "../design/tokens";

export type ViewId =
  | "devices"
  | "monitor"
  | "current"
  | "settings"
  | "macros"
  | "logs";

export interface ViewDefinition {
  id: ViewId;
  label: string;
}

/**
 * 左ナビの並び。用語は docs/DESIGN_TAURI.md §6.3 の採用表記に従う
 * （入力モニター / 現在値。旧称は使わない）。
 */
export const VIEWS: readonly ViewDefinition[] = [
  { id: "devices", label: "デバイス" },
  { id: "monitor", label: VOCAB.inputMonitor },
  { id: "current", label: VOCAB.currentValues },
  { id: "settings", label: "設定" },
  { id: "macros", label: "マクロ" },
  { id: "logs", label: "ログ" },
];
