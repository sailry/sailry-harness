package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAdmittedPluginStateSurvivesUserMerge(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		source, err := CreateUser(ctx, db, "Admitted source")
		if err != nil {
			return err
		}
		target, err := CreateUser(ctx, db, "Winning target")
		if err != nil {
			return err
		}
		sourceCtx, targetCtx := WithUser(ctx, source), WithUser(ctx, target)
		admitted := WithCapturedUser(sourceCtx, source.ID)
		info := pluginTestInfo("admitted-merge", 'a')
		schema, err := plugins.ValidateSettings([]byte(`{"$schema":"` + plugins.SettingsSchema + `","type":"object","additionalProperties":false,"properties":{"label":{"type":"string"}}}`))
		if err != nil {
			return err
		}
		info.Settings = &schema
		references := make([]protocol.PluginReference, 2)
		jobs := make([]protocol.DispatchJob, 2)
		for index, owner := range []context.Context{sourceCtx, targetCtx} {
			installed, err := InstallPlugin(owner, db, info, 0, true)
			if err != nil {
				return err
			}
			label := []string{"source", "target"}[index]
			body, _ := json.Marshal(label)
			settings, err := SavePluginSettings(owner, db, protocol.CommandSavePluginSettingsPayload{Package: plugins.PackageReference(installed), Values: map[string]json.RawMessage{"label": body}, Secrets: map[string]protocol.PluginSettingsSecretUpdate{}})
			if err != nil {
				return err
			}
			references[index] = settings.Package
			if _, err := WritePluginValue(owner, db, settings.Package, "record", body, 0, nil); err != nil {
				return err
			}
			handler := protocol.DispatchHandler{Name: "same-handler", Enabled: true, Queue: "same-queue", Source: protocol.DispatchSource{Package: info.Summary.Name, Topic: "fixture"}, Callback: protocol.DispatchCallback{Completion: protocol.DispatchCompletionCommand, Bindings: map[string]string{}, Command: &protocol.Command{Kind: "read_plugin_value", Data: &protocol.CommandReadPluginValuePayload{Key: "record"}}}}
			handler, err = saveDispatchHandler(owner, db, settings.Package, handler)
			if err != nil {
				return err
			}
			jobs[index], err = enqueueDispatch(owner, db, &settings.Package, handler, protocol.DispatchEvent{ID: protocol.NewEventID(), Source: handler.Source, Payload: json.RawMessage("null")}, 100, false)
			if err != nil {
				return err
			}
		}
		if references[0] != references[1] {
			return fmt.Errorf("fixture did not collide user-local revisions")
		}
		work, _, _, err := claimDispatchJobs(sourceCtx, db, node, 200)
		if err != nil {
			return err
		}
		if len(work) != 1 {
			return fmt.Errorf("source job was not admitted")
		}
		if _, err := MergeIdentityUsers(ctx, db, source.ID, target.ID); err != nil {
			return err
		}
		saved, err := SavePluginSettings(admitted, db, protocol.CommandSavePluginSettingsPayload{Package: references[0], Values: map[string]json.RawMessage{"label": json.RawMessage(`"source-after-merge"`)}, Secrets: map[string]protocol.PluginSettingsSecretUpdate{}})
		if err != nil {
			return err
		}
		winner, err := ReadPluginSettings(targetCtx, db, references[1])
		if err != nil {
			return err
		}
		if string(winner.Values["label"]) != `"target"` {
			return fmt.Errorf("admitted source overwrote target's winning settings")
		}
		if _, err := WritePluginValue(admitted, db, saved.Package, "record", json.RawMessage(`"source-after-merge"`), 1, nil); err != nil {
			return err
		}
		value, err := ReadPluginValue(targetCtx, db, references[1], "record")
		if err != nil {
			return err
		}
		if string(value.Value) != `"target"` {
			return fmt.Errorf("admitted source overwrote target's winning value")
		}
		if _, err := FinishDispatch(admitted, db, jobs[0].ID, nil, 300); err != nil {
			return err
		}
		promoted, err := dispatchRequired[protocol.DispatchJob](admitted, db, "dispatch_jobs", info.Summary.Name, "id", jobs[0].ID.String())
		if err != nil || promoted.Status != protocol.DispatchStatusCompleted {
			return fmt.Errorf("source capture could not settle promoted job: %v", err)
		}
		_, _, err = ExecuteDispatch(admitted, db, node, saved.Package, protocol.DispatchCommand{Kind: "read_job", Data: protocol.DispatchCommandReadJobPayload{ID: jobs[1].ID}}, 300)
		if failure, ok := err.(*protocol.Fault); !ok || failure.Code != protocol.ErrorCodeNotFound {
			return fmt.Errorf("source capture saw target-only job: %v", err)
		}
		sourceInfo, err := ReadPlugin(admitted, db, info.Summary.Name)
		if err != nil {
			return err
		}
		if _, err := SetPluginEnabled(admitted, db, info.Summary.Name, sourceInfo.Summary.Revision, false); err != nil {
			return err
		}
		targetInfo, err := ReadPlugin(targetCtx, db, info.Summary.Name)
		if err != nil || targetInfo == nil || !targetInfo.Summary.Enabled {
			return fmt.Errorf("admitted source disabled target's winning installation: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
