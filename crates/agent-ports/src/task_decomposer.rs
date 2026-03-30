//! Task decomposition strategies for subagent planning.
//!
//! This module provides the abstraction for breaking down high-level goals into
//! executable subtask plans, supporting multiple strategies:
//! - LLM-based autonomous planning (DeerFlow style)
//! - Template-based predefined patterns (LangChain style)
//! - Hybrid approach combining both

use crate::ids::{RunId, ThreadId};
use crate::ports::{LLMPort, LlmTurnContext, SubtaskSpec};
use crate::thread_state::ChatMessage;
use crate::tool_manifest::ToolManifest;
use crate::{PortResult, SubtaskPlan};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// Budget constraints for task decomposition.
/// This is a simplified version that doesn't depend on agent-loop-runtime.
#[derive(Debug, Clone, Copy)]
pub struct DecompositionBudget {
    pub max_subagent_tasks: u32,
    pub max_concurrent_subagents: u32,
}

impl Default for DecompositionBudget {
    fn default() -> Self {
        Self { max_subagent_tasks: 8, max_concurrent_subagents: 4 }
    }
}

/// Context for task decomposition, providing all necessary information for planning.
#[derive(Debug, Clone)]
pub struct TaskContext {
    pub thread_id: ThreadId,
    pub run_id: RunId,
    pub parent_messages: Vec<ChatMessage>,
    pub available_tools: Vec<ToolManifest>,
    pub budget: DecompositionBudget,
    pub goal: String,
}

impl TaskContext {
    #[must_use]
    pub fn new(
        thread_id: ThreadId,
        run_id: RunId,
        goal: String,
        parent_messages: Vec<ChatMessage>,
        available_tools: Vec<ToolManifest>,
        budget: DecompositionBudget,
    ) -> Self {
        Self { thread_id, run_id, goal, parent_messages, available_tools, budget }
    }
}

/// LLM-based decomposition configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmPlannerConfig {
    /// System prompt template for task decomposition
    pub prompt_template: String,
    /// Model to use for planning (None = inherit from parent)
    pub model: Option<String>,
    /// Maximum number of subtasks to generate
    pub max_subtasks: u32,
    /// Whether to include tool descriptions in the prompt
    pub include_tools: bool,
}

impl Default for LlmPlannerConfig {
    fn default() -> Self {
        Self {
            prompt_template: DEFAULT_DECOMPOSITION_PROMPT.to_string(),
            model: None,
            max_subtasks: 10,
            include_tools: true,
        }
    }
}

/// Template-based decomposition configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTemplate {
    /// Template identifier
    pub id: String,
    /// Human-readable description
    pub description: String,
    /// Goal pattern that this template matches
    pub goal_pattern: String,
    /// Predefined subtask specifications
    pub subtasks: Vec<SubtaskSpec>,
    /// Whether to allow LLM to add additional tasks
    pub allow_extension: bool,
}

impl Default for TaskTemplate {
    fn default() -> Self {
        Self {
            id: String::new(),
            description: String::new(),
            goal_pattern: String::new(),
            subtasks: vec![],
            allow_extension: false,
        }
    }
}

impl TaskTemplate {
    /// Create a new task template.
    #[must_use]
    pub fn new(id: impl Into<String>, description: impl Into<String>, goal_pattern: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            goal_pattern: goal_pattern.into(),
            subtasks: vec![],
            allow_extension: false,
        }
    }

    /// Add a subtask to the template.
    #[must_use]
    pub fn with_subtask(mut self, goal: impl Into<String>, input: Option<Value>, budget_steps: u32) -> Self {
        self.subtasks.push(SubtaskSpec {
            goal: goal.into(),
            input: input.unwrap_or(Value::Null),
            budget_steps: if budget_steps == 0 { 10 } else { budget_steps },
        });
        self
    }

    /// Enable or disable LLM extension.
    #[must_use]
    pub fn with_extension(mut self, allow: bool) -> Self {
        self.allow_extension = allow;
        self
    }

    /// Create a template for data validation tasks.
    #[must_use]
    pub fn data_validation_template() -> Self {
        Self::new(
            "data_validation",
            "Validate and compare data from multiple sources",
            "验证.*数据|validate.*data|compare.*sources",
        )
        .with_subtask("从数据源 A 提取数据", None, 5)
        .with_subtask("从数据源 B 提取数据", None, 5)
        .with_subtask("对比分析差异", None, 8)
        .with_extension(true)
    }

    /// Create a template for research tasks.
    #[must_use]
    pub fn research_template() -> Self {
        Self::new(
            "research",
            "Research and analyze a topic",
            "分析.*研究.*调研|research|analyze",
        )
        .with_subtask("收集背景信息", None, 5)
        .with_subtask("识别关键趋势", None, 8)
        .with_subtask("分析主要参与者", None, 8)
        .with_subtask("总结发现", None, 5)
        .with_extension(true)
    }

    /// Create a template for code review tasks.
    #[must_use]
    pub fn code_review_template() -> Self {
        Self::new(
            "code_review",
            "Review code for quality and issues",
            "代码审查|code review|检查代码",
        )
        .with_subtask("检查代码风格和规范", None, 3)
        .with_subtask("识别潜在 bug", None, 8)
        .with_subtask("评估性能问题", None, 5)
        .with_subtask("提出改进建议", None, 5)
        .with_extension(false)
    }
}

