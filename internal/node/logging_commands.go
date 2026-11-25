package node

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"path/filepath"
	"reflect"
	"slices"
	"strings"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/codeindexapi"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Command diagnostics: the kind and identifiers of every command that changes
// data, never a payload or a result.

// observation is how a command without durable admission is observed.
type observation uint8

const (
	// changing commands alter stored or system state: a full record. Every
	// durable command, and any command this file does not name, is changing.
	changing observation = iota + 1
	// traced reads reach a registry or the network: a full record.
	traced
	// slow reads are recorded only when slow, failed or uncertain.
	slow
	// controllerRead identifies workspace reads also presented by the controller.
	controllerRead
	// readOnly commands return, probe or cancel a read and store nothing.
	readOnly
	// transfer commands open or cancel a byte stream. The durable command
	// that commits the bytes is the recorded change.
	transfer
	// interactive commands carry terminal focus, keystrokes and geometry,
	// or one anonymous feature count: one record each would bury the log.
	interactive
)

// transientCommands classifies every command protocol.Command.Durable admits
// without a durable receipt. A kind absent from both is recorded in full, so
// a new command is never silently unobserved.
var transientCommands = func() map[string]observation {
	classes := map[string]observation{}
	for class, kinds := range map[observation][]string{
		changing: {"complete_browser", "refresh_model_catalog", "dismiss_installation", "cancel_local_model", "request_computer_permission"},
		traced:   {"search_skill_catalog", "discover_registry_skill", "read_skill_document"},
		slow: {
			"read_conversation", "read_conversation_runs", "read_conversation_run", "read_turn_capture", "read_turn", "read_command_page",
			"read_tool_output", "read_tool_detail", "read_session", "read_plugin_callback", "read_plugin_value", "read_plugin_conversation_value",
			"list_ssh", "list_databases", "list_plugin_models", "read_model_catalog", "read_model_scores", "read_project_catalog",
		},
		controllerRead: {"list_directory", "inspect_git", "list_worktrees", "search_files", "discover_models"},
		transfer: {
			"open_port", "cancel_port", "download_ssh_file", "stage_ssh_upload", "upload_plugin", "download_backup", "upload_backup",
			"download_file", "upload_file", "upload_attachment", "download_attachment", "download_image", "cancel_file_transfer",
		},
		interactive: {"input_terminal", "resize_terminal", "set_terminal_appearance", "count_feature"},
		readOnly: {
			"snapshot", "read_artifact", "list_artifacts", "list_artifact_revisions", "read_activity_catalog", "read_worktree_catalog",
			"read_host_install", "list_wsl_distributions", "read_wsl_host_install", "browse_ssh_directory", "list_ssh_terminals", "list_plugins",
			"list_connector_types", "inspect_connector_cli", "list_connections", "discover_skills", "discover_skill_upload", "discover_local_skills",
			"search_plugin_catalog", "read_catalog_plugin", "read_catalog_plugin_info", "inspect_plugin_source", "check_plugin_update", "check_plugin_updates",
			"inspect_plugin_upload", "read_plugin", "read_plugin_version", "read_plugin_settings", "read_plugin_mcp", "list_plugin_keys",
			"search_plugin_values", "resolve_plugin_model", "read_plugin_connection", "read_plugin_view", "list_roles", "list_teams",
			"read_compaction_settings", "read_team_settings", "list_providers", "read_provider_directory", "read_provider_key", "list_channel_presets",
			"request_provider_http", "list_associated_sessions", "read_usage", "read_activity",
			"search_conversation", "list_conversation_assets", "list_file_checkpoints", "read_turn_diff", "read_turn_hunks",
			"read_queued_turn", "list_terminals", "list_terminal_tools", "list_commands", "read_command", "read_terminal_settings",
			"read_power_settings", "read_telemetry_settings", "send_diagnostics", "read_network_settings", "list_network_presets", "probe_network",
			"read_sandbox_settings", "read_search_settings", "read_backup_settings", "export_configuration", "list_backups", "read_python_settings", "read_installations",
			"decide", "read_local_model_settings", "read_local_models", "refresh_local_model", "list_shells", "read_plugin_secret",
			"list_media_models", "read_media_settings", "synthesize_speech", "search_models", "read_model_versions", "resolve_model", "query_knowledge",
			"read_knowledge_citation", "list_knowledge_collections", "read_resource_catalog", "inspect_terminal", "inspect_request", "list_credentials", "browse_database",
			"test_database", "inspect_host", "read_host_metrics", "read_plugin_memory", "read_computer_permissions", "read_computer_settings", "read_external_browser_settings", "read_memory_catalog", "read_memory_entry", "browse_files",
			"read_file", "search_code", "outline_code", "graph_code", "cancel_code_index_query", "fetch_web",
			"cancel_web_fetch", "search_web", "cancel_web_search", "test_search_driver", "read_code_index_settings", "read_hooks", "read_personal_instructions",
			"read_file_lines", "read_image", "find_files", "read_office", "read_notebook", "read_attachment",
			"read_git_head", "list_git_branches", "resolve_git_revision",
			"read_git_stash", "read_git_output", "list_git_remote_tags", "read_git_log", "read_git_commit", "read_git_show",
			"list_git_changes", "read_git_change", "read_git_context", "read_git_blame", "read_git_diff",
		},
	} {
		for _, kind := range kinds {
			classes[kind] = class
		}
	}
	return classes
}()

