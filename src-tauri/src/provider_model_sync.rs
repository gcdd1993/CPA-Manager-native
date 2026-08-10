use std::{collections::HashMap, fs, path::Path, time::Duration};

use regex::Regex;
use reqwest::{header::HeaderName, StatusCode};
use serde::Deserialize;
use serde_yaml::{Mapping, Value};

use crate::{
    config::{ProviderModelAliasRule, ProviderModelSyncSettings},
    error::{message, AppResult},
    state::AppState,
};

#[derive(Debug)]
struct ProviderDefinition {
    name: String,
    base_url: String,
    api_keys: Vec<String>,
    headers: HashMap<String, String>,
    disabled: bool,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    #[serde(default)]
    data: Vec<UpstreamModel>,
}

#[derive(Debug, Deserialize)]
struct UpstreamModel {
    id: String,
}

struct CompiledAliasRule {
    provider_pattern: Regex,
    model_pattern: Regex,
    alias_replacement: String,
    force_mapping: bool,
}

pub async fn synchronize(state: &AppState) -> AppResult<bool> {
    let settings = state.settings().provider_model_sync;
    if !settings.enabled {
        return Ok(false);
    }
    let rules = compile_rules(&settings)?;
    let config_path = state
        .component_data_dir(crate::models::ComponentId::Cliproxyapi)
        .join("config.yaml");
    if !config_path.exists() {
        return Ok(false);
    }
    let original = fs::read(&config_path)?;
    let mut root: Value = serde_yaml::from_slice(&original)
        .map_err(|error| message(format!("解析 CLIProxyAPI 配置失败：{error}")))?;
    let mut changed = disable_legacy_plugin(&mut root);
    let mut alias_logs = Vec::new();
    let providers = mapping_value_mut(root_mapping_mut(&mut root)?, "openai-compatibility")
        .and_then(Value::as_sequence_mut)
        .ok_or_else(|| message("openai-compatibility 必须是 YAML 数组"))?;

    let mut success_count = 0usize;
    let mut errors = Vec::new();
    for provider_value in providers.iter_mut() {
        let definition = read_provider(provider_value)?;
        if definition.disabled || definition.base_url.trim().is_empty() {
            continue;
        }
        match fetch_models(state, &definition).await {
            Ok(models) => {
                changed |= merge_models(
                    provider_value,
                    &definition.name,
                    &models,
                    &rules,
                    &mut alias_logs,
                )?;
                success_count += 1;
            }
            Err(error) => errors.push(format!("{}: {error}", definition.name)),
        }
    }
    if success_count == 0 && !errors.is_empty() {
        return Err(message(format!(
            "全部 Provider 同步失败：{}",
            errors.join("；")
        )));
    }
    if changed {
        write_config(&config_path, &original, &root)?;
    }
    for log in alias_logs {
        state.log(
            crate::models::LogSource::App,
            crate::models::LogLevel::Info,
            format!("[模型别名] {log}"),
        );
    }
    if errors.is_empty() {
        state.log(
            crate::models::LogSource::App,
            crate::models::LogLevel::Info,
            format!("Provider 模型同步完成，共处理 {success_count} 个 Provider"),
        );
    } else {
        state.log(
            crate::models::LogSource::App,
            crate::models::LogLevel::Warn,
            format!("Provider 模型同步部分失败：{}", errors.join("；")),
        );
    }
    Ok(changed)
}

pub fn validate_settings(settings: &ProviderModelSyncSettings) -> AppResult<()> {
    if settings.interval_minutes < 1 {
        return Err(message("模型同步间隔不能小于 1 分钟"));
    }
    compile_rules(settings).map(|_| ())
}

pub fn disable_legacy_plugin_in_file(config_path: &Path) -> AppResult<bool> {
    if !config_path.exists() {
        return Ok(false);
    }
    let original = fs::read(config_path)?;
    let mut root: Value = serde_yaml::from_slice(&original)
        .map_err(|error| message(format!("解析 CLIProxyAPI 配置失败：{error}")))?;
    if !disable_legacy_plugin(&mut root) {
        return Ok(false);
    }
    write_config(config_path, &original, &root)?;
    Ok(true)
}

fn compile_rules(settings: &ProviderModelSyncSettings) -> AppResult<Vec<CompiledAliasRule>> {
    settings
        .alias_rules
        .iter()
        .filter(|rule| rule.enabled)
        .map(compile_rule)
        .collect()
}

