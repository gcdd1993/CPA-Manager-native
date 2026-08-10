package main

import (
	"bytes"
	"context"
	"encoding/json"
	"html/template"
	"net/http"
	"sort"
	"strings"
	"time"

	"github.com/router-for-me/CLIProxyAPI/v7/sdk/pluginapi"
	"gopkg.in/yaml.v3"
)

type managementRegistrationResponse struct {
	StatusCode int                 `json:"status_code"`
	Headers    map[string][]string `json:"headers"`
	Body       []byte              `json:"body"`
}

type managementProviderView struct {
	Name        string
	BaseURL     string
	Disabled    bool
	ModelCount  int
	AliasCount  int
	LastAttempt string
	LastSuccess string
	LastError   string
}

type managementView struct {
	Configured   bool
	ConfigPath   string
	SyncInterval string
	LastAttempt  string
	LastSuccess  string
	LastError    string
	Providers    []managementProviderView
	Rules        []aliasRule
	RulesJSON    template.JS
	SyncMessage  string
	SyncFailed   bool
}

type saveRulesRequest struct {
	Rules []aliasRule `json:"rules"`
}

var managementTemplate = template.Must(template.New("status").Parse(`<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Provider 模型同步</title>
<style>
:root{color-scheme:dark;font-family:Inter,"Microsoft YaHei",sans-serif;background:#0f1724;color:#e7edf5}*{box-sizing:border-box}body{margin:0;background:#0f1724}main{max-width:1180px;margin:0 auto;padding:24px}header{display:flex;align-items:center;justify-content:space-between;gap:16px;margin-bottom:18px}h1{margin:0;font-size:22px;font-weight:650;letter-spacing:0}h2{font-size:15px;margin:0 0 10px;color:#cbd5e1}button{border:1px solid #3b82f6;background:#2563eb;color:#fff;min-height:36px;padding:0 14px;border-radius:6px;cursor:pointer;font-weight:600}button:hover{background:#1d4ed8}.toolbar{display:flex;gap:8px;flex-wrap:wrap}.notice{padding:10px 13px;margin-bottom:14px;border:1px solid #245b40;background:#10291f;border-radius:6px;color:#a7f3d0}.notice.error{border-color:#7f1d1d;background:#2b1518;color:#fecaca}.summary{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));border:1px solid #283548;border-radius:6px;overflow:hidden;margin-bottom:18px}.metric{min-height:82px;padding:14px 16px;background:#141d2b;border-right:1px solid #283548}.metric:last-child{border-right:0}.label{color:#91a0b5;font-size:12px;margin-bottom:7px}.value{font-size:18px;font-weight:650;overflow-wrap:anywhere}.ok{color:#6ee7a8}.waiting{color:#fbbf62}section{margin-top:18px}.panel{border:1px solid #283548;background:#141d2b;border-radius:6px;overflow:auto}table{width:100%;border-collapse:collapse;min-width:720px}th,td{padding:10px 12px;border-bottom:1px solid #283548;text-align:left;vertical-align:top;overflow-wrap:anywhere}th{color:#91a0b5;font-size:12px;font-weight:600}tr:last-child td{border-bottom:0}code{color:#bfdbfe;font-family:Consolas,monospace}.rules{padding:12px}.rule{display:grid;grid-template-columns:34px 1.2fr 1.2fr 1.2fr 120px 70px;gap:8px;align-items:center;padding:8px 0;border-bottom:1px solid #283548}.rule:last-child{border-bottom:0}.rule input{width:100%;min-height:34px;background:#0f1724;color:#e7edf5;border:1px solid #34445a;border-radius:5px;padding:0 8px}.rule label{font-size:12px;color:#91a0b5}.rule .remove{border-color:#7f1d1d;background:#3a171b;padding:0 8px}.key{min-height:34px;background:#0f1724;color:#e7edf5;border:1px solid #34445a;border-radius:5px;padding:0 8px;min-width:260px}.empty{color:#91a0b5;padding:16px}@media(max-width:800px){main{padding:16px}header{align-items:flex-start;flex-direction:column}.summary{grid-template-columns:repeat(2,minmax(0,1fr))}.metric:nth-child(2){border-right:0}.metric:nth-child(-n+2){border-bottom:1px solid #283548}.rule{grid-template-columns:28px 1fr 1fr}.rule input:nth-of-type(3),.rule input:nth-of-type(4){grid-column:2}.rule .remove{grid-column:2;width:max-content}}
</style></head><body><main>
<header><h1>Provider 模型同步</h1><div class="toolbar"><input id="key" class="key" type="password" placeholder="Management Key"><button type="button" onclick="syncNow()">立即同步</button></div></header>
{{if .SyncMessage}}<div class="notice{{if .SyncFailed}} error{{end}}">{{.SyncMessage}}</div>{{end}}
<div class="summary"><div class="metric"><div class="label">状态</div><div class="value {{if .Configured}}ok{{else}}waiting{{end}}">{{if .Configured}}已配置{{else}}不可用{{end}}</div></div><div class="metric"><div class="label">Provider</div><div class="value">{{len .Providers}}</div></div><div class="metric"><div class="label">最近成功</div><div class="value">{{.LastSuccess}}</div></div><div class="metric"><div class="label">规则</div><div class="value">{{len .Rules}}</div></div></div>
<section><h2>Provider 状态</h2><div class="panel"><table><tr><th>Provider</th><th>模型接口</th><th>状态</th><th>模型</th><th>别名</th><th>最近错误</th></tr>{{range .Providers}}<tr><td>{{.Name}}</td><td><code>{{.BaseURL}}</code></td><td>{{if .Disabled}}已禁用{{else if .LastError}}失败{{else}}已启用{{end}}</td><td>{{.ModelCount}}</td><td>{{.AliasCount}}</td><td>{{if .LastError}}{{.LastError}}{{else}}-{{end}}</td></tr>{{else}}<tr><td colspan="6" class="empty">没有找到 openai-compatibility 配置</td></tr>{{end}}</table></div></section>
<section><h2>别名规则</h2><div class="notice">内置规则：统一小写、空白转为连字符、去除末尾日期后缀。下面规则用于额外覆盖或补充。</div><div class="panel rules"><div id="rules"></div><div class="toolbar" style="margin-top:12px"><button type="button" onclick="addRule()">新增规则</button><button type="button" onclick="saveRules()">保存规则</button></div></div></section>
<section><h2>配置文件</h2><div class="panel" style="padding:12px"><code>{{.ConfigPath}}</code><br><span style="color:#91a0b5">同步周期：{{.SyncInterval}}　最近尝试：{{.LastAttempt}}　错误：{{if .LastError}}{{.LastError}}{{else}}-{{end}}</span></div></section>
</main><script>
const initialRules={{.RulesJSON}};
function render(){const root=document.getElementById('rules');root.innerHTML='';initialRules.forEach((r,i)=>{const row=document.createElement('div');row.className='rule';row.innerHTML='<input type="checkbox" data-field="enabled" '+(r.enabled===false?'':'checked')+'><input data-field="provider_pattern" placeholder="Provider 正则" value="'+esc(r.provider_pattern||'')+'"><input data-field="model_pattern" placeholder="模型正则" value="'+esc(r.model_pattern||'')+'"><input data-field="alias_replacement" placeholder="别名替换" value="'+esc(r.alias_replacement||'')+'"><label><input type="checkbox" data-field="force_mapping" '+(r.force_mapping?'checked':'')+'> 强制回源</label><button type="button" class="remove" onclick="this.parentElement.remove()">删除</button>';root.appendChild(row)})}
function esc(v){return String(v).replaceAll('&','&amp;').replaceAll('"','&quot;').replaceAll('<','&lt;').replaceAll('>','&gt;')}
function addRule(){const root=document.getElementById('rules');const row=document.createElement('div');row.className='rule';row.innerHTML='<input type="checkbox" data-field="enabled" checked><input data-field="provider_pattern" placeholder="Provider 正则"><input data-field="model_pattern" placeholder="模型正则"><input data-field="alias_replacement" placeholder="别名替换"><label><input type="checkbox" data-field="force_mapping"> 强制回源</label><button type="button" class="remove" onclick="this.parentElement.remove()">删除</button>';root.appendChild(row)}
function collect(){return [...document.querySelectorAll('.rule')].map(row=>{const out={};row.querySelectorAll('[data-field]').forEach(el=>{const k=el.dataset.field;if(el.type==='checkbox'){if(k==='enabled'&&el.checked===false)out[k]=false;else if(k==='force_mapping')out[k]=el.checked}else out[k]=el.value});return out})}
async function call(path,body){const key=document.getElementById('key').value;const headers={'Content-Type':'application/json'};if(key){headers.Authorization='Bearer '+key;headers['X-Management-Key']=key}const r=await fetch('/v0/management/provider-model-sync/'+path,{method:'POST',headers,body:JSON.stringify(body||{})});if(!r.ok)throw new Error('HTTP '+r.status);return r.json()}
async function saveRules(){try{await call('rules',{rules:collect()});location.reload()}catch(e){alert('保存失败：'+e.message)}}
async function syncNow(){try{await call('sync');location.reload()}catch(e){alert('同步失败：'+e.message)}}
render();
</script></body></html>`))