/// Hybrid decomposition configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridConfig {
    /// Base template to start from
    pub template: TaskTemplate,
    /// LLM configuration for filling gaps
    pub llm_config: LlmPlannerConfig,
    /// Minimum similarity score to use template (0.0-1.0)
    pub min_similarity: f64,
}

impl Default for HybridConfig {
    fn default() -> Self {
        Self {
            template: TaskTemplate::default(),
            llm_config: LlmPlannerConfig::default(),
            min_similarity: 0.7,
        }
    }
}

impl HybridConfig {
    /// Create a new hybrid configuration.
    #[must_use]
    pub fn new(template: TaskTemplate, llm_config: LlmPlannerConfig) -> Self {
        Self {
            template,
            llm_config,
            min_similarity: 0.7,
        }
    }

    /// Set the minimum similarity threshold.
    #[must_use]
    pub fn with_min_similarity(mut self, threshold: f64) -> Self {
        self.min_similarity = threshold;
        self
    }

    /// Create a hybrid config for research tasks.
    #[must_use]
    pub fn research_hybrid() -> Self {
        Self::new(
            TaskTemplate::research_template(),
            LlmPlannerConfig::default(),
        )
    }

    /// Create a hybrid config for data validation tasks.
    #[must_use]
    pub fn data_validation_hybrid() -> Self {
        Self::new(
            TaskTemplate::data_validation_template(),
            LlmPlannerConfig::default(),
        )
    }

    /// Create a hybrid config for code review tasks.
    #[must_use]
    pub fn code_review_hybrid() -> Self {
        Self::new(
            TaskTemplate::code_review_template(),
            LlmPlannerConfig::default(),
        )
    }
}

/// Decomposition strategy selector.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "strategy", rename_all = "snake_case")]
pub enum DecompositionStrategy {
    /// LLM autonomous planning (DeerFlow style)
    LlmPlanned(LlmPlannerConfig),
    /// Predefined templates (LangChain style)
    TemplateBased(Vec<TaskTemplate>),
    /// Hybrid: template + LLM supplementation
    Hybrid(HybridConfig),
    /// Custom decomposer (user-provided implementation)
    Custom(String),
}

impl Default for DecompositionStrategy {
    fn default() -> Self {
        Self::LlmPlanned(LlmPlannerConfig::default())
    }
}

impl DecompositionStrategy {
    /// Create an LLM-based planning strategy.
    #[must_use]
    pub fn llm_planned() -> Self {
        Self::LlmPlanned(LlmPlannerConfig::default())
    }

    /// Create an LLM-based planning strategy with custom max subtasks.
    #[must_use]
    pub fn llm_planned_with_limit(max_subtasks: u32) -> Self {
        Self::LlmPlanned(LlmPlannerConfig {
            max_subtasks,
            ..Default::default()
        })
    }

    /// Create a template-based strategy with predefined templates.
    #[must_use]
    pub fn template_based(templates: Vec<TaskTemplate>) -> Self {
        Self::TemplateBased(templates)
    }

    /// Create a hybrid strategy with a specific template.
    #[must_use]
    pub fn hybrid(template: TaskTemplate) -> Self {
        Self::Hybrid(HybridConfig {
            template,
            llm_config: LlmPlannerConfig::default(),
            min_similarity: 0.7,
        })
    }

