//! Default adapters for `agent-ports` (no LangChain/rig types exposed).

pub mod checkpoint_memory;
pub mod llm_heuristic;
pub mod llm_openai;
pub mod memory_default;
pub mod persisting_tool_port;
pub mod skill_default;
pub mod subagent_default;
pub mod tool_registry_adapter;

pub use checkpoint_memory::MemoryCheckpointAdapter;
pub use llm_heuristic::{ChatModelLlmAdapter, HeuristicLlmAdapter};
pub use llm_openai::OpenAiChatLlmAdapter;
pub use memory_default::DefaultMemoryAdapter;
pub use persisting_tool_port::PersistingToolPort;
pub use skill_default::DefaultSkillAdapter;
pub use subagent_default::DefaultSubagentAdapter;
pub use tool_registry_adapter::{default_echo_manifests, EchoTool, RegistryToolAdapter};