fn compile_rule(rule: &ProviderModelAliasRule) -> AppResult<CompiledAliasRule> {
    let provider_pattern = if rule.provider_pattern.is_empty() {
        ".*"
    } else {
        &rule.provider_pattern
    };
    let model_pattern = if rule.model_pattern.is_empty() {
        ".*"
    } else {
        &rule.model_pattern
    };
    Ok(CompiledAliasRule {
        provider_pattern: Regex::new(provider_pattern)
            .map_err(|error| message(format!("Provider 正则无效：{error}")))?,
        model_pattern: Regex::new(model_pattern)
            .map_err(|error| message(format!("模型正则无效：{error}")))?,
        alias_replacement: rule.alias_replacement.clone(),
        force_mapping: rule.force_mapping,
    })
}

fn read_provider(value: &Value) -> AppResult<ProviderDefinition> {
    let mapping = value
        .as_mapping()
        .ok_or_else(|| message("Provider 配置必须是 YAML 对象"))?;
    let name = string_value(mapping, "name");
    let base_url = string_value(mapping, "base-url");
    let disabled = bool_value(mapping, "disabled");
    let api_keys = mapping_value(mapping, "api-key-entries")
        .and_then(Value::as_sequence)
        .map(|entries| {
            entries
                .iter()
                .filter_map(Value::as_mapping)
                .map(|entry| string_value(entry, "api-key"))
                .filter(|key| !key.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let headers = mapping_value(mapping, "headers")
        .and_then(Value::as_mapping)
        .map(|values| {
            values
                .iter()
                .filter_map(|(key, value)| {
                    Some((key.as_str()?.to_string(), value.as_str()?.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(ProviderDefinition {
        name,
        base_url,
        api_keys,
        headers,
        disabled,
    })
}

async fn fetch_models(state: &AppState, provider: &ProviderDefinition) -> AppResult<Vec<String>> {
    let endpoint = format!("{}/models", provider.base_url.trim_end_matches('/'));
    let keys = if provider.api_keys.is_empty() {
        vec![String::new()]
    } else {
        provider.api_keys.clone()
    };
    let mut last_error = None;
    for key in keys {
        let mut request = state.client.get(&endpoint).timeout(Duration::from_secs(20));
        if !key.is_empty() {
            request = request.bearer_auth(&key);
        }
        for (name, value) in &provider.headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|error| message(format!("请求头名称无效 {name}: {error}")))?;
            request = request.header(header_name, value);
        }
        match request.send().await {
            Ok(response) if response.status().is_success() => {
                let payload: ModelsResponse = response.json().await.map_err(|error| {
                    message(format!("解析 {} 模型响应失败：{error}", provider.name))
                })?;
                let mut models: Vec<String> = payload
                    .data
                    .into_iter()
                    .map(|model| model.id)
                    .filter(|id| !id.trim().is_empty())
                    .collect();
                models.sort();
                models.dedup();
                return Ok(models);
            }
            Ok(response)
                if response.status() == StatusCode::UNAUTHORIZED
                    || response.status() == StatusCode::FORBIDDEN =>
            {
                last_error = Some(format!("HTTP {}", response.status()));
            }
            Ok(response) => return Err(message(format!("HTTP {}", response.status()))),
            Err(error) => return Err(message(format!("请求模型列表失败：{error}"))),
        }
    }
    Err(message(
        last_error.unwrap_or_else(|| "没有可用 API Key".to_string()),
    ))
}

fn merge_models(
    provider_value: &mut Value,
    provider_name: &str,
    upstream_models: &[String],
    rules: &[CompiledAliasRule],
    alias_logs: &mut Vec<String>,
) -> AppResult<bool> {
    let provider = provider_value
        .as_mapping_mut()
        .ok_or_else(|| message("Provider 配置必须是 YAML 对象"))?;
    let models_value = provider
        .entry(Value::String("models".to_string()))
        .or_insert_with(|| Value::Sequence(Vec::new()));
    let models = models_value
        .as_sequence_mut()
        .ok_or_else(|| message("Provider models 必须是 YAML 数组"))?;
    let mut indexes = HashMap::new();
    for (index, model) in models.iter().enumerate() {
        if let Some(mapping) = model.as_mapping() {
            let name = string_value(mapping, "name");
            if !name.is_empty() {
                indexes.insert(name, index);
            }
        }
    }
    let mut changed = false;
    for model_name in upstream_models {
        let index = if let Some(index) = indexes.get(model_name) {
            *index
        } else {
            let mut mapping = Mapping::new();
            mapping.insert(
                Value::String("name".to_string()),
                Value::String(model_name.clone()),
            );
            models.push(Value::Mapping(mapping));
            let index = models.len() - 1;
            indexes.insert(model_name.clone(), index);
            changed = true;
            index
        };
        let mapping = models[index]
            .as_mapping_mut()
            .ok_or_else(|| message("模型配置必须是 YAML 对象"))?;
        let existing_alias = string_value(mapping, "alias");
        let (alias, force_mapping) =
            generate_alias(provider_name, model_name, &existing_alias, rules);
        if existing_alias != alias {
            alias_logs.push(format!("{provider_name} | {model_name} -> {alias}"));
            set_value(mapping, "alias", Value::String(alias));
            changed = true;
        }
        if let Some(force_mapping) = force_mapping {
            if bool_value(mapping, "force-mapping") != force_mapping {
                set_value(mapping, "force-mapping", Value::Bool(force_mapping));
                changed = true;
            }
        }
    }
    Ok(changed)
}

fn generate_alias(
    provider: &str,
    model: &str,
    existing_alias: &str,
    rules: &[CompiledAliasRule],
) -> (String, Option<bool>) {
    for rule in rules {
        if rule.provider_pattern.is_match(provider) && rule.model_pattern.is_match(model) {
            let alias = rule
                .model_pattern
                .replace_all(model, rule.alias_replacement.as_str());
            return (normalize_alias(&alias), Some(rule.force_mapping));
        }
    }
    if !existing_alias.is_empty() {
        return (normalize_alias(existing_alias), None);
    }
    (normalize_alias(model), None)
}

fn normalize_alias(value: &str) -> String {
    let last_segment = value
        .rsplit('/')
        .next()
        .unwrap_or(value)
        .trim()
        .to_lowercase();
    let whitespace = Regex::new(r"\s+").expect("valid whitespace regex");
    let repeated_hyphen = Regex::new(r"-+").expect("valid hyphen regex");
    let date_suffix =
        Regex::new(r"(?:-(?:\d{4}-\d{2}-\d{2}|\d{8}|\d{4}))$").expect("valid date regex");
    let alias = whitespace.replace_all(&last_segment, "-");
    let alias = repeated_hyphen.replace_all(&alias, "-");
    date_suffix
        .replace(&alias, "")
        .trim_matches('-')
        .to_string()
}

fn disable_legacy_plugin(root: &mut Value) -> bool {
    let Ok(root) = root_mapping_mut(root) else {
        return false;
    };
    let Some(plugins) = mapping_value_mut(root, "plugins").and_then(Value::as_mapping_mut) else {
        return false;
    };
    let Some(configs) = mapping_value_mut(plugins, "configs").and_then(Value::as_mapping_mut)
    else {
        return false;
    };
    let Some(plugin) =
        mapping_value_mut(configs, "provider-model-sync").and_then(Value::as_mapping_mut)
    else {
        return false;
    };
    if bool_value(plugin, "enabled") {
        set_value(plugin, "enabled", Value::Bool(false));
        return true;
    }
    false
}

fn write_config(path: &Path, original: &[u8], root: &Value) -> AppResult<()> {
    let updated = serde_yaml::to_string(root)
        .map_err(|error| message(format!("编码 CLIProxyAPI 配置失败：{error}")))?;
    if original == updated.as_bytes() {
        return Ok(());
    }
    let backup = path.with_extension("yaml.provider-model-sync.bak");
    fs::write(backup, original)?;
    let temporary = path.with_extension("yaml.provider-model-sync.tmp");
    fs::write(&temporary, updated.as_bytes())?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn root_mapping_mut(value: &mut Value) -> AppResult<&mut Mapping> {
    value
        .as_mapping_mut()
        .ok_or_else(|| message("CLIProxyAPI 配置根节点必须是 YAML 对象"))
}

fn mapping_value<'a>(mapping: &'a Mapping, key: &str) -> Option<&'a Value> {
    mapping.get(Value::String(key.to_string()))
}

fn mapping_value_mut<'a>(mapping: &'a mut Mapping, key: &str) -> Option<&'a mut Value> {
    mapping.get_mut(Value::String(key.to_string()))
}

fn string_value(mapping: &Mapping, key: &str) -> String {
    mapping_value(mapping, key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn bool_value(mapping: &Mapping, key: &str) -> bool {
    mapping_value(mapping, key)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn set_value(mapping: &mut Mapping, key: &str, value: Value) {
    mapping.insert(Value::String(key.to_string()), value);
}

#[cfg(test)]
mod tests {
    use super::normalize_alias;

    #[test]
    fn normalizes_builtin_aliases() {
        assert_eq!(normalize_alias("GLM 5.2"), "glm-5.2");
        assert_eq!(normalize_alias("z-ai/glm-5.2"), "glm-5.2");
        assert_eq!(
            normalize_alias("deepseek-v4-flash-0731"),
            "deepseek-v4-flash"
        );
    }
}
