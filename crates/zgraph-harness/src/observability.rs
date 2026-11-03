//! v0.5/F3a1（DR-018 §1–§3）：计费 / 用量 / 预算核心。
//!
//! - [`PricingTable`]：外部价格表（不内置价；缺/坏文件 → 配置错误；未知模型 →
//!   费用不可用 + warning，不 fail run）；
//! - [`BudgetConfig`] / [`RunObservability`]：`max_tokens`（run 总 token）/`max_cost`
//!   （USD）默认关；累计 usage 跨 agent_call（executor 按 `run_id` 持有）；每次 LLM
//!   请求**发起前**检查（`>=` 上限），超限立即停止，不发起该请求；
//! - [`RunUsageStats`]：run/node/agent 维度 token/耗时/调用数聚合（F3b trace/`--stats`
//!   消费；不新增表/列）。
//!
//! **与 kernel 的 seam（F3a2 负责最终映射）**：超限经
//! [`crate::agent_loop::RunFailure::BudgetExceeded`] 返回结构化 [`BudgetStop`]；
//! F3a2 在 kernel 映射 `BUDGET_EXCEEDED` + stop_reason `Other("budget_exceeded")`。
//! 本模块不改 kernel、不写 trace。
//!
//! **外部 harness 限制（DR-018 §1.2/DR-020）**：DSH ACP `usage_update` 只有
//! used/size，无法填 input/output tokens → 外部 DSH token/cost **不可统计**（标注
//! unavailable，不伪造、不计入预算）；Codex usage 按其记录值统计、`cached_input_tokens`
//! 丢弃（v0.3 口径）。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::Value;

use crate::constants::CODE_BUDGET_EXCEEDED;
use crate::error::HarnessError;
use crate::llm::{Usage, UsageSource};

/// 价格表：货币 + model → input/output USD per 1M tokens（DR-018 §2）。
#[derive(Debug, Clone, PartialEq)]
pub struct PricingTable {
    currency: String,
    models: BTreeMap<String, ModelPrice>,
}

/// 单个模型价格（USD / 1M tokens）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPrice {
    /// 输入价格（USD / 1M tokens）。
    pub input_per_mtok: f64,
    /// 输出价格（USD / 1M tokens）。
    pub output_per_mtok: f64,
}

impl PricingTable {
    /// 从 JSON 文本解析（缺/坏 → `HARNESS_CONFIG_LOAD{reason:"pricing_invalid"}`）。
    pub fn parse_json(text: &str) -> Result<Self, HarnessError> {
        let value: Value = serde_json::from_str(text)
            .map_err(|error| pricing_invalid(format!("pricing JSON is invalid: {error}")))?;
        Self::from_value(&value)
    }

