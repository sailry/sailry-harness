package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"log/slog"
	"runtime"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (service *pluginStatusService) read(ctx context.Context, scope protocol.PluginUiStatusScope, info protocol.PluginInfo) ([]protocol.PluginUiStatusEntry, error) {
	if ctx.Err() != nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "Status evaluation stopped"}
	}
	select {
	case <-ctx.Done():
		return nil, &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "Status evaluation stopped"}
	case service.permits <- struct{}{}:
	}
	defer func() { <-service.permits }()
	request := protocol.NewRequest(service.node.ID(), protocol.Command{
		Kind: "read_plugin_callback",
		Data: protocol.CommandReadPluginCallbackPayload{Handler: info.Extension.Host.Status.Read, Input: json.RawMessage(`{}`)},
	})
	request.Plugin = &protocol.PluginContext{
		Surface: protocol.PluginDesktopSurface{Kind: "composer"}, Package: plugins.PackageReference(info),
		Worktree: scope.Worktree, Session: scope.Session,
	}
	output, err := service.node.dispatchCallbackResult(ctx, request)
	if err != nil {
		if ctx.Err() != nil {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "Status evaluation stopped"}
		}
		return nil, err
	}
	if output.Kind != "plugin_result" {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid Status callback output"}
	}
	value, captured := dispatchPayload[json.RawMessage](output.Data)
	if !captured {
		value, err = json.Marshal(output.Data)
		if err != nil {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid Status callback output"}
		}
	}
	states, err := store.ValidatePluginStatusStates(info, value)
	if err != nil {
		return nil, err
	}
	return store.PluginStatusEntries(info, states), nil
}

func pluginStatusPublicFault(err error) *protocol.Fault {
	if err == nil {
		return nil
	}
	failure := pluginFailure(err)
	return &protocol.Fault{Code: failure.Code, Message: failure.Message}
}

func (node *Node) reportPluginStatusFailure(ctx context.Context, err error, info protocol.PluginInfo) {
	failure := pluginFailure(err)
	if failure.Details == nil || failure.Details.Kind != "system_diagnostic" {
		return
	}
	report, valid := dispatchPayload[protocol.NotificationSystemSystemDiagnostic](failure.Details.Data)
	if !valid || failure.Code == protocol.ErrorCodeCancelled ||
		report.Reason == protocol.NotificationSystemDiagnosticReasonCancelled && failure.Code != protocol.ErrorCodeOutcomeUnknown {
		return
	}
	version := "unknown"
	if info.Summary.Version != nil {
		version = *info.Summary.Version
	}
	report.Plugin = &protocol.NotificationSystemDiagnosticPlugin{Name: info.Summary.Name, Version: version}
	if report.Version == "" {
		report.Version = link.ProductVersion
	}
	if report.Platform == "" {
		report.Platform = runtime.GOOS + "-" + runtime.GOARCH
	}
	var changes []protocol.Event
	saveErr := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		changes, err = store.RecordPluginStatusDiagnostic(ctx, db, report, node.pluginStatus.clock.Now().UnixMilli())
		if err == nil {
			_, err = store.RecordEvents(ctx, db, node.ID(), changes)
		}
		return err
	})
	if saveErr != nil {
		slog.Warn("plugin Status diagnostic could not be recorded", "incident", report.Incident.String(), "package", info.Summary.Name, "code", pluginStatusPublicFault(saveErr).Code)
		return
	}
	if len(changes) != 0 {
		node.publish()
		slog.Warn("plugin Status evaluation failed", "incident", report.Incident.String(), "package", info.Summary.Name, "code", report.Code, "phase", report.Phase, "reason", report.Reason)
	}
}
