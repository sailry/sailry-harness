package telemetry

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net"
	"net/http"
	"slices"
	"sort"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Anonymous counters are aggregated locally in one bounded window and sent
// fifteen minutes after its first observation. Only these content-free
// dimensions may enter a payload.
const (
	metricsEndpoint = "https://api.telemetry.sh/log"
	metricsPeriod   = 15 * time.Minute
	metricsGroups   = 256
	metricsAttempts = 4
)

var metricTables = []string{"app_launches", "turns", "tool_calls", "model_requests", "search_requests"}

// Tokens are a model request's reported usage.
type Tokens struct {
	Input      uint64 `json:"input"`
	Output     uint64 `json:"output"`
	CacheRead  uint64 `json:"cache_read"`
	CacheWrite uint64 `json:"cache_write"`
}

// Observation is one counted fact.
type Observation struct {
	table      string
	dimensions any
	elapsed    *time.Duration
	peak       bool
	tokens     *Tokens
}

// Tool names a called tool without its arguments: a built-in by name, a
// connected server only as "mcp", any package only as "third_party".
type Tool struct {
	Source string `json:"source"`
	Name   string `json:"name,omitempty"`
}

var builtinTools = []string{"ask_user", "plan_review", "list_sessions", "read_session", "search_session", "read_tool_output", "load_skill", "read_skill_resource", "tool_search", "manage_plugins", "manage_mcp", "manage_skills"}

// CaptureTool classifies a tool by its name alone. A workspace tool that is
// neither listed, a package's nor a connected server's is not counted.
func CaptureTool(name string) (Tool, bool) {
	switch {
	case strings.HasPrefix(name, "mcp_"):
		return Tool{Source: "mcp"}, true
	case slices.Contains(builtinTools, name):
		return Tool{Source: "builtin", Name: name}, true
	case strings.HasPrefix(name, "plugin_"):
		// The alias of a package's own tool. Its package stays unnamed.
		return Tool{Source: "third_party"}, true
	}
	return Tool{}, false
}

// Model is a request's interface, authentication and model. A model outside
// the built-in presets' static lists is "custom".
type Model struct {
	API            protocol.ConversationModelApi `json:"api"`
	Authentication protocol.Authentication       `json:"authentication"`
	Model          string                        `json:"model"`
}

// Model captures a provider's model for counting.
func (c *Control) Model(provider protocol.ConversationProvider, model string) Model {
	name := "custom"
	if c != nil && provider.Preset != nil && provider.Preset.Source.Kind == "builtin" && slices.Contains(c.models[provider.Preset.ID], model) {
		name = model
	}
	return Model{API: provider.Api, Authentication: provider.Authentication, Model: name}
}

// Launch counts the process's start.
func Launch() Observation { return Observation{table: "app_launches", dimensions: struct{}{}} }

// Turn counts a finished task turn. result is completed, failed or cancelled.
func Turn(result string, mode protocol.WorkMode, subagent bool, fault *protocol.ErrorCode) Observation {
	return Observation{table: "turns", dimensions: struct {
		Result    string              `json:"result"`
		Mode      protocol.WorkMode   `json:"mode"`
		Subagent  bool                `json:"subagent"`
		ErrorCode *protocol.ErrorCode `json:"error_code"`
	}{result, mode, subagent, fault}}
}

// ToolCall counts a tool call. result is success, error, denied or cancelled.
func ToolCall(tool Tool, result string, fault *protocol.ErrorCode, elapsed time.Duration) Observation {
	return Observation{table: "tool_calls", elapsed: &elapsed, peak: true, dimensions: struct {
		Tool      Tool                `json:"tool"`
		Result    string              `json:"result"`
		ErrorCode *protocol.ErrorCode `json:"error_code"`
	}{tool, result, fault}}
}

