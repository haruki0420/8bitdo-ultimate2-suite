# 常駐セッション運用ガイド（BLE / `80ND`）

`eightbitdo session`（BLE App Mode: 広告名 `80ND`、service `FF10`）の接続競合・degraded・物理操作の運用手順です。

## 1. 前提: 切断すると `80ND` は消える

App Mode（`80ND`）は **BLE 接続中のみ有効**で、切断・プロセス終了・`quit` / EOF / SIGINT で通常モードへ戻り広告が消えます。そのため degraded 時の自動再接続はできず、再開には **Profile ボタン約 3 秒押し**が必要です。

## 2. 押下依頼前の確認手順（必須）

Profile 押下を依頼する前に、必ず `80ND` の広告有無を確認する。

1. 既存の `session` や常駐ループが動いていないか確認する（稼働中は BLE ロック保持中のため第 2 の scan/connect を重ねない）。
2. 停止後 **2 秒以上待ってから** `eightbitdo scan --check --scan-timeout 5` を実行する（exit **0** = `80ND` または `FF10` あり / **3** = なし / その他 = エラー）。
3. exit 0 なら押下不要で接続する。exit 3 のときだけ Profile 約 3 秒押しを依頼する。その他（ロック競合等）は他プロセスの終了を待つ。

## 3. per-user BLE 単一所有者ロック

`scan` / `connect` / `session` および `BleTransport::connect` を使う全 CLI コマンドが BLE 操作前に排他 `flock` を取得する（別ユーザー同士は排他しない）。

- **パス優先順**: ①`EIGHTBITDO_BLE_LOCK`（テスト/ラッパー用の信頼された上書き）②Unix: NSS（`getpwuid_r`）の euid ホーム由来 `~/.cache/8bitdo/8bitdo-ble.lock`（`$HOME`・`XDG_RUNTIME_DIR`・`XDG_CACHE_HOME` は非参照）③ホーム不明時のみ `<temp>/8bitdo-ble-<euid>/8bitdo-ble.lock`。
- **保持と解放**: `scan` は scan 中のみ、接続系は切断またはプロセス終了まで保持（`SIGKILL` 含むカーネル解放、inode 二重取得防止のためロックファイルは削除しない）。切断処理失敗時はロック保持のまま `Err` を返す。
- **安全性**: Unix では `O_NOFOLLOW` + dirfd（`openat`）で TOCTOU を閉じ、ロックファイルは通常ファイル・euid 所有・mode `0600`、既定パスの親ディレクトリは非 symlink・euid 所有・group/other write なし（自動作成 `0700`）を検証する（`EIGHTBITDO_BLE_LOCK` は親の所有者/mode 検証のみ省略。Windows は `symlink_metadata` による非 symlink 検証のみ）。競合時は待たずに失敗する。

## 4. `session` の終了コードと SIGINT

| 終了コード | 条件 | `session-loop.sh` の動作 |
| --- | --- | --- |
| `0` | 明示 `quit` / EOF / REPL 中 SIGINT | 2 秒待機 → `scan --check`。見えたら指数 backoff（2→4→…最大 15 秒、60 秒生存でリセット）で再起動、見えなければ押下案内して exit 3 |
| `1` | 接続失敗・ロック競合・コマンド観測の `Disconnected` 等 | 再起動せず停止 |
| `3` | keepalive の degraded（**連続 I/O エラー 3 回**または `Disconnected`。無応答は失敗に数えない） | 再起動せず押下案内して停止 |

- **SIGINT**: REPL 中は exit 0。接続前・`info` / `config dump` / `config map show` / `macro backup` / `backup` / `monitor` / `keepalive` は警告を出して exit **130**。flash 書込（`config/macro write/set/remap/restore`）は prepare 段階（ファイル読込・pre-write backup・確認プロンプト）がキャンセル可（フレーム未送信で exit 130）、**最初の flash フレーム以降のみ非キャンセル（遅延）**で完了後に復旧案内 + exit 130（書込失敗時は exit 1）。同時発生時はコマンド完了優先。

## 5. 常駐ループ `scripts/session-loop.sh` と互換性

- 起動: `scripts/session-loop.sh [--allow-macro-write ...]`（制御は `.tmp/sess.cmd` へ追記。`tail -f` を 1 本維持するため backoff 中のコマンドも消失しない）。
- 依存: **bash 4+**（`coproc`）と **GNU `date`**（macOS / BSD 非対応）。回帰テスト: `scripts/test-session-loop.sh`。
- **トラブルシューティング**: `In Progress` / ロック競合 → 他プロセス終了を待つ。`refusing to use BLE lock file` → パス種別・権限を確認。`keepalive degraded` → `scan --check` で `80ND` を確認し不在時のみ Profile 3 秒押し。
- **pre-1.0 互換変更**: `BleOptions` に `lock_path: Option<PathBuf>` 追加、既定ロックパスを `/tmp` 直下から NSS 由来 `~/.cache/8bitdo/8bitdo-ble.lock` へ変更、`scan --check` 追加。
