# v3/v4/v5 trace seed 与 expected trace 的生成方式（AC-14 h / ISSUE-104 / AC-16 e / AC-17 i）

> 资产：
> - `schema_v3_seed.sql`（v3 schema + 一条 Completed 旧 run，`PRAGMA user_version=3`）、
>   `v0.1.0_expected_trace.json`（v3 seed 迁移后旧 run 的 `zgraph trace --json` 冻结 golden）、
>   `seed_graph.json`（生成 seed 的确定性小图）；
> - **W3a / AC-16 e 新增**：`schema_v4_seed.sql`（v4 schema + 一条 Completed 旧 run，
>   `PRAGMA user_version=4`）与 `v4_expected_trace.json`（**v4-era CLI 在迁移前**生成的
>   旧 run `zgraph trace --json` 冻结 golden；用于断言 v4→v5 迁移不改写历史事件）；
> - **W4b / AC-17 i 新增**：`schema_v5_seed.sql`（v5 schema + 一条 Completed 旧 run，
>   `PRAGMA user_version=5`）与 `v5_expected_trace.json`（v5-era CLI 迁移前 golden；
>   用于断言 v5→v6 迁移不改写历史事件、旧 run 的 item 列默认 0/NULL）。
>
> 背景：v3 seed / v0.1.0 expected 是 W1 的遗留缺口（**ISSUE-104**），由 W2a e2e 交付（DR-005 §4.3/§7 h）。
> 旧 run 的 `events.payload` 保持 v3 形态（`GateResult.record` **不含** `deferred/attempt/verdict`）；
> v3→v4 只补 `gate_results` SQL 列默认值（0/0/NULL），**不改写历史 payload**（06 §6.2.8）。
> v4 seed 的 payload 是真实 v4 形态（`GateResult.record` 含 `deferred/attempt/verdict`），
> v4→v5 只重建 `instances`/`transitions`（CHECK 增 `WaitingHuman`）并新建 `human_decisions`，
> **不改写任何 `events` 行**（DR-006 §6.2），因此迁移前后 `trace --json` 必须逐字节一致。

## 1. 前置

- `v0.1.0` tag = M6 收尾（本仓库为 commit `5d43c58`，schema v3）。
- 环境：`export CARGO_HOME=<repo>/.cargo-home`；离线构建。
- 工具：`sqlite3`、`python3`。

## 2. 生成 v3 DB（v0.1.0 代码，一次性）

```bash
cd <repo>
git worktree add --detach /tmp/zgraph-v010 v0.1.0

export CARGO_HOME=<repo>/.cargo-home
CARGO_TARGET_DIR=/tmp/zgraph-v010-target \
  cargo --manifest-path /tmp/zgraph-v010/Cargo.toml build -p zgraph-cli --bin zgraph --offline

/tmp/zgraph-v010-target/debug/zgraph run \
  <repo>/tests/fixtures/trace/seed_graph.json \
  --inputs '{"start":{"message":"seed"}}' \
  --db /tmp/seed_v3.db \
  --python python3 --json
# 期望：ok=true / status=Completed / 7 条事件（1 条 GateResult passed=false）
```

`seed_graph.json` 的设计：`start` 声明 `message`，`start→b.x` 的
`check_output{path:"message",equals:"never"}` 恒 false → settle 后 `b` Skipped；不执行任何
Python 节点、不消耗 LLM，事件数与形状最小且确定。

## 3. 固定化（`sqlite3 .dump` + 归一化）

```bash
sqlite3 /tmp/seed_v3.db "PRAGMA wal_checkpoint(TRUNCATE);"
sqlite3 /tmp/seed_v3.db .dump > /tmp/seed_v3_raw.sql
python3 /tmp/freeze_seed.py < /tmp/seed_v3_raw.sql > tests/fixtures/trace/schema_v3_seed.sql
```

`freeze_seed.py`（一次性脚本，不提交仓库）的规则：

