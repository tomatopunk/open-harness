use agent_ports::{
    BashCommandClassification, BashCommandRisk, ExecutionPolicyAction, ProcessSandboxProfile,
    ToolAdapterKind, ToolExecutionSecurityContext, ToolRuntimeRequest,
};
use state_abstraction::{SessionPolicy, SessionRecord};

#[derive(Debug, Clone)]
pub struct SecurityChainEvaluation {
    pub context: ToolExecutionSecurityContext,
    pub command: Option<String>,
}

#[derive(Debug, Default)]
pub struct RuntimeSecurityChain;

impl RuntimeSecurityChain {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn evaluate(
        &self,
        request: &ToolRuntimeRequest,
        session: &SessionRecord,
    ) -> SecurityChainEvaluation {
        let policy_sandbox = sandbox_from_policy(&session.policy);
        let mut context = ToolExecutionSecurityContext {
            policy_action: ExecutionPolicyAction::Allow,
            policy_reason: policy_reason(&session.policy, policy_sandbox),
            sandbox_profile: policy_sandbox,
            bash_classification: None,
        };
        let command = extract_command(request);

        if request.adapter_kind == ToolAdapterKind::Bash {
            if let Some(classification) = classify_bash_command(command.as_deref()) {
                context.bash_classification = Some(classification.clone());
                match classification.risk {
                    BashCommandRisk::High => {
                        context.policy_action = ExecutionPolicyAction::Block;
                        context.sandbox_profile = ProcessSandboxProfile::Restricted;
                        context.policy_reason = format!(
                            "bash classifier blocked high-risk command: {}",
                            classification.reason
                        );
                    }
                    BashCommandRisk::Medium => {
                        context.policy_action = ExecutionPolicyAction::Downgrade;
                        context.sandbox_profile = ProcessSandboxProfile::Restricted;
                        context.policy_reason = format!(
                            "bash classifier downgraded command to restricted sandbox: {}",
                            classification.reason
                        );
                    }
                    BashCommandRisk::Low => {}
                }
            }
        }

        SecurityChainEvaluation { context, command }
    }
}

fn sandbox_from_policy(policy: &SessionPolicy) -> ProcessSandboxProfile {
    match policy.get("sandbox").and_then(|value| value.as_str()) {
        Some("restricted") => ProcessSandboxProfile::Restricted,
        _ => ProcessSandboxProfile::Standard,
    }
}

fn policy_reason(policy: &SessionPolicy, sandbox_profile: ProcessSandboxProfile) -> String {
    match sandbox_profile {
        ProcessSandboxProfile::Restricted => policy
            .get("sandbox")
            .and_then(|value| value.as_str())
            .map(|value| format!("session policy requested {value} process sandbox"))
            .unwrap_or_else(|| "allow-by-default policy applied".to_string()),
        ProcessSandboxProfile::Standard => "allow-by-default policy applied".to_string(),
    }
}

fn extract_command(request: &ToolRuntimeRequest) -> Option<String> {
    let args = &request.call.args;
    if let Some(value) = args.as_str() {
        return Some(value.to_string());
    }

    ["command", "cmd", "script"]
        .into_iter()
        .find_map(|key| args.get(key).and_then(|value| value.as_str()).map(ToString::to_string))
}

fn classify_bash_command(command: Option<&str>) -> Option<BashCommandClassification> {
    let command = command?.trim();
    if command.is_empty() {
        return None;
    }

    let normalized = command.to_ascii_lowercase();
    let high_risk_patterns = [
        ("rm -rf /", "destructive root-level recursive delete pattern detected"),
        ("--no-preserve-root", "explicit root-preserving safeguards disabled for rm"),
        ("curl ", "network-fetched shell execution pattern detected"),
        ("wget ", "network-fetched shell execution pattern detected"),
        (":(){", "fork bomb shell pattern detected"),
        ("mkfs", "filesystem formatting command detected"),
        ("shutdown", "system shutdown command detected"),
        ("reboot", "system reboot command detected"),
    ];
    for (pattern, reason) in high_risk_patterns {
        if normalized.contains(pattern)
            && (!matches!(pattern, "curl " | "wget ") || normalized.contains("| sh"))
        {
            return Some(BashCommandClassification {
                risk: BashCommandRisk::High,
                reason: reason.to_string(),
            });
        }
    }

    let medium_risk_patterns = [
        ("rm -rf", "recursive delete command requires restricted sandbox"),
        ("sudo ", "privilege escalation command requires restricted sandbox"),
        ("chmod -r 777", "broad permission rewrite requires restricted sandbox"),
        ("chown -r", "recursive ownership change requires restricted sandbox"),
    ];
    for (pattern, reason) in medium_risk_patterns {
        if normalized.contains(pattern) {
            return Some(BashCommandClassification {
                risk: BashCommandRisk::Medium,
                reason: reason.to_string(),
            });
        }
    }

    Some(BashCommandClassification {
        risk: BashCommandRisk::Low,
        reason: "command matched no dangerous bash classifier rules".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_ports::{RunId, ThreadId, ToolCallSpec, ToolProviderType};
    use serde_json::json;
    use state_abstraction::{CreateSessionRequest, SessionContext, SessionCore, SessionPolicy};
    use uuid::Uuid;

    fn bash_request(command: &str) -> ToolRuntimeRequest {
        ToolRuntimeRequest {
            session_id: Uuid::nil(),
            run_id: RunId::new_v4(),
            thread_id: ThreadId::new_v4(),
            adapter_kind: ToolAdapterKind::Bash,
            provider_type: ToolProviderType::Local,
            provider_name: "bash".to_string(),
            call: ToolCallSpec {
                name: "bash".to_string(),
                args: json!({"command": command}),
                call_id: "call-bash".to_string(),
            },
            security: None,
        }
    }

    #[tokio::test]
    async fn security_chain_allows_by_default_for_harmless_commands() {
        let core = SessionCore::new();
        let session = core
            .create_session(CreateSessionRequest {
                attached_thread_id: None,
                context: SessionContext::new(),
                policy: SessionPolicy::new(),
            })
            .await
            .unwrap();

        let evaluation = RuntimeSecurityChain::new().evaluate(&bash_request("pwd"), &session);

        assert_eq!(evaluation.context.policy_action, ExecutionPolicyAction::Allow);
        assert_eq!(evaluation.context.sandbox_profile, ProcessSandboxProfile::Standard);
        assert_eq!(evaluation.context.bash_classification.unwrap().risk, BashCommandRisk::Low);
    }

    #[tokio::test]
    async fn security_chain_blocks_high_risk_bash_commands() {
        let core = SessionCore::new();
        let session = core
            .create_session(CreateSessionRequest {
                attached_thread_id: None,
                context: SessionContext::new(),
                policy: SessionPolicy::new(),
            })
            .await
            .unwrap();

        let evaluation = RuntimeSecurityChain::new()
            .evaluate(&bash_request("rm -rf / --no-preserve-root"), &session);

        assert_eq!(evaluation.context.policy_action, ExecutionPolicyAction::Block);
        assert_eq!(evaluation.context.sandbox_profile, ProcessSandboxProfile::Restricted);
        assert!(evaluation.context.policy_reason.contains("blocked high-risk command"));
    }
}
