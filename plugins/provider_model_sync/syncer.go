package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"sync"
	"time"

	"gopkg.in/yaml.v3"
)

const (
	defaultSyncInterval   = 5 * time.Minute
	minimumSyncInterval   = 10 * time.Second
	defaultRequestTimeout = 15 * time.Second
	maximumResponseSize   = 16 << 20
	maxProviderWorkers    = 4
)

type pluginConfig struct {
	SyncIntervalSeconds   int         `yaml:"sync_interval_seconds" json:"sync_interval_seconds"`
	RequestTimeoutSeconds int         `yaml:"request_timeout_seconds" json:"request_timeout_seconds"`
	IncludeDisabled       bool        `yaml:"include_disabled" json:"include_disabled"`
	RemoveMissingModels   bool        `yaml:"remove_missing_models" json:"remove_missing_models"`
	AliasRules            []aliasRule `yaml:"alias_rules" json:"alias_rules"`
}

type aliasRule struct {
	Enabled          *bool  `yaml:"enabled" json:"enabled,omitempty"`
	ProviderPattern  string `yaml:"provider_pattern" json:"provider_pattern"`
	ModelPattern     string `yaml:"model_pattern" json:"model_pattern"`
	AliasReplacement string `yaml:"alias_replacement" json:"alias_replacement"`
	ForceMapping     bool   `yaml:"force_mapping" json:"force_mapping"`
}

type compiledConfig struct {
	ConfigPath          string
	SyncInterval        time.Duration
	RequestTimeout      time.Duration
	IncludeDisabled     bool
	RemoveMissingModels bool
	AliasRules          []compiledAliasRule
	RawAliasRules       []aliasRule
}

type compiledAliasRule struct {
	ProviderPattern  *regexp.Regexp
	ModelPattern     *regexp.Regexp
	AliasReplacement string
	ForceMapping     bool
}

type providerDefinition struct {
	Index      int
	Name       string
	BaseURL    string
	APIKeys    []string
	Headers    map[string]string
	Disabled   bool
	ModelsNode *yaml.Node
}

type providerSyncResult struct {
	Provider providerDefinition
	Models   []upstreamModel
	Error    error
}

type providerStatus struct {
	Name        string
	BaseURL     string
	Disabled    bool
	ModelCount  int
	AliasCount  int
	LastAttempt time.Time
	LastSuccess time.Time
	LastError   string
}

type upstreamModelsResponse struct {
	Data []upstreamModel `json:"data"`
}

type upstreamModel struct {
	ID string `json:"id"`
}

type runtimeState struct {
	mu          sync.RWMutex
	syncMu      sync.Mutex
	config      compiledConfig
	configured  bool
	providers   map[string]providerStatus
	lastAttempt time.Time
	lastSuccess time.Time
	lastError   string
	cancel      context.CancelFunc
	done        chan struct{}
}

var pluginRuntime = &runtimeState{providers: map[string]providerStatus{}}

var errConfigPathNotFound = errors.New("CLIProxyAPI --config path was not found")

var builtinDateSuffixPattern = regexp.MustCompile(`-(20[0-9]{2}[-_]?[0-9]{2}[-_]?[0-9]{2}|[0-9]{8}|[0-9]{4})$`)

var builtinRepeatedHyphenPattern = regexp.MustCompile(`-+`)

var hostLog = func(level string, message string, fields map[string]any) {
	_ = level
	_ = message
	_ = fields
}

func decodeConfig(raw []byte) (compiledConfig, error) {
	var config pluginConfig
	if len(raw) > 0 {
		if errUnmarshal := yaml.Unmarshal(raw, &config); errUnmarshal != nil {
			return compiledConfig{}, fmt.Errorf("decode plugin config: %w", errUnmarshal)
		}
	}
	configPath, errPath := discoverConfigPath(os.Args)
	if errPath != nil {
		return compiledConfig{}, errPath
	}
	return compileConfig(config, configPath)
}

func discoverConfigPath(args []string) (string, error) {
	for index := 0; index < len(args); index++ {
		argument := strings.TrimSpace(args[index])
		var value string
		switch {
		case argument == "--config" && index+1 < len(args):
			value = args[index+1]
		case strings.HasPrefix(argument, "--config="):
			value = strings.TrimPrefix(argument, "--config=")
		}
		value = strings.Trim(strings.TrimSpace(value), "\"")
		if value == "" {
			continue
		}
		absolutePath, errAbsolute := filepath.Abs(value)
		if errAbsolute != nil {
			return "", fmt.Errorf("resolve config path: %w", errAbsolute)
		}
		return filepath.Clean(absolutePath), nil
	}
	return "", errConfigPathNotFound
}