func handleManagement(request pluginapi.ManagementRequest) pluginapi.ManagementResponse {
	if request.Method == http.MethodPost && strings.HasSuffix(request.Path, "/rules") {
		return saveRules(request.Body)
	}
	if request.Method == http.MethodPost && strings.HasSuffix(request.Path, "/sync") {
		if errSync := pluginRuntime.SyncNow(context.Background()); errSync != nil {
			return jsonManagement(http.StatusBadGateway, map[string]string{"error": errSync.Error()})
		}
		return jsonManagement(http.StatusOK, map[string]bool{"ok": true})
	}
	view := pluginRuntime.ManagementView()
	var body bytes.Buffer
	if errExecute := managementTemplate.Execute(&body, view); errExecute != nil {
		return pluginapi.ManagementResponse{StatusCode: http.StatusInternalServerError, Headers: http.Header{"Content-Type": []string{"text/plain; charset=utf-8"}}, Body: []byte(errExecute.Error())}
	}
	return pluginapi.ManagementResponse{StatusCode: http.StatusOK, Headers: http.Header{"Content-Type": []string{"text/html; charset=utf-8"}}, Body: body.Bytes()}
}

func saveRules(raw []byte) pluginapi.ManagementResponse {
	var request saveRulesRequest
	if errDecode := json.Unmarshal(raw, &request); errDecode != nil {
		return jsonManagement(http.StatusBadRequest, map[string]string{"error": errDecode.Error()})
	}
	state := pluginRuntime
	state.mu.RLock()
	config := state.config
	state.mu.RUnlock()
	if !state.configured {
		return jsonManagement(http.StatusConflict, map[string]string{"error": "plugin configuration is not ready"})
	}
	if _, errCompile := compileConfig(pluginConfig{AliasRules: request.Rules, SyncIntervalSeconds: int(config.SyncInterval / time.Second), RequestTimeoutSeconds: int(config.RequestTimeout / time.Second)}, config.ConfigPath); errCompile != nil {
		return jsonManagement(http.StatusBadRequest, map[string]string{"error": errCompile.Error()})
	}
	original, root, _, errLoad := loadHostConfig(config.ConfigPath)
	if errLoad != nil {
		return jsonManagement(http.StatusBadGateway, map[string]string{"error": errLoad.Error()})
	}
	pluginNode := documentMapping(root)
	pluginsNode := mappingValue(pluginNode, "plugins")
	configsNode := mappingValue(pluginsNode, "configs")
	if configsNode == nil || configsNode.Kind != yaml.MappingNode {
		return jsonManagement(http.StatusBadGateway, map[string]string{"error": "plugins.configs is missing"})
	}
	configNode := mappingValue(configsNode, "provider-model-sync")
	if configNode == nil || configNode.Kind != yaml.MappingNode {
		configNode = &yaml.Node{Kind: yaml.MappingNode, Tag: "!!map"}
		setMappingValue(configsNode, "provider-model-sync", configNode)
	}
	rulesNode := &yaml.Node{}
	if errEncode := rulesNode.Encode(request.Rules); errEncode != nil {
		return jsonManagement(http.StatusBadRequest, map[string]string{"error": errEncode.Error()})
	}
	setMappingValue(configNode, "alias_rules", rulesNode)
	updated, errEncode := encodeYAML(root)
	if errEncode != nil {
		return jsonManagement(http.StatusBadGateway, map[string]string{"error": errEncode.Error()})
	}
	if errWrite := writeConfigAtomically(config.ConfigPath, original, updated); errWrite != nil {
		return jsonManagement(http.StatusConflict, map[string]string{"error": errWrite.Error()})
	}
	if errRestart := requestHostRestart(config.ConfigPath); errRestart != nil {
		return jsonManagement(http.StatusConflict, map[string]string{"error": errRestart.Error()})
	}
	return jsonManagement(http.StatusOK, map[string]bool{"ok": true})
}