    /// 从文件加载（不存在 → `reason:"pricing_file_not_found"`；内容错误 → `pricing_invalid`）。
    pub fn load_file(path: &Path) -> Result<Self, HarnessError> {
        let text = std::fs::read_to_string(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                HarnessError::config_load(
                    "pricing_file_not_found",
                    Some(&path.display().to_string()),
                    None,
                    Some(Value::from("existing pricing JSON file")),
                    Some(Value::from(error.to_string())),
                    format!("pricing file '{}' does not exist", path.display()),
                )
            } else {
                HarnessError::config_load(
                    "pricing_invalid",
                    Some(&path.display().to_string()),
                    None,
                    Some(Value::from("readable pricing JSON file")),
                    Some(Value::from(error.to_string())),
                    format!("pricing file '{}' cannot be read: {error}", path.display()),
                )
            }
        })?;
        Self::parse_json(&text)
    }

    /// 从已解析 JSON 校验（`{"currency":"USD","models":{...}}`）。
    pub fn from_value(value: &Value) -> Result<Self, HarnessError> {
        let Some(object) = value.as_object() else {
            return Err(pricing_invalid("pricing root must be an object"));
        };
        let currency = object
            .get("currency")
            .and_then(Value::as_str)
            .filter(|currency| !currency.trim().is_empty())
            .ok_or_else(|| pricing_invalid("pricing.currency must be a non-empty string"))?;
        // REV-123 C1：仅接受 USD（不做「静默按数值当 USD」）；预算按 USD 计。
        if !currency.eq_ignore_ascii_case("USD") {
            return Err(pricing_invalid(format!(
                "pricing.currency must be \"USD\" (got '{currency}'); cost budgets are USD-only"
            )));
        }
        let Some(models) = object.get("models").and_then(Value::as_object) else {
            return Err(pricing_invalid("pricing.models must be an object"));
        };
        let mut parsed = BTreeMap::new();
        for (model, price) in models {
            let Some(price) = price.as_object() else {
                return Err(pricing_invalid(format!(
                    "pricing.models['{model}'] must be an object"
                )));
            };
            let input = validate_price(price.get("input_per_mtok"), model, "input_per_mtok")?;
            let output = validate_price(price.get("output_per_mtok"), model, "output_per_mtok")?;
            parsed.insert(
                model.clone(),
                ModelPrice {
                    input_per_mtok: input,
                    output_per_mtok: output,
                },
            );
        }
        Ok(Self {
            currency: currency.to_string(),
            models: parsed,
        })
    }

    /// 货币（DR-018 口径：`USD`；`--max-cost` 仅 USD）。
    pub fn currency(&self) -> &str {
        &self.currency
    }

    /// 是否含该模型价格。
    pub fn has_model(&self, model: &str) -> bool {
        self.models.contains_key(model)
    }

    /// 单次调用费用（USD；未知模型 → `None` = 费用不可用，不伪造）。
    ///
    /// `cost = prompt/1e6*input + completion/1e6*output`，结果保留 6 位小数。
    pub fn cost_usd(&self, model: &str, prompt_tokens: u64, completion_tokens: u64) -> Option<f64> {
        let price = self.models.get(model)?;
        let cost = prompt_tokens as f64 / 1_000_000.0 * price.input_per_mtok
            + completion_tokens as f64 / 1_000_000.0 * price.output_per_mtok;
        Some(round_usd(cost))
    }
}

fn validate_price(value: Option<&Value>, model: &str, field: &str) -> Result<f64, HarnessError> {
    let Some(value) = value.and_then(Value::as_f64) else {
        return Err(pricing_invalid(format!(
            "pricing.models['{model}'].{field} must be a number"
        )));
    };
    if !value.is_finite() || value < 0.0 {
        return Err(pricing_invalid(format!(
            "pricing.models['{model}'].{field} must be finite and >= 0"
        )));
    }
    Ok(value)
}

fn pricing_invalid(message: impl Into<String>) -> HarnessError {
    HarnessError::config_load(
        "pricing_invalid",
        Some("<pricing>"),
        Some("$.pricing"),
        Some(Value::from("{\"currency\":\"USD\",\"models\":{...}}")),
        None,
        message,
    )
}

/// USD 6 位小数（四舍五入；预算 `>=` 判定/输出口径）。
pub fn round_usd(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

/// 预算配置（默认关；DR-018 §3）。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BudgetConfig {
    /// run 总 token 上限（`--max-tokens`）。
    pub max_tokens: Option<u64>,
    /// run 总费用上限（USD；`--max-cost`）。
    pub max_cost: Option<f64>,
}

impl BudgetConfig {
    /// 是否启用了任意预算。
    pub fn is_enabled(&self) -> bool {
        self.max_tokens.is_some() || self.max_cost.is_some()
    }

