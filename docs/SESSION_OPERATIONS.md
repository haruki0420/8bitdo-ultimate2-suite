# 常駐セッション運用ガイド（BLE / `80ND`）

`eightbitdo session` を常駐させる場合の、接続競合・degraded・物理操作の扱いをまとめます。
対象は BLE App Mode（広告名 `80ND`、service `FF10`）です。

---

## 1. 前提: 切断すると `80ND` は広告されない

8BitDo Ultimate 2 の App Mode（`80ND`）は **BLE 接続中だけ**有効です。
接続が切れるとコントローラは通常モードに戻り、**App Mode の広告を出しません**。

このため、

- 「degraded になったら自動で再接続する」ことは **できません**。
- 再接続には **コントローラの Profile ボタンを約 3 秒押す**物理操作が必要です。
- 接続中のプロセスを終了すると、その時点で `80ND` が消えます（次回も押下が必要）。

常駐ループはこの前提で「正常終了だけ再起動」「degraded は押下待ちで停止」にしています。

---

## 2. 押下を依頼する前の確認手順（必須）

ユーザーに Profile 押下を依頼する前に、**必ず `scan` で `80ND` が見えるか確認**してください。

1. **別の BLE 操作が動いていないことを確認する**
   - `session` が動いている場合、そのプロセスが BLE 単一所有者ロックを保持しています。
     ロック保持中に `scan` を実行すると
     `another process is using the BLE adapter (lock file ...)` で失敗します。
   - まず `status`（REPL）や `session` のログで状態を確認し、勝手に第 2 の scan/connect を
     重ねないでください。重ねると `BLE error: In Progress` の原因になります。
2. **`80ND` が見えるか scan する**
   ```sh
   eightbitdo scan --scan-timeout 5
   ```
   - **`80ND` が見える**: まだ App Mode です。**押下を依頼せず**、接続を試みるか、
     常駐セッションが生きているならそのまま待ちます（自動再接続を待つ）。
   - **`80ND` が見えない**: App Mode を抜けています。この場合だけ
     「Profile を約 3 秒押してください」と案内します。
3. **押下後は再度 scan してから接続する**
   - `80ND` が現れるまで、scan → 押下案内を繰り返します。
   - `80ND` が無い状態で `session` / `connect` を連続実行しないでください。
     無駄な再試行は BLE スタックの競合（`In Progress`）と再押下要求を増やします。

> 実装側の `session` は、degraded 時に「押下が必要」であることをメッセージと終了コード 3 で
> 明示します。ユーザーへの中継時も、まず scan の結果を確認してから依頼してください。

---

## 3. BLE 単一所有者ロック

`scan` / `connect` / `session` は、BLE アダプタを触る前に排他ロック（`flock`）を取得します。

| 項目 | 内容 |
| --- | --- |
| パス | `$XDG_RUNTIME_DIR/8bitdo-ble.lock`（`XDG_RUNTIME_DIR` が絶対パスのとき）<br>無ければ `/tmp/8bitdo-ble.lock`（Windows は一時ディレクトリ） |
| 上書き | 環境変数 `EIGHTBITDO_BLE_LOCK`（テスト・ラッパー用） |
| 保持期間 | `scan`: scan 中のみ / `connect`・`session`: 接続保持中ずっと（切断またはプロセス終了で解放） |
| 競合時 | 待たずに失敗し、`another process is using the BLE adapter (lock file <path>)` を表示する |
| 解放 | プロセス終了（`SIGKILL` 含む）でカーネルが解放する。ロックファイルは削除しない（別 inode での二重取得を防ぐため） |

- `session` の実行中は、別プロセスの `scan` / `connect` が拒否されます。これが正しい動作です。
- 手動で `flock` を取って確認する場合:
  ```sh
  flock /tmp/8bitdo-ble.lock -c 'eightbitdo scan --scan-timeout 2'   # 失敗する
  ```
