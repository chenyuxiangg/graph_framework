//! D5c（v0.4 / M9）AC-23：`builtin__bash` + OS 沙箱（**环境感知**）**真 CLI 链路**。
//!
//! 语义依据：DR-013 §3.8/§4/§5/§9、spec 07 AC-23、05 §5.15.3。
//! **真实隔离证据边界**：本 CI/agent 环境本身运行在嵌套 Seatbelt 内
//! （`sandbox-exec` 最小 profile exit **71**），因此 e2e 必须实测「嵌套 → 预检 exit 3、
//! 不产生 run、不建 DB、`sandbox_nested`」这一 fail-closed 分支；非嵌套环境（普通
//! 用户终端）则实测隔离/网络/输出上限等分支。真实隔离的独立证据仍由用户普通终端
//! `crates/zgraph-harness/scripts/verify-sandbox.sh` 提供（D6 标注，不伪造）。
//!
//! 覆盖：a fail-closed（嵌套/不可用/profile 非法语义）+ 非嵌套隔离分支；b opt-in
//! 双层与 flag 用法错误；c 默认无 bash：零探测/零副作用/工具不可见。

#![cfg(unix)]

#[path = "cli_support/mod.rs"]
mod cli_support;
#[path = "v04_support/mod.rs"]
mod support;
#[path = "v03_support/mod.rs"]
mod v03;

use serde_json::json;

use cli_support::{assert_exit, json_stdout};
use support::{
    agent_with_tools, builtin_flags, calls_with_text, completed_run_id, run_scenario, tool_call,
    tree_snapshot, workspace, write_text,
};

/// 沙箱预检探测：`Some(71)` 嵌套 / `Some(0)` 可用 / `None` 不可用。
fn probe() -> Option<i32> {
    support::sandbox_probe_exit()
}

// ---------------------------------------------------------------------------
// AC-23 a/d：fail-closed（本环境嵌套 Seatbelt）+ 非嵌套隔离分支
// ---------------------------------------------------------------------------