    /// 校验非法值（REV-123 P2：`max_tokens`/`max_cost` 必须 **> 0**；0/NaN/负值 →
    /// `HARNESS_CONFIG_LOAD{reason:"budget_invalid"}`，不视为立即超限）。
    pub fn validate(&self) -> Result<(), HarnessError> {
        if let Some(limit) = self.max_tokens {
            if limit == 0 {
                return Err(HarnessError::config_load(
                    "budget_invalid",
                    None,
                    Some("$.max_tokens"),
                    Some(Value::from("integer > 0")),
                    Some(Value::from(limit)),
                    "--max-tokens must be > 0",
                ));
            }
        }
        if let Some(limit) = self.max_cost {
            if !limit.is_finite() || limit <= 0.0 {
                return Err(HarnessError::config_load(
                    "budget_invalid",
                    None,
                    Some("$.max_cost"),
                    Some(Value::from("finite USD > 0")),
                    Some(Value::from(limit)),
                    "--max-cost must be a finite USD amount > 0",
                ));
            }
        }
        Ok(())
    }
}

/// 预算超限种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetStopKind {
    /// run 总 token 达到/超过上限。
    Tokens,
    /// run 总费用达到/超过上限。
    Cost,
    /// 费用不可用（pricing 缺失/未知模型/usage 不可得）→ 无法执行 cost 预算（fail-closed）。
    CostUnavailable,
}

/// 结构化预算停止结果（F3a2 映射 kernel `BUDGET_EXCEEDED`）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BudgetStop {
    /// 超限种类。
    pub kind: BudgetStopKind,
    /// 已用 token（run 累计）。
    pub used_tokens: u64,
    /// token 上限（未配置 = `None`）。
    pub max_tokens: Option<u64>,
    /// 已用费用（USD；费用不可用时 `None`）。
    pub used_cost_usd: Option<f64>,
    /// 费用上限（未配置 = `None`）。
    pub max_cost_usd: Option<f64>,
    /// 人类可读描述。
    pub message: String,
}

impl BudgetStop {
    /// 序列化 detail（trace/CLI additive；F3a2 用）。
    pub fn to_detail(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| Value::Null)
    }

    /// 错误码字面量（`BUDGET_EXCEEDED`；F3a2 映射 kernel 时用）。
    pub fn error_code(&self) -> &'static str {
        CODE_BUDGET_EXCEEDED
    }
}

/// node 维度聚合键：`(instance_id, attempt)`（DR-018 §1.1）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct NodeUsageKey {
    /// 实例 id。
    pub instance_id: String,
    /// attempt。
    pub attempt: u32,
}

/// agent 维度聚合键：`(agent_id, model)`（DR-018 §1.1）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct AgentUsageKey {
    /// agent id。
    pub agent_id: String,
    /// model 名。
    pub model: String,
}

/// token/耗时/调用数聚合（run/node/agent 共用；未知 usage 不臆造 0）。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct UsageTotals {
    /// prompt token 合计。
    pub prompt_tokens: u64,
    /// completion token 合计。
    pub completion_tokens: u64,
    /// total token 合计（provider 口径）。
    pub total_tokens: u64,
    /// reasoning token 合计（provider 提供时）。
    pub reasoning_tokens: u64,
    /// LLM 调用次数。
    pub llm_calls: u64,
    /// usage 不可得的调用数（不计入 token；输出 warning）。
    pub unknown_usage_calls: u64,
    /// 费用不可用的调用数（模型无价格/无价格表）。
    pub unpriced_calls: u64,
    /// LLM 调用累计耗时（毫秒，成功响应）。
    pub duration_ms: u64,
    /// 费用合计（USD；任一调用不可用/未知 = `None`，不伪造部分和）。
    pub cost_usd: Option<f64>,
}

