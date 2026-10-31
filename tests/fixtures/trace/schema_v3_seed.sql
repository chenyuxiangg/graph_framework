PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE schema_migrations (
  version     INTEGER PRIMARY KEY,
  description TEXT    NOT NULL,
  applied_at  TEXT    NOT NULL
);
INSERT INTO schema_migrations VALUES(1,'v1: base trace schema','2026-01-01T00:00:00.000Z');
INSERT INTO schema_migrations VALUES(2,'v2: reproducibility columns/tables','2026-01-01T00:00:00.000Z');
INSERT INTO schema_migrations VALUES(3,'v3: runs.global_vars_json (ISSUE-087)','2026-01-01T00:00:00.000Z');
CREATE TABLE graphs (
  graph_hash  TEXT PRIMARY KEY,
  graph_name  TEXT,
  dsl_version TEXT NOT NULL,
  dsl_json    TEXT NOT NULL,
  created_at  TEXT NOT NULL
);
INSERT INTO graphs VALUES('1d306ffb07a46182','trace-v3-seed','1.0','{"version":"1.0","metadata":{"name":"trace-v3-seed","description":"AC-14 h / ISSUE-104 的 v3 trace seed 生成图：确定性、无 Python/LLM 执行（b 被 gate 拦截后 Skipped），恰好一条 GateResult"},"entry_points":["start"],"nodes":[{"id":"start","type":"start","inputs":[{"name":"message","type":"string","required":true}],"config":{}},{"id":"b","type":"python_node","inputs":[{"name":"x","type":"any","required":true}],"trigger":"all","config":{"module":"workers.echo","function":"run"}}],"edges":[{"from":"start","to_node":"b","to_input_port":"x","gate":{"function":"check_output","params":{"path":"message","equals":"never"}}}]}','2026-01-01T00:00:00.001Z');
CREATE TABLE runs (
  run_id           TEXT PRIMARY KEY,
  graph_hash       TEXT NOT NULL REFERENCES graphs(graph_hash),
  status           TEXT NOT NULL
                   CHECK (status IN ('Running','Paused','Completed','Failed','Cancelled')),
  user_id          TEXT,
  started_at       TEXT NOT NULL,
  finished_at      TEXT,
  entry_inputs     TEXT,
  output_json      TEXT,
  error_code       TEXT,
  error_message    TEXT,
  owner_pid        INTEGER,
  owner_host       TEXT,
  heartbeat_at     TEXT,
  interrupted      INTEGER NOT NULL DEFAULT 0 CHECK (interrupted IN (0,1)),
  recovered_at     TEXT,
  last_seq         INTEGER NOT NULL DEFAULT 0,
  event_count      INTEGER NOT NULL DEFAULT 0,
  streaming_dropped INTEGER NOT NULL DEFAULT 0,
  abandoned_tasks  INTEGER NOT NULL DEFAULT 0,
  trace_backpressure INTEGER NOT NULL DEFAULT 0,
  seed             INTEGER NOT NULL DEFAULT 0,
  worker_versions_json TEXT,
  model_params_json TEXT,
  record_mode      TEXT,
  killed_workers   INTEGER NOT NULL DEFAULT 0,
  global_vars_json TEXT
);
INSERT INTO runs VALUES('0192f0c4-7a1b-7c3d-8e4f-000000000001','1d306ffb07a46182','Completed','local','2026-01-01T00:00:00.001Z','2026-01-01T00:00:00.002Z','{"start":{"message":"seed"}}',NULL,NULL,NULL,0,'seed-host','2026-01-01T00:00:00.002Z',0,NULL,7,7,0,0,0,0,'[]','[]','hash',0,'{}');
CREATE TABLE instances (
  instance_id  TEXT PRIMARY KEY,
  run_id       TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  node_id      TEXT NOT NULL,
  node_type    TEXT NOT NULL CHECK (node_type IN
               ('start','end','python_node','agent_call','subgraph')),
  attempt      INTEGER NOT NULL DEFAULT 0,
  status       TEXT NOT NULL CHECK (status IN
               ('Pending','Ready','Running','Completed','Crashed','TimedOut','Cancelled','Skipped')),
  input        TEXT,
  output_json  TEXT,
  error        TEXT,
  error_code   TEXT,
  created_seq  INTEGER,
  started_at   TEXT,
  finished_at  TEXT,
  finished_seq INTEGER,
  UNIQUE (run_id, node_id, attempt)
);
INSERT INTO instances VALUES('0192f0c5-7a1b-7c3d-8e4f-000000000001','0192f0c4-7a1b-7c3d-8e4f-000000000001','start','start',0,'Completed','{"message":"seed"}','{"message":"seed"}',NULL,NULL,NULL,'2026-01-01T00:00:00.002Z','2026-01-01T00:00:00.002Z',4);
INSERT INTO instances VALUES('0192f0c5-7a1b-7c3d-8e4f-000000000002','0192f0c4-7a1b-7c3d-8e4f-000000000001','b','python_node',0,'Skipped',NULL,NULL,NULL,NULL,NULL,NULL,'2026-01-01T00:00:00.002Z',6);
CREATE TABLE events (
  seq         INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id      TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  kind        TEXT NOT NULL CHECK (kind IN
              ('RunStarted','Transition','GateResult','NodeTimedOut',
               'NodeAbandoned','Streaming','RunFinished')),
  severity    TEXT NOT NULL DEFAULT 'info' CHECK (severity IN ('info','warn','error')),
  at          TEXT NOT NULL,
  instance_id TEXT,
  node_id     TEXT,
  payload     TEXT NOT NULL
);
INSERT INTO events VALUES(1,'0192f0c4-7a1b-7c3d-8e4f-000000000001','RunStarted','info','2026-01-01T00:00:00.001Z',NULL,NULL,'{"status":"Running","graph_hash":"1d306ffb07a46182","user_id":"local"}');
INSERT INTO events VALUES(2,'0192f0c4-7a1b-7c3d-8e4f-000000000001','Transition','info','2026-01-01T00:00:00.002Z','0192f0c5-7a1b-7c3d-8e4f-000000000001','start','{"record":{"run_id":"0192f0c4-7a1b-7c3d-8e4f-000000000001","instance_id":"0192f0c5-7a1b-7c3d-8e4f-000000000001","node_id":"start","attempt":0,"from":"Pending","to":"Ready","reason":"inputs ready","ts":"2026-01-01T00:00:00.002Z"}}');
INSERT INTO events VALUES(3,'0192f0c4-7a1b-7c3d-8e4f-000000000001','Transition','info','2026-01-01T00:00:00.002Z','0192f0c5-7a1b-7c3d-8e4f-000000000001','start','{"record":{"run_id":"0192f0c4-7a1b-7c3d-8e4f-000000000001","instance_id":"0192f0c5-7a1b-7c3d-8e4f-000000000001","node_id":"start","attempt":0,"from":"Ready","to":"Running","reason":"dispatch","ts":"2026-01-01T00:00:00.002Z"}}');
INSERT INTO events VALUES(4,'0192f0c4-7a1b-7c3d-8e4f-000000000001','Transition','info','2026-01-01T00:00:00.002Z','0192f0c5-7a1b-7c3d-8e4f-000000000001','start','{"record":{"run_id":"0192f0c4-7a1b-7c3d-8e4f-000000000001","instance_id":"0192f0c5-7a1b-7c3d-8e4f-000000000001","node_id":"start","attempt":0,"from":"Running","to":"Completed","reason":"execution ok","ts":"2026-01-01T00:00:00.002Z"}}');
INSERT INTO events VALUES(5,'0192f0c4-7a1b-7c3d-8e4f-000000000001','GateResult','warn','2026-01-01T00:00:00.002Z','0192f0c5-7a1b-7c3d-8e4f-000000000001','start','{"record":{"run_id":"0192f0c4-7a1b-7c3d-8e4f-000000000001","instance_id":"0192f0c5-7a1b-7c3d-8e4f-000000000001","node_id":"start","edge_index":0,"to_node":"b","to_port":"x","gate_fn":"check_output","passed":false,"error":null,"ts":"2026-01-01T00:00:00.002Z"}}');
INSERT INTO events VALUES(6,'0192f0c4-7a1b-7c3d-8e4f-000000000001','Transition','info','2026-01-01T00:00:00.002Z','0192f0c5-7a1b-7c3d-8e4f-000000000002','b','{"record":{"run_id":"0192f0c4-7a1b-7c3d-8e4f-000000000001","instance_id":"0192f0c5-7a1b-7c3d-8e4f-000000000002","node_id":"b","attempt":0,"from":"Pending","to":"Skipped","reason":"all_sources_blocked","ts":"2026-01-01T00:00:00.002Z"}}');
INSERT INTO events VALUES(7,'0192f0c4-7a1b-7c3d-8e4f-000000000001','RunFinished','info','2026-01-01T00:00:00.002Z',NULL,NULL,'{"status":"Completed"}');
CREATE TABLE transitions (
  transition_id INTEGER PRIMARY KEY AUTOINCREMENT,
  seq           INTEGER NOT NULL UNIQUE REFERENCES events(seq) ON DELETE CASCADE,
  run_id        TEXT NOT NULL,
  instance_id   TEXT NOT NULL,
  node_id       TEXT NOT NULL,
  attempt       INTEGER NOT NULL,
  from_status   TEXT NOT NULL CHECK (from_status IN
                ('Pending','Ready','Running','Completed','Crashed','TimedOut','Cancelled','Skipped')),
  to_status     TEXT NOT NULL CHECK (to_status IN
                ('Pending','Ready','Running','Completed','Crashed','TimedOut','Cancelled','Skipped')),
  reason        TEXT,
  error_code    TEXT,
  error         TEXT,
  at            TEXT NOT NULL
);
INSERT INTO transitions VALUES(1,2,'0192f0c4-7a1b-7c3d-8e4f-000000000001','0192f0c5-7a1b-7c3d-8e4f-000000000001','start',0,'Pending','Ready','inputs ready',NULL,NULL,'2026-01-01T00:00:00.002Z');
INSERT INTO transitions VALUES(2,3,'0192f0c4-7a1b-7c3d-8e4f-000000000001','0192f0c5-7a1b-7c3d-8e4f-000000000001','start',0,'Ready','Running','dispatch',NULL,NULL,'2026-01-01T00:00:00.002Z');
INSERT INTO transitions VALUES(3,4,'0192f0c4-7a1b-7c3d-8e4f-000000000001','0192f0c5-7a1b-7c3d-8e4f-000000000001','start',0,'Running','Completed','execution ok',NULL,NULL,'2026-01-01T00:00:00.002Z');
INSERT INTO transitions VALUES(4,6,'0192f0c4-7a1b-7c3d-8e4f-000000000001','0192f0c5-7a1b-7c3d-8e4f-000000000002','b',0,'Pending','Skipped','all_sources_blocked',NULL,NULL,'2026-01-01T00:00:00.002Z');
CREATE TABLE gate_results (
  gate_result_id INTEGER PRIMARY KEY AUTOINCREMENT,
  seq            INTEGER NOT NULL UNIQUE REFERENCES events(seq) ON DELETE CASCADE,
  run_id         TEXT NOT NULL,
  instance_id    TEXT NOT NULL,
  node_id        TEXT NOT NULL,
  edge_index     INTEGER NOT NULL,
  to_node        TEXT NOT NULL,
  to_port        TEXT NOT NULL,
  gate_fn        TEXT,
  passed         INTEGER NOT NULL CHECK (passed IN (0,1)),
  error          TEXT,
  at             TEXT NOT NULL
);
INSERT INTO gate_results VALUES(1,5,'0192f0c4-7a1b-7c3d-8e4f-000000000001','0192f0c5-7a1b-7c3d-8e4f-000000000001','start',0,'b','x','check_output',0,NULL,'2026-01-01T00:00:00.002Z');
CREATE TABLE streaming_events (
  streaming_event_id INTEGER PRIMARY KEY AUTOINCREMENT,
  seq            INTEGER NOT NULL UNIQUE REFERENCES events(seq) ON DELETE CASCADE,
  run_id         TEXT NOT NULL,
  instance_id    TEXT NOT NULL,
  kind           TEXT NOT NULL CHECK (kind IN
                 ('token','reasoning','tool_call_start','tool_call_end','progress')),
  segment_index  INTEGER NOT NULL,
  text           TEXT,
  text_chars     INTEGER,
  text_bytes     INTEGER,
  truncated      INTEGER NOT NULL DEFAULT 0 CHECK (truncated IN (0,1)),
  delta_count    INTEGER,
  call_id        TEXT,
  tool_name      TEXT,
  ok             INTEGER,
  summary        TEXT,
  percent        INTEGER,
  first_at       TEXT NOT NULL,
  last_at        TEXT NOT NULL
);
CREATE TABLE recorded_outputs (
  recorded_output_id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id        TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  instance_id   TEXT NOT NULL,
  node_id       TEXT NOT NULL,
  attempt       INTEGER NOT NULL,
  output_hash   TEXT NOT NULL,
  output_json   TEXT,
  recorded_at   TEXT NOT NULL,
  UNIQUE (run_id, instance_id, attempt)
);
CREATE TABLE llm_calls (
  llm_call_id   INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id        TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  instance_id   TEXT NOT NULL,
  call_index    INTEGER NOT NULL,
  request_json  TEXT NOT NULL,
  response_json TEXT,
  status        TEXT NOT NULL,
  recorded_at   TEXT NOT NULL,
  UNIQUE (run_id, instance_id, call_index)
);
INSERT INTO sqlite_sequence VALUES('events',7);
INSERT INTO sqlite_sequence VALUES('transitions',4);
INSERT INTO sqlite_sequence VALUES('gate_results',1);
CREATE INDEX idx_runs_status_started ON runs(status, started_at);
CREATE INDEX idx_runs_unfinished ON runs(status) WHERE status IN ('Running','Paused');
CREATE INDEX idx_instances_run_node   ON instances(run_id, node_id);
CREATE INDEX idx_instances_run_status ON instances(run_id, status);
CREATE INDEX idx_instances_error_code ON instances(error_code) WHERE error_code IS NOT NULL;
CREATE INDEX idx_events_run_seq  ON events(run_id, seq);
CREATE INDEX idx_events_kind     ON events(kind, seq);
CREATE INDEX idx_events_instance ON events(instance_id, seq) WHERE instance_id IS NOT NULL;
CREATE INDEX idx_transitions_run_seq    ON transitions(run_id, seq);
CREATE INDEX idx_transitions_instance   ON transitions(instance_id, seq);
CREATE INDEX idx_transitions_run_status ON transitions(run_id, to_status);
CREATE INDEX idx_gate_run_seq  ON gate_results(run_id, seq);
CREATE INDEX idx_gate_run_edge ON gate_results(run_id, edge_index);
CREATE INDEX idx_gate_passed   ON gate_results(run_id, passed);
CREATE INDEX idx_stream_run_seq   ON streaming_events(run_id, seq);
CREATE INDEX idx_stream_instance  ON streaming_events(run_id, instance_id, segment_index);
CREATE INDEX idx_stream_tool      ON streaming_events(run_id, call_id) WHERE call_id IS NOT NULL;
CREATE INDEX idx_stream_kind      ON streaming_events(run_id, kind, segment_index);
CREATE INDEX idx_recorded_outputs_run ON recorded_outputs(run_id);
CREATE INDEX idx_llm_calls_run ON llm_calls(run_id);
COMMIT;
UPDATE runs SET owner_pid = 0, owner_host = 'seed-host';
UPDATE runs SET worker_versions_json = '[]', model_params_json = '[]';
PRAGMA user_version = 3;