- 将来の GUI（別ブランチ）も同じロックを取得する予定です。GUI と CLI を同時に使う場合も、
  片方だけが BLE を操作できます。

---

## 4. `session` の終了条件と終了コード

| 終了コード | 意味 | 常駐ループの動作 |
| --- | --- | --- |
| `0` | 明示 `quit`、入力終端（EOF）、SIGINT | 2 秒以上待って再起動（指数 backoff、最大 15 秒） |
| `1` | 接続失敗・ロック競合・その他のエラー | **再起動しない**（`80ND` 不在なら押下が必要） |
| `3` | keepalive degraded（連続失敗 3 回または `Disconnected`） | **再起動しない**。押下待ちとしてログを出して停止 |

- degraded のメッセージは既存の `keepalive degraded (consecutive failures N): ...` を再利用し、
  末尾に `session exit code 3: ... Physical controller action (Profile, about 3 seconds) is required ...`
  を表示します。
- SIGINT（Ctrl-C）は従来どおりセッションを終了しますが、終了直前に
  「BLE が切れて `80ND` が消え、再開に Profile 約 3 秒押しが必要」である旨を stderr に表示します。
- `quit` 以外の内部エラー（コマンド失敗など）は、それだけではセッションを終了しません。
  終了するのは明示 `quit`、入力終端、SIGINT、degraded、および復旧不能な I/O エラーです。

---

## 5. 常駐ループ `scripts/session-loop.sh`

```sh
# 既定: .tmp/sess.cmd を読み、target/release/eightbitdo を起動する
scripts/session-loop.sh

# 追加の session 引数を渡す
scripts/session-loop.sh --allow-macro-write --allow-config-write --scan-timeout 30
```

- 制御コマンドは従来どおり **`.tmp/sess.cmd` に追記**します（`tail -f` で読む）。
  ```sh
  printf 'status\n' >> .tmp/sess.cmd
  printf 'quit\n' >> .tmp/sess.cmd
  ```
- 正常終了（0）時のみ再起動します。再起動前は **最低 2 秒**待ち（旧リンクの teardown との競合回避）、
  その後 2 → 4 → … → 最大 15 秒の指数 backoff です。セッションが 60 秒以上生存した場合は
  backoff を初期値に戻します。
- 終了コード 3（degraded）では再起動せず、
  `WAITING FOR PHYSICAL ACTION: press Profile ... NOT retried automatically` をログに出して停止します。
- その他の非 0 も再起動しません（無限リトライによる競合と押下要求の連発を防ぐため）。
  ロック競合（`another process is using the BLE adapter`）で停止した場合は、他の BLE プロセスが
  終了してからループを再実行してください。接続失敗（`device not found`）で停止した場合は
  `scan` で `80ND` を確認し、必要なら押下を依頼してから再実行します。
- ループはセッション終了ごとに `tail` を後始末します（`tail -f | session` のパイプラインが
  終了できずプロセスが残る事故を防ぐため）。

回帰テスト:

```sh
scripts/test-session-loop.sh
```

degraded で即停止すること、正常終了後に 2 秒以上待つこと、その他の非 0 で停止することを
スタブバイナリで確認します（実機不要）。

---

## 6. トラブルシューティング

| 症状 | 確認・対処 |
| --- | --- |
| `BLE error: In Progress` | 別プロセスの scan/connect が同時に動いていないか。単一所有者ロックがあるため、通常はロック競合メッセージで先に拒否される。 |
| `another process is using the BLE adapter` | `session` などがロック保持中。`ps` で確認し、必要ならそのセッションを `quit` で終了してから再実行する。 |
| `keepalive degraded` | リンクが切れている。**scan で `80ND` を確認**し、見えなければ Profile 約 3 秒押下を依頼する。見える場合は押下不要。 |
| ループが停止した | 終了コードを確認する。3 またはその他の非 0 は意図的な停止。ログの案内に従う。 |
| `80ND` が見えない | コントローラが App Mode ではない。Profile 約 3 秒押下後に再 scan。 |
