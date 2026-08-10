//go:build cgo

package main

import (
	"encoding/json"
	"testing"
)

func TestPluginRegisterWithoutProviderConfigStillRegisters(t *testing.T) {
	request, errMarshal := json.Marshal(lifecycleRequest{ConfigYAML: []byte("enabled: true\n")})
	if errMarshal != nil {
		t.Fatalf("marshal lifecycle request: %v", errMarshal)
	}
	raw, errHandle := handleMethod("plugin.register", request)
	if errHandle != nil {
		t.Fatalf("handleMethod(plugin.register) error = %v", errHandle)
	}
	var response envelope
	if errUnmarshal := json.Unmarshal(raw, &response); errUnmarshal != nil {
		t.Fatalf("decode envelope: %v", errUnmarshal)
	}
	if !response.OK {
		t.Fatalf("plugin.register envelope = %#v", response)
	}
	var registered registration
	if errUnmarshal := json.Unmarshal(response.Result, &registered); errUnmarshal != nil {
		t.Fatalf("decode registration: %v", errUnmarshal)
	}
	if registered.Metadata.Name != "Provider 模型同步" {
		t.Fatalf("Metadata.Name = %q", registered.Metadata.Name)
	}
	if !registered.Capabilities.ManagementAPI {
		t.Fatalf("Capabilities = %#v", registered.Capabilities)
	}
	if len(registered.Metadata.ConfigFields) == 0 {
		t.Fatal("ConfigFields is empty")
	}
}
