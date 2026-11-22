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
- REPL の `quit` / EOF / SIGINT も同じくリンクを切るため、次回は押下が必要です。

常駐ループはこの前提で「終了コード 0 でも scan で `80ND` を確認してから再起動」「degraded は押下待ちで停止」にしています。

---

## 2. 押下を依頼する前の確認手順（必須）

ユーザーに Profile 押下を依頼する前に、**必ず `scan` で `80ND` が見えるか確認**してください。

1. **常駐ループ / `session` が動いていないか確認する**
   - `session` が動いている間は、そのプロセスが BLE 単一所有者ロックを保持しています。
     ロック保持中に `scan` を実行すると
     `another process is using the BLE adapter (lock file ...)` で失敗します。
   - まず `status`（REPL）や `session` のログ、`ps` で状態を確認し、勝手に第 2 の scan/connect を
     重ねないでください。重ねると `BLE error: In Progress` の原因になります。
2. **ループ / `session` が停止していることを確認してから scan する**
   - ループが停止済みなら、その時点でリンクは切れています。
   - `scan` を実行します。
     ```sh
     eightbitdo scan --scan-timeout 5
     ```
   - **`80ND` が見える**: まだ App Mode です。**押下を依頼せず**、接続（新しい `session` /
     ループの再起動）を試みます。ループはこの scan 確認を自動で行います。
   - **`80ND` が見えない**: App Mode を抜けています。この場合だけ
     「Profile を約 3 秒押してください」と案内します。
   - scan 自体がロック競合で失敗した場合はまだ別プロセスが BLE を使用中です。
     押下を依頼せず、そのプロセスの終了を待ちます。
3. **押下後は再度 scan してから接続する**
   - `80ND` が現れるまで、scan → 押下案内を繰り返します。
   - `80ND` が無い状態で `session` / `connect` を連続実行しないでください。
     無駄な再試行は BLE スタックの競合（`In Progress`）と再押下要求を増やします。

> 実装側の `session` は、degraded 時に「押下が必要」であることをメッセージと終了コード 3 で
> 明示します。ユーザーへの中継時も、まず scan の結果を確認してから依頼してください。

---

## 3. BLE 単一所有者ロック（per-user）

`scan` / `connect` / `session` に加え、`BleTransport::connect` を使う **すべての CLI コマンド**
（`info` / `config *` / `macro *` / `backup` / `monitor` / `keepalive`）が、BLE アダプタを
触る前に排他ロック（`flock`）を取得します。

| 項目 | 内容 |
| --- | --- |
| パス（優先順） | 1. `EIGHTBITDO_BLE_LOCK`<br>2. `$XDG_RUNTIME_DIR/8bitdo-ble.lock`（絶対パスのとき）<br>3. `$XDG_CACHE_HOME/8bitdo/8bitdo-ble.lock`<br>4. `$HOME/.cache/8bitdo/8bitdo-ble.lock`<br>5. `<temp dir>/8bitdo-ble-<euid>/8bitdo-ble.lock`（`TMPDIR` 尊重） |
| ディレクトリ | 自動作成する場合は mode `0700`。既存ディレクトリ（テストの一時 dir 等）は chmod しない |
| 保持期間 | `scan`: scan 中のみ / `connect`・`session`・各コマンド: 接続保持中ずっと（切断またはプロセス終了で解放） |
| 競合時 | 待たずに失敗し、`another process is using the BLE adapter (lock file <path>)` を表示する |
| 解放 | プロセス終了（`SIGKILL` 含む）でカーネルが解放する。ロックファイルは削除しない（別 inode での二重取得を防ぐため） |

**ロックは per-user スコープです。** 別ユーザーの `eightbitdo` とは排他しません
（`XDG_RUNTIME_DIR` や `~/.cache` がユーザーごとに分かれるため）。同一ユーザー内の
CLI / 将来の GUI / 手動 `flock` の間だけを排他します。

### 安全性

- Unix では `O_NOFOLLOW` で開き、**symlink を追従しません**。
- 開いたファイル記述子が **通常ファイル** かつ **実効ユーザー所有** であることを検証します
  （所有者検証は Unix のみ。Windows は `symlink_metadata` による「symlink でない通常ファイル」の
  事前検証のみで、fd の所有者検証は行いません）。
- 事前チェック（`symlink_metadata`）に加えて fd 側でも検証するため、共有ディレクトリで
  symlink / FIFO / ディレクトリ / 他ユーザー所有ファイルを仕込まれても拒否されます
  （`refusing to use BLE lock file ...`）。
- ロックファイルは mode `0600` にします。
- `EIGHTBITDO_BLE_LOCK` で明示指定した場合も同じ検証を行います。

`session` の実行中は、別プロセスの `scan` / `connect` が拒否されます。これが正しい動作です。
手動で `flock` を取って確認する場合（ロックパスは XDG 優先の既定値に合わせる）:

```sh
flock "${XDG_RUNTIME_DIR:-$HOME/.cache}/8bitdo-ble.lock" -c 'eightbitdo scan --scan-timeout 2'   # 失敗する
```

`XDG_RUNTIME_DIR` が無い場合の実際のパスは `eightbitdo scan` のエラーメッセージ
（`lock file ...`）で確認してください。

---

## 4. `session` の終了条件と終了コード

| 終了コード | 条件 | 常駐ループの動作 |
| --- | --- | --- |
| `0` | REPL 内の明示 `quit`、入力終端（EOF）、REPL 実行中の SIGINT | scan で `80ND` を確認。見えれば最低 2 秒 + 指数 backoff で再起動、見えなければ押下案内して停止 |
| `1` | 接続失敗、ロック競合、コマンドが観測した `Disconnected`、その他のエラー | **再起動しない**（`80ND` 不在なら押下が必要） |
| `3` | keepalive タスクの degraded（**連続する keepalive I/O エラー 3 回**または `Disconnected`） | **再起動しない**。押下待ちとしてログを出して停止 |