func compileConfig(config pluginConfig, configPath string) (compiledConfig, error) {
	if strings.TrimSpace(configPath) == "" {
		return compiledConfig{}, errConfigPathNotFound
	}
	syncInterval := defaultSyncInterval
	if config.SyncIntervalSeconds != 0 {
		syncInterval = time.Duration(config.SyncIntervalSeconds) * time.Second
	}
	if syncInterval < minimumSyncInterval {
		return compiledConfig{}, fmt.Errorf("sync_interval_seconds must be at least %d", int(minimumSyncInterval/time.Second))
	}
	requestTimeout := defaultRequestTimeout
	if config.RequestTimeoutSeconds != 0 {
		requestTimeout = time.Duration(config.RequestTimeoutSeconds) * time.Second
	}
	if requestTimeout <= 0 || requestTimeout > 2*time.Minute {
		return compiledConfig{}, errors.New("request_timeout_seconds must be between 1 and 120")
	}
	rules := make([]compiledAliasRule, 0, len(config.AliasRules))
	for index, rule := range config.AliasRules {
		if rule.Enabled != nil && !*rule.Enabled {
			continue
		}
		providerPatternText := strings.TrimSpace(rule.ProviderPattern)
		if providerPatternText == "" {
			providerPatternText = ".*"
		}
		modelPatternText := strings.TrimSpace(rule.ModelPattern)
		if modelPatternText == "" {
			return compiledConfig{}, fmt.Errorf("alias_rules[%d].model_pattern is required", index)
		}
		providerPattern, errProvider := regexp.Compile(providerPatternText)
		if errProvider != nil {
			return compiledConfig{}, fmt.Errorf("compile alias_rules[%d].provider_pattern: %w", index, errProvider)
		}
		modelPattern, errModel := regexp.Compile(modelPatternText)
		if errModel != nil {
			return compiledConfig{}, fmt.Errorf("compile alias_rules[%d].model_pattern: %w", index, errModel)
		}
		rules = append(rules, compiledAliasRule{
			ProviderPattern: providerPattern, ModelPattern: modelPattern,
			AliasReplacement: rule.AliasReplacement, ForceMapping: rule.ForceMapping,
		})
	}
	return compiledConfig{
		ConfigPath: configPath, SyncInterval: syncInterval, RequestTimeout: requestTimeout,
		IncludeDisabled: config.IncludeDisabled, RemoveMissingModels: config.RemoveMissingModels,
		AliasRules: rules, RawAliasRules: cloneAliasRules(config.AliasRules),
	}, nil
}

func (state *runtimeState) Reconfigure(config compiledConfig) error {
	state.stopWorker()
	if _, errStat := os.Stat(config.ConfigPath); errStat != nil {
		return fmt.Errorf("access CLIProxyAPI config: %w", errStat)
	}
	state.mu.Lock()
	state.config = config
	state.configured = true
	state.lastError = ""
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	state.cancel = cancel
	state.done = done
	state.mu.Unlock()
	go state.run(ctx, done, config.SyncInterval)
	return nil
}

func (state *runtimeState) SetConfigurationError(configError error) {
	state.stopWorker()
	state.mu.Lock()
	state.configured = false
	state.lastAttempt = time.Now().UTC()
	state.lastError = configError.Error()
	state.mu.Unlock()
}

func (state *runtimeState) Shutdown() {
	state.stopWorker()
}

func (state *runtimeState) stopWorker() {
	state.mu.Lock()
	cancel := state.cancel
	done := state.done
	state.cancel = nil
	state.done = nil
	state.mu.Unlock()
	if cancel != nil {
		cancel()
	}
	if done != nil {
		<-done
	}
}

func (state *runtimeState) run(ctx context.Context, done chan struct{}, interval time.Duration) {
	defer close(done)
	if errSync := state.syncOnce(ctx); errSync != nil && !errors.Is(errSync, context.Canceled) {
		hostLog("warn", "OpenAI-compatible model sync failed", map[string]any{"error": errSync.Error()})
	}
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			if errSync := state.syncOnce(ctx); errSync != nil && !errors.Is(errSync, context.Canceled) {
				hostLog("warn", "OpenAI-compatible model sync failed", map[string]any{"error": errSync.Error()})
			}
		}
	}
}

