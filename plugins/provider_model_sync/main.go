package main

/*
#include <stdint.h>
#include <stdlib.h>

typedef struct {
	void* ptr;
	size_t len;
} cliproxy_buffer;

typedef int (*cliproxy_host_call_fn)(void*, const char*, const uint8_t*, size_t, cliproxy_buffer*);
typedef void (*cliproxy_host_free_fn)(void*, size_t);

typedef struct {
	uint32_t abi_version;
	void* host_ctx;
	cliproxy_host_call_fn call;
	cliproxy_host_free_fn free_buffer;
} cliproxy_host_api;

typedef int (*cliproxy_plugin_call_fn)(char*, uint8_t*, size_t, cliproxy_buffer*);
typedef void (*cliproxy_plugin_free_fn)(void*, size_t);
typedef void (*cliproxy_plugin_shutdown_fn)(void);

typedef struct {
	uint32_t abi_version;
	cliproxy_plugin_call_fn call;
	cliproxy_plugin_free_fn free_buffer;
	cliproxy_plugin_shutdown_fn shutdown;
} cliproxy_plugin_api;

extern int cliproxyPluginCall(char*, uint8_t*, size_t, cliproxy_buffer*);
extern void cliproxyPluginFree(void*, size_t);
extern void cliproxyPluginShutdown(void);

static const cliproxy_host_api* stored_host;

static void store_host_api(const cliproxy_host_api* host) {
	stored_host = host;
}

static int call_host_api(const char* method, const uint8_t* request, size_t request_len, cliproxy_buffer* response) {
	if (stored_host == NULL || stored_host->call == NULL) {
		return 1;
	}
	return stored_host->call(stored_host->host_ctx, method, request, request_len, response);
}

static void free_host_buffer(void* ptr, size_t len) {
	if (stored_host != NULL && stored_host->free_buffer != NULL && ptr != NULL) {
		stored_host->free_buffer(ptr, len);
	}
}
*/
import "C"

import (
	"encoding/json"
	"fmt"
	"unsafe"

	"github.com/router-for-me/CLIProxyAPI/v7/sdk/pluginabi"
	"github.com/router-for-me/CLIProxyAPI/v7/sdk/pluginapi"
)

type envelope struct {
	OK     bool            `json:"ok"`
	Result json.RawMessage `json:"result,omitempty"`
	Error  *envelopeError  `json:"error,omitempty"`
}

type envelopeError struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

type lifecycleRequest struct {
	ConfigYAML []byte `json:"config_yaml"`
}

type registration struct {
	SchemaVersion uint32                   `json:"schema_version"`
	Metadata      pluginapi.Metadata       `json:"metadata"`
	Capabilities  registrationCapabilities `json:"capabilities"`
}

type registrationCapabilities struct {
	ManagementAPI bool `json:"management_api"`
}

type managementRegistration struct {
	Routes    []managementRoute    `json:"routes"`
	Resources []managementResource `json:"resources"`
}

type managementRoute struct {
	Method      string `json:"Method"`
	Path        string `json:"Path"`
	Description string `json:"Description"`
}

type managementResource struct {
	Path        string `json:"Path"`
	Menu        string `json:"Menu"`
	Description string `json:"Description"`
}

func main() {}

func init() {
	hostLog = sendLogToHost
}

//export cliproxy_plugin_init
func cliproxy_plugin_init(host *C.cliproxy_host_api, plugin *C.cliproxy_plugin_api) C.int {
	if plugin == nil {
		return 1
	}
	C.store_host_api(host)
	plugin.abi_version = C.uint32_t(pluginabi.ABIVersion)
	plugin.call = C.cliproxy_plugin_call_fn(C.cliproxyPluginCall)
	plugin.free_buffer = C.cliproxy_plugin_free_fn(C.cliproxyPluginFree)
	plugin.shutdown = C.cliproxy_plugin_shutdown_fn(C.cliproxyPluginShutdown)
	return 0
}

//export cliproxyPluginCall
func cliproxyPluginCall(method *C.char, request *C.uint8_t, requestLen C.size_t, response *C.cliproxy_buffer) C.int {
	if response != nil {
		response.ptr = nil
		response.len = 0
	}
	if method == nil {
		writeResponse(response, errorEnvelope("invalid_method", "method is required"))
		return 1
	}

	var requestBytes []byte
	if request != nil && requestLen > 0 {
		requestBytes = C.GoBytes(unsafe.Pointer(request), C.int(requestLen))
	}
	raw, errHandle := handleMethod(C.GoString(method), requestBytes)
	if errHandle != nil {
		writeResponse(response, errorEnvelope("plugin_error", errHandle.Error()))
		return 1
	}
	writeResponse(response, raw)
	return 0
}

//export cliproxyPluginFree
func cliproxyPluginFree(ptr unsafe.Pointer, length C.size_t) {
	if ptr != nil {
		C.free(ptr)
	}
	_ = length
}

//export cliproxyPluginShutdown
func cliproxyPluginShutdown() {
	pluginRuntime.Shutdown()
}

