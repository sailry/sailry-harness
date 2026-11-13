package acceptance

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const defaultCaseTimeout = 2 * time.Minute

type backendCase struct {
	id, title string
	quick     bool
	packages  []string
	run       func(*caseRun) error
}

type caseIssue struct{ status, detail string }

func (issue *caseIssue) Error() string { return issue.detail }

func fail(detail string) error { return &caseIssue{"failed", detail} }

func blocked(detail string) error { return &caseIssue{"blocked", detail} }

// RunBackend is opt-in: ordinary tests never call it with a real model. Each
// case creates only unassigned sessions and confines its fixtures to their
// workspaces. The caller retains ownership of the already started Client.
func RunBackend(ctx context.Context, controller *client.Client, options Options, emit func(CaseResult)) (report Report, resultErr error) {
	report = Report{Version: 1, Suite: options.Suite, Started: time.Now().UTC(), Cases: []CaseResult{}}
	defer func() { report.Finished = time.Now().UTC() }()
	if report.Suite == "" {
		report.Suite = "quick"
	}
	if !slices.Contains([]string{"quick", "backend", "full"}, report.Suite) {
		return report, errors.New("acceptance suite must be quick, backend or full")
	}
	if controller == nil {
		return report, errors.New("acceptance requires an existing Client")
	}
	if options.CaseTimeout < 0 {
		return report, errors.New("acceptance case timeout must be positive")
	}
	timeout := options.CaseTimeout
	if timeout == 0 {
		timeout = defaultCaseTimeout
	}
	cases, err := selectCases(report.Suite, options.Cases)
	if err != nil {
		return report, err
	}
	config, err := selectConfig(controller.Snapshot(), options.Config)
	if err != nil {
		return report, err
	}
	report.Provider, report.Model = config.Provider.String(), config.Model
	report.Effort = &config.Effort
	stop := false
	for _, definition := range cases {
		result := CaseResult{ID: definition.id, Group: "backend", Title: definition.title, Turns: []string{}, Evidence: []Evidence{}}
		if stop {
			result.Status, result.Detail = "blocked", "A preceding case has an unconfirmed admission or cleanup; no further model request was sent"
		} else if ctx.Err() != nil {
			result.Status, result.Detail = "canceled", "Acceptance stopped before this case started"
		} else if missing := missingPackage(controller.Snapshot(), definition.packages); missing != "" {
			result.Status, result.Detail = "skipped", "Required package is disabled or incompatible: "+missing
		} else {
			started := time.Now()
			scoped, cancel := context.WithTimeout(ctx, timeout)
			run := &caseRun{ctx: scoped, controller: controller, config: config, result: &result, wake: make(chan struct{}, 1)}
			err := definition.run(run)
			if err == nil && scoped.Err() != nil {
				err = scoped.Err()
			}
			cancel()
			cleanupErr := run.cleanup()
			for _, release := range run.releases {
				release()
			}
			result.Status, result.Detail = "passed", "All backend assertions passed"
			if err != nil {
				var issue *caseIssue
				switch {
				case ctx.Err() != nil:
					result.Status, result.Detail = "canceled", "Acceptance canceled; cleanup requested for its sessions"
				case errors.As(err, &issue):
					result.Status, result.Detail = issue.status, issue.detail
				case errors.Is(err, context.DeadlineExceeded):
					result.Status, result.Detail = "failed", "Case deadline exceeded; cleanup requested for its sessions"
				default:
					result.Status, result.Detail = "failed", safeError(err)
				}
			}
			if run.unconfirmed != nil {
				result.Uncertain = true
				result.Status, result.Detail = "blocked", "Admission is unconfirmed; retain the recorded request ID and do not resubmit"
				stop = true
			}
			if cleanupErr != nil {
				result.Uncertain = true
				result.Status = "blocked"
				result.Detail = "Cleanup is unconfirmed; inspect the recorded sessions before running more cases"
				stop = true
				run.evidence("cleanup", safeError(cleanupErr))
			}
			result.Duration = time.Since(started)
		}
		report.Cases = append(report.Cases, result)
		if emit != nil {
			emit(cloneResult(result))
		}
	}
	if err := ctx.Err(); err != nil {
		return report, err
	}
	if stop {
		return report, blocked("Backend acceptance stopped after unconfirmed admission or cleanup")
	}
	return report, nil
}

func selectCases(suite string, requested []string) ([]backendCase, error) {
	definitions := backendCases()
	for _, id := range requested {
		if !slices.ContainsFunc(definitions, func(value backendCase) bool { return value.id == id }) {
			return nil, fmt.Errorf("unknown backend acceptance case %q", id)
		}
	}
	selected := []backendCase{}
	for _, definition := range definitions {
		if len(requested) > 0 && !slices.Contains(requested, definition.id) {
			continue
		}
		if len(requested) == 0 && suite == "quick" && !definition.quick {
			continue
		}
		selected = append(selected, definition)
	}
	return selected, nil
}