func jsonManagement(status int, value any) pluginapi.ManagementResponse {
	body, _ := json.Marshal(value)
	return pluginapi.ManagementResponse{StatusCode: status, Headers: http.Header{"Content-Type": []string{"application/json"}}, Body: body}
}

func (state *runtimeState) ManagementView() managementView {
	state.mu.RLock()
	defer state.mu.RUnlock()
	providers := make([]managementProviderView, 0, len(state.providers))
	for _, status := range state.providers {
		providers = append(providers, managementProviderView{Name: status.Name, BaseURL: status.BaseURL, Disabled: status.Disabled, ModelCount: status.ModelCount, AliasCount: status.AliasCount, LastAttempt: timeText(status.LastAttempt), LastSuccess: timeText(status.LastSuccess), LastError: status.LastError})
	}
	sort.Slice(providers, func(left int, right int) bool { return providers[left].Name < providers[right].Name })
	rules := cloneAliasRules(state.config.RawAliasRules)
	rulesJSON, _ := json.Marshal(rules)
	return managementView{Configured: state.configured, ConfigPath: state.config.ConfigPath, SyncInterval: state.config.SyncInterval.String(), LastAttempt: timeText(state.lastAttempt), LastSuccess: timeText(state.lastSuccess), LastError: state.lastError, Providers: providers, Rules: rules, RulesJSON: template.JS(rulesJSON)}
}

func timeText(value time.Time) string {
	if value.IsZero() {
		return "-"
	}
	return value.Local().Format("2006-01-02 15:04:05")
}