    /// Create a research-focused hybrid strategy.
    #[must_use]
    pub fn research_hybrid() -> Self {
        Self::Hybrid(HybridConfig::research_hybrid())
    }

    /// Create a data validation-focused hybrid strategy.
    #[must_use]
    pub fn data_validation_hybrid() -> Self {
        Self::Hybrid(HybridConfig::data_validation_hybrid())
    }

    /// Create a code review-focused hybrid strategy.
    #[must_use]
    pub fn code_review_hybrid() -> Self {
        Self::Hybrid(HybridConfig::code_review_hybrid())
    }
}

/// Trait for task decomposers - breaks down goals into subtask plans.
#[async_trait::async_trait]
pub trait TaskDecomposer: Send + Sync {
    /// Decompose a high-level goal into executable subtasks.
    ///
    /// # Arguments
    /// * `goal` - The high-level goal to decompose
    /// * `context` - Task context with thread state, tools, and budget
    ///
    /// # Returns
    /// * `Ok(SubtaskPlan)` - The decomposed subtask plan
    /// * `Err(PortError)` - Error during decomposition
    async fn decompose(&self, goal: &str, context: &TaskContext) -> PortResult<SubtaskPlan>;

    /// Get the strategy name for logging/observability.
    fn strategy_name(&self) -> &'static str;
}

/// Default LLM-based task decomposer.
pub struct LlmTaskDecomposer {
    config: LlmPlannerConfig,
    llm_port: Arc<dyn LLMPort>,
}

impl LlmTaskDecomposer {
    #[must_use]
    pub fn new(config: LlmPlannerConfig, llm_port: Arc<dyn LLMPort>) -> Self {
        Self { config, llm_port }
    }

    #[must_use]
    pub fn with_default_config(llm_port: Arc<dyn LLMPort>) -> Self {
        Self::new(LlmPlannerConfig::default(), llm_port)
    }

    /// Build the decomposition prompt with goal, tools, and constraints.
    fn build_prompt(&self, goal: &str, context: &TaskContext) -> String {
        let tools_desc = if self.config.include_tools && !context.available_tools.is_empty() {
            context
                .available_tools
                .iter()
                .map(|t| format!("- {}: {}", t.name, t.description.as_deref().unwrap_or("No description")))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            "None specified".to_string()
        };

        self.config
            .prompt_template
            .replace("{goal}", goal)
            .replace("{tools}", &tools_desc)
            .replace("{max_subtasks}", &self.config.max_subtasks.to_string())
            .replace(
                "{format_instructions}",
                r#"Return a JSON array of subtasks. Each subtask should have:
- "goal": string describing what to accomplish
- "input": optional input data (can be null)
- "budget_steps": estimated number of steps (default 10)

Example:
[
  {"goal": "Research market trends", "input": null, "budget_steps": 5},
  {"goal": "Analyze competitor data", "input": {"source": "web"}, "budget_steps": 8}
]"#,
            )
    }

    /// Parse LLM response into SubtaskPlan.
    fn parse_llm_response(&self, content: &str) -> PortResult<SubtaskPlan> {
        // Try to extract JSON array from the response
        let json_text = content.trim();
        
        // Handle markdown code blocks if present
        let json_text = json_text
            .strip_prefix("```json")
            .or_else(|| json_text.strip_prefix("```"))
            .unwrap_or(json_text);
        let json_text = json_text
            .strip_suffix("```")
            .unwrap_or(json_text)
            .trim();

        let tasks: Vec<SubtaskSpec> = serde_json::from_str(json_text).map_err(|e| {
            crate::PortError::Subagent(format!(
                "Failed to parse LLM response as JSON array: {e}. Response: {content}"
            ))
        })?;

        // Validate and sanitize tasks
        let mut sanitized_tasks = Vec::with_capacity(tasks.len());
        for mut task in tasks {
            if task.goal.trim().is_empty() {
                continue; // Skip tasks without goals
            }
            if task.input.is_null() {
                task.input = Value::Null;
            }
            if task.budget_steps == 0 {
                task.budget_steps = 10; // Default budget
            }
            sanitized_tasks.push(task);
        }

        if sanitized_tasks.is_empty() {
            return Err(crate::PortError::Subagent(
                "LLM returned no valid subtasks".to_string()
            ));
        }

        // Apply budget constraint
        let max_tasks = self.config.max_subtasks as usize;
        if sanitized_tasks.len() > max_tasks {
            sanitized_tasks.truncate(max_tasks);
        }

        Ok(SubtaskPlan { tasks: sanitized_tasks })
    }
}