func (state *runtimeState) SyncNow(ctx context.Context) error {
	state.mu.RLock()
	configured := state.configured
	timeout := state.config.RequestTimeout
	state.mu.RUnlock()
	if !configured {
		return errors.New("plugin configuration is not ready")
	}
	if timeout <= 0 {
		timeout = defaultRequestTimeout
	}
	syncContext, cancel := context.WithTimeout(ctx, timeout*time.Duration(maxProviderWorkers+1))
	defer cancel()
	return state.syncOnce(syncContext)
}

func (state *runtimeState) syncOnce(ctx context.Context) error {
	state.syncMu.Lock()
	defer state.syncMu.Unlock()
	if errContext := ctx.Err(); errContext != nil {
		return errContext
	}
	state.mu.RLock()
	config := state.config
	state.mu.RUnlock()
	now := time.Now().UTC()
	state.mu.Lock()
	state.lastAttempt = now
	state.mu.Unlock()

	original, root, providers, errLoad := loadHostConfig(config.ConfigPath)
	if errLoad != nil {
		state.recordGlobalError(errLoad)
		return errLoad
	}
	state.seedProviderStatuses(providers, now)
	results := syncProviders(ctx, config, providers)
	changed := false
	succeeded := 0
	errorsByProvider := make([]string, 0)
	for _, result := range results {
		if result.Error != nil {
			state.recordProviderError(result.Provider, result.Error, now)
			errorsByProvider = append(errorsByProvider, result.Provider.Name+": "+result.Error.Error())
			continue
		}
		providerChanged, modelCount, aliasCount := mergeProviderModels(result.Provider, result.Models, config)
		changed = changed || providerChanged
		succeeded++
		state.recordProviderSuccess(result.Provider, modelCount, aliasCount, now)
	}
	if changed {
		updated, errEncode := encodeYAML(root)
		if errEncode != nil {
			state.recordGlobalError(errEncode)
			return errEncode
		}
		if errWrite := writeConfigAtomically(config.ConfigPath, original, updated); errWrite != nil {
			state.recordGlobalError(errWrite)
			return errWrite
		}
		if errRestart := requestHostRestart(config.ConfigPath); errRestart != nil {
			state.recordGlobalError(errRestart)
			return errRestart
		}
	}
	state.mu.Lock()
	if succeeded > 0 {
		state.lastSuccess = now
	}
	if len(errorsByProvider) == 0 {
		state.lastError = ""
	} else {
		state.lastError = strings.Join(errorsByProvider, "; ")
	}
	state.mu.Unlock()
	if succeeded == 0 && len(results) > 0 {
		return errors.New("all Provider model syncs failed")
	}
	return nil
}

// requestHostRestart asks CPA Manager Native to restart the managed
// CLIProxyAPI process after the configuration has been written. The manager
// owns the process lifecycle; the plugin never terminates or replaces it.
func requestHostRestart(configPath string) error {
	markerPath := filepath.Join(filepath.Dir(configPath), ".provider-model-sync.restart")
	temporaryPath := markerPath + ".tmp"
	contents := []byte(time.Now().UTC().Format(time.RFC3339Nano) + "\n")
	if errWrite := os.WriteFile(temporaryPath, contents, 0o600); errWrite != nil {
		return fmt.Errorf("create CPA restart request: %w", errWrite)
	}
	if errRename := os.Rename(temporaryPath, markerPath); errRename != nil {
		_ = os.Remove(temporaryPath)
		return fmt.Errorf("publish CPA restart request: %w", errRename)
	}
	return nil
}

