package store

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func testPluginMessageCommandResolution(t *testing.T) {
	t.Run("Unicode input and deterministic children", func(t *testing.T) {
		storage, node, session := pluginMessageSessionFixture(t)
		err := storage.Node.Write(t.Context(), func(db *sql.Conn) error {
			info, err := InstallPlugin(t.Context(), db, pluginMessageInfo("command-fixture", 'a', "fixture"), 0, true)
			if err != nil {
				return err
			}
			input := protocol.DefaultConversationInput()
			input.Text = "\u3000/fixture\u2003 你好\t世界 \n"
			input.Attachments = []protocol.AttachmentID{protocol.NewAttachmentID()}
			request := protocol.NewRequest(node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
				Session: session.ID, ExpectedRevision: session.Revision, Message: input,
			}})
			first, err := ResolvePluginMessageCommand(t.Context(), db, node, request)
			if err != nil || first == nil {
				return fmt.Errorf("Unicode command was not resolved: %v", err)
			}
			if first.Command.Kind != "call_plugin" || first.Plugin == nil || first.Plugin.Package != plugins.PackageReference(info) ||
				first.Plugin.Session == nil || *first.Plugin.Session != session.ID || first.Plugin.Worktree == nil || *first.Plugin.Worktree != session.Worktree ||
				first.Plugin.Surface.Kind != "workspace" || first.Plugin.Invocation != nil || first.Plugin.Turn != nil {
				return fmt.Errorf("command callback lost captured authority: %#v", first)
			}
			callback, err := providerPayload[protocol.CommandCallPluginPayload](first.Command)
			if err != nil {
				return err
			}
			var captured struct {
				Arguments string                     `json:"arguments"`
				Message   protocol.ConversationInput `json:"message"`
			}
			if err := json.Unmarshal(callback.Input, &captured); err != nil {
				return err
			}
			if callback.Handler != "command" || captured.Arguments != "你好\t世界" || !reflect.DeepEqual(captured.Message, input) {
				return fmt.Errorf("slash parsing changed callback input: %#v", captured)
			}
			repeated, err := ResolvePluginMessageCommand(t.Context(), db, node, request)
			if err != nil || repeated == nil || repeated.ID != first.ID || first.ID[6]>>4 != 8 || first.ID[8]&0xc0 != 0x80 {
				return fmt.Errorf("command child identity was not stable UUID v8: %v", err)
			}
			request.ID = protocol.NewRequestID()
			another, err := ResolvePluginMessageCommand(t.Context(), db, node, request)
			if err != nil || another == nil || another.ID == first.ID {
				return fmt.Errorf("distinct parent reused command identity: %v", err)
			}
			request.Command = protocol.Command{Kind: "submit_task", Data: protocol.CommandSubmitTaskPayload{Session: session.ID, Message: input}}
			task, err := ResolvePluginMessageCommand(t.Context(), db, node, request)
			if err != nil || task == nil {
				return fmt.Errorf("task without a revision did not resolve: %v", err)
			}
			request.Plugin = task.Plugin
			if child, err := ResolvePluginMessageCommand(t.Context(), db, node, request); err != nil || child != nil {
				return fmt.Errorf("plugin submission reentered command routing: %v", err)
			}
			request.Plugin = nil
			for _, text := range []string{"ordinary /fixture text", "/missing", "/", "\u3000/fixture-other"} {
				input.Text = text
				request.Command = protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: input}}
				if child, err := ResolvePluginMessageCommand(t.Context(), db, node, request); err != nil || child != nil {
					return fmt.Errorf("unmatched text %q was routed: %v", text, err)
				}
			}
			request.Command = protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision + 1, Message: protocol.ConversationInput{Text: "/fixture"}}}
			if _, err := ResolvePluginMessageCommand(t.Context(), db, node, request); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
				return fmt.Errorf("stale slash request bypassed session revision: %v", err)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	t.Run("Agent inventory excludes unrelated callbacks and rejects ambiguity", func(t *testing.T) {
		storage, node, session := pluginMessageSessionFixture(t)
		err := storage.Node.Write(t.Context(), func(db *sql.Conn) error {
			cases := []struct {
				name    string
				digest  byte
				prepare func(*protocol.PluginInfo)
				enabled bool
				matches bool
			}{
				{"tools-fixture", 'a', nil, true, true},
				{"skills-fixture", 'b', func(info *protocol.PluginInfo) {
					info.Extension.Tools = nil
					info.Skills = []protocol.PluginSkill{{Name: "fixture", Description: "Fixture", Path: "SKILL.md"}}
				}, true, true},
				{"mcp-fixture", 'c', func(info *protocol.PluginInfo) {
					info.Extension.Tools = nil
					info.Mcp = []protocol.PluginMcp{{Name: "fixture", Transport: protocol.PluginMcpTransportStdio, Availability: protocol.PluginMcpAvailabilityAvailable}}
				}, true, true},
				{"ui-fixture", 'd', func(info *protocol.PluginInfo) { info.Extension.Tools = nil }, true, false},
				{"desktop-fixture", 'e', func(info *protocol.PluginInfo) { info.Extension.Scope = protocol.PluginScopeDesktop }, true, false},
				{"disabled-fixture", 'f', nil, false, false},
			}
			for _, scenario := range cases {
				info := pluginMessageInfo(scenario.name, scenario.digest, scenario.name)
				if scenario.prepare != nil {
					scenario.prepare(&info)
				}
				if _, err := InstallPlugin(t.Context(), db, info, 0, scenario.enabled); err != nil {
					return err
				}
				request := protocol.NewRequest(node, protocol.Command{Kind: "submit_task", Data: protocol.CommandSubmitTaskPayload{Session: session.ID, Message: protocol.ConversationInput{Text: "/" + scenario.name}}})
				child, err := ResolvePluginMessageCommand(t.Context(), db, node, request)
				if err != nil || (child != nil) != scenario.matches {
					return fmt.Errorf("%s command capture: %#v %v", scenario.name, child, err)
				}
			}
			duplicate := pluginMessageInfo("duplicate-fixture", '1', "tools-fixture")
			if _, err := InstallPlugin(t.Context(), db, duplicate, 0, true); err != nil {
				return err
			}
			request := protocol.NewRequest(node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "/tools-fixture"}}})
			if _, err := ResolvePluginMessageCommand(t.Context(), db, node, request); !providerFixtureFault(err, protocol.ErrorCodeConflict) {
				return fmt.Errorf("ambiguous slash command did not conflict: %v", err)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	t.Run("assistant captures only its owner and declared package tools", func(t *testing.T) {
		storage, node, session := pluginMessageSessionFixture(t)
		err := storage.Node.Write(t.Context(), func(db *sql.Conn) error {
			dependency, err := InstallPlugin(t.Context(), db, pluginMessageInfo("dependency-fixture", 'a', "dependency"), 0, true)
			if err != nil {
				return err
			}
			unrelated, err := InstallPlugin(t.Context(), db, pluginMessageInfo("unrelated-fixture", 'b', "unrelated"), 0, true)
			if err != nil {
				return err
			}
			owner := pluginMessageInfo("assistant-fixture", 'c', "assistant")
			owner.Extension.Scope = protocol.PluginScopeDesktop
			owner.Extension.Desktop = new(protocol.DefaultPluginDesktopManifest())
			owner.Extension.Desktop.Conversations = []protocol.PluginConversationDeclaration{{
				ID: "assistant", Resource: protocol.PluginConversationResourceWorkspace, Context: "Fixture",
				Tools: []protocol.PluginConversationTool{
					{Kind: "package", Data: protocol.PluginConversationToolPackagePayload{Package: owner.Summary.Name, Name: "command"}},
					{Kind: "package", Data: protocol.PluginConversationToolPackagePayload{Package: dependency.Summary.Name, Name: "command"}},
					{Kind: "package", Data: protocol.PluginConversationToolPackagePayload{Package: dependency.Summary.Name, Name: "command"}},
				},
			}}
			owner, err = InstallPlugin(t.Context(), db, owner, 0, true)
			if err != nil {
				return err
			}
			binding := protocol.PluginConversationBinding{Package: plugins.PackageReference(owner), ID: "assistant"}
			session.Config.Assistant = &binding
			session.Revision++
			if err := insertSessionRevision(t.Context(), db, session); err != nil {
				return err
			}
			if _, err := db.ExecContext(t.Context(), "UPDATE sessions SET revision=?2 WHERE id=?1", session.ID.String(), session.Revision); err != nil {
				return err
			}
			for _, scenario := range []struct {
				name    string
				present bool
			}{{"assistant", true}, {"dependency", true}, {"unrelated", false}} {
				request := protocol.NewRequest(node, protocol.Command{Kind: "submit_task", Data: protocol.CommandSubmitTaskPayload{Session: session.ID, Message: protocol.ConversationInput{Text: "/" + scenario.name}}})
				child, err := ResolvePluginMessageCommand(t.Context(), db, node, request)
				if err != nil || (child != nil) != scenario.present {
					return fmt.Errorf("assistant command %s: %#v %v", scenario.name, child, err)
				}
			}
			packages, err := assistantPackages(t.Context(), db, binding)
			if err != nil || len(packages) != 2 {
				return fmt.Errorf("assistant dependency references were duplicated: %#v %v", packages, err)
			}
			if _, err := SetPluginEnabled(t.Context(), db, unrelated.Summary.Name, unrelated.Summary.Revision, false); err != nil {
				return err
			}
			if _, err := assistantPackages(t.Context(), db, binding); err != nil {
				return fmt.Errorf("unrelated package affected assistant commands: %v", err)
			}
			disabled, err := SetPluginEnabled(t.Context(), db, dependency.Summary.Name, dependency.Summary.Revision, false)
			if err != nil {
				return err
			}
			if _, err := assistantPackages(t.Context(), db, binding); !providerFixtureFault(err, protocol.ErrorCodeNotConfigured) {
				return fmt.Errorf("disabled assistant dependency remained available: %v", err)
			}
			if _, err := SetPluginEnabled(t.Context(), db, disabled.Summary.Name, disabled.Summary.Revision, true); err != nil {
				return err
			}
			if _, err := RemovePlugin(t.Context(), db, owner.Summary.Name, owner.Summary.Revision); err != nil {
				return err
			}
			if _, err := assistantPackages(t.Context(), db, binding); !providerFixtureFault(err, protocol.ErrorCodeNotConfigured) {
				return fmt.Errorf("removed assistant owner remained available: %v", err)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
}

func pluginMessageSessionFixture(t *testing.T) (*Store, protocol.NodeID, protocol.Session) {
	t.Helper()
	profile, node, storage := schemaFixture(t)
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = protocol.NewProviderID(), "fixture"
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	var session protocol.Session
	if err := storage.Node.Write(t.Context(), func(db *sql.Conn) error {
		output, _, err := createSession(t.Context(), db, node, profile, protocol.CommandCreateSessionPayload{Config: &config})
		if err == nil {
			session = output.Data.(protocol.Session)
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return storage, node, session
}

func pluginMessageInfo(name string, digest byte, command string) protocol.PluginInfo {
	info := pluginTestInfo(name, digest)
	info.Extension = new(protocol.DefaultPluginExtension())
	info.Extension.ApiVersion = "v1"
	info.Extension.Host = &protocol.PluginHostManifest{
		Entry: "dev.sailry.platform/host.js", Resources: []string{"dev.sailry.platform/host.js"}, Handlers: []string{"command"},
		Commands: []protocol.PluginHostCommand{{Name: command, Handler: "command"}},
	}
	tool := protocol.DefaultToolDeclaration()
	tool.Name, tool.Operation = "command", new(protocol.ToolOperationReadFile)
	info.Extension.Tools = []protocol.ToolDeclaration{tool}
	return info
}