// ModelRequest counts a model request. result is success, error or cancelled.
func ModelRequest(model Model, result string, status *uint16, fault *protocol.ErrorCode, retries *uint64, tokens Tokens) Observation {
	class := "unknown"
	if status != nil && *status/100 >= 1 && *status/100 <= 5 {
		class = string(rune('0'+*status/100)) + "xx"
	}
	return Observation{table: "model_requests", tokens: &tokens, dimensions: struct {
		Model
		Result          string              `json:"result"`
		HTTPStatusClass string              `json:"http_status_class"`
		ErrorCode       *protocol.ErrorCode `json:"error_code"`
		Retries         *uint64             `json:"retries"`
	}{model, result, class, fault, retries}}
}

// Search counts a web search. result is success, error or cancelled.
func Search(driver *protocol.SearchDriver, result string, fault *protocol.ErrorCode, elapsed time.Duration) Observation {
	return Observation{table: "search_requests", elapsed: &elapsed, dimensions: struct {
		Driver    *protocol.SearchDriver `json:"driver"`
		Result    string                 `json:"result"`
		ErrorCode *protocol.ErrorCode    `json:"error_code"`
	}{driver, result, fault}}
}

type counts struct {
	dimensions json.RawMessage
	count      uint64
	total, max *uint64
	tokens     *Tokens
}

// window is one consent epoch's counters. Identities never survive it.
type window struct {
	started time.Time
	common  map[string]any
	groups  map[string]*counts // by table and dimensions
}

type batch struct {
	Table string           `json:"table"`
	Data  []map[string]any `json:"data"`
}

func (c *Control) clearWindowLocked() {
	if c.stop != nil {
		c.stop()
	}
	c.window, c.stop = nil, nil
}