func loadHostConfig(path string) ([]byte, *yaml.Node, []providerDefinition, error) {
	raw, errRead := os.ReadFile(path)
	if errRead != nil {
		return nil, nil, nil, fmt.Errorf("read CLIProxyAPI config: %w", errRead)
	}
	var root yaml.Node
	if errUnmarshal := yaml.Unmarshal(raw, &root); errUnmarshal != nil {
		return nil, nil, nil, fmt.Errorf("decode CLIProxyAPI config: %w", errUnmarshal)
	}
	providersNode := mappingValue(documentMapping(&root), "openai-compatibility")
	if providersNode == nil || providersNode.Kind != yaml.SequenceNode {
		return raw, &root, nil, errors.New("openai-compatibility must be a YAML sequence")
	}
	providers := make([]providerDefinition, 0, len(providersNode.Content))
	for index, node := range providersNode.Content {
		if node.Kind != yaml.MappingNode {
			continue
		}
		provider := providerDefinition{
			Index: index, Name: scalarValue(mappingValue(node, "name")),
			BaseURL:    scalarValue(mappingValue(node, "base-url")),
			Disabled:   boolValue(mappingValue(node, "disabled")),
			Headers:    stringMap(mappingValue(node, "headers")),
			ModelsNode: mappingValue(node, "models"),
		}
		provider.APIKeys = apiKeys(mappingValue(node, "api-key-entries"))
		if provider.ModelsNode == nil {
			provider.ModelsNode = &yaml.Node{Kind: yaml.SequenceNode, Tag: "!!seq"}
			setMappingValue(node, "models", provider.ModelsNode)
		}
		providers = append(providers, provider)
	}
	return raw, &root, providers, nil
}

func syncProviders(ctx context.Context, config compiledConfig, providers []providerDefinition) []providerSyncResult {
	tasks := make(chan providerDefinition)
	results := make(chan providerSyncResult, len(providers))
	workers := maxProviderWorkers
	if len(providers) < workers {
		workers = len(providers)
	}
	var waitGroup sync.WaitGroup
	for worker := 0; worker < workers; worker++ {
		waitGroup.Add(1)
		go func() {
			defer waitGroup.Done()
			for provider := range tasks {
				models, errFetch := fetchProviderModels(ctx, config.RequestTimeout, provider)
				results <- providerSyncResult{Provider: provider, Models: models, Error: errFetch}
			}
		}()
	}
	go func() {
		defer close(tasks)
		for _, provider := range providers {
			if provider.Disabled && !config.IncludeDisabled {
				continue
			}
			select {
			case <-ctx.Done():
				return
			case tasks <- provider:
			}
		}
	}()
	waitGroup.Wait()
	close(results)
	out := make([]providerSyncResult, 0, len(providers))
	for result := range results {
		out = append(out, result)
	}
	sort.Slice(out, func(left int, right int) bool { return out[left].Provider.Index < out[right].Provider.Index })
	return out
}

func fetchProviderModels(ctx context.Context, timeout time.Duration, provider providerDefinition) ([]upstreamModel, error) {
	name := strings.TrimSpace(provider.Name)
	baseURL := strings.TrimRight(strings.TrimSpace(provider.BaseURL), "/")
	if name == "" || baseURL == "" {
		return nil, errors.New("name and base-url are required")
	}
	modelsURL := baseURL + "/models"
	parsedURL, errParse := url.Parse(modelsURL)
	if errParse != nil || (parsedURL.Scheme != "http" && parsedURL.Scheme != "https") || parsedURL.Host == "" {
		return nil, fmt.Errorf("invalid models URL %q", modelsURL)
	}
	keys := provider.APIKeys
	if len(keys) == 0 {
		keys = []string{""}
	}
	var lastError error
	for _, apiKey := range keys {
		models, statusCode, errFetch := fetchModelsWithKey(ctx, timeout, modelsURL, apiKey, provider.Headers)
		if errFetch == nil {
			return models, nil
		}
		lastError = errFetch
		if statusCode != http.StatusUnauthorized && statusCode != http.StatusForbidden {
			break
		}
	}
	return nil, lastError
}

