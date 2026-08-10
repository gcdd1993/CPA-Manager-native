package main

import (
	"strings"
	"testing"

	"github.com/router-for-me/CLIProxyAPI/v7/sdk/pluginapi"
)

func TestManagementPageShowsProviderConfiguration(t *testing.T) {
	previousRuntime := pluginRuntime
	pluginRuntime = &runtimeState{
		configured: true,
		config:     compiledConfig{ConfigPath: `C:\config.yaml`, SyncInterval: 5 * 60 * 1e9, RawAliasRules: []aliasRule{{ModelPattern: `^gpt`, AliasReplacement: `codex-$0`}}},
		providers:  map[string]providerStatus{"联云": {Name: "联云", BaseURL: "https://provider.example/v1", ModelCount: 3, AliasCount: 2}},
	}
	t.Cleanup(func() { pluginRuntime = previousRuntime })
	view := pluginRuntime.ManagementView()
	if !view.Configured || len(view.Providers) != 1 || view.Providers[0].AliasCount != 2 {
		t.Fatalf("ManagementView() = %#v", view)
	}
	response := handleManagement(pluginapi.ManagementRequest{Method: "GET", Path: "/v0/resource/plugins/provider-model-sync/status"})
	if response.StatusCode != 200 || !strings.Contains(string(response.Body), "联云") || !strings.Contains(string(response.Body), "别名规则") {
		t.Fatalf("management response = %d, %s", response.StatusCode, response.Body)
	}
}

func TestSaveRulesRejectsInvalidRule(t *testing.T) {
	previousRuntime := pluginRuntime
	pluginRuntime = &runtimeState{configured: false}
	t.Cleanup(func() { pluginRuntime = previousRuntime })
	response := saveRules([]byte(`{"rules":[{"model_pattern":"["}]}`))
	if response.StatusCode != 409 {
		t.Fatalf("saveRules() status = %d, want 409 for unconfigured runtime", response.StatusCode)
	}
}