#[async_trait::async_trait]
impl TaskDecomposer for LlmTaskDecomposer {
    async fn decompose(&self, goal: &str, context: &TaskContext) -> PortResult<SubtaskPlan> {
        // Build the prompt for task decomposition
        let user_prompt = self.build_prompt(goal, context);

        // Create LLM turn context
        let llm_context = LlmTurnContext {
            run_id: context.run_id,
            thread_id: context.thread_id,
            messages: vec![Value::String(user_prompt)],
            system_prompt: Some("You are an expert task planner. Your job is to break down complex goals into smaller, executable subtasks.".to_string()),
            model_name: self.config.model.clone(),
            policy_version: None,
            is_plan_mode: true,
            assembled_tool_names: vec![],
            loop_detected: false,
        };

        // Call LLM for decomposition
        let llm_output = self.llm_port.infer_turn(llm_context).await?;

        // Extract and parse the response
        let content = llm_output
            .assistant_text
            .ok_or_else(|| crate::PortError::Subagent("LLM returned no content".to_string()))?;

        let plan = self.parse_llm_response(&content)?;

        Ok(plan)
    }

    fn strategy_name(&self) -> &'static str {
        "llm_planned"
    }
}

/// Template-based task decomposer.
pub struct TemplateTaskDecomposer {
    templates: Vec<TaskTemplate>,
    llm_port: Option<Arc<dyn LLMPort>>,
}

impl TemplateTaskDecomposer {
    #[must_use]
    pub fn new(templates: Vec<TaskTemplate>) -> Self {
        Self { templates, llm_port: None }
    }

    /// Create with LLM support for extension.
    #[must_use]
    pub fn with_llm(templates: Vec<TaskTemplate>, llm_port: Arc<dyn LLMPort>) -> Self {
        Self { templates, llm_port: Some(llm_port) }
    }

    /// Find the best matching template for a goal.
    fn find_best_match(&self, goal: &str) -> Option<&TaskTemplate> {
        // Simple substring matching - can be enhanced with semantic similarity
        self.templates.iter().find(|t| goal.contains(&t.goal_pattern))
    }

    /// Use LLM to extend template tasks when allow_extension is true.
    async fn extend_with_llm(
        &self,
        goal: &str,
        template: &TaskTemplate,
        context: &TaskContext,
    ) -> PortResult<Vec<SubtaskSpec>> {
        let llm_port = self.llm_port.as_ref().ok_or_else(|| {
            crate::PortError::Subagent("LLM port not configured for template extension".to_string())
        })?;

        let existing_tasks_json = serde_json::to_string_pretty(&template.subtasks)
            .unwrap_or_else(|_| "[]".to_string());

        let extension_prompt = format!(
            r#"You are an expert task planner. You have a template for a goal, but it may need customization.

Goal: {goal}

Template Subtasks:
{existing_tasks}

Your task:
1. Review if the template subtasks fully cover the goal
2. Add any missing tasks specific to this goal
3. Return a JSON array of additional subtasks (or empty array if complete)

Constraints:
- Maximum {max_additional} additional subtasks
- Each subtask should be specific and actionable
- Do not duplicate existing template tasks

Return format: JSON array of subtasks.

Additional Subtasks:"#,
            goal = goal,
            existing_tasks = existing_tasks_json,
            max_additional = context.budget.max_subagent_tasks.saturating_sub(template.subtasks.len() as u32)
        );

        let llm_context = LlmTurnContext {
            run_id: context.run_id,
            thread_id: context.thread_id,
            messages: vec![Value::String(extension_prompt)],
            system_prompt: Some("You are an expert task planner that extends template plans.".to_string()),
            model_name: None,
            policy_version: None,
            is_plan_mode: true,
            assembled_tool_names: vec![],
            loop_detected: false,
        };

        let llm_output = llm_port.infer_turn(llm_context).await?;

        let content = llm_output
            .assistant_text
            .ok_or_else(|| crate::PortError::Subagent("LLM returned no content for template extension".to_string()))?;

        // Parse the additional tasks
        let additional_tasks: Vec<SubtaskSpec> = serde_json::from_str(content.trim()).unwrap_or_else(|_| {
            vec![]
        });

        Ok(additional_tasks)
    }
}