1. **UUID 固定化**：按首次出现顺序把 run_id/instance_id 映射为固定 UUIDv7 形态的测试 id
   （run = `0192f0c4-7a1b-7c3d-8e4f-000000000001`，instance 依次
   `0192f0c5-7a1b-7c3d-8e4f-00000000000N`），跨表引用一致。
2. **时间戳按表角色固定化**（与 v0.1.0 一次 run 内的写入毫秒抖动解耦，重生成字节稳定）：
   `schema_migrations.applied_at` → `2026-01-01T00:00:00.000Z`；`graphs.created_at`、
   `runs.started_at`、`RunStarted` 事件 → `…00.001Z`；`runs.finished_at/heartbeat_at`、
   其余 `events`/`instances`/`transitions`/`gate_results`/`streaming_events` → `…00.002Z`。
   由此保持 `started_at < finished_at`、事件信封与明细表 `at` 的相等关系；本 seed 的
   实际落盘结果与「distinct 值升序 +1ms」等价。
3. **易变 run 列固定化**：`owner_pid=0`、`owner_host='seed-host'`（并把
   `worker_versions_json`/`model_params_json` 置 `[]`；本 seed 本无 worker）。
4. 末尾追加 **`PRAGMA user_version = 3;`**（`.dump` 不携带 user_version）。

校验：

```bash
sqlite3 /tmp/check_seed.db < tests/fixtures/trace/schema_v3_seed.sql
sqlite3 /tmp/check_seed.db \
  "PRAGMA user_version; SELECT run_id,status FROM runs; SELECT deferred FROM gate_results;"
# user_version=3、1 条 Completed run、gate_results 无 v4 列（deferred 列不存在）
```

## 4. 生成 expected trace（当前 W2a CLI，一次性 golden）

```bash
# 复制 seed → 用当前（schema v4）CLI 跑一次 run 触发 v3→v4 迁移
cp /tmp/check_seed.db /tmp/expected.db
cargo build -p zgraph-cli --bin zgraph --offline
target/debug/zgraph run tests/fixtures/trace/seed_graph.json \
  --inputs '{"start":{"message":"seed"}}' --db /tmp/expected.db --json

# 旧 run 的 NDJSON → JSON 数组落盘
target/debug/zgraph trace 0192f0c4-7a1b-7c3d-8e4f-000000000001 \
  --db /tmp/expected.db --json > /tmp/expected.ndjson
python3 - <<'PY'
import json
lines = [json.loads(line) for line in open("/tmp/expected.ndjson") if line.strip()]
assert len(lines) == 7
assert "deferred" not in lines[4]["payload"]["record"], "旧 payload 必须保持 v3 形态"
json.dump(lines, open("tests/fixtures/trace/v0.1.0_expected_trace.json", "w"),
          ensure_ascii=False, indent=2)
PY
```

校验（e2e 内同样断言）：

- 迁移后 `user_version=6`、`schema_migrations` 1..6、`.bak.v3-*`/`.bak.v4-*`/`.bak.v5-*`
  各一份且备份版本分别为 3/4/5（v3→v4→v5→v6 链式；各步迁移 SQL 本身不变）；
- 旧 `gate_results` 三列 = `0 / 0 / NULL`；旧 `GateResult.record` 不含三键；
- `zgraph trace <old> --json` == `v0.1.0_expected_trace.json`；
- `zgraph replay --run <old> --mode events --json` 与 `trace --json` 逐字节一致；
- 只读 `trace` 对未迁移的 v3 库 exit 4（`unsupported schema version=3, expected 6`）且不迁移。

## 5. v4 seed 与 v4 expected trace（W3a AC-16 e）

生成方式与 v3 seed 完全同构，唯一区别是 source 代码与 **golden 生成时点**：

1. **v4-era 代码**：W2b 收口后的 main（本仓库为 commit `1ed37ab`，`TRACE_SCHEMA_VERSION=4`）。
   用 `git archive 1ed37ab | tar -x -C /tmp/zgraph-v4-src` 取源码（不建 worktree、不污染仓库），
   `CARGO_TARGET_DIR=/tmp/zgraph-v4-target cargo build -p zgraph-cli --bin zgraph --offline`。
