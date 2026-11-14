package store

import (
	"database/sql"
	"fmt"
	"reflect"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAssistantTurnPackageUpdates(t *testing.T) {
	t.Parallel()
	storage, node, ordinary, _, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info := pluginMessageInfo("assistant-update-fixture", 'a', "fixture")
		info.Extension.Desktop = new(protocol.DefaultPluginDesktopManifest())
		info.Extension.Desktop.Conversations = []protocol.PluginConversationDeclaration{{
			ID: "ssh", Resource: protocol.PluginConversationResourceSsh, Context: "Old SSH assistant context",
		}}
		info.Extension.Host.Entry = "old-host.js"
		owner, err := InstallPlugin(ctx, db, info, 0, true)
		if err != nil {
			return err
		}
		oldReference := plugins.PackageReference(owner)
		output, _, err := ExecuteSshCommand(ctx, db, protocol.Command{Kind: "save_ssh", Data: &protocol.CommandSaveSshPayload{
			Profile:    protocol.SshProfile{ID: protocol.NewSshID(), Name: "Primary", Host: "fixture.invalid", Port: 22, Username: "fixture", Authentication: protocol.SshAuthenticationAgent},
			Credential: &protocol.SshCredential{Kind: "agent"},
		}})
		if err != nil {
			return err
		}
		primary := output.Data.(protocol.SshProfile)
		config := ordinary.Config
		config.Assistant = &protocol.PluginConversationBinding{Package: oldReference, ID: "ssh"}
		config.Resource = &protocol.ConnectionResource{Kind: "ssh", Data: primary.ID}
		output, _, err = createSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &config})
		if err != nil {
			return err
		}
		session := output.Data.(protocol.Session)
		baseline, err := ReadSession(ctx, db, session.ID)
		if err != nil {
			return err
		}
		admit := func() (protocol.QueuedTurn, error) {
			request := protocol.NewRequest(node, protocol.Command{Kind: "submit_turn", Data: &protocol.CommandSubmitTurnPayload{
				Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Inspect another saved SSH connection"},
			}})
			body, err := encode(request)
			if err != nil {
				return protocol.QueuedTurn{}, err
			}
			if _, err = admission.Admit(ctx, db, node, request, body); err != nil {
				return protocol.QueuedTurn{}, err
			}
			return admitAgentTurn(ctx, db, node, node, request, session, protocol.ConversationRunKindTask, true)
		}
		first, err := admit()
		if err != nil {
			return err
		}
		running, _, err := claimAgentInvocation(ctx, db, first.ID, now)
		if err != nil || running == nil || running.Assistant == nil || running.Assistant.Context != "Old SSH assistant context" {
			return fmt.Errorf("old assistant did not start: %v", err)
		}
		queued, err := admit()
		if err != nil {
			return err
		}
		info.Summary.Digest = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
		version := "0.1.1"
		info.Summary.Version = &version
		info.Extension.Host.Entry = "new-host.js"
		info.Extension.Desktop.Conversations[0].Context = "Current SSH assistant context"
		owner, err = InstallPlugin(ctx, db, info, owner.Summary.Revision, true)
		if err != nil {
			return err
		}
		current := plugins.PackageReference(owner)
		latest, err := admit()
		if err != nil {
			return err
		}
		if latest.Config.Assistant.Package != current || !slices.Contains(latest.Plugins, current) || slices.Contains(latest.Plugins, oldReference) {
			return fmt.Errorf("existing assistant pinned a new turn to its old package: %+v", latest)
		}
		for _, expected := range []struct {
			turn      protocol.QueuedTurn
			reference protocol.PluginReference
			context   string
			entry     string
		}{
			{first, oldReference, "Old SSH assistant context", "old-host.js"},
			{queued, oldReference, "Old SSH assistant context", "old-host.js"},
			{latest, current, "Current SSH assistant context", "new-host.js"},
		} {
			captured, err := readTurnCapture(ctx, db, expected.turn.ID)
			if err != nil {
				return err
			}
			inv := AgentInvocation{Turn: captured}
			if err = captureAssistant(ctx, db, &inv); err != nil {
				return err
			}
			packageInfo, err := ReadPluginVersion(ctx, db, inv.Turn.Config.Assistant.Package)
			if err != nil || inv.Turn.Config.Assistant.Package != expected.reference || inv.Assistant.Context != expected.context || packageInfo.Extension.Host.Entry != expected.entry {
				return fmt.Errorf("turn did not retain its admitted assistant and script: %+v, %v", inv.Turn.Config.Assistant, err)
			}
			if inv.Turn.Config.Assistant.ID != "ssh" || !sameAssistant(inv.Turn.Config.Resource, session.Config.Resource) {
				return fmt.Errorf("package refresh changed the assistant or its primary SSH resource")
			}
		}
		// An inherited turn follows its frozen inventory even if the source
		// session binding was resolved after the package update.
		inherited := latest
		inherited.Plugins = slices.Clone(first.Plugins)
		inv := AgentInvocation{Turn: inherited}
		if err = captureAssistant(ctx, db, &inv); err != nil || inv.Turn.Config.Assistant.Package != oldReference || inv.Assistant.Context != "Old SSH assistant context" {
			return fmt.Errorf("inherited capture read the live inventory: %v", err)
		}
		if _, _, err = FinishTurn(ctx, db, first.ID, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		claimed, _, err := claimAgentInvocation(ctx, db, queued.ID, now)
		if err != nil || claimed == nil || claimed.Assistant.Context != "Old SSH assistant context" || claimed.Turn.Config.Assistant.Package != oldReference {
			return fmt.Errorf("queued turn changed its assistant after an update: %v", err)
		}
		if _, _, err = FinishTurn(ctx, db, queued.ID, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		claimed, _, err = claimAgentInvocation(ctx, db, latest.ID, now)
		if err != nil || claimed == nil || claimed.Assistant.Context != "Current SSH assistant context" || claimed.Turn.Config.Assistant.Package != current {
			return fmt.Errorf("new turn did not claim the upgraded assistant: %v", err)
		}
		stored, err := ReadSession(ctx, db, session.ID)
		if err != nil {
			return err
		}
		for _, expected := range []struct {
			name string
			got  protocol.PluginReference
			want protocol.PluginReference
		}{
			{"running capture", running.Turn.Config.Assistant.Package, oldReference},
			{"first admission", first.Config.Assistant.Package, oldReference},
			{"new admission", latest.Config.Assistant.Package, current},
			{"source configuration", config.Assistant.Package, oldReference},
		} {
			if expected.got != expected.want {
				return fmt.Errorf("package refresh mutated %s: got %+v, want %+v", expected.name, expected.got, expected.want)
			}
		}
		if !reflect.DeepEqual(stored.Config, baseline.Config) {
			before, _ := encode(baseline.Config)
			after, _ := encode(stored.Config)
			return fmt.Errorf("package refresh mutated stored configuration: before %s, after %s", before, after)
		}
		if stored.Revision != baseline.Revision {
			return fmt.Errorf("package refresh changed session revision: got %d, want %d", stored.Revision, baseline.Revision)
		}
		if stored.Worktree != baseline.Worktree {
			return fmt.Errorf("package refresh changed session worktree: got %s, want %s", stored.Worktree, baseline.Worktree)
		}
		// An update cannot change the existing conversation's resource kind.
		info.Summary.Digest = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
		version = "0.1.2"
		info.Summary.Version = &version
		info.Extension.Desktop.Conversations[0].Resource = protocol.PluginConversationResourcePlugin
		if _, err = InstallPlugin(ctx, db, info, owner.Summary.Revision, true); err != nil {
			return err
		}
		if _, err = admit(); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
			return fmt.Errorf("changed assistant resource was admitted: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