func fetchModelsWithKey(ctx context.Context, timeout time.Duration, modelsURL string, apiKey string, headers map[string]string) ([]upstreamModel, int, error) {
	request, errRequest := http.NewRequestWithContext(ctx, http.MethodGet, modelsURL, nil)
	if errRequest != nil {
		return nil, 0, fmt.Errorf("create models request: %w", errRequest)
	}
	request.Header.Set("Accept", "application/json")
	for key, value := range headers {
		request.Header.Set(key, value)
	}
	if strings.TrimSpace(apiKey) != "" {
		request.Header.Set("Authorization", "Bearer "+apiKey)
	}
	client := &http.Client{Timeout: timeout}
	response, errDo := client.Do(request)
	if errDo != nil {
		return nil, 0, fmt.Errorf("request models: %w", errDo)
	}
	defer response.Body.Close()
	body, errRead := io.ReadAll(io.LimitReader(response.Body, maximumResponseSize+1))
	if errRead != nil {
		return nil, response.StatusCode, fmt.Errorf("read models: %w", errRead)
	}
	if len(body) > maximumResponseSize {
		return nil, response.StatusCode, fmt.Errorf("models response exceeds %d bytes", maximumResponseSize)
	}
	if response.StatusCode < http.StatusOK || response.StatusCode >= http.StatusMultipleChoices {
		return nil, response.StatusCode, fmt.Errorf("models endpoint returned HTTP %d: %s", response.StatusCode, truncate(string(body), 256))
	}
	var wrapped upstreamModelsResponse
	if errUnmarshal := json.Unmarshal(body, &wrapped); errUnmarshal == nil && wrapped.Data != nil {
		return normalizeUpstreamModels(wrapped.Data), response.StatusCode, nil
	}
	var bare []upstreamModel
	if errUnmarshal := json.Unmarshal(body, &bare); errUnmarshal != nil {
		return nil, response.StatusCode, fmt.Errorf("decode models response: %w", errUnmarshal)
	}
	return normalizeUpstreamModels(bare), response.StatusCode, nil
}

func normalizeUpstreamModels(models []upstreamModel) []upstreamModel {
	seen := make(map[string]struct{}, len(models))
	out := make([]upstreamModel, 0, len(models))
	for _, model := range models {
		model.ID = strings.TrimSpace(model.ID)
		key := strings.ToLower(model.ID)
		if model.ID == "" {
			continue
		}
		if _, exists := seen[key]; exists {
			continue
		}
		seen[key] = struct{}{}
		out = append(out, model)
	}
	return out
}

func mergeProviderModels(provider providerDefinition, upstream []upstreamModel, config compiledConfig) (bool, int, int) {
	modelsNode := provider.ModelsNode
	if modelsNode.Kind != yaml.SequenceNode {
		modelsNode.Kind = yaml.SequenceNode
		modelsNode.Tag = "!!seq"
		modelsNode.Content = nil
	}
	before, _ := yaml.Marshal(modelsNode)
	existing := make(map[string]*yaml.Node, len(modelsNode.Content))
	for _, modelNode := range modelsNode.Content {
		name := scalarValue(mappingValue(modelNode, "name"))
		if name != "" {
			existing[strings.ToLower(name)] = modelNode
		}
	}
	seen := make(map[string]struct{}, len(upstream))
	assignedAliases := make(map[string]string, len(upstream))
	next := make([]*yaml.Node, 0, len(upstream)+len(existing))
	for _, modelNode := range modelsNode.Content {
		name := scalarValue(mappingValue(modelNode, "name"))
		alias := scalarValue(mappingValue(modelNode, "alias"))
		if name != "" && alias != "" {
			assignedAliases[strings.ToLower(alias)] = name
		}
	}
	aliasCount := 0
	for _, model := range upstream {
		key := strings.ToLower(model.ID)
		seen[key] = struct{}{}
		modelNode := existing[key]
		if modelNode == nil {
			modelNode = &yaml.Node{Kind: yaml.MappingNode, Tag: "!!map"}
			setMappingScalar(modelNode, "name", model.ID)
		}
		if rule, matched := matchingAliasRule(config.AliasRules, provider.Name, model.ID); matched {
			alias := strings.TrimSpace(rule.ModelPattern.ReplaceAllString(model.ID, rule.AliasReplacement))
			setMappingScalar(modelNode, "alias", alias)
			if alias != "" && rule.ForceMapping {
				setMappingBool(modelNode, "force-mapping", true)
			} else {
				deleteMappingValue(modelNode, "force-mapping")
			}
		} else if currentAlias := scalarValue(mappingValue(modelNode, "alias")); currentAlias != "" {
			if alias := builtinModelAlias(currentAlias); alias != "" {
				setMappingScalar(modelNode, "alias", alias)
			}
		} else {
			if alias := builtinModelAlias(model.ID); alias != "" && !strings.EqualFold(alias, model.ID) {
				aliasKey := strings.ToLower(alias)
				if owner, exists := assignedAliases[aliasKey]; !exists || strings.EqualFold(owner, model.ID) {
					setMappingScalar(modelNode, "alias", alias)
					assignedAliases[aliasKey] = model.ID
				}
			}
		}
		if alias := scalarValue(mappingValue(modelNode, "alias")); alias != "" {
			aliasCount++
			assignedAliases[strings.ToLower(alias)] = model.ID
		}
		next = append(next, modelNode)
	}
	if !config.RemoveMissingModels {
		for _, modelNode := range modelsNode.Content {
			name := scalarValue(mappingValue(modelNode, "name"))
			if _, exists := seen[strings.ToLower(name)]; exists {
				continue
			}
			if scalarValue(mappingValue(modelNode, "alias")) != "" {
				aliasCount++
			}
			next = append(next, modelNode)
		}
	}
	modelsNode.Content = next
	after, _ := yaml.Marshal(modelsNode)
	return !bytes.Equal(before, after), len(next), aliasCount
}