func handleMethod(method string, request []byte) ([]byte, error) {
	switch method {
	case pluginabi.MethodPluginRegister:
		if errConfigure := configure(request, true); errConfigure != nil {
			return nil, errConfigure
		}
		return okEnvelope(pluginRegistration())
	case pluginabi.MethodPluginReconfigure:
		if errConfigure := configure(request, false); errConfigure != nil {
			return nil, errConfigure
		}
		return okEnvelope(pluginRegistration())
	case pluginabi.MethodPluginShutdown:
		pluginRuntime.Shutdown()
		return okEnvelope(struct{}{})
	case pluginabi.MethodManagementRegister:
		return okEnvelope(managementRegistration{
			Routes: []managementRoute{
				{Method: "POST", Path: "/provider-model-sync/rules", Description: "保存 Provider 模型别名规则。"},
				{Method: "POST", Path: "/provider-model-sync/sync", Description: "立即同步 Provider 模型。"},
			},
			Resources: []managementResource{{
				Path: "/status", Menu: "Provider 模型同步",
				Description: "查看 Provider 同步状态并配置别名规则。",
			}},
		})
	case pluginabi.MethodManagementHandle:
		var managementRequest pluginapi.ManagementRequest
		if errUnmarshal := json.Unmarshal(request, &managementRequest); errUnmarshal != nil {
			return nil, fmt.Errorf("decode management request: %w", errUnmarshal)
		}
		return okEnvelope(handleManagement(managementRequest))
	default:
		return errorEnvelope("unknown_method", "unknown method: "+method), nil
	}
}

func configure(raw []byte, tolerateInvalid bool) error {
	var request lifecycleRequest
	if len(raw) > 0 {
		if errUnmarshal := json.Unmarshal(raw, &request); errUnmarshal != nil {
			return fmt.Errorf("decode lifecycle request: %w", errUnmarshal)
		}
	}
	config, errCompile := decodeConfig(request.ConfigYAML)
	if errCompile != nil {
		if tolerateInvalid {
			pluginRuntime.SetConfigurationError(errCompile)
			return nil
		}
		return errCompile
	}
	return pluginRuntime.Reconfigure(config)
}

func pluginRegistration() registration {
	return registration{
		SchemaVersion: pluginabi.SchemaVersion,
		Metadata: pluginapi.Metadata{
			Name:             "Provider 模型同步",
			Version:          "0.3.0",
			Author:           "CPA Manager Native contributors",
			GitHubRepository: "https://github.com/gcdd1993/CPA-Manager-native",
			ConfigFields: []pluginapi.ConfigField{
				{Name: "sync_interval_seconds", Type: pluginapi.ConfigFieldTypeInteger, Description: "Provider model refresh interval in seconds. Minimum 10; default 300."},
				{Name: "request_timeout_seconds", Type: pluginapi.ConfigFieldTypeInteger, Description: "Timeout for each Provider /models request. Default 15 seconds."},
				{Name: "include_disabled", Type: pluginapi.ConfigFieldTypeBoolean, Description: "Also synchronize disabled openai-compatibility entries."},
				{Name: "remove_missing_models", Type: pluginapi.ConfigFieldTypeBoolean, Description: "Remove configured models no longer returned by the Provider."},
				{Name: "alias_rules", Type: pluginapi.ConfigFieldTypeArray, Description: "Ordered Provider/model regular-expression rules written to models.alias."},
			},
		},
		Capabilities: registrationCapabilities{ManagementAPI: true},
	}
}

func okEnvelope(value any) ([]byte, error) {
	raw, errMarshal := json.Marshal(value)
	if errMarshal != nil {
		return nil, errMarshal
	}
	return json.Marshal(envelope{OK: true, Result: raw})
}

func errorEnvelope(code string, message string) []byte {
	raw, _ := json.Marshal(envelope{OK: false, Error: &envelopeError{Code: code, Message: message}})
	return raw
}

func writeResponse(response *C.cliproxy_buffer, raw []byte) {
	if response == nil || len(raw) == 0 {
		return
	}
	ptr := C.CBytes(raw)
	if ptr == nil {
		return
	}
	response.ptr = ptr
	response.len = C.size_t(len(raw))
}

func sendLogToHost(level string, message string, fields map[string]any) {
	payload, errMarshal := json.Marshal(map[string]any{
		"level": level, "message": message, "fields": fields,
	})
	if errMarshal != nil {
		return
	}
	callHost(pluginabi.MethodHostLog, payload)
}

func callHost(method string, payload []byte) {
	cMethod := C.CString(method)
	defer C.free(unsafe.Pointer(cMethod))
	var response C.cliproxy_buffer
	var request *C.uint8_t
	if len(payload) > 0 {
		request = (*C.uint8_t)(C.CBytes(payload))
		defer C.free(unsafe.Pointer(request))
	}
	if C.call_host_api(cMethod, request, C.size_t(len(payload)), &response) == 0 && response.ptr != nil {
		C.free_host_buffer(response.ptr, response.len)
	}
}
