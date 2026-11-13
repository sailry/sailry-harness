package plugins

import (
	"context"
	"encoding/json"
)

type hostModuleNames struct {
	module string
	sync   []string
	async  []string
}

var nativeModuleNames = []hostModuleNames{
	{module: "sailry", sync: []string{"theme", "focusTerminal", "releaseTerminal"}, async: []string{"header_action", "nextAssistantTools"}},
	{module: "sailry/models", sync: []string{"register", "prepare"}, async: []string{"list", "search", "versions", "resolve"}},
	{module: "sailry/forms", sync: []string{
		"createText", "readText", "setText", "focusText", "isTextFocused", "releaseText",
		"createDateTime", "readDateTime", "releaseDateTime",
		"createSlider", "readSlider", "setSlider", "releaseSlider",
	}, async: []string{"nextTextEvent", "nextSliderEvent"}},
	{module: "sailry/credentials", sync: []string{"createSecret", "releaseSecret", "describeSecret", "clearSecret"}, async: []string{"nextSecretEvent", "chooseSecretFile"}},
	{module: "sailry/ui", sync: []string{
		"pageInsets", "openActivityTarget", "readBrowser", "readBrowserSettings", "setBrowserPersistent",
		"readModelContext", "revealComposerEntry", "readComposer", "focusEntry", "registerShortcuts",
		"closePanel", "inspectFilePath", "openFileImage", "collapseOpen", "readLocation",
		"selectSettingsHost", "openSettings", "openSession", "toast", "dismissToast",
		"pane", "openPane", "updatePane", "closePane", "publishResourceTitles", "fileResource",
		"readProviderContexts", "writeClipboard", "readResourceTarget", "showWorkspaceDetails", "workspaceMode",
	}, async: []string{
		"nextCardEvent", "nextBrowserChange", "nextBrowserShortcut", "browserAction", "submitBrowserAnnotation",
		"listBrowserProfiles", "importBrowserProfile", "nextModelContext", "nextComposerChange", "setComposerOption",
		"nextControlEvent", "nextNavigationTabEvent", "nextTableEvent", "nextTreeEvent", "nextMenuEvent", "nextContextMenuEvent",
		"modal_closed", "nextToastEvent", "nextPaneEvent", "nextPickerEvent", "nextProviderContexts",
		"invokeContribution", "canInvokeContribution", "nextLocationChange", "selectLocation", "worktreeHasDrafts",
		"releaseWorktree", "openResource", "openFile", "openProjectCreation", "canInsertFileReferences",
		"insertFileReferences", "confirmAction", "nextResourceTarget", "nextWindowActivation",
	}},
	{module: "sailry/ssh-transfers", sync: []string{"uploadDropped", "transferAction", "previewFile"}, async: []string{"selectUpload", "selectDownload", "nextTransfers"}},
	{module: "sailry/file-transfers", sync: []string{"readClipboard", "systemFileManager", "readTransfers"}, async: []string{
		"captureClipboard", "preparePaste", "prepareMove", "selectUpload", "startDownload", "openSystem", "revealSystem", "transferAction", "nextTransferChange",
	}},
	{module: "sailry/documents", sync: []string{"readDocuments", "refreshDocuments"}, async: []string{"nextDocumentChange", "openDocument", "documentAction", "saveDocument", "closeDocument"}},
	{module: "sailry/artifacts", sync: []string{"artifactResource"}, async: []string{"readArtifact", "listArtifactRevisions", "nextArtifactChange", "nextArtifactEvent", "exportArtifact"}},
	{module: "sailry/card", sync: []string{"state", "theme", "publishProps", "toast"}, async: []string{"status", "nextChange"}},
}

func (owner *Host) Modules() ([]HostModule, error) {
	modules := []HostModule{
		owner.baseModule(), owner.sdkModule(), owner.connectorModule(), owner.installationModule(),
		owner.connectionModule(), owner.settingsModule(), owner.searchModule(),
	}
	for _, names := range nativeModuleNames {
		exports := make([]HostExport, 0, len(names.sync)+len(names.async))
		for _, name := range names.sync {
			operation := owner.nativeOperation(names.module, name)
			if names.module == "sailry/models" {
				operation = owner.modelOperation(name)
			}
			exports = append(exports, hostSync(name, operation))
		}
		for _, name := range names.async {
			operation := owner.nativeOperation(names.module, name)
			if names.module == "sailry/models" {
				operation = owner.modelOperation(name)
			}
			exports = append(exports, hostAsync(name, operation))
		}
		found := false
		for index, module := range modules {
			if module.Name == names.module {
				modules[index].Exports = append(module.Exports, exports...)
				found = true
				break
			}
		}
		if !found {
			modules = append(modules, HostModule{Name: names.module, Exports: exports})
		}
	}
	for moduleIndex := range modules {
		for exportIndex := range modules[moduleIndex].Exports {
			module := modules[moduleIndex].Name
			name := modules[moduleIndex].Exports[exportIndex].Name
			wrap := func(operation HostOperation) HostOperation {
				if operation == nil {
					return nil
				}
				return func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
					if err := owner.check(ctx); err != nil {
						return nil, hostNativeError(module, name, err)
					}
					lifetime, cancel := context.WithCancel(ctx)
					stop := context.AfterFunc(owner.ctx, cancel)
					defer func() { stop(); cancel() }()
					value, err := operation(lifetime, arguments)
					if err != nil {
						return nil, hostNativeError(module, name, err)
					}
					return value, nil
				}
			}
			export := &modules[moduleIndex].Exports[exportIndex]
			export.Sync, export.Async = wrap(export.Sync), wrap(export.Async)
		}
	}
	return modules, nil
}

func (owner *Host) baseModule() HostModule {
	return HostModule{Name: "sailry", Exports: []HostExport{
		hostSync("context", func(context.Context, []json.RawMessage) (json.RawMessage, error) {
			scope := owner.scope()
			value, _ := hostJSON(scope)
			decoded, err := decodeHostJSON(value)
			if err != nil {
				return nil, err
			}
			hostStringRevision(decoded, "package", "settings_revision")
			object := decoded.(map[string]any)
			object["api_version"], object["node"], object["locale"] = "v1", owner.target(), owner.binding.Locale
			encoded, err := json.Marshal(object)
			if err != nil {
				return nil, err
			}
			return hostJSON(string(encoded))
		}),
		hostSync("prepare", func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			body, err := hostValue[string](arguments, 0)
			if err != nil {
				return nil, err
			}
			command, err := decodeHostCommand([]byte(body))
			if err != nil {
				return nil, err
			}
			return owner.prepare(ctx, command)
		}),
		hostAsync("execute", func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			identity, err := hostValue[string](arguments, 0)
			if err != nil {
				return nil, err
			}
			value, err := owner.complete(ctx, identity, false)
			if err != nil {
				return nil, err
			}
			return hostJSON(string(value))
		}),
		hostSync("forget", func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			identity, err := hostValue[string](arguments, 0)
			if err != nil {
				return nil, err
			}
			return owner.forget(ctx, identity)
		}),
		hostAsync("outcome", func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			identity, err := hostValue[string](arguments, 0)
			if err != nil {
				return nil, err
			}
			value, err := owner.outcome(ctx, identity)
			if err != nil {
				value, _ = hostJSON(map[string]any{"Err": hostFault(err)})
			} else {
				value, _ = hostJSON(map[string]any{"Ok": value})
			}
			return hostJSON(string(value))
		}),
		hostAsync("next_change", func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			return owner.changes.nextFiles(ctx, arguments)
		}),
	}}
}
