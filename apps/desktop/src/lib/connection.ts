/**
 * 接続中のデバイス情報。Rust 側（BLE 接続の実装後）が返す値だけを表示に使い、
 * UI にデバイス名をハードコードしない。BLE 接続は未実装のため、いまは常に
 * `null`（未接続）として扱う。
 */
export interface ConnectionInfo {
  /** デバイスが名乗る名前 */
  name: string;
  /** BLE アドレス（設定モードで接続したときの値） */
  address: string;
  /** ネゴシエート済みの ATT MTU */
  mtu: number;
}