- exit 3 になるのは **keepalive タスクの degraded だけ**です。コマンド実行中に
  `Disconnected` を観測した場合は exit 1 です（リンクは既に切れていますが degraded 判定とは別）。
- degraded は「I/O エラー（送信・受信・プロトコルエラー）が連続 3 回」で成立します。
  `0x0003` の応答が来ないだけ（無応答）は failure に数えません。keepalive の 1 tick は
  送信→短い受信 drain で、レポート未着は正常な idle として扱います。
- degraded のメッセージは既存の `keepalive degraded (consecutive failures N): ...` を再利用し、
  末尾に `session exit code 3: ... Physical controller action (Profile, about 3 seconds) is required ...`
  を表示します。
- SIGINT の扱い:
  - REPL 実行中: 警告を出してセッションを終了（exit 0）。
  - 接続前〜REPL 開始前（connect の scan、`0x0007` 送信）: 警告を出して exit **130**。
  - `info` / `config` / `macro`: SIGINT でコマンドを中断し、警告を出して exit **130**。
  - `monitor` / `keepalive` / `backup --hold`: 保持ループを止め、警告を出して通常終了（exit 0）。
  - `scan`: リンクを持たないため警告なし（従来の SIGINT 終了）。
- 警告文は「BLE が切れて `80ND` が消える。再開には Profile 約 3 秒押しが必要」を明示します。
- 切断処理（unsubscribe / disconnect）が失敗した場合、`BleTransport` は
  **単一所有者ロックを保持したまま** `Err` を返し、CLI は警告を表示します。
  プロセス終了時にカーネルが解放しますが、その間は別プロセスから BLE を操作できません。
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
- `tail` はスーパーバイザ起動時に 1 つだけ起動し、再起動をまたいで同じ制御ストリームを使います。
  **backoff 中に追記したコマンドも次のセッションに渡ります**（消失しません）。
- セッション終了コードごとの動作:
  - **0**: まず `eightbitdo scan --scan-timeout 5` で `80ND` を確認します。
    - 見える → 最低 2 秒待ち、その後 2 → 4 → … → 最大 15 秒の指数 backoff で再起動。
      セッションが 60 秒以上生存した場合は backoff を初期値に戻します。
    - 見えない → `WAITING FOR PHYSICAL ACTION: press Profile ...` を出して停止（exit 3）。
    - scan 自体が失敗（別プロセスがロック保持 / アダプタエラー）→ 再起動せず停止（exit 1）。
  - **3**: 再起動せず、押下待ちログを出して停止（exit 3）。
  - **その他の非 0**: 再起動しません（無限リトライによる競合と押下要求の連発を防ぐため）。
    ロック競合なら他プロセスの終了を待ち、接続失敗なら scan で `80ND` を確認してから再実行します。
- `SIGINT` / `SIGTERM` は子セッションと `tail` を後始末して終了します（130 / 143）。
- 終了時は `tail` を必ず後始末します（`tail -f | session` のパイプラインが終了できず
  プロセスが残る事故を防ぐため）。
- **依存**: bash 4 以上（`coproc`）と GNU coreutils の `date`。macOS / BSD の
  ユーザーランド（bash 3.2、BSD date）では動作しません。

回帰テスト（実機・BLE 不要）:

```sh
scripts/test-session-loop.sh
```

degraded 即停止 / scan 確認つき再起動 / `80ND` 不在時の押下停止 / scan 失敗時の停止 /
backoff の増加とリセット / backoff 中のコマンド保持 / SIGTERM をスタブで確認します。

---

## 6. トラブルシューティング

| 症状 | 確認・対処 |
| --- | --- |
| `BLE error: In Progress` | 別プロセスの scan/connect が同時に動いていないか。単一所有者ロックがあるため、通常はロック競合メッセージで先に拒否される。 |
| `another process is using the BLE adapter` | `session` などがロック保持中。`ps` で確認し、必要ならそのセッションを `quit` で終了してから再実行する。 |
| `refusing to use BLE lock file ...` | ロックパスが symlink / 非通常ファイル / 他ユーザー所有。`EIGHTBITDO_BLE_LOCK` の指定やディレクトリの所有者・パーミッションを確認する。 |
| `keepalive degraded` | リンクが切れている。**scan で `80ND` を確認**し、見えなければ Profile 約 3 秒押下を依頼する。見える場合は押下不要。 |
| ループが停止した | 終了コードを確認する。3 またはその他の非 0 は意図的な停止。ログの案内に従う。 |
| ループが scan 失敗で停止した | 別の BLE プロセスがロックを保持している。その終了後にループを再実行する。 |
| `80ND` が見えない | コントローラが App Mode ではない。Profile 約 3 秒押下後に再 scan。 |

---

## 7. 互換性 / 破壊的変更（pre-1.0）

- `eightbitdo_core::BleOptions` に公開フィールド **`lock_path: Option<PathBuf>`** を追加しました。
  外部で構造体リテラルを構築しているコードは `lock_path: None` を追加する必要があります
  （または `..BleOptions::default()` を使用してください）。pre-1.0 のためマイナー変更として扱います。
- 既定のロックパスは **`/tmp` 直下から per-user ディレクトリに変更**されました。
  旧 `flock /tmp/8bitdo-ble.lock` を使う外部スクリプトは、新しいパス
  （`$XDG_RUNTIME_DIR/8bitdo-ble.lock` など）に合わせてください。
- ロックは per-user であり、別ユーザー同士は排他しません。