func builtinModelAlias(modelID string) string {
	alias := strings.ToLower(strings.TrimSpace(modelID))
	if slashIndex := strings.LastIndex(alias, "/"); slashIndex >= 0 {
		alias = alias[slashIndex+1:]
	}
	alias = strings.Join(strings.Fields(alias), "-")
	alias = builtinRepeatedHyphenPattern.ReplaceAllString(alias, "-")
	alias = builtinDateSuffixPattern.ReplaceAllString(alias, "")
	return strings.Trim(alias, "-")
}

func matchingAliasRule(rules []compiledAliasRule, provider string, model string) (compiledAliasRule, bool) {
	for _, rule := range rules {
		if rule.ProviderPattern.MatchString(provider) && rule.ModelPattern.MatchString(model) {
			return rule, true
		}
	}
	return compiledAliasRule{}, false
}

func encodeYAML(root *yaml.Node) ([]byte, error) {
	var output bytes.Buffer
	encoder := yaml.NewEncoder(&output)
	encoder.SetIndent(2)
	if errEncode := encoder.Encode(root); errEncode != nil {
		return nil, fmt.Errorf("encode CLIProxyAPI config: %w", errEncode)
	}
	if errClose := encoder.Close(); errClose != nil {
		return nil, fmt.Errorf("close YAML encoder: %w", errClose)
	}
	return output.Bytes(), nil
}

func writeConfigAtomically(path string, expected []byte, updated []byte) error {
	if bytes.Equal(expected, updated) {
		return nil
	}
	current, errRead := os.ReadFile(path)
	if errRead != nil {
		return fmt.Errorf("re-read CLIProxyAPI config: %w", errRead)
	}
	if !bytes.Equal(current, expected) {
		return errors.New("CLIProxyAPI config changed during model sync; retrying on the next cycle")
	}
	info, errStat := os.Stat(path)
	if errStat != nil {
		return fmt.Errorf("stat CLIProxyAPI config: %w", errStat)
	}
	backupPath := path + ".provider-model-sync.bak"
	if errBackup := os.WriteFile(backupPath, expected, info.Mode().Perm()); errBackup != nil {
		return fmt.Errorf("backup CLIProxyAPI config: %w", errBackup)
	}
	tempFile, errCreate := os.CreateTemp(filepath.Dir(path), ".provider-model-sync-*.yaml")
	if errCreate != nil {
		return fmt.Errorf("create temporary config: %w", errCreate)
	}
	tempPath := tempFile.Name()
	defer os.Remove(tempPath)
	if errChmod := tempFile.Chmod(info.Mode().Perm()); errChmod != nil {
		tempFile.Close()
		return fmt.Errorf("set temporary config permissions: %w", errChmod)
	}
	if _, errWrite := tempFile.Write(updated); errWrite != nil {
		tempFile.Close()
		return fmt.Errorf("write temporary config: %w", errWrite)
	}
	if errSync := tempFile.Sync(); errSync != nil {
		tempFile.Close()
		return fmt.Errorf("flush temporary config: %w", errSync)
	}
	if errClose := tempFile.Close(); errClose != nil {
		return fmt.Errorf("close temporary config: %w", errClose)
	}
	if errReplace := replaceFile(tempPath, path); errReplace != nil {
		return fmt.Errorf("replace CLIProxyAPI config: %w", errReplace)
	}
	return nil
}

func (state *runtimeState) seedProviderStatuses(providers []providerDefinition, attempt time.Time) {
	state.mu.Lock()
	defer state.mu.Unlock()
	if state.providers == nil {
		state.providers = map[string]providerStatus{}
	}
	for _, provider := range providers {
		status := state.providers[provider.Name]
		status.Name = provider.Name
		status.BaseURL = provider.BaseURL
		status.Disabled = provider.Disabled
		if !provider.Disabled || state.config.IncludeDisabled {
			status.LastAttempt = attempt
		}
		state.providers[provider.Name] = status
	}
}

