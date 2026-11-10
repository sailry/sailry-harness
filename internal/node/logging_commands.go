package node

import (
	"context"
	"encoding/json"
	"errors"
	"log/slog"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Command diagnostics: the kind and identifiers of selected commands, never
// their payloads or results.

// loggedCommands are the commands observed for a controller, with whether
// the session and which turn field identify them. Every command of a package
// is observed as "plugin_command".
var loggedCommands = map[string]struct {
	name    string // the recorded name when it differs from the kind
	session bool
	turn    string
	query   bool
}{
	"submit_turn": {session: true}, "queue_turn": {session: true}, "submit_task": {session: true},
	"guide_turn": {turn: "recipient"}, "guide_queued_turn": {turn: "turn"}, "withdraw_guidance": {turn: "turn"}, "stop_turn": {turn: "turn"},
	"resolve_question": {session: true}, "resolve_approval": {session: true},
	"search_skill_catalog": {}, "discover_registry_skill": {}, "read_skill_document": {},
	"read_conversation": {session: true, query: true}, "read_conversation_runs": {session: true, query: true},
	"read_conversation_run": {session: true, turn: "turn", query: true}, "read_turn_capture": {session: true, turn: "turn", query: true},
	"read_turn": {session: true, turn: "turn", query: true}, "read_command_page": {session: true, query: true},
	"read_tool_output": {turn: "turn", query: true}, "read_tool_detail": {session: true, turn: "turn", query: true},
	"read_session": {session: true, query: true}, "read_plugin_callback": {query: true}, "read_plugin_value": {query: true},
	"read_plugin_conversation_value": {name: "read_conversation_value", query: true},
	"list_ssh":                       {query: true}, "list_databases": {query: true}, "list_plugin_models": {query: true},
	"read_model_catalog": {query: true}, "read_model_scores": {query: true}, "read_project_catalog": {query: true},
}

// attachLogging gives the process's log to the Node that owns its profile:
// earlier daily files retire and failed operations reach error reporting.
func (node *Node) attachLogging() {
	if logging.Directory() != filepath.Join(node.profile.Ownership.Path, "logs") {
		return
	}
	node.logs = true
	_ = logging.Retain(node.profile.Ownership.Path)
	logging.SetReport(node.telemetry.Record)
}

func (node *Node) detachLogging() {
	if node.logs {
		logging.SetReport(nil)
	}
}

// commandSpan begins the observation of a command, nil for one that is not
// observed or a process without a log.
func (node *Node) commandSpan(caller protocol.NodeID, request protocol.Request) *logging.Span {
	if !node.logs {
		return nil
	}
	entry, listed := loggedCommands[request.Command.Kind]
	name := request.Command.Kind
	var identifiers map[string]json.RawMessage
	switch {
	case listed:
		if entry.name != "" {
			name = entry.name
		}
		// Identifiers only: no other field of the command is read.
		if encoded, err := json.Marshal(request.Command.Data); err == nil {
			_ = json.Unmarshal(encoded, &identifiers)
		}
	case request.Plugin != nil:
		name = "plugin_command"
	default:
		return nil
	}
	identifier := func(key string) string {
		var text string
		_ = json.Unmarshal(identifiers[key], &text)
		return text
	}
	attrs := []slog.Attr{slog.String("command", name), slog.String("operation", name), slog.String("node", node.ID().String()), slog.String("caller", caller.String()), slog.String("request", request.ID.String())}
	if listed && entry.session {
		attrs = append(attrs, slog.String("session", identifier("session")))
	}
	if listed && entry.turn != "" {
		attrs = append(attrs, slog.String("turn", identifier(entry.turn)))
	}
	if plugin := request.Plugin; plugin != nil {
		attrs = append(attrs, slog.String("package", plugin.Package.Name), slog.String("digest", plugin.Package.Digest), slog.Uint64("settings_revision", plugin.Package.SettingsRevision))
		if plugin.Invocation != nil {
			attrs = append(attrs, slog.String("invocation", plugin.Invocation.String()))
		}
		if !listed {
			if plugin.Session != nil {
				attrs = append(attrs, slog.String("session", plugin.Session.String()))
			}
			if plugin.Turn != nil {
				attrs = append(attrs, slog.String("turn", plugin.Turn.String()))
			}
		}
	}
	if request.Command.Kind == "request_provider_http" {
		attrs = append(attrs, slog.String("stage", "provider_http"), slog.String("provider", identifier("provider")))
	}
	switch {
	case request.Plugin != nil && !request.Command.Durable():
		return logging.NewBackground("command", attrs...)
	case listed && entry.query:
		return logging.NewQuery("command", attrs...)
	}
	return logging.NewSpan("command", attrs...)
}

// rejectCommand records a refusal. Expected refusals are warnings.
func rejectCommand(span *logging.Span, fault *protocol.Fault) {
	diagnostic := fault.Details != nil && fault.Details.Kind == "system_diagnostic"
	if report, found := faultDiagnostic(fault); found {
		if report.Cause != nil {
			span.Set(slog.String("cause_code", string(*report.Cause)))
		}
		if report.Operation != nil && *report.Operation == "inspect_git" {
			span.Set(slog.String("operation", "inspect_git"))
		}
	}
	expected := fault.Code == protocol.ErrorCodeNotFound || fault.Code == protocol.ErrorCodeInvalidRequest ||
		fault.Code == protocol.ErrorCodeUnavailable && fault.Details != nil && fault.Details.Kind == "network"
	span.Finish(logging.Failed, string(fault.Code), expected && !diagnostic)
}

func faultDiagnostic(fault *protocol.Fault) (protocol.NotificationSystemSystemDiagnostic, bool) {
	if fault.Details == nil || fault.Details.Kind != "system_diagnostic" {
		return protocol.NotificationSystemSystemDiagnostic{}, false
	}
	switch report := fault.Details.Data.(type) {
	case protocol.NotificationSystemSystemDiagnostic:
		return report, true
	case *protocol.NotificationSystemSystemDiagnostic:
		return *report, report != nil
	}
	return protocol.NotificationSystemSystemDiagnostic{}, false
}

// Dispatch admits a command and observes the selected ones.
func (node *Node) Dispatch(ctx context.Context, caller protocol.NodeID, request protocol.Request) (link.Admission, error) {
	span := node.commandSpan(caller, request)
	accepted, err := node.admitCommand(ctx, caller, request)
	if span == nil {
		return accepted, err
	}
	if err != nil {
		var fault *protocol.Fault
		if !errors.As(err, &fault) {
			fault = &protocol.Fault{Code: protocol.ErrorCodeInternal}
		}
		rejectCommand(span, fault)
		return accepted, err
	}
	span.Set(slog.Bool("durable", accepted.Receipt.Durable))
	span.Record(logging.Admitted)
	original, completion := accepted.Completion, make(chan link.Response, 1)
	accepted.Completion = completion
	go func() {
		defer close(completion)
		result, delivered := <-original
		switch {
		case !delivered:
			span.Finish(logging.Interrupted, string(protocol.ErrorCodeOutcomeUnknown), false)
			return
		case result.Fault != nil:
			rejectCommand(span, result.Fault)
		default:
			observeOutput(span, request, result.Output)
			span.Finish(logging.Completed, "", false)
		}
		completion <- result
	}()
	return accepted, nil
}

// observeOutput adds a result's identifiers and status, never its content.
func observeOutput(span *logging.Span, request protocol.Request, output protocol.Output) {
	switch value := output.Data.(type) {
	case protocol.QueuedTurn:
		span.Set(slog.String("session", value.Session.String()))
		span.Set(slog.String("turn", value.ID.String()))
	case *protocol.QueuedTurn:
		span.Set(slog.String("session", value.Session.String()))
		span.Set(slog.String("turn", value.ID.String()))
	case protocol.PluginHttpResponse:
		observeHTTP(span, request, value.Status)
	case *protocol.PluginHttpResponse:
		observeHTTP(span, request, value.Status)
	}
}

func observeHTTP(span *logging.Span, request protocol.Request, status uint16) {
	if request.Command.Kind != "request_provider_http" {
		return
	}
	reason := "http_rejected"
	if status >= 200 && status < 300 {
		reason = "http_success"
	}
	span.Set(slog.Uint64("upstream_status", uint64(status)))
	span.Set(slog.String("reason", reason))
}