impl UsageTotals {
    /// `cost = None` = 未启用价格表（不计算/不告警）；`Some(None)` = 模型无价格；
    /// `Some(Some(_))` = 已计价。
    fn add(&mut self, usage: &Usage, cost: Option<Option<f64>>, duration_ms: u64) {
        self.llm_calls = self.llm_calls.saturating_add(1);
        self.duration_ms = self.duration_ms.saturating_add(duration_ms);
        if usage.source == UsageSource::Unavailable {
            self.unknown_usage_calls = self.unknown_usage_calls.saturating_add(1);
            // usage 不可得 → 费用也不可得。
            if cost.is_some() {
                self.cost_usd = None;
            }
            return;
        }
        self.prompt_tokens = self
            .prompt_tokens
            .saturating_add(u64::from(usage.prompt_tokens));
        self.completion_tokens = self
            .completion_tokens
            .saturating_add(u64::from(usage.completion_tokens));
        self.total_tokens = self
            .total_tokens
            .saturating_add(u64::from(usage.total_tokens));
        self.reasoning_tokens = self
            .reasoning_tokens
            .saturating_add(u64::from(usage.reasoning_tokens.unwrap_or(0)));
        match cost {
            None => {}
            Some(Some(cost)) => {
                if self.unpriced_calls == 0 {
                    let current = self.cost_usd.unwrap_or(0.0);
                    self.cost_usd = Some(round_usd(current + cost));
                }
            }
            Some(None) => {
                self.unpriced_calls = self.unpriced_calls.saturating_add(1);
                self.cost_usd = None;
            }
        }
    }
}

/// run 维度统计（含 node/agent 分维；DR-018 §1）。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RunUsageStats {
    /// run 汇总。
    pub run: UsageTotals,
    /// node 维度（`(instance_id, attempt)`）。
    pub by_node: BTreeMap<NodeUsageKey, UsageTotals>,
    /// agent 维度（`(agent_id, model)`）。
    pub by_agent: BTreeMap<AgentUsageKey, UsageTotals>,
    /// warning（未知价格/usage 不可得；去重）。
    pub warnings: Vec<String>,
    /// 最近一次预算超限（未超限 = `None`；F3a2 映射）。
    pub budget_stop: Option<BudgetStop>,
}

#[derive(Debug, Default)]
struct ObservabilityState {
    stats: RunUsageStats,
    /// 已用 token（budget 用；run 累计）。
    tokens_used: u64,
    /// 已用费用（budget 用；仅当费用完整可算时有意义）。
    cost_used: f64,
    /// 费用是否完整（全部调用有价格且 usage 可得）。
    cost_complete: bool,
    warning_set: BTreeSet<String>,
}

impl ObservabilityState {
    fn new() -> Self {
        Self {
            stats: RunUsageStats::default(),
            tokens_used: 0,
            cost_used: 0.0,
            cost_complete: true,
            warning_set: BTreeSet::new(),
        }
    }
}

/// run 级可观测/预算状态（executor 按 `run_id` 持有；跨 agent_call 累计）。
#[derive(Debug)]
pub struct RunObservability {
    pricing: Option<Arc<PricingTable>>,
    budget: BudgetConfig,
    state: Mutex<ObservabilityState>,
}