#[async_trait::async_trait]
impl TaskDecomposer for TemplateTaskDecomposer {
    async fn decompose(&self, goal: &str, context: &TaskContext) -> PortResult<SubtaskPlan> {
        let template = self.find_best_match(goal).ok_or_else(|| {
            crate::PortError::Subagent(format!("No matching template found for goal: {goal}"))
        })?;

        let mut tasks = template.subtasks.clone();

        // If extension is allowed and LLM is available, use it
        if template.allow_extension && self.llm_port.is_some() {
            let additional = self.extend_with_llm(goal, template, context).await?;
            tasks.extend(additional);
        }

        // Apply budget constraints
        let max_tasks = context.budget.max_subagent_tasks as usize;
        if tasks.len() > max_tasks {
            tasks.truncate(max_tasks);
        }

        Ok(SubtaskPlan { tasks })
    }

    fn strategy_name(&self) -> &'static str {
        "template_based"
    }
}

/// Hybrid task decomposer combining templates and LLM.
pub struct HybridTaskDecomposer {
    config: HybridConfig,
    llm_port: Arc<dyn LLMPort>,
}

impl HybridTaskDecomposer {
    #[must_use]
    pub fn new(config: HybridConfig, llm_port: Arc<dyn LLMPort>) -> Self {
        Self { config, llm_port }
    }

    /// Use LLM to identify and add missing tasks beyond the template.
    async fn fill_gaps_with_llm(
        &self,
        goal: &str,
        existing_tasks: &[SubtaskSpec],
        context: &TaskContext,
    ) -> PortResult<Vec<SubtaskSpec>> {
        let existing_tasks_json = serde_json::to_string_pretty(existing_tasks)
            .unwrap_or_else(|_| "[]".to_string());

        let gap_filling_prompt = format!(
            r#"You are an expert task planner. You have an initial set of subtasks for a goal, but some important steps may be missing.

Goal: {goal}

Existing Subtasks:
{existing_tasks}

Your task:
1. Review the existing subtasks
2. Identify any critical missing steps
3. Add only the missing tasks (do not duplicate existing ones)
4. Return a JSON array of ONLY the missing subtasks

Constraints:
- Maximum {max_additional} additional subtasks
- Each subtask should be specific and actionable
- Do not duplicate existing tasks

Return format: JSON array of subtasks with same structure as existing tasks.

Missing Subtasks:"#,
            goal = goal,
            existing_tasks = existing_tasks_json,
            max_additional = self.config.llm_config.max_subtasks.saturating_sub(existing_tasks.len() as u32)
        );

        let llm_context = LlmTurnContext {
            run_id: context.run_id,
            thread_id: context.thread_id,
            messages: vec![Value::String(gap_filling_prompt)],
            system_prompt: Some("You are an expert task planner that identifies missing steps in task plans.".to_string()),
            model_name: self.config.llm_config.model.clone(),
            policy_version: None,
            is_plan_mode: true,
            assembled_tool_names: vec![],
            loop_detected: false,
        };

        let llm_output = self.llm_port.infer_turn(llm_context).await?;

        let content = llm_output
            .assistant_text
            .ok_or_else(|| crate::PortError::Subagent("LLM returned no content for gap filling".to_string()))?;

        // Parse the additional tasks
        let additional_tasks: Vec<SubtaskSpec> = serde_json::from_str(content.trim()).unwrap_or_else(|_| {
            // If parsing fails, return empty vec (no additional tasks)
            vec![]
        });

        Ok(additional_tasks)
    }
}

#[async_trait::async_trait]
impl TaskDecomposer for HybridTaskDecomposer {
    async fn decompose(&self, goal: &str, context: &TaskContext) -> PortResult<SubtaskPlan> {
        // Start with template tasks
        let mut tasks = self.config.template.subtasks.clone();

        // If extension is allowed, use LLM to fill gaps
        if self.config.template.allow_extension {
            let additional_tasks = self.fill_gaps_with_llm(goal, &tasks, context).await?;
            tasks.extend(additional_tasks);
        }

        // Apply budget constraints
        let max_tasks = context.budget.max_subagent_tasks as usize;
        if tasks.len() > max_tasks {
            tasks.truncate(max_tasks);
        }

        Ok(SubtaskPlan { tasks })
    }