func commandObservation(command protocol.Command) observation {
	if command.Durable() {
		return changing
	}
	if command.Kind == "call_host_handler" {
		return slow
	}
	if command.Kind == "dispatch" || command.Kind == "identity" || command.Kind == "configuration_follow" {
		// Its listing and reading actions.
		return readOnly
	}
	if class, listed := transientCommands[command.Kind]; listed {
		return class
	}
	return changing
}

// identifierFields are the typed identifiers a record may name, by the
// payload field that carries them. Fields of any other type are never read.
var identifierFields = map[reflect.Type]map[string]string{
	reflect.TypeFor[protocol.SessionID]():  {"session": "session"},
	reflect.TypeFor[protocol.TurnID]():     {"turn": "turn", "recipient": "turn"},
	reflect.TypeFor[protocol.ProviderID](): {"provider": "provider"},
	reflect.TypeFor[protocol.WorktreeID](): {"worktree": "worktree"},
}

// commandIdentifiers names the scope a command addresses from the top level
// of its payload.
func commandIdentifiers(data any) []slog.Attr {
	payload := reflect.ValueOf(data)
	for payload.Kind() == reflect.Pointer && !payload.IsNil() {
		payload = payload.Elem()
	}
	if payload.Kind() != reflect.Struct {
		return nil
	}
	var attrs []slog.Attr
	for index := range payload.NumField() {
		field := payload.Field(index)
		if field.Kind() == reflect.Pointer && !field.IsNil() {
			field = field.Elem()
		}
		name, _, _ := strings.Cut(payload.Type().Field(index).Tag.Get("json"), ",")
		key, named := identifierFields[field.Type()][name]
		if !named || field.IsZero() || slices.ContainsFunc(attrs, func(attr slog.Attr) bool { return attr.Key == key }) {
			continue
		}
		attrs = append(attrs, slog.String(key, field.Interface().(fmt.Stringer).String()))
	}
	return attrs
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
// observed or a process without a log. A package's unrecorded commands are
// observed as "plugin_command".
func (node *Node) commandSpan(caller protocol.NodeID, request protocol.Request) *logging.Span {
	if !node.logs {
		return nil
	}
	class, name := commandObservation(request.Command), request.Command.Kind
	var sourceKind string
	switch {
	case name == "read_plugin_conversation_value":
		sourceKind, name = name, "read_conversation_value"
	case class == changing || class == traced || class == slow:
	case request.Plugin != nil:
		// Only classified reads and other known transient commands reach this
		// branch. Keep their kind locally without changing remote grouping.
		sourceKind, name = name, "plugin_command"
	case class == controllerRead || class == readOnly || class == transfer:
	default:
		return nil
	}
	attrs := []slog.Attr{slog.String("command", name), slog.String("operation", name), slog.String("node", node.ID().String()), slog.String("caller", caller.String()), slog.String("request", request.ID.String())}
	if sourceKind != "" {
		attrs = append(attrs, slog.String("source_kind", sourceKind))
	}
	attrs = append(attrs, commandIdentifiers(request.Command.Data)...)
	if plugin := request.Plugin; plugin != nil {
		attrs = append(attrs, slog.String("package", plugin.Package.Name), slog.String("digest", plugin.Package.Digest), slog.Uint64("settings_revision", plugin.Package.SettingsRevision))
		if plugin.Invocation != nil {
			attrs = append(attrs, slog.String("invocation", plugin.Invocation.String()))
		}
		named := func(key string) bool {
			return slices.ContainsFunc(attrs, func(attr slog.Attr) bool { return attr.Key == key })
		}
		if plugin.Session != nil && !named("session") {
			attrs = append(attrs, slog.String("session", plugin.Session.String()))
		}
		if plugin.Turn != nil && !named("turn") {
			attrs = append(attrs, slog.String("turn", plugin.Turn.String()))
		}
	}
	if request.Command.Kind == "request_provider_http" {
		attrs = append(attrs, slog.String("stage", "provider_http"))
	}
	switch {
	case class == changing:
		return logging.NewSpan("command", attrs...)
	case request.Plugin != nil:
		return logging.NewBackground("command", attrs...)
	case class == slow || class == controllerRead || class == readOnly || class == transfer:
		return logging.NewQuery("command", attrs...)
	}
	return logging.NewSpan("command", attrs...)
}

// rejectCommand records a refusal. Expected refusals are warnings; a
// cancelled command, or any refusal of a read whose caller has gone, is the
// caller's own act and not a failure.
func rejectCommand(span *logging.Span, kind string, fault *protocol.Fault, abandoned bool) {
	if abandoned || fault.Code == protocol.ErrorCodeCancelled {
		span.Finish(logging.Cancelled, string(protocol.ErrorCodeCancelled), false)
		return
	}
	diagnostic := fault.Details != nil && fault.Details.Kind == "system_diagnostic"
	if kind == "inspect_git" {
		if stage := fault.GitInspectionReason.Stage(); stage != "" {
			span.Set(slog.String("stage", stage))
			span.Set(slog.String("reason", string(fault.GitInspectionReason)))
		}
	}
	if kind == "check_ssh" || kind == "install_host" {
		if stage := fault.SSHReason.Stage(); stage != "" {
			span.Set(slog.String("stage", stage))
			span.Set(slog.String("reason", string(fault.SSHReason)))
		}
	}
	if report, found := faultDiagnostic(fault); found {
		if report.Cause != nil {
			span.Set(slog.String("cause_code", string(*report.Cause)))
		}
		if report.Operation != nil && *report.Operation == "inspect_git" {
			span.Set(slog.String("operation", "inspect_git"))
		}
	}
	expected := fault.Code == protocol.ErrorCodeNotFound || fault.Code == protocol.ErrorCodeInvalidRequest ||
		fault.Code == protocol.ErrorCodePermissionDenied && fault.Message == resourceAccessDenied ||
		fault.Code == protocol.ErrorCodeUnavailable && fault.Details != nil && fault.Details.Kind == "network"
	if codeIndexCommand(kind) && fault.Code == protocol.ErrorCodeUnavailable && fault.Message == codeIndexUnavailable {
		expected = true
	}
	switch kind {
	case "search_code", "outline_code", "graph_code":
		if codeindexapi.ReaderWaitExpired(fault) {
			span.Set(slog.String("reason_code", "code_reader_wait"))
			expected = true
		}
	}
	if reason, race := store.QueueRejection(kind, fault); reason != "" {
		span.Set(slog.String("reason_code", reason))
		expected = race
	}
	if fault.Code == protocol.ErrorCodeNotConfigured && fault.Details == nil && (kind == "inspect_git" || kind == "list_worktrees") {
		// The native Git service reports a missing executable without a configuration reason.
		expected = true
	}
	if reason := fault.ConfigurationReason(); reason.Known() {
		span.Set(slog.String("configuration_reason", string(reason)))
		if fault.Code == protocol.ErrorCodeNotConfigured && reason.Expected() {
			expected = true
		}
	}
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
		if report != nil {
			return *report, true
		}
	}
	return protocol.NotificationSystemSystemDiagnostic{}, false
}