2. **生成 v4 DB**（全新库，不经过迁移，v4 DDL 由 v4-era 代码直接建出）：

   ```bash
   /tmp/zgraph-v4-target/debug/zgraph run \
     <repo>/tests/fixtures/trace/seed_graph.json \
     --inputs '{"start":{"message":"seed"}}' \
     --db /tmp/seed_v4.db --python python3 --json
   # 期望：ok=true / status=Completed / 7 条事件 / 1 条 GateResult passed=false
   ```

3. **冻结（`sqlite3 .dump` + 归一化）**：归一化规则与 v3 seed **完全相同**（§3 的 1–4 条），
   另将 `PRAGMA user_version = 4;` 追加在末尾。v4 的 `gate_results` 三列保持真实运行值
   （`deferred=0 / attempt=0 / verdict_json='"seed"'`），`GateResult.record` 为真实 v4 形态。
4. **生成 v4 expected golden（迁移前！）**：直接用 **v4-era CLI** 对 v4 seed 取

   ```bash
   /tmp/zgraph-v4-target/debug/zgraph trace 0192f0c4-7a1b-7c3d-8e4f-000000000001 \
     --db /tmp/seed_v4.db --json > /tmp/v4_expected.ndjson
   python3 - <<'PY'
   import json
   lines = [json.loads(line) for line in open("/tmp/v4_expected.ndjson") if line.strip()]
   assert len(lines) == 7
   assert lines[4]["payload"]["record"]["deferred"] is False, "v4 形态必须含 deferred"
   json.dump(lines, open("tests/fixtures/trace/v4_expected_trace.json", "w"),
             ensure_ascii=False, indent=2)
   PY
   ```

   与 v3 golden 的关键区别：v4 golden 是**迁移前**（v4-era CLI）产物，因此 e2e 里
   「v4→v5 迁移后 `zgraph trace <old> --json` == `v4_expected_trace.json`」直接证明
   **迁移不改写历史事件**（DR-006 §6.2）。

校验（e2e 内同样断言）：

- `schema_v4_seed.sql` 建库后 `user_version=4`、1 条 Completed run、7 events、
  2 instances（start Completed / b Skipped）、4 transitions、1 gate_result；
- 当前 v6 CLI 只读 `trace`/`replay` 对 v4 → exit 4（`unsupported schema version=4, expected 6`），
  `user_version` 仍 4、无 `.bak.*` 备份；
- 当前 v6 CLI `run` 打开 v4 → 链式迁移 v5→v6：`.bak.v4-*`/`.bak.v5-*` 各一份且备份
  `user_version=4`/`5`、`user_version=6`、`schema_migrations` 1..6、`human_decisions` +
  pending 索引存在、`foreach_expansions` 表存在、`instances` 新列 `item_index=0`/`item_key=NULL`、
  `instances`/`transitions`/`gate_results`/`events` 行逐列保留；
- 迁移后旧 run `trace --json` == `v4_expected_trace.json`，`replay --mode=events --json`
  与 `trace --json` 逐字节一致。

## 6. 变更约束

- **不得手工编辑** `schema_v3_seed.sql` / `v0.1.0_expected_trace.json` /
  `schema_v4_seed.sql` / `v4_expected_trace.json`；需要重建时按上述步骤重跑并在
  commit message / 评审记录写明「重新生成 + 归一化规则未变」。
- seed 的 DDL 必须来自对应年代代码：v3 用 `v0.1.0`（`5d43c58`），v4 用 W2b 收口后的
  main（`1ed37ab`）；不能用当前 v5 DDL 降级伪造。
- expected golden 由对应年代 CLI 生成（v3 golden 为迁移后 golden、v4 golden 为迁移前
  golden），生成命令与归一化规则需在评审中可复核。