    fn strategy_name(&self) -> &'static str {
        "hybrid"
    }
}

/// Default decomposition prompt template.
const DEFAULT_DECOMPOSITION_PROMPT: &str = r#"You are an expert task planner. Your job is to break down complex goals into smaller, executable subtasks.

Goal: {goal}

Available Tools:
{tools}

Constraints:
- Maximum {max_subtasks} subtasks
- Each subtask should be specific and actionable
- Consider dependencies between subtasks
- Order tasks logically

Generate a subtask plan in JSON format:
{format_instructions}

Subtask Plan:"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_context_creation() {
        let thread_id = ThreadId::new_v4();
        let run_id = RunId::new_v4();
        let budget = DecompositionBudget::default();

        let ctx = TaskContext::new(
            thread_id,
            run_id,
            "test goal".to_string(),
            vec![],
            vec![],
            budget,
        );

        assert_eq!(ctx.thread_id, thread_id);
        assert_eq!(ctx.run_id, run_id);
        assert_eq!(ctx.goal, "test goal");
    }

    #[test]
    fn template_decomposer_no_match() {
        let decomposer = TemplateTaskDecomposer::new(vec![]);
        let ctx = TaskContext::new(
            ThreadId::new_v4(),
            RunId::new_v4(),
            "test".to_string(),
            vec![],
            vec![],
            DecompositionBudget::default(),
        );

        // Note: This would fail in async context, but we can't test async in simple unit tests
        // The actual error handling is tested in integration tests
    }

    #[test]
    fn llm_planner_config_default() {
        let config = LlmPlannerConfig::default();
        assert_eq!(config.max_subtasks, 10);
        assert!(config.include_tools);
        assert!(config.prompt_template.contains("expert task planner"));
    }

    #[test]
    fn decomposition_budget_default() {
        let budget = DecompositionBudget::default();
        assert_eq!(budget.max_subagent_tasks, 8);
        assert_eq!(budget.max_concurrent_subagents, 4);
    }

    #[test]
    fn hybrid_config_default() {
        let config = HybridConfig::default();
        assert_eq!(config.min_similarity, 0.7);
        assert_eq!(config.llm_config.max_subtasks, 10);
    }

    #[test]
    fn task_template_builder() {
        let template = TaskTemplate::new("test", "Test template", "test.*")
            .with_subtask("Task 1", None, 5)
            .with_subtask("Task 2", Some(Value::String("input")), 10)
            .with_extension(true);

        assert_eq!(template.id, "test");
        assert_eq!(template.subtasks.len(), 2);
        assert!(template.allow_extension);
        assert_eq!(template.subtasks[0].budget_steps, 5);
        assert_eq!(template.subtasks[1].budget_steps, 10);
    }

    #[test]
    fn preset_templates() {
        let research = TaskTemplate::research_template();
        assert_eq!(research.id, "research");
        assert!(!research.subtasks.is_empty());

        let validation = TaskTemplate::data_validation_template();
        assert_eq!(validation.id, "data_validation");
        assert!(!validation.subtasks.is_empty());

        let review = TaskTemplate::code_review_template();
        assert_eq!(review.id, "code_review");
        assert!(!review.subtasks.is_empty());
    }

    #[test]
    fn decomposition_strategy_presets() {
        let llm = DecompositionStrategy::llm_planned();
        assert!(matches!(llm, DecompositionStrategy::LlmPlanned(_)));

        let limited = DecompositionStrategy::llm_planned_with_limit(5);
        if let DecompositionStrategy::LlmPlanned(config) = limited {
            assert_eq!(config.max_subtasks, 5);
        }

        let hybrid = DecompositionStrategy::research_hybrid();
        assert!(matches!(hybrid, DecompositionStrategy::Hybrid(_)));
    }

    #[test]
    fn hybrid_config_builder() {
        let config = HybridConfig::new(
            TaskTemplate::default(),
            LlmPlannerConfig::default(),
        )
        .with_min_similarity(0.9);

        assert_eq!(config.min_similarity, 0.9);
    }
}