// ListBackendCases describes the explicit suite without accessing a Client.
func ListBackendCases(suite string) ([]CaseResult, error) {
	if suite == "" {
		suite = "quick"
	}
	if !slices.Contains([]string{"quick", "backend", "full"}, suite) {
		return nil, errors.New("acceptance suite must be quick, backend or full")
	}
	definitions, err := selectCases(suite, nil)
	if err != nil {
		return nil, err
	}
	results := make([]CaseResult, 0, len(definitions))
	for _, definition := range definitions {
		results = append(results, CaseResult{ID: definition.id, Group: "backend", Title: definition.title})
	}
	return results, nil
}

func selectConfig(snapshot protocol.Snapshot, requested *protocol.SessionConfig) (protocol.SessionConfig, error) {
	valid := func(config *protocol.SessionConfig) bool {
		return config != nil && config.Model != "" && slices.ContainsFunc(snapshot.Providers, func(provider protocol.ConversationProvider) bool {
			return provider.ID == config.Provider && provider.Enabled && (provider.DefaultModel == config.Model || slices.ContainsFunc(provider.Models, func(model protocol.ConversationModel) bool { return model.ID == config.Model }))
		})
	}
	selected := requested
	if selected == nil && valid(snapshot.Defaults.Config) {
		selected = snapshot.Defaults.Config
	}
	if selected == nil {
		choices := []protocol.SessionConfig{}
		for _, provider := range snapshot.Providers {
			if !provider.Enabled {
				continue
			}
			model := provider.DefaultModel
			if model == "" && len(provider.Models) == 1 {
				model = provider.Models[0].ID
			}
			if model == "" {
				continue
			}
			config := protocol.DefaultSessionConfig()
			config.Provider, config.Model, config.Credential = provider.ID, model, provider.Credential
			choices = append(choices, config)
		}
		if len(choices) != 1 {
			return protocol.SessionConfig{}, errors.New("choose a configured provider and model before acceptance")
		}
		selected = &choices[0]
	}
	if !valid(selected) {
		return protocol.SessionConfig{}, errors.New("acceptance model is not enabled in this execution Node")
	}
	config := *selected
	// Provider/model-only callers need a complete local configuration before
	// the protocol decoder makes the independent copy.
	config.Assistant, config.Resource = nil, nil
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	if config.Effort.Kind == "" {
		config.Effort = protocol.Effort{Kind: "default"}
	}
	// A provider/model-only selection uses the advertised model default.
	// Reasoning catalogs may list only concrete efforts, in which case the
	// protocol's generic "default" is not an admissible model choice.
	if config.Effort.Kind == "default" {
		for _, provider := range snapshot.Providers {
			if provider.ID != config.Provider {
				continue
			}
			for _, model := range provider.Models {
				if model.ID == config.Model && model.Reasoning && len(model.Efforts) > 0 && model.DefaultEffort.Kind != "" && !slices.ContainsFunc(model.Efforts, func(value protocol.Effort) bool { return value.Kind == "default" }) {
					config.Effort = model.DefaultEffort
				}
			}
		}
	}
	config, err := decode[protocol.SessionConfig](config)
	if err != nil {
		return config, errors.New("acceptance model configuration could not be copied")
	}
	for _, provider := range snapshot.Providers {
		if provider.ID == config.Provider && config.Credential == nil {
			config.Credential = provider.Credential
		}
	}
	// Acceptance owns an unassigned workspace, never a selected connection or
	// assistant workspace. These case-local choices do not update defaults.
	config.Assistant, config.Resource = nil, nil
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	return config, nil
}

func missingPackage(snapshot protocol.Snapshot, names []string) string {
	for _, name := range names {
		if !slices.ContainsFunc(snapshot.Plugins, func(installed protocol.PluginSummary) bool {
			return installed.Name == name && installed.Enabled && installed.Compatibility.Kind == "compatible"
		}) {
			return name
		}
	}
	return ""
}

func decode[T any](data any) (T, error) {
	var value T
	encoded, err := json.Marshal(data)
	if err == nil {
		err = json.Unmarshal(encoded, &value)
	}
	return value, err
}

func cloneResult(result CaseResult) CaseResult {
	result.Turns = slices.Clone(result.Turns)
	result.Evidence = slices.Clone(result.Evidence)
	return result
}

// Node/provider diagnostics can contain remote response bodies. Reports retain
// fault codes and test-owned assertions instead of copying those bodies.
func safeError(err error) string {
	if errors.Is(err, context.Canceled) {
		return "Observation canceled"
	}
	if errors.Is(err, context.DeadlineExceeded) {
		return "Observation deadline exceeded"
	}
	var issue *caseIssue
	if errors.As(err, &issue) {
		return issue.detail
	}
	if fault, ok := nodeFault(err); ok {
		return "Node request failed: " + string(fault.Code)
	}
	return "Acceptance operation failed"
}

func nodeFault(err error) (protocol.Fault, bool) {
	var value protocol.Fault
	if errors.As(err, &value) {
		return value, true
	}
	var pointer *protocol.Fault
	if errors.As(err, &pointer) && pointer != nil {
		return *pointer, true
	}
	return protocol.Fault{}, false
}

func bounded(value string, maximum int) string {
	if len(value) <= maximum {
		return value
	}
	return strings.ToValidUTF8(value[:maximum], "") + "..."
}