// takeWindowLocked ends the window and returns its rows by table.
func (c *Control) takeWindowLocked() ([]batch, uint64) {
	w := c.window
	c.clearWindowLocked()
	if w == nil {
		return nil, c.epoch
	}
	ended := time.Now().UTC()
	if ended.Before(w.started) {
		ended = w.started
	}
	keys := make([]string, 0, len(w.groups))
	for key := range w.groups {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	var batches []batch
	for _, table := range metricTables {
		var rows []map[string]any
		for _, key := range keys {
			if !strings.HasPrefix(key, table+"\x00") {
				continue
			}
			group := w.groups[key]
			row := map[string]any{"event_id": uuid.NewString(), "window_start": w.started.Format(time.RFC3339Nano), "window_end": ended.Format(time.RFC3339Nano), "count": group.count}
			for name, value := range w.common {
				row[name] = value
			}
			var dimensions map[string]any
			if json.Unmarshal(group.dimensions, &dimensions) != nil {
				continue
			}
			for name, value := range dimensions {
				row[name] = value
			}
			if group.total != nil {
				row["elapsed_us_total"] = *group.total
			}
			if group.max != nil {
				row["elapsed_us_max"] = *group.max
			}
			if group.tokens != nil {
				row["input"], row["output"], row["cache_read"], row["cache_write"] = group.tokens.Input, group.tokens.Output, group.tokens.CacheRead, group.tokens.CacheWrite
			}
			rows = append(rows, row)
		}
		if rows != nil {
			batches = append(batches, batch{Table: table, Data: rows})
		}
	}
	return batches, c.epoch
}

func saturating(a, b uint64) uint64 {
	if a+b < a {
		return ^uint64(0)
	}
	return a + b
}

// Observe counts an observation under the current consent. With usage
// metrics off it does nothing and starts nothing.
func (c *Control) Observe(observation Observation) {
	if c == nil {
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	c.observeLocked(observation)
}

func (c *Control) observeLocked(observation Observation) {
	if !c.metricsLocked() {
		return
	}
	dimensions, err := json.Marshal(observation.dimensions)
	if err != nil {
		return
	}
	if c.window == nil {
		c.window = &window{started: time.Now().UTC(), groups: map[string]*counts{}, common: map[string]any{}}
		for name, value := range c.tagsLocked() {
			c.window.common[name] = value
		}
		c.window.common["release"] = c.release
		epoch := c.epoch
		c.stop = c.timer(metricsPeriod, func() { c.flush(epoch) })
	}
	key := observation.table + "\x00" + string(dimensions)
	group := c.window.groups[key]
	if group == nil {
		if len(c.window.groups) >= metricsGroups {
			return
		}
		group = &counts{dimensions: dimensions}
		if observation.elapsed != nil {
			group.total = new(uint64)
			if observation.peak {
				group.max = new(uint64)
			}
		}
		if observation.tokens != nil {
			group.tokens = &Tokens{}
		}
		c.window.groups[key] = group
	}
	group.count = saturating(group.count, 1)
	if observation.elapsed != nil && group.total != nil {
		elapsed := uint64(max(observation.elapsed.Microseconds(), 0))
		*group.total = saturating(*group.total, elapsed)
		if group.max != nil {
			*group.max = max(*group.max, elapsed)
		}
	}
	if observation.tokens != nil && group.tokens != nil {
		group.tokens.Input = saturating(group.tokens.Input, observation.tokens.Input)
		group.tokens.Output = saturating(group.tokens.Output, observation.tokens.Output)
		group.tokens.CacheRead = saturating(group.tokens.CacheRead, observation.tokens.CacheRead)
		group.tokens.CacheWrite = saturating(group.tokens.CacheWrite, observation.tokens.CacheWrite)
	}
}

// Capture is the consent an operation started under: its observation is
// counted only if that consent still holds when it ends.
type Capture struct {
	control *Control
	epoch   uint64
}

// Capture is nil while usage metrics are off.
func (c *Control) Capture() *Capture {
	if c == nil {
		return nil
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if !c.metricsLocked() {
		return nil
	}
	return &Capture{control: c, epoch: c.epoch}
}

// Record counts the operation's one observation.
func (capture *Capture) Record(observation Observation) {
	if capture == nil {
		return
	}
	c := capture.control
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.epoch == capture.epoch {
		c.observeLocked(observation)
	}
}

// flush sends the window its timer was started for.
func (c *Control) flush(epoch uint64) {
	c.mu.Lock()
	if c.epoch != epoch || !c.metricsLocked() {
		c.mu.Unlock()
		return
	}
	c.stop = nil
	pending, _ := c.takeWindowLocked()
	scope := c.scope
	c.mu.Unlock()
	c.deliver(scope, pending, epoch)
}

// deliver posts each table's rows once, serialized once so a retry reuses
// the same rows and event identities. An attempted batch is never replayed.
func (c *Control) deliver(ctx context.Context, pending []batch, epoch uint64) {
	client := &http.Client{Transport: c.transport, Timeout: 5 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	for _, rows := range pending {
		body, err := json.Marshal(rows)
		if err != nil {
			continue
		}
		for attempt := range metricsAttempts {
			c.mu.Lock()
			allowed := c.permittedLocked() && c.key != "" && c.metrics && c.epoch == epoch
			c.mu.Unlock()
			if !allowed || ctx.Err() != nil {
				return
			}
			if !retryMetrics(c.post(ctx, client, body)) || attempt+1 == metricsAttempts {
				break
			}
			select {
			case <-ctx.Done():
				return
			case <-time.After(250 * time.Millisecond << min(attempt, 3)):
			}
		}
	}
}

// post reports the status, 0 for a connection that was never made and -1
// for any other failure. No upstream body or error text is observed.
func (c *Control) post(ctx context.Context, client *http.Client, body []byte) int {
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, metricsEndpoint, bytes.NewReader(body))
	if err != nil {
		return -1
	}
	request.Header.Set("Authorization", "Bearer "+c.key)
	request.Header.Set("Content-Type", "application/json")
	response, err := client.Do(request)
	if err != nil {
		var dial *net.OpError
		if errors.As(err, &dial) && dial.Op == "dial" {
			return 0
		}
		return -1
	}
	_ = response.Body.Close()
	return response.StatusCode
}

func retryMetrics(status int) bool {
	return status == 0 || status == 429 || status == 502 || status == 503 || status == 504
}
