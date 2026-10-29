# graph 配置索引

约定：配置文件名、配置内 `name` 字段、`src/worker/<name>/` worker 包三者同名。
`name` 被用于两处解析——dispatcher 按 `src.worker.{name}.{node}_worker` 拉起子进程，
worker 再按 `config/{name}_graph.json` 回读本配置，改名时三处必须同步。

- `software_team_graph.json`: 软件开发全流程 SOP，12 节点 / 25 边。角色链：
  PM(planer) → SE(archer) → {MDE ‖ TE}；MDE → FO，TE → MTE → MTO、TE → STE → STO；
  FO → MTO / STO（返工后重拉起），MTO → STO → QA → release → release_done(sink)。
  每个角色用独立 gate（`check_output` op）自环到产物落盘；5 条 manual 边表示人工介入：
  需求评审(planer_to_archer)、设计评审(archer_to_mde / archer_to_te)、
  QA 评审(qa_to_release)、发布审批(release_publish)。
  两处 parallel fan-out：SE 同时放行 MDE/TE，TE 同时放行 MTE/STE。
  注意：除 planer/archer 外各节点 `proc` 为空（走 `proc_bypass`），
  产物需由用户自建 worker 写入，尚未内置。
- `test_for_e2e_graph.json`: e2e 测试图。harvester 自环 3 次（gate 按 exit_cnt 阈值翻转）后切到 qa_reviewer，
  qa → publisher 是 manual 边，需外部 trigger 才推进。覆盖自环阈值、manual trigger、多产物文件、跨实例并发隔离。
- `test_for_complex_graph.json`: 并行 fan-out 测试图。fetcher 经 3 条 parallel 边同时派发 worker_a/b/c，
  joiner 等三者产物齐全后推进，verifier 首轮 invalid 自环、次轮 valid 才放行到 publisher。
