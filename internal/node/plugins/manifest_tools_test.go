package plugins

import (
	"encoding/json"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func testToolDeclarations(t *testing.T) {
	operation := func(value protocol.ToolOperation) *protocol.ToolOperation { return &value }
	var desktop protocol.PluginDesktopManifest
	var command protocol.PluginUiContribution
	for value, source := range map[any]string{
		&desktop: `{"ui_entry":"dev.sailry.platform/view.js","resources":["dev.sailry.platform/view.js"]}`,
		&command: `{"id":"remember","slot":"commands","kind":"button","label":{"label":"Remember"},"icon":"reicon:ui/target","handler":"command","command":{"name":"remember","kind":"message"}}`,
	} {
		if err := json.Unmarshal([]byte(source), value); err != nil {
			t.Fatal(err)
		}
	}
	for _, current := range []struct {
		name   string
		change func(*protocol.PluginExtension)
		valid  bool
	}{
		{"declared tools, routes and turn", func(*protocol.PluginExtension) {}, true},
		{"tool operation without its action", func(extension *protocol.PluginExtension) {
			extension.Tools[2].Operation = operation(protocol.ToolOperationWriteFile)
		}, false},
		{"handler operation without its action", func(extension *protocol.PluginExtension) {
			extension.Tools[0].Handler.Operation = operation(protocol.ToolOperationHttp)
		}, false},
		{"flow operation without its action", func(extension *protocol.PluginExtension) {
			extension.Tools[1].Handler.Flow.Operations = []protocol.ToolOperation{protocol.ToolOperationHttp}
		}, false},
		{"flow that reads, sends a request and writes its own values", func(extension *protocol.PluginExtension) {
			extension.Actions = append(extension.Actions, protocol.PluginActionHttp, protocol.PluginActionReadCommands)
			extension.Tools[1].Handler.Flow.Operations = []protocol.ToolOperation{
				protocol.ToolOperationReadFile, protocol.ToolOperationReadCommand, protocol.ToolOperationHttp,
				protocol.ToolOperationGetValue, protocol.ToolOperationSetValue, protocol.ToolOperationStorageTransaction,
			}
		}, true},
		{"flow without operations", func(extension *protocol.PluginExtension) { extension.Tools[1].Handler.Flow.Operations = nil }, false},
		{"flow repeating an operation", func(extension *protocol.PluginExtension) {
			extension.Tools[1].Handler.Flow.Operations = slices.Repeat([]protocol.ToolOperation{protocol.ToolOperationStorageTransaction}, 2)
		}, false},
		{"flow with a result handler", func(extension *protocol.PluginExtension) { extension.Tools[1].Handler.Result = new("shape") }, false},
		{"result handler without an operation", func(extension *protocol.PluginExtension) { extension.Tools[0].Handler.Result = new("shape") }, false},
		{"result handler of an operation", func(extension *protocol.PluginExtension) {
			extension.Tools[0].Handler.Operation, extension.Tools[0].Handler.Result = operation(protocol.ToolOperationGetValue), new("shape")
		}, true},
		{"handler preparing a guidance", func(extension *protocol.PluginExtension) {
			extension.Actions = append(extension.Actions, protocol.PluginActionDelegateAgents)
			extension.Tools[0].Handler.Operation = operation(protocol.ToolOperationGuideAgent)
		}, true},
		{"handler waiting for an agent", func(extension *protocol.PluginExtension) {
			extension.Actions = append(extension.Actions, protocol.PluginActionDelegateAgents)
			extension.Tools[0].Handler.Operation = operation(protocol.ToolOperationWaitAgent)
		}, false},
		{"handler reading settings itself", func(extension *protocol.PluginExtension) {
			extension.Tools[0].Handler.Operation = operation(protocol.ToolOperationReadSettings)
		}, false},
		{"handler tool without a description", func(extension *protocol.PluginExtension) { extension.Tools[0].Description = nil }, false},
		{"handler parameters that are no object schema", func(extension *protocol.PluginExtension) {
			extension.Tools[0].Handler.Parameters = json.RawMessage(`{"type":"string"}`)
		}, false},
		{"handler tool on a server", func(extension *protocol.PluginExtension) { extension.Tools[0].Server = new("search") }, false},
		{"handler tool with a spaced name", func(extension *protocol.PluginExtension) { extension.Tools[0].Name = "recall notes" }, false},
		{"server tool with a long name", func(extension *protocol.PluginExtension) {
			extension.Tools = append(extension.Tools, protocol.ToolDeclaration{Name: strings.Repeat("n", 200), Server: new("search")})
		}, true},
		{"retired operation", func(extension *protocol.PluginExtension) {
			extension.Tools[2].Operation = operation(protocol.ToolOperationCodeHover)
		}, false},
		{"retired action", func(extension *protocol.PluginExtension) {
			extension.Actions = append(extension.Actions, protocol.PluginActionReadCode)
		}, false},
		{"unknown platform", func(extension *protocol.PluginExtension) { extension.Tools[2].Platforms = []string{"plan9"} }, false},
		{"initial tool inside package storage", func(extension *protocol.PluginExtension) {
			extension.Tools[0].Initial, extension.Tools[1].Initial = true, true
		}, true},
		{"initial tool that reads files", func(extension *protocol.PluginExtension) { extension.Tools[2].Initial = true }, false},
		{"initial handler that may write", func(extension *protocol.PluginExtension) {
			extension.Tools[0].Initial, extension.Tools[0].Handler.ReadOnly = true, false
		}, false},
		{"undeclared turn handler", func(extension *protocol.PluginExtension) { extension.Host.Turn.AfterTurn = new("missing") }, false},
		{"undeclared command handler", func(extension *protocol.PluginExtension) { extension.Host.Commands[0].Handler = "missing" }, false},
		{"command name with capitals", func(extension *protocol.PluginExtension) { extension.Host.Commands[0].Name = "Remember" }, false},
		{"duplicate command", func(extension *protocol.PluginExtension) {
			extension.Host.Commands = append(extension.Host.Commands, extension.Host.Commands[0])
		}, false},
		{"handler name a script cannot export", func(extension *protocol.PluginExtension) {
			extension.Host.Handlers = append(extension.Host.Handlers, "on-save")
		}, false},
		{"host resource that is no script", func(extension *protocol.PluginExtension) {
			extension.Host.Resources = append(extension.Host.Resources, "dev.sailry.platform/data.json")
		}, false},
		{"message command with its route", func(extension *protocol.PluginExtension) {
			extension.Desktop = &desktop
			extension.Ui = []protocol.PluginUiContribution{command}
		}, true},
		{"message command without a route", func(extension *protocol.PluginExtension) {
			extension.Host.Commands = nil
			extension.Desktop = &desktop
			extension.Ui = []protocol.PluginUiContribution{command}
		}, false},
		{"artifact view without the artifact actions", func(extension *protocol.PluginExtension) {
			view := protocol.DefaultArtifactView()
			extension.ArtifactView = &view
		}, false},
		{"blank display", func(extension *protocol.PluginExtension) {
			extension.Display = &protocol.PluginDesktopNavigation{Label: " "}
		}, false},
		{"display with a brand mark", func(extension *protocol.PluginExtension) {
			extension.Display = &protocol.PluginDesktopNavigation{Label: "Notes", Icon: &protocol.PluginDesktopIcon{Kind: "Name", Data: "ailogo:notes"}}
		}, true},
		{"icon outside the namespace", func(extension *protocol.PluginExtension) { extension.Icon = new("icon.png") }, false},
		{"enabled service without its action", func(extension *protocol.PluginExtension) {
			service := protocol.PluginServiceCodeIndex
			extension.EnabledService = &service
		}, false},
		{"conversation options choosing a model", func(extension *protocol.PluginExtension) {
			options := json.RawMessage(`{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","additionalProperties":false,"properties":{"model":{"type":"string","title":"Model","x-sailry-model":true,"minLength":1}}}`)
			extension.ConversationOptions = &options
		}, false},
		{"conversation options of public scalars", func(extension *protocol.PluginExtension) {
			options := json.RawMessage(`{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","additionalProperties":false,"properties":{"name":{"type":"string","title":"Name","default":"","maxLength":24}}}`)
			extension.ConversationOptions = &options
		}, true},
		{"model adapter without its action", func(extension *protocol.PluginExtension) {
			extension.ModelAdapters = []protocol.PluginModelAdapterDeclaration{protocol.DefaultPluginModelAdapterDeclaration()}
		}, false},
		{"model provider naming an undeclared adapter", func(extension *protocol.PluginExtension) {
			extension.ModelProviders = []protocol.ConversationPresetsPreset{{ID: "notes", Adapter: new("missing")}}
		}, false},
		{"model provider naming an undeclared sign-in", func(extension *protocol.PluginExtension) {
			extension.ModelProviders = []protocol.ConversationPresetsPreset{{
				ID: "notes", Auth: protocol.ConversationPresetsAuth{Kind: "plugin", Data: &protocol.ConversationPresetsAuthPluginPayload{Flow: "missing"}},
			}}
		}, false},
		{"permission with declared callbacks", func(extension *protocol.PluginExtension) {
			extension.Permissions = []protocol.PermissionsContribution{{Kind: "accessibility", Subject: "dev.fixture", Check: new("recall"), Request: new("save"), Actions: []protocol.PluginAction{protocol.PluginActionReadStorage}}}
		}, true},
		{"permission checked by a handler that may write", func(extension *protocol.PluginExtension) {
			extension.Permissions = []protocol.PermissionsContribution{{Kind: "accessibility", Subject: "dev.fixture", Check: new("save")}}
		}, false},
		{"permission requested by an undeclared handler", func(extension *protocol.PluginExtension) {
			extension.Permissions = []protocol.PermissionsContribution{{Kind: "accessibility", Subject: "dev.fixture", Check: new("recall"), Request: new("missing")}}
		}, false},
		{"permission using an undeclared action", func(extension *protocol.PluginExtension) {
			extension.Permissions = []protocol.PermissionsContribution{{Kind: "accessibility", Subject: "dev.fixture", Check: new("recall"), Actions: []protocol.PluginAction{protocol.PluginActionControlComputer}}}
		}, false},
		{"permission without a check", func(extension *protocol.PluginExtension) {
			extension.Permissions = []protocol.PermissionsContribution{{Kind: "accessibility", Subject: "dev.fixture"}}
		}, false},
	} {
		t.Run(current.name, func(t *testing.T) {
			var extension protocol.PluginExtension
			if err := json.Unmarshal([]byte(`{
				"api_version":"v1","scope":"host","actions":["storage.read","storage.write","files.read"],
				"host":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],
					"handlers":["recall","save","shape"],"read_only":["recall"],
					"turn":{"initialize":"recall","before_model":"recall","after_turn":"save"},
					"commands":[{"name":"remember","handler":"save"}]},
				"tools":[
					{"name":"recall","description":"Recall","handler":{"name":"recall","parameters":{"type":"object"},"read_only":true}},
					{"name":"save","description":"Save","handler":{"name":"save","parameters":{"type":"object"},"flow":{"operations":["storage.transaction"]}}},
					{"name":"list","operation":"files.list"}]
			}`), &extension); err != nil {
				t.Fatal(err)
			}
			current.change(&extension)
			if err := validateExtension(extension); (err == nil) != current.valid {
				t.Fatalf("extension declaration: %v", err)
			}
		})
	}

	// A flow step has no approval of its own. A flow that declares a change
	// outside its package's values, or an operation that is the model's own,
	// is refused at install by name, whatever actions the package declares.
	t.Run("flow operations a step cannot run", func(t *testing.T) {
		for operation, action := range map[protocol.ToolOperation]protocol.PluginAction{
			protocol.ToolOperationWriteFile:       protocol.PluginActionWriteFiles,
			protocol.ToolOperationEditFile:        protocol.PluginActionWriteFiles,
			protocol.ToolOperationDeleteFile:      protocol.PluginActionWriteFiles,
			protocol.ToolOperationMoveFile:        protocol.PluginActionWriteFiles,
			protocol.ToolOperationCreateDirectory: protocol.PluginActionWriteFiles,
			protocol.ToolOperationApplyChanges:    protocol.PluginActionWriteFiles,
			protocol.ToolOperationEditNotebook:    protocol.PluginActionWriteFiles,
			protocol.ToolOperationWriteArtifact:   protocol.PluginActionWriteArtifacts,
			protocol.ToolOperationRunCommand:      protocol.PluginActionControlCommands,
			protocol.ToolOperationWriteCommand:    protocol.PluginActionControlCommands,
			protocol.ToolOperationExecuteDatabase: protocol.PluginActionControlDatabases,
			protocol.ToolOperationRunSsh:          protocol.PluginActionControlSsh,
			protocol.ToolOperationRemoveWorktree:  protocol.PluginActionWriteWorktrees,
			protocol.ToolOperationSaveRole:        protocol.PluginActionWriteRoles,
			protocol.ToolOperationKnowledgeCreate: protocol.PluginActionWriteKnowledge,
			protocol.ToolOperationReadBrowser:     protocol.PluginActionReadBrowser,
			protocol.ToolOperationControlComputer: protocol.PluginActionControlComputer,
			protocol.ToolOperationGenerateImage:   protocol.PluginActionGenerateImage,
			protocol.ToolOperationDelegateAgent:   protocol.PluginActionDelegateAgents,
			protocol.ToolOperationWaitAgent:       protocol.PluginActionDelegateAgents,
			protocol.ToolOperationProgress:        "",
			protocol.ToolOperationReadSettings:    "",
		} {
			var extension protocol.PluginExtension
			if err := json.Unmarshal([]byte(`{
				"api_version":"v1","scope":"host","actions":["files.read"],
				"host":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],"handlers":["step"]},
				"tools":[{"name":"steps","description":"Steps","handler":{"name":"step","parameters":{"type":"object"},"flow":{"operations":["files.read"]}}}]
			}`), &extension); err != nil {
				t.Fatal(err)
			}
			if err := validateExtension(extension); err != nil {
				t.Fatalf("reading flow: %v", err)
			}
			if action != "" {
				extension.Actions = append(extension.Actions, action)
			}
			extension.Tools[0].Handler.Flow.Operations = append(extension.Tools[0].Handler.Flow.Operations, operation)
			if err := validateExtension(extension); err == nil || !strings.Contains(err.Error(), "plugin flows cannot run "+string(operation)) {
				t.Errorf("flow declaring %s: %v", operation, err)
			}
		}
	})
}
