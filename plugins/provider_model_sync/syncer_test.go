package main

import (
	"context"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
	"time"

	"gopkg.in/yaml.v3"
)

func TestDiscoverConfigPath(t *testing.T) {
	path, errPath := discoverConfigPath([]string{"cli-proxy-api.exe", "--config", `E:\data\config.yaml`})
	if errPath != nil || !strings.HasSuffix(path, `data\config.yaml`) {
		t.Fatalf("discoverConfigPath() = %q, %v", path, errPath)
	}
}

func TestCompileConfigRules(t *testing.T) {
	enabled := true
	config, errCompile := compileConfig(pluginConfig{
		SyncIntervalSeconds: 60,
		AliasRules:          []aliasRule{{Enabled: &enabled, ProviderPattern: `薄荷`, ModelPattern: `^deepseek-(.+)$`, AliasReplacement: `ds-$1`, ForceMapping: true}},
	}, `C:\config.yaml`)
	if errCompile != nil {
		t.Fatalf("compileConfig() error = %v", errCompile)
	}
	if config.SyncInterval != time.Minute || len(config.AliasRules) != 1 {
		t.Fatalf("compiled config = %#v", config)
	}
	if !config.AliasRules[0].ProviderPattern.MatchString("薄荷 API") || !config.AliasRules[0].ModelPattern.MatchString("deepseek-v4") {
		t.Fatal("compiled rule does not match expected provider/model")
	}
}

func TestBuiltinModelAliasNormalizesCaseSpacesAndDates(t *testing.T) {
	tests := map[string]string{
		"GLM 5.2":                     "glm-5.2",
		"z-ai/glm-5.2":                "glm-5.2",
		"deepseek-ai/DeepSeek-V4-Pro": "deepseek-v4-pro",
		"deepseek-v4-flash-0731":      "deepseek-v4-flash",
		"DeepSeek V4 Flash 20250731":  "deepseek-v4-flash",
		"model-2025-07-31":            "model",
		"gpt-5.4":                     "gpt-5.4",
	}
	for input, expected := range tests {
		if actual := builtinModelAlias(input); actual != expected {
			t.Errorf("builtinModelAlias(%q) = %q, want %q", input, actual, expected)
		}
	}
}

func TestMergeProviderModelsWritesAliasAndForceMapping(t *testing.T) {
	modelsNode := &yaml.Node{Kind: yaml.SequenceNode, Tag: "!!seq", Content: []*yaml.Node{{
		Kind: yaml.MappingNode, Tag: "!!map",
		Content: []*yaml.Node{{Kind: yaml.ScalarNode, Value: "name"}, {Kind: yaml.ScalarNode, Value: "gpt-5.4"}},
	}}}
	config := compiledConfig{AliasRules: []compiledAliasRule{{
		ProviderPattern: regexp.MustCompile(`联云`), ModelPattern: regexp.MustCompile(`^gpt-(.+)$`),
		AliasReplacement: `codex-$1`, ForceMapping: true,
	}}}
	changed, modelCount, aliasCount := mergeProviderModels(providerDefinition{Name: "联云", ModelsNode: modelsNode}, []upstreamModel{{ID: "gpt-5.4"}}, config)
	if !changed || modelCount != 1 || aliasCount != 1 {
		t.Fatalf("mergeProviderModels() = %v, %d, %d", changed, modelCount, aliasCount)
	}
	model := modelsNode.Content[0]
	if scalarValue(mappingValue(model, "alias")) != "codex-5.4" || !boolValue(mappingValue(model, "force-mapping")) {
		t.Fatalf("model node = %#v", model)
	}
}

func TestMergeProviderModelsKeepsManualAliasAndAddsBuiltinAlias(t *testing.T) {
	modelsNode := &yaml.Node{Kind: yaml.SequenceNode, Tag: "!!seq", Content: []*yaml.Node{
		{Kind: yaml.MappingNode, Tag: "!!map", Content: []*yaml.Node{{Kind: yaml.ScalarNode, Value: "name"}, {Kind: yaml.ScalarNode, Value: "GLM 5.2"}}},
		{Kind: yaml.MappingNode, Tag: "!!map", Content: []*yaml.Node{{Kind: yaml.ScalarNode, Value: "name"}, {Kind: yaml.ScalarNode, Value: "deepseek-v4-flash-0731"}, {Kind: yaml.ScalarNode, Value: "alias"}, {Kind: yaml.ScalarNode, Value: "manual-deepseek"}}},
		{Kind: yaml.MappingNode, Tag: "!!map", Content: []*yaml.Node{{Kind: yaml.ScalarNode, Value: "name"}, {Kind: yaml.ScalarNode, Value: "GLM 5.3"}, {Kind: yaml.ScalarNode, Value: "alias"}, {Kind: yaml.ScalarNode, Value: "GLM-5.3"}}},
	}}
	_, _, _ = mergeProviderModels(providerDefinition{Name: "provider", ModelsNode: modelsNode}, []upstreamModel{{ID: "GLM 5.2"}, {ID: "deepseek-v4-flash-0731"}, {ID: "GLM 5.3"}}, compiledConfig{})
	if got := scalarValue(mappingValue(modelsNode.Content[0], "alias")); got != "glm-5.2" {
		t.Fatalf("builtin alias = %q, want glm-5.2", got)
	}
	if got := scalarValue(mappingValue(modelsNode.Content[1], "alias")); got != "manual-deepseek" {
		t.Fatalf("manual alias = %q, want manual-deepseek", got)
	}
	if got := scalarValue(mappingValue(modelsNode.Content[2], "alias")); got != "glm-5.3" {
		t.Fatalf("normalized existing alias = %q, want glm-5.3", got)
	}
}

func TestFetchProviderModelsUsesConfiguredAPIKey(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(response http.ResponseWriter, request *http.Request) {
		if request.Header.Get("Authorization") != "Bearer secret" {
			t.Fatalf("Authorization = %q", request.Header.Get("Authorization"))
		}
		_, _ = response.Write([]byte(`{"data":[{"id":"gpt-5.4"}]}`))
	}))
	defer server.Close()
	models, errFetch := fetchProviderModels(context.Background(), time.Second, providerDefinition{Name: "test", BaseURL: server.URL, APIKeys: []string{"secret"}})
	if errFetch != nil || len(models) != 1 || models[0].ID != "gpt-5.4" {
		t.Fatalf("fetchProviderModels() = %#v, %v", models, errFetch)
	}
}

func TestWriteConfigAtomicallyPreservesBackup(t *testing.T) {
	directory := t.TempDir()
	path := filepath.Join(directory, "config.yaml")
	original := []byte("openai-compatibility: []\n")
	updated := []byte("openai-compatibility:\n  - name: test\n")
	if errWrite := os.WriteFile(path, original, 0o600); errWrite != nil {
		t.Fatal(errWrite)
	}
	if errWrite := writeConfigAtomically(path, original, updated); errWrite != nil {
		t.Fatalf("writeConfigAtomically() error = %v", errWrite)
	}
	got, _ := os.ReadFile(path)
	backup, _ := os.ReadFile(path + ".provider-model-sync.bak")
	if string(got) != string(updated) || string(backup) != string(original) {
		t.Fatalf("updated = %q, backup = %q", got, backup)
	}
}