impl RunObservability {
    /// 新建（pricing/budget 可同时为 `None` = 默认关；统计仍会累计）。
    pub fn new(pricing: Option<Arc<PricingTable>>, budget: BudgetConfig) -> Arc<Self> {
        Arc::new(Self {
            pricing,
            budget,
            state: Mutex::new(ObservabilityState::new()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ObservabilityState> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// 每次 LLM 请求**发起前**检查（`>=` 上限；超限 → 结构化 [`BudgetStop`]）。
    pub fn check_before_call(&self) -> Result<(), BudgetStop> {
        if !self.budget.is_enabled() {
            return Ok(());
        }
        let mut state = self.lock();
        if let Some(limit) = self.budget.max_tokens {
            if state.tokens_used >= limit {
                let stop = BudgetStop {
                    kind: BudgetStopKind::Tokens,
                    used_tokens: state.tokens_used,
                    max_tokens: Some(limit),
                    used_cost_usd: state.cost_complete.then_some(round_usd(state.cost_used)),
                    max_cost_usd: self.budget.max_cost,
                    message: format!(
                        "run token budget exceeded: used {} >= max_tokens {}",
                        state.tokens_used, limit
                    ),
                };
                state.stats.budget_stop = Some(stop.clone());
                return Err(stop);
            }
        }
        if let Some(limit) = self.budget.max_cost {
            if !state.cost_complete {
                let stop = BudgetStop {
                    kind: BudgetStopKind::CostUnavailable,
                    used_tokens: state.tokens_used,
                    max_tokens: self.budget.max_tokens,
                    used_cost_usd: None,
                    max_cost_usd: Some(limit),
                    message: "run cost budget cannot be enforced: cost unavailable \
                              (pricing missing/unknown model or usage unavailable)"
                        .to_string(),
                };
                state.stats.budget_stop = Some(stop.clone());
                return Err(stop);
            }
            let used = round_usd(state.cost_used);
            if used >= limit {
                let stop = BudgetStop {
                    kind: BudgetStopKind::Cost,
                    used_tokens: state.tokens_used,
                    max_tokens: self.budget.max_tokens,
                    used_cost_usd: Some(used),
                    max_cost_usd: Some(limit),
                    message: format!(
                        "run cost budget exceeded: used ${used:.6} >= max_cost ${limit:.6}"
                    ),
                };
                state.stats.budget_stop = Some(stop.clone());
                return Err(stop);
            }
        }
        Ok(())
    }

    /// 记录一次成功 LLM 调用的 usage/耗时/费用（跨 agent_call 累计）。
    pub fn record_llm_call(
        &self,
        node: NodeUsageKey,
        agent: AgentUsageKey,
        usage: &Usage,
        duration_ms: u64,
    ) {
        // `None` = 未启用价格表（不计算费用/不告警）；`Some(Option)` = 启用后计价结果。
        let cost = self.pricing.as_ref().map(|pricing| {
            pricing.cost_usd(
                &agent.model,
                u64::from(usage.prompt_tokens),
                u64::from(usage.completion_tokens),
            )
        });
        let mut state = self.lock();
        let cost_complete_before = state.cost_complete;
        state.stats.run.add(usage, cost, duration_ms);
        state
            .stats
            .by_node
            .entry(node)
            .or_default()
            .add(usage, cost, duration_ms);
        let agent_model = agent.model.clone();
        state
            .stats
            .by_agent
            .entry(agent)
            .or_default()
            .add(usage, cost, duration_ms);
        if usage.source == UsageSource::Unavailable {
            if self.pricing.is_some() {
                state.cost_complete = false;
            }
            let warning = "usage unavailable for an LLM call (external DSH usage is not \
                           input/output tokens); token/cost stats are incomplete"
                .to_string();
            if state.warning_set.insert(warning.clone()) {
                state.stats.warnings.push(warning);
            }
            return;
        }
        state.tokens_used = state
            .tokens_used
            .saturating_add(u64::from(usage.total_tokens));
        match cost {
            None => {}
            Some(Some(cost)) => {
                if cost_complete_before {
                    state.cost_used = round_usd(state.cost_used + cost);
                }
            }
            Some(None) => {
                state.cost_complete = false;
                let warning = format!(
                    "model '{agent_model}' has no pricing entry; cost unavailable (not fabricated)"
                );
                if state.warning_set.insert(warning.clone()) {
                    state.stats.warnings.push(warning);
                }
            }
        }
    }

    /// 统计快照（F3b trace/`--stats` 消费；additive）。
    pub fn snapshot(&self) -> RunUsageStats {
        self.lock().stats.clone()
    }

    /// 最近一次预算超限（未超限 = `None`）。
    pub fn last_budget_stop(&self) -> Option<BudgetStop> {
        self.lock().stats.budget_stop.clone()
    }

    /// 预算配置（诊断用）。
    pub fn budget(&self) -> BudgetConfig {
        self.budget
    }

    /// 价格表（诊断用）。
    pub fn pricing(&self) -> Option<&Arc<PricingTable>> {
        self.pricing.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::UsageSource;

    fn usage(prompt: u32, completion: u32) -> Usage {
        Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
            reasoning_tokens: None,
            source: UsageSource::Provider,
        }
    }

    fn pricing() -> Arc<PricingTable> {
        Arc::new(
            PricingTable::parse_json(
                r#"{"currency":"USD","models":{"m":{"input_per_mtok":1.0,"output_per_mtok":2.0}}}"#,
            )
            .expect("valid pricing"),
        )
    }

    fn node(id: &str, attempt: u32) -> NodeUsageKey {
        NodeUsageKey {
            instance_id: id.to_string(),
            attempt,
        }
    }

    fn agent() -> AgentUsageKey {
        AgentUsageKey {
            agent_id: "parser".to_string(),
            model: "m".to_string(),
        }
    }

    #[test]
    fn parses_pricing_and_computes_cost() {
        let table = pricing();
        assert_eq!(table.currency(), "USD");
        assert!(table.has_model("m"));
        // 1M input + 0.5M output = 1.0 + 1.0 = 2.0 USD。
        assert_eq!(table.cost_usd("m", 1_000_000, 500_000), Some(2.0));
        assert_eq!(table.cost_usd("m", 1, 1), Some(0.000003));
        assert_eq!(table.cost_usd("unknown", 1, 1), None);
    }

    #[test]
    fn rejects_missing_or_malformed_pricing() {
        assert_eq!(
            PricingTable::parse_json("not json")
                .expect_err("bad json")
                .detail["reason"],
            serde_json::json!("pricing_invalid")
        );
        assert_eq!(
            PricingTable::parse_json(r#"{"models":{}}"#)
                .expect_err("currency")
                .detail["reason"],
            serde_json::json!("pricing_invalid")
        );
        assert_eq!(
            PricingTable::parse_json(
                r#"{"currency":"USD","models":{"m":{"input_per_mtok":-1,"output_per_mtok":1}}}"#
            )
            .expect_err("negative price")
            .detail["reason"],
            serde_json::json!("pricing_invalid")
        );
        assert_eq!(
            PricingTable::parse_json(r#"{"currency":"USD","models":{"m":{"input_per_mtok":1}}}"#)
                .expect_err("missing output")
                .detail["reason"],
            serde_json::json!("pricing_invalid")
        );
        let missing = PricingTable::load_file(Path::new("/definitely/not/a/pricing.json"))
            .expect_err("missing file");
        assert_eq!(
            missing.detail["reason"],
            serde_json::json!("pricing_file_not_found")
        );
    }

    #[test]
    fn budget_checks_tokens_at_greater_or_equal_boundary() {
        let obs = RunObservability::new(
            None,
            BudgetConfig {
                max_tokens: Some(100),
                max_cost: None,
            },
        );
        obs.check_before_call().expect("empty run allowed");
        obs.record_llm_call(node("i1", 0), agent(), &usage(60, 40), 5);
        // used = 100 >= 100 → 停止。
        let stop = obs.check_before_call().expect_err("budget stop");
        assert_eq!(stop.kind, BudgetStopKind::Tokens);
        assert_eq!(stop.used_tokens, 100);
        assert_eq!(stop.max_tokens, Some(100));
        assert_eq!(obs.last_budget_stop(), Some(stop));
        // 预算不可续：再次检查仍超限。
        assert!(obs.check_before_call().is_err());
    }

    #[test]
    fn budget_checks_cost_boundary_and_unavailable_cost() {
        let obs = RunObservability::new(
            Some(pricing()),
            BudgetConfig {
                max_tokens: None,
                max_cost: Some(0.5),
            },
        );
        obs.record_llm_call(node("i1", 0), agent(), &usage(1_000_000, 0), 1);
        let stop = obs.check_before_call().expect_err("cost stop");
        assert_eq!(stop.kind, BudgetStopKind::Cost);
        assert_eq!(stop.used_cost_usd, Some(1.0));
        assert_eq!(stop.max_cost_usd, Some(0.5));

        // 未知模型 → 费用不可用 → cost 预算 fail-closed。
        let obs = RunObservability::new(
            Some(pricing()),
            BudgetConfig {
                max_tokens: None,
                max_cost: Some(10.0),
            },
        );
        obs.record_llm_call(
            node("i1", 0),
            AgentUsageKey {
                agent_id: "parser".to_string(),
                model: "unknown".to_string(),
            },
            &usage(10, 5),
            1,
        );
        let stop = obs.check_before_call().expect_err("cost unavailable");
        assert_eq!(stop.kind, BudgetStopKind::CostUnavailable);
        let stats = obs.snapshot();
        assert_eq!(stats.run.unpriced_calls, 1);
        assert!(stats.run.cost_usd.is_none());
        assert!(stats
            .warnings
            .iter()
            .any(|warning| warning.contains("no pricing")));
    }

    #[test]
    fn aggregates_run_node_and_agent_dimensions() {
        let obs = RunObservability::new(Some(pricing()), BudgetConfig::default());
        obs.record_llm_call(node("n1", 0), agent(), &usage(10, 5), 100);
        obs.record_llm_call(node("n1", 0), agent(), &usage(20, 10), 50);
        obs.record_llm_call(node("n2", 1), agent(), &usage(5, 5), 25);
        let stats = obs.snapshot();
        assert_eq!(stats.run.prompt_tokens, 35);
        assert_eq!(stats.run.completion_tokens, 20);
        assert_eq!(stats.run.total_tokens, 55);
        assert_eq!(stats.run.llm_calls, 3);
        assert_eq!(stats.run.duration_ms, 175);
        assert!(stats.run.cost_usd.is_some());
        assert_eq!(stats.by_node.len(), 2);
        assert_eq!(stats.by_node[&node("n1", 0)].llm_calls, 2);
        assert_eq!(stats.by_node[&node("n2", 1)].total_tokens, 10);
        assert_eq!(stats.by_agent.len(), 1);
        assert_eq!(stats.by_agent[&agent()].llm_calls, 3);
    }

    #[test]
    fn unavailable_usage_is_unknown_not_zero_and_warns() {
        let obs = RunObservability::new(Some(pricing()), BudgetConfig::default());
        let unavailable = Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            reasoning_tokens: None,
            source: UsageSource::Unavailable,
        };
        obs.record_llm_call(node("i1", 0), agent(), &unavailable, 0);
        let stats = obs.snapshot();
        assert_eq!(stats.run.unknown_usage_calls, 1);
        assert_eq!(stats.run.llm_calls, 1);
        assert_eq!(stats.run.total_tokens, 0);
        assert!(stats.run.cost_usd.is_none());
        assert!(
            stats
                .warnings
                .iter()
                .any(|warning| warning.contains("usage unavailable")),
            "{:?}",
            stats.warnings
        );
    }

    #[test]
    fn no_budget_is_inert_and_validates_invalid_cost() {
        let obs = RunObservability::new(None, BudgetConfig::default());
        obs.record_llm_call(node("i1", 0), agent(), &usage(10, 5), 1);
        obs.check_before_call().expect("no budget never stops");
        // REV-123 P2：0/NaN/负值均为配置错误（不视为立即超限）。
        for budget in [
            BudgetConfig {
                max_tokens: Some(0),
                max_cost: None,
            },
            BudgetConfig {
                max_tokens: None,
                max_cost: Some(0.0),
            },
            BudgetConfig {
                max_tokens: None,
                max_cost: Some(-1.0),
            },
            BudgetConfig {
                max_tokens: Some(10),
                max_cost: Some(f64::NAN),
            },
            BudgetConfig {
                max_tokens: Some(0),
                max_cost: Some(f64::NAN),
            },
        ] {
            let error = budget.validate().expect_err("invalid budget");
            assert_eq!(
                error.detail["reason"],
                serde_json::json!("budget_invalid"),
                "{budget:?}"
            );
        }
        BudgetConfig {
            max_tokens: Some(1),
            max_cost: Some(0.000001),
        }
        .validate()
        .expect("positive budget is legal");
    }
}