#[test]
fn ac23a_bash_sandbox_is_fail_closed_or_isolated() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = workspace(dir.path());
    write_text(&ws.join("seed.txt"), "seed\n");
    let snapshot = tree_snapshot(&ws);
    let calls = calls_with_text(vec![tool_call(
        "b1",
        "builtin__bash",
        json!({"command": "printf 'ok'"}),
    )]);
    let mut flags = builtin_flags(&ws, &[]);
    flags.push("--allow-bash".to_string());
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&["builtin__bash"]),
        calls,
        &flags,
    );
    let db = &outcome.db;

    match probe() {
        Some(71) => {
            // 嵌套 Seatbelt：配置/启动前 fail-closed → exit 3、不产生 run、不建 DB。
            assert_exit(&outcome.output, 3, "嵌套 Seatbelt 预检");
            assert!(!db.exists(), "预检失败不得建 DB");
            assert!(
                String::from_utf8_lossy(&outcome.output.stdout)
                    .trim()
                    .is_empty(),
                "预检失败不打印 JSON"
            );
            let stderr = v03::err(&outcome.output);
            assert!(stderr.contains("HARNESS_CONFIG_LOAD"), "{stderr}");
            assert!(
                stderr.contains("sandbox_nested"),
                "reason=sandbox_nested（exit 71）: {stderr}"
            );
            assert!(
                stderr.contains("exit=Some(71)") || stderr.contains("exit 71"),
                "stderr 保留 sandbox-exec 退出码证据: {stderr}"
            );
            // 零 workspace 副作用、无 trace 记录。
            assert_eq!(tree_snapshot(&ws), snapshot, "预检失败不得触碰 workspace");
        }
        Some(0) => {
            // 非嵌套（普通终端）：真实沙箱内执行；本环境不执行该分支。
            let run_id = completed_run_id(&outcome, "非嵌套 bash 隔离分支");
            let row = support::tool_row(db, &run_id, "b1");
            assert_eq!(row.status, "ok", "bash 应成功: {:?}", row.error);
            let result = row.result.expect("bash result");
            assert_eq!(result["exit_code"], json!(0));
            assert_eq!(result["stdout"], json!("ok"));
            assert_eq!(
                result["sandbox"],
                json!("macos-sandbox-exec"),
                "结果标注 OS 沙箱"
            );
            assert!(row.bash_command.is_some(), "trace 记录 command");
            assert!(row.bash_cwd.is_some(), "trace 记录 cwd");
            assert!(
                row.sandbox_profile_sha256
                    .as_deref()
                    .is_some_and(|sha| sha.len() == 64),
                "trace 记录渲染后 profile sha256"
            );
            // 沙箱内 workspace 写成功、workspace 外写被拒（命令自身 EPERM 作为工具结果）。
            let calls = calls_with_text(vec![
                tool_call(
                    "inside",
                    "builtin__bash",
                    json!({"command": "printf hi > inside.txt"}),
                ),
                tool_call(
                    "outside",
                    "builtin__bash",
                    json!({"command": "printf x > /tmp/zgraph-e2e-outside.txt"}),
                ),
            ]);
            let mut flags = builtin_flags(&ws, &[]);
            flags.push("--allow-bash".to_string());
            let outcome = run_scenario(
                dir.path(),
                &agent_with_tools(&["builtin__bash"]),
                calls,
                &flags,
            );
            let run_id = completed_run_id(&outcome, "非嵌套 bash 隔离写分支");
            let inside = support::tool_row(&outcome.db, &run_id, "inside");
            assert_eq!(inside.result.expect("inside")["exit_code"], json!(0));
            assert_eq!(
                std::fs::read_to_string(ws.join("inside.txt")).expect("inside.txt"),
                "hi"
            );
            let outside = support::tool_row(&outcome.db, &run_id, "outside");
            let result = outside.result.expect("outside");
            assert_ne!(result["exit_code"], json!(0), "jail 外写必须失败: {result}");
            assert!(!std::path::Path::new("/tmp/zgraph-e2e-outside.txt").exists());
            // 超时 + 输出 cap（16 KiB head+tail）。
            let calls = calls_with_text(vec![
                tool_call(
                    "slow",
                    "builtin__bash",
                    json!({"command": "sleep 5", "timeout_ms": 200}),
                ),
                tool_call(
                    "loud",
                    "builtin__bash",
                    json!({"command": "yes x | head -c 40000"}),
                ),
            ]);
            let outcome = run_scenario(
                dir.path(),
                &agent_with_tools(&["builtin__bash"]),
                calls,
                &flags,
            );
            let run_id = completed_run_id(&outcome, "非嵌套 bash 超时/输出分支");
            let slow = support::tool_row(&outcome.db, &run_id, "slow");
            let result = slow.result.expect("slow");
            assert_eq!(result["status"], json!("timeout"));
            let loud = support::tool_row(&outcome.db, &run_id, "loud");
            let result = loud.result.expect("loud");
            let stdout = result["stdout"].as_str().expect("stdout");
            assert!(stdout.len() <= 16_384 + 128, "stdout 16 KiB head+tail");
            assert_eq!(
                result["truncated"],
                json!(true),
                "超限 truncated 标记: {result}"
            );
        }
        other => {
            // 二进制缺失/不可执行 → sandbox_unavailable。
            assert_eq!(other, None, "unexpected sandbox probe: {other:?}");
            assert_exit(&outcome.output, 3, "sandbox-exec 不可用 → exit 3");
            assert!(!db.exists(), "预检失败不得建 DB");
            assert!(v03::err(&outcome.output).contains("sandbox_unavailable"));
        }
    }
}

// ---------------------------------------------------------------------------
// AC-23 a/用法错误：opt-in 双层 + 依赖 flag + 无逃生门
// ---------------------------------------------------------------------------