func (state *runtimeState) recordProviderSuccess(provider providerDefinition, modelCount int, aliasCount int, now time.Time) {
	state.mu.Lock()
	status := state.providers[provider.Name]
	status.Name = provider.Name
	status.BaseURL = provider.BaseURL
	status.Disabled = provider.Disabled
	status.ModelCount = modelCount
	status.AliasCount = aliasCount
	status.LastAttempt = now
	status.LastSuccess = now
	status.LastError = ""
	state.providers[provider.Name] = status
	state.mu.Unlock()
}

func (state *runtimeState) recordProviderError(provider providerDefinition, syncError error, now time.Time) {
	state.mu.Lock()
	status := state.providers[provider.Name]
	status.Name = provider.Name
	status.BaseURL = provider.BaseURL
	status.Disabled = provider.Disabled
	status.LastAttempt = now
	status.LastError = syncError.Error()
	state.providers[provider.Name] = status
	state.mu.Unlock()
}

func (state *runtimeState) recordGlobalError(syncError error) {
	state.mu.Lock()
	state.lastError = syncError.Error()
	state.mu.Unlock()
}

func documentMapping(root *yaml.Node) *yaml.Node {
	if root == nil {
		return nil
	}
	if root.Kind == yaml.DocumentNode && len(root.Content) > 0 {
		return root.Content[0]
	}
	return root
}

func mappingValue(mapping *yaml.Node, key string) *yaml.Node {
	if mapping == nil || mapping.Kind != yaml.MappingNode {
		return nil
	}
	for index := 0; index+1 < len(mapping.Content); index += 2 {
		if mapping.Content[index].Value == key {
			return mapping.Content[index+1]
		}
	}
	return nil
}

func setMappingValue(mapping *yaml.Node, key string, value *yaml.Node) {
	for index := 0; index+1 < len(mapping.Content); index += 2 {
		if mapping.Content[index].Value == key {
			mapping.Content[index+1] = value
			return
		}
	}
	mapping.Content = append(mapping.Content,
		&yaml.Node{Kind: yaml.ScalarNode, Tag: "!!str", Value: key}, value,
	)
}

func setMappingScalar(mapping *yaml.Node, key string, value string) {
	setMappingValue(mapping, key, &yaml.Node{Kind: yaml.ScalarNode, Tag: "!!str", Value: value})
}

func setMappingBool(mapping *yaml.Node, key string, value bool) {
	text := "false"
	if value {
		text = "true"
	}
	setMappingValue(mapping, key, &yaml.Node{Kind: yaml.ScalarNode, Tag: "!!bool", Value: text})
}

func deleteMappingValue(mapping *yaml.Node, key string) {
	for index := 0; index+1 < len(mapping.Content); index += 2 {
		if mapping.Content[index].Value == key {
			mapping.Content = append(mapping.Content[:index], mapping.Content[index+2:]...)
			return
		}
	}
}

func scalarValue(node *yaml.Node) string {
	if node == nil {
		return ""
	}
	return strings.TrimSpace(node.Value)
}

func boolValue(node *yaml.Node) bool {
	return node != nil && strings.EqualFold(strings.TrimSpace(node.Value), "true")
}

func stringMap(node *yaml.Node) map[string]string {
	out := map[string]string{}
	if node == nil || node.Kind != yaml.MappingNode {
		return out
	}
	for index := 0; index+1 < len(node.Content); index += 2 {
		out[node.Content[index].Value] = node.Content[index+1].Value
	}
	return out
}

func apiKeys(node *yaml.Node) []string {
	if node == nil || node.Kind != yaml.SequenceNode {
		return nil
	}
	keys := make([]string, 0, len(node.Content))
	for _, entry := range node.Content {
		key := scalarValue(mappingValue(entry, "api-key"))
		if key != "" {
			keys = append(keys, key)
		}
	}
	return keys
}

func cloneAliasRules(rules []aliasRule) []aliasRule {
	out := make([]aliasRule, len(rules))
	copy(out, rules)
	for index := range out {
		if rules[index].Enabled != nil {
			enabled := *rules[index].Enabled
			out[index].Enabled = &enabled
		}
	}
	return out
}

func truncate(value string, limit int) string {
	if len(value) <= limit {
		return value
	}
	return value[:limit]
}
