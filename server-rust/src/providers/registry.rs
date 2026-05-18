//! Provider registry — register and look up provider implementations.

use std::collections::HashMap;
use std::sync::Arc;

use crate::providers::claude::ClaudeProvider;
use crate::providers::codex::CodexProvider;
use crate::providers::cursor::CursorProvider;
use crate::providers::gemini::GeminiProvider;
use crate::shared::providers::IProvider;

/// Registry of all available LLM providers.
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn IProvider>>,
}

impl ProviderRegistry {
    /// Create a new registry with all built-in providers registered.
    pub fn new() -> Self {
        let mut registry = Self {
            providers: HashMap::new(),
        };
        registry.register(Arc::new(ClaudeProvider::new()));
        registry.register(Arc::new(CodexProvider::new()));
        registry.register(Arc::new(GeminiProvider::new()));
        registry.register(Arc::new(CursorProvider::new()));
        registry
    }

    /// Register a provider by its [`LlmProvider`] string ID.
    pub fn register(&mut self, provider: Arc<dyn IProvider>) {
        self.providers.insert(provider.id().as_str().to_string(), provider);
    }

    /// Look up a provider by its string ID (e.g. `"claude"`, `"codex"`).
    pub fn get(&self, id: &str) -> Option<Arc<dyn IProvider>> {
        self.providers.get(id).cloned()
    }

    /// Return a list of all registered provider IDs.
    pub fn list_ids(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    /// Return all registered providers.
    pub fn list_all(&self) -> Vec<Arc<dyn IProvider>> {
        self.providers.values().cloned().collect()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}
