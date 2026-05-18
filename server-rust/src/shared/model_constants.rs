//! Model constants — mirrors shared/modelConstants.js
//!
//! Centralized model definitions for all supported AI providers.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModels {
    pub options: Vec<ModelOption>,
    pub default: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRegistry {
    pub id: String,
    pub name: String,
    pub models: ProviderModels,
}

pub fn claude_models() -> ProviderModels {
    ProviderModels {
        options: vec![
            ModelOption { value: "opus".into(), label: "Opus".into() },
            ModelOption { value: "sonnet".into(), label: "Sonnet".into() },
            ModelOption { value: "haiku".into(), label: "Haiku".into() },
            ModelOption { value: "claude-opus-4-6".into(), label: "Opus 4.6".into() },
            ModelOption { value: "opusplan".into(), label: "Opus Plan".into() },
            ModelOption { value: "sonnet[1m]".into(), label: "Sonnet [1M]".into() },
            ModelOption { value: "opus[1m]".into(), label: "Opus [1M]".into() },
        ],
        default: "opus".into(),
    }
}

pub fn cursor_models() -> ProviderModels {
    ProviderModels {
        options: vec![
            ModelOption { value: "opus-4.6-thinking".into(), label: "Claude 4.6 Opus (Thinking)".into() },
            ModelOption { value: "gpt-5.3-codex".into(), label: "GPT-5.3".into() },
            ModelOption { value: "gpt-5.2-high".into(), label: "GPT-5.2 High".into() },
            ModelOption { value: "gemini-3-pro".into(), label: "Gemini 3 Pro".into() },
            ModelOption { value: "opus-4.5-thinking".into(), label: "Claude 4.5 Opus (Thinking)".into() },
            ModelOption { value: "gpt-5.2".into(), label: "GPT-5.2".into() },
            ModelOption { value: "gpt-5.1".into(), label: "GPT-5.1".into() },
            ModelOption { value: "gpt-5.1-high".into(), label: "GPT-5.1 High".into() },
            ModelOption { value: "composer-1".into(), label: "Composer 1".into() },
            ModelOption { value: "auto".into(), label: "Auto".into() },
            ModelOption { value: "sonnet-4.5".into(), label: "Claude 4.5 Sonnet".into() },
            ModelOption { value: "sonnet-4.5-thinking".into(), label: "Claude 4.5 Sonnet (Thinking)".into() },
            ModelOption { value: "opus-4.5".into(), label: "Claude 4.5 Opus".into() },
            ModelOption { value: "gpt-5.1-codex".into(), label: "GPT-5.1 Codex".into() },
            ModelOption { value: "gpt-5.1-codex-high".into(), label: "GPT-5.1 Codex High".into() },
            ModelOption { value: "gpt-5.1-codex-max".into(), label: "GPT-5.1 Codex Max".into() },
            ModelOption { value: "gpt-5.1-codex-max-high".into(), label: "GPT-5.1 Codex Max High".into() },
            ModelOption { value: "opus-4.1".into(), label: "Claude 4.1 Opus".into() },
            ModelOption { value: "grok".into(), label: "Grok".into() },
        ],
        default: "gpt-5.3-codex".into(),
    }
}

pub fn codex_models() -> ProviderModels {
    ProviderModels {
        options: vec![
            ModelOption { value: "gpt-5.5".into(), label: "GPT-5.5".into() },
            ModelOption { value: "gpt-5.4".into(), label: "GPT-5.4".into() },
            ModelOption { value: "gpt-5.4-mini".into(), label: "GPT-5.4 mini".into() },
            ModelOption { value: "gpt-5.3-codex".into(), label: "GPT-5.3 Codex".into() },
            ModelOption { value: "gpt-5.2-codex".into(), label: "GPT-5.2 Codex".into() },
            ModelOption { value: "gpt-5.2".into(), label: "GPT-5.2".into() },
            ModelOption { value: "gpt-5.1-codex-max".into(), label: "GPT-5.1 Codex Max".into() },
            ModelOption { value: "o3".into(), label: "O3".into() },
            ModelOption { value: "o4-mini".into(), label: "O4-mini".into() },
        ],
        default: "gpt-5.4".into(),
    }
}

pub fn gemini_models() -> ProviderModels {
    ProviderModels {
        options: vec![
            ModelOption { value: "gemini-3.1-pro-preview".into(), label: "Gemini 3.1 Pro Preview".into() },
            ModelOption { value: "gemini-3-pro-preview".into(), label: "Gemini 3 Pro Preview".into() },
            ModelOption { value: "gemini-3-flash-preview".into(), label: "Gemini 3 Flash Preview".into() },
            ModelOption { value: "gemini-2.5-flash".into(), label: "Gemini 2.5 Flash".into() },
            ModelOption { value: "gemini-2.5-pro".into(), label: "Gemini 2.5 Pro".into() },
            ModelOption { value: "gemini-2.0-flash-lite".into(), label: "Gemini 2.0 Flash Lite".into() },
            ModelOption { value: "gemini-2.5-flash-lite".into(), label: "Gemini 2.5 Flash Lite".into() },
            ModelOption { value: "gemini-2.0-flash".into(), label: "Gemini 2.0 Flash".into() },
            ModelOption { value: "gemini-2.0-pro-exp".into(), label: "Gemini 2.0 Pro Experimental".into() },
            ModelOption { value: "gemini-2.0-flash-thinking-exp".into(), label: "Gemini 2.0 Flash Thinking".into() },
        ],
        default: "gemini-3.1-pro-preview".into(),
    }
}

pub fn providers() -> Vec<ProviderRegistry> {
    vec![
        ProviderRegistry { id: "claude".into(), name: "Anthropic".into(), models: claude_models() },
        ProviderRegistry { id: "codex".into(), name: "OpenAI".into(), models: codex_models() },
        ProviderRegistry { id: "gemini".into(), name: "Google".into(), models: gemini_models() },
        ProviderRegistry { id: "cursor".into(), name: "Cursor".into(), models: cursor_models() },
    ]
}