// reportFault correlates a single failure delivery, not the durable request.
// Never modify a fault owned by a stored receipt, shared result or handler.
func reportFault(span *logging.Span, original *protocol.Fault) *protocol.Fault {
	if original.Code == protocol.ErrorCodeCancelled {
		return original
	}
	f := *original
	f.ReportRef = uuid.NewString()
	f.ReportOwned = true
	span.Set(slog.String("report_ref", f.ReportRef))
	if f.Details != nil && f.Details.Kind == "model_request" {
		if details, ok := dispatchPayload[protocol.FaultDetailsModelRequestPayload](f.Details.Data); ok {
			switch details.Reason {
			case protocol.ModelFailureRequest, protocol.ModelFailureUnauthorized, protocol.ModelFailureRateLimited, protocol.ModelFailureQuota:
				span.Set(slog.String("cause_category", string(details.Reason)))
			}
		}
	}
	return &f
}

// Dispatch admits a command and observes every one that changes data.
func (node *Node) Dispatch(ctx context.Context, caller protocol.NodeID, request protocol.Request) (link.Admission, error) {
	kind := request.Command.Kind
	span := node.commandSpan(caller, request)
	accepted, err := node.admitCommand(ctx, caller, request)
	if span == nil {
		return accepted, err
	}
	if err != nil {
		abandoned := !request.Command.Durable() && ctx.Err() != nil
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault == nil {
			var value protocol.Fault
			if errors.As(err, &value) {
				fault = &value
			}
		}
		if fault == nil {
			fault = nodeFault(err).(*protocol.Fault)
		}
		if !abandoned {
			if delivered := reportFault(span, fault); delivered != fault {
				fault, err = delivered, delivered
			}
		}
		span.Set(slog.String("command_stage", "admission"))
		rejectCommand(span, kind, fault, abandoned)
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
			if ctx.Err() != nil || node.ctx.Err() != nil {
				span.Finish(logging.Interrupted, string(protocol.ErrorCodeOutcomeUnknown), false)
				return
			}
			span.Set(slog.String("command_stage", "completion"))
			span.Finish(logging.Failed, string(protocol.ErrorCodeOutcomeUnknown), false)
			return
		case result.Fault != nil:
			abandoned := !accepted.Receipt.Durable && ctx.Err() != nil
			if !abandoned {
				result.Fault = reportFault(span, result.Fault)
			}
			span.Set(slog.String("command_stage", "completion"))
			rejectCommand(span, kind, result.Fault, abandoned)
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