#[test]
fn ac23b_opt_in_layering_and_usage_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = workspace(dir.path());

    // ① `--allow-builtin-tools` 但未 `--allow-bash`：bash 未注册 → 加载期 fail-closed。
    let case = dir.path().join("no-bash-flag");
    std::fs::create_dir_all(&case).expect("case dir");
    let flags = builtin_flags(&ws, &[]);
    let outcome = run_scenario(
        &case,
        &agent_with_tools(&["builtin__bash"]),
        support::text_reply("x"),
        &flags,
    );
    assert_exit(&outcome.output, 1, "有文件工具但无 --allow-bash");
    let payload = json_stdout(&outcome.output);
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();
    let (_, code, message, _, _, _) = v03::run_row(&outcome.db, &run_id);
    assert_eq!(code.as_deref(), Some("HARNESS_CONFIG_LOAD"));
    assert!(
        message
            .as_deref()
            .is_some_and(|text| text.contains("tool_not_registered")),
        "bash 未注册: {message:?}"
    );
    assert_eq!(support::tool_call_count(&outcome.db, &run_id), 0);

    // ② 无任何 opt-in：builtin__bash 引用 → builtin_tools_not_allowed。
    let case = dir.path().join("no-optin");
    std::fs::create_dir_all(&case).expect("case dir");
    let outcome = run_scenario(
        &case,
        &agent_with_tools(&["builtin__bash"]),
        support::text_reply("x"),
        &[],
    );
    assert_exit(&outcome.output, 1, "无 opt-in");
    let payload = json_stdout(&outcome.output);
    let run_id = payload["run_id"].as_str().expect("run_id").to_string();
    let (_, _, message, _, _, _) = v03::run_row(&outcome.db, &run_id);
    assert!(
        message
            .as_deref()
            .is_some_and(|text| text.contains("builtin_tools_not_allowed")),
        "{message:?}"
    );

    // ③ `--allow-network`/`--allow-exec-path` 依赖 `--allow-bash` → 用法错误 exit 2。
    for flag in ["--allow-network", "--allow-exec-path"] {
        let db = case.join(format!("{flag}.db"));
        let mut args = vec![
            "run".to_string(),
            cli_support::arg(&support::graph()),
            "--inputs".to_string(),
            v03::INPUTS.to_string(),
            "--db".to_string(),
            cli_support::arg(&db),
            "--assets-dir".to_string(),
            cli_support::arg(&dir.path().join("assets-unused")),
            "--fake-llm".to_string(),
        ];
        if flag == "--allow-exec-path" {
            args.push(flag.to_string());
            args.push(cli_support::arg(&ws));
        } else {
            args.push(flag.to_string());
        }
        let output = cli_support::run_cli(&args, dir.path());
        assert_exit(&output, 2, &format!("{flag} 需要 --allow-bash"));
        let stderr = v03::err(&output);
        assert!(stderr.contains("require --allow-bash"), "{flag}: {stderr}");
        assert!(!db.exists(), "用法错误不得建 DB");
    }

    // ④ 不提供 `--unsafe-bash` 逃生门 → unknown option exit 2。
    let output = cli_support::run_cli(
        &[
            "run".to_string(),
            cli_support::arg(&support::graph()),
            "--inputs".to_string(),
            v03::INPUTS.to_string(),
            "--unsafe-bash".to_string(),
        ],
        dir.path(),
    );
    assert_exit(&output, 2, "--unsafe-bash 不存在");
    assert!(
        v03::err(&output).contains("--unsafe-bash"),
        "{}",
        v03::err(&output)
    );
}

// ---------------------------------------------------------------------------
// AC-23 e：默认无 bash（无 opt-in/仅文件工具）零探测、零副作用、工具不可见
// ---------------------------------------------------------------------------

#[test]
fn ac23c_default_and_file_only_paths_never_probe_sandbox() {
    // 默认（无任何 opt-in）：不注册内置工具、不探测 sandbox、无副作用。
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = workspace(dir.path());
    write_text(&ws.join("a.txt"), "orig\n");
    let snapshot = tree_snapshot(&ws);
    let outcome = run_scenario(
        dir.path(),
        &agent_with_tools(&[]),
        support::text_reply("default"),
        &[],
    );
    let run_id = completed_run_id(&outcome, "默认无 opt-in");
    assert_eq!(support::tool_call_count(&outcome.db, &run_id), 0);
    assert_eq!(tree_snapshot(&ws), snapshot);
    let stderr = v03::err(&outcome.output);
    assert!(
        !stderr.contains("sandbox"),
        "默认路径不得探测 sandbox: {stderr}"
    );

    // 仅文件工具 opt-in：注册 read，不注册/不探测 bash。
    let case = dir.path().join("file-only");
    std::fs::create_dir_all(&case).expect("case dir");
    let flags = builtin_flags(&ws, &[]);
    let outcome = run_scenario(
        &case,
        &agent_with_tools(&["builtin__read"]),
        support::text_reply("file only"),
        &flags,
    );
    let run_id = completed_run_id(&outcome, "仅文件工具");
    let requests = v03::llm_requests(&outcome.db, &run_id);
    let tool_names: Vec<String> = requests[0]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect();
    assert_eq!(tool_names, vec!["builtin__read".to_string()], "bash 不可见");
    assert!(
        !v03::err(&outcome.output).contains("sandbox"),
        "仅文件工具路径不得探测 sandbox: {}",
        v03::err(&outcome.output)
    );
    assert_eq!(
        tree_snapshot(&ws),
        snapshot,
        "仅文件工具不产生 workspace 副作用"
    );
}
