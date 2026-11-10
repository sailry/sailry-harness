//go:build integration

package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/jonboulle/clockwork"
	sailry "github.com/sailry/sailry-harness"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	desktop "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	packages "github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type entry struct {
	Manifest string
	Path     string
	Surface  protocol.PluginDesktopSurface
}

type result struct {
	Package     string   `json:"package"`
	Entry       string   `json:"entry"`
	Example     bool     `json:"example"`
	Surface     string   `json:"surface"`
	Passed      bool     `json:"passed"`
	Texts       []string `json:"texts,omitempty"`
	Elements    int      `json:"elements"`
	Callbacks   int      `json:"callbacks"`
	Pending     int      `json:"pending"`
	Unavailable []string `json:"unavailable,omitempty"`
	Error       string   `json:"error,omitempty"`
}

func main() {
	root := flag.String("root", ".", "repository root containing the pinned packages")
	profile := flag.String("data-dir", ".runtime/profiles/b9-desktop-spike", "isolated immutable package cache")
	match := flag.String("package", "", "optional exact package name for diagnostics")
	flag.Parse()
	if err := run(*root, *profile, *match); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run(root, profile, match string) error {
	absolute, err := filepath.Abs(profile)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(absolute, 0o700); err != nil {
		return err
	}
	manager := packages.NewManager(absolute)
	entries, err := inventory()
	if err != nil {
		return err
	}
	ctx := context.Background()
	var failures int
	var count int
	names := map[string]bool{}
	encoder := json.NewEncoder(os.Stdout)
	for _, current := range entries {
		raw, err := fs.ReadFile(sailry.Plugins, current.Manifest)
		if err != nil {
			return err
		}
		manifest, err := packages.ValidateManifest(raw)
		if err != nil {
			return fmt.Errorf("%s: %w", current.Manifest, err)
		}
		if match != "" && manifest.Name != match {
			continue
		}
		names[manifest.Name] = true
		count++
		var info protocol.PluginInfo
		directory := path.Dir(current.Manifest)
		if path.Dir(directory) == "plugins" {
			info, err = manager.InstallBundled(ctx, manifest.Name)
		} else {
			info, err = manager.Install(ctx, filepath.Join(root, filepath.FromSlash(directory)), "", manifest.Name)
		}
		report := result{Package: manifest.Name, Entry: current.Path, Surface: current.Surface.Kind, Example: strings.Contains(current.Manifest, "/examples/")}
		if err == nil {
			var bundle protocol.PluginDesktopBundle
			bundle, err = manager.ReadDesktop(ctx, info, current.Surface)
			if err == nil {
				bundle.Entry = current.Path
				report, err = render(ctx, info, bundle, current, report)
			}
		}
		report.Passed = err == nil
		if err != nil {
			report.Error = err.Error()
			failures++
		}
		if err := encoder.Encode(report); err != nil {
			return err
		}
	}
	if err := encoder.Encode(map[string]any{
		"kind": "fixture_summary", "packages": len(names), "entries": count, "failed": failures,
		"boundary": "Production moejs and MyGo rendering with controlled Host fixtures; live backend acceptance is separate",
	}); err != nil {
		return err
	}
	if count == 0 {
		return errors.New("no declared desktop entries matched")
	}
	if failures != 0 {
		return fmt.Errorf("%d of %d declared desktop entries failed", failures, count)
	}
	return nil
}

func inventory() ([]entry, error) {
	var entries []entry
	err := fs.WalkDir(sailry.Plugins, "plugins", func(name string, directory fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if directory.IsDir() || path.Base(name) != "plugin.json" {
			return nil
		}
		body, err := fs.ReadFile(sailry.Plugins, name)
		if err != nil {
			return err
		}
		var manifest struct {
			Extensions map[string]json.RawMessage `json:"extensions"`
		}
		if err := json.Unmarshal(body, &manifest); err != nil {
			return fmt.Errorf("%s: %w", name, err)
		}
		extensionBody, exists := manifest.Extensions["dev.sailry.platform"]
		if !exists {
			return nil
		}
		var extension struct {
			Desktop *struct {
				Entry   *string `json:"entry"`
				UiEntry *string `json:"ui_entry"`
			} `json:"desktop"`
			SettingsPage *struct {
				Entry *string `json:"entry"`
			} `json:"settings_page"`
			Cards []struct {
				Entry string `json:"entry"`
			} `json:"cards"`
		}
		if err := json.Unmarshal(extensionBody, &extension); err != nil {
			return fmt.Errorf("%s: %w", name, err)
		}
		seen := map[string]bool{}
		add := func(script *string, surface protocol.PluginDesktopSurface) {
			if script != nil && !seen[*script] {
				seen[*script] = true
				entries = append(entries, entry{Manifest: name, Path: *script, Surface: surface})
			}
		}
		if extension.Desktop != nil {
			add(extension.Desktop.Entry, protocol.PluginDesktopSurface{Kind: "workspace"})
			add(extension.Desktop.UiEntry, protocol.PluginDesktopSurface{Kind: "composer"})
		}
		if extension.SettingsPage != nil {
			add(extension.SettingsPage.Entry, protocol.PluginDesktopSurface{Kind: "settings"})
		}
		for _, card := range extension.Cards {
			add(&card.Entry, protocol.PluginDesktopSurface{Kind: "card"})
		}
		return nil
	})
	return entries, err
}

func render(ctx context.Context, info protocol.PluginInfo, bundle protocol.PluginDesktopBundle, current entry, report result) (result, error) {
	fake := clockwork.NewFakeClock()
	scope := protocol.DefaultPluginContext()
	scope.Package, scope.Surface = bundle.Package, current.Surface
	worktree, session := protocol.NewWorktreeID(), protocol.NewSessionID()
	if scope.Surface.Kind != "settings" {
		scope.Worktree, scope.Session = &worktree, &session
	}
	if scope.Surface.Kind == "card" {
		scope.Surface.Data = protocol.PluginCardReceipt{Session: session, Turn: protocol.NewTurnID(), Call: protocol.NewRequestID()}
	}
	target := protocol.NodeID{1}
	registry := desktop.NewRegistry(desktop.RegistryOptions{Node: target, Context: scope})
	defer registry.Close()
	hooks, err := registry.Hooks(scope.Package, info.Extension.Ui)
	if err != nil {
		return report, err
	}
	posted := make(chan func(), 1024)
	native := desktop.NewNativeBinding(ctx)
	native.Post = func(function func(*ui.Context)) bool {
		posted <- func() { function(nil) }
		return true
	}
	native.Theme = func() json.RawMessage { return desktop.ThemeSnapshot(theme.Look("", false)) }
	native.Operations = fixtureOperations(target, scope, native.Theme)
	native.Components = fixtureSurfaces(scope)
	host := desktop.NewHost(desktop.HostBinding{
		Context: ctx, Target: target, Scope: scope, Locale: "en", Native: &native, Contributions: hooks, Clock: fake,
	})
	defer host.Close()
	modules, err := host.Modules()
	if err != nil {
		return report, err
	}
	overrides := fixtureReads(info)
	unavailable := make(chan string, 1024)
	for moduleIndex := range modules {
		for exportIndex := range modules[moduleIndex].Exports {
			export := &modules[moduleIndex].Exports[exportIndex]
			key := modules[moduleIndex].Name + "." + export.Name
			if operation := overrides[key]; operation != nil {
				if export.Async != nil {
					export.Async = operation
				} else {
					export.Sync = operation
				}
			}
			wrap := func(operation desktop.HostOperation) desktop.HostOperation {
				if operation == nil {
					return nil
				}
				return func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
					value, err := operation(ctx, arguments)
					if err != nil && !errors.Is(err, context.Canceled) {
						unavailable <- key + ": " + err.Error()
					}
					return value, err
				}
			}
			export.Sync, export.Async = wrap(export.Sync), wrap(export.Async)
		}
	}
	components, err := desktop.NativeComponents(host.Native())
	if err != nil {
		return report, err
	}
	runtime, err := desktop.NewRuntime(desktop.RuntimeBindings{
		Modules: modules, Components: components, Contributions: hooks, Post: func(function func()) { posted <- function },
		BindActions: host.Native().BindActions,
	})
	if err != nil {
		return report, err
	}
	defer runtime.Close()
	if err := runtime.SetClock(fake); err != nil {
		return report, err
	}
	mounted, err := runtime.Mount(bundle, scope)
	if err != nil {
		return report, err
	}
	if err := registry.SetActivity(scope.Package, true, false); err != nil {
		return report, err
	}
	if err := settle(mounted, posted); err != nil {
		return report, err
	}
	var renderErr error
	tester := ui.NewTester(func(context *ui.Context) {
		theme.Apply(context, "", theme.Light)
		_, renderErr = mounted.Render(context)
	}, 1280, 820)
	tester.Frame()
	if renderErr != nil {
		return report, renderErr
	}
	fake.Advance(16 * time.Millisecond)
	if err := settle(mounted, posted); err != nil {
		return report, err
	}
	tester.Frame()
	if renderErr != nil {
		return report, renderErr
	}
	snapshot, err := mounted.Snapshot()
	if err != nil {
		return report, err
	}
	var count func(*desktop.NativeElement)
	count = func(element *desktop.NativeElement) {
		if element == nil {
			return
		}
		report.Elements++
		report.Callbacks += len(element.Callbacks)
		for _, child := range element.Children {
			count(child)
		}
		for _, children := range element.Slots {
			for _, child := range children {
				count(child)
			}
		}
	}
	count(snapshot)
	report.Texts, report.Pending = tester.Texts(), mounted.Pending()
	for {
		select {
		case failure := <-unavailable:
			if !slices.Contains(report.Unavailable, failure) {
				report.Unavailable = append(report.Unavailable, failure)
			}
		default:
			return report, nil
		}
	}
}

func settle(mounted *desktop.Mounted, posted <-chan func()) error {
	timer := time.NewTimer(20 * time.Millisecond)
	defer timer.Stop()
	for {
		select {
		case function := <-posted:
			function()
			if err := mounted.Error(); err != nil {
				return err
			}
			if !timer.Stop() {
				select {
				case <-timer.C:
				default:
				}
			}
			timer.Reset(20 * time.Millisecond)
		case <-timer.C:
			return mounted.Pump()
		}
	}
}

func constant(value any) desktop.HostOperation {
	body, err := json.Marshal(value)
	return func(context.Context, []json.RawMessage) (json.RawMessage, error) { return body, err }
}

func fixtureSurfaces(scope protocol.PluginContext) []desktop.NativeComponent {
	messages := conversation.New()
	actions := &fixtureConversationActions{}
	return []desktop.NativeComponent{
		{Module: "sailry", Export: "Terminal", Render: func(view *ui.Context, element desktop.NativeElement, _ func(), _ func(desktop.CallbackRef, json.RawMessage)) (ui.Element, error) {
			var props struct {
				Terminal *protocol.TerminalID `json:"terminal"`
			}
			if err := json.Unmarshal(element.Props, &props); err != nil {
				return ui.Element{}, err
			}
			if props.Terminal != nil {
				return ui.Element{}, errors.New("the entry fixture has no attached terminal")
			}
			return ui.Box(view), nil
		}},
		{Module: "sailry", Export: "Conversation", Render: func(view *ui.Context, _ desktop.NativeElement, _ func(), _ func(desktop.CallbackRef, json.RawMessage)) (ui.Element, error) {
			if scope.Session == nil {
				return ui.Element{}, errors.New("the entry fixture has no captured conversation")
			}
			result := ui.Column(view).Grow(1).MinHeight(0).Children(func() {
				conversation.View(view, messages, scope.Session.String(), nil, &conversation.Session{}, actions)
			})
			return result, actions.failure
		}},
	}
}

func fixtureOperations(target protocol.NodeID, scope protocol.PluginContext, theme func() json.RawMessage) map[string]desktop.HostOperation {
	wait := func(ctx context.Context, _ []json.RawMessage) (json.RawMessage, error) {
		<-ctx.Done()
		return nil, ctx.Err()
	}
	operations := map[string]desktop.HostOperation{
		"sailry.header_action": wait, "sailry.nextAssistantTools": wait,
		"sailry/ui.pageInsets": constant(map[string]any{"rail_width": shell.RailWidth}),
		"sailry/ui.pane":       constant(nil),
		"sailry/ui.readComposer": constant(map[string]any{
			"cursor": "1", "connected": true, "session": scope.Session, "busy": false, "readonly": false,
			"mode": "code", "command": nil, "options": map[string]any{}, "text": "", "references": []any{},
		}),
		"sailry/ui.readModelContext":     constant(map[string]any{"cursor": "1", "connected": true, "provider": nil}),
		"sailry/ui.readProviderContexts": constant(map[string]any{"cursor": "1", "providers": []any{}, "connected": true}),
		"sailry/ui.readBrowser": constant(map[string]any{
			"cursor": "1", "tabs": []any{}, "selected": nil, "annotation": nil, "annotation_error": nil,
		}),
		"sailry/ui.readBrowserSettings":           constant(map[string]any{"persistent": false, "revision": "0"}),
		"sailry/ui.readLocation":                  constant(map[string]any{"cursor": "1", "node": target, "worktree": scope.Worktree, "session": scope.Session, "projects": []any{}}),
		"sailry/ui.readResourceTarget":            constant(nil),
		"sailry/ui.fileResource":                  constant(nil),
		"sailry/ui.workspaceMode":                 constant("main"),
		"sailry/ui.toast":                         constant(nil),
		"sailry/artifacts.artifactResource":       constant(nil),
		"sailry/file-transfers.systemFileManager": constant(map[string]any{"available": false, "platform": "macos"}),
		"sailry/file-transfers.readClipboard":     constant(nil),
		"sailry/documents.readDocuments":          constant(map[string]any{"cursor": "0", "documents": []any{}, "operations": []any{}, "reveal": nil}),
		"sailry/file-transfers.readTransfers":     constant(map[string]any{"cursor": "0", "transfers": []any{}}),
		"sailry/card.state": constant(map[string]any{
			"cursor": "1", "locale": "en", "props": map[string]any{"configured": false},
			"configured": false, "login": nil, "action": nil, "can_cancel": false, "can_open": false,
		}),
		"sailry/card.theme":  func(context.Context, []json.RawMessage) (json.RawMessage, error) { return theme(), nil },
		"sailry/card.status": constant(nil), "sailry/card.nextChange": wait, "sailry/card.toast": constant(nil),
		"sailry/sdk.readActivityCatalog": constant(map[string]any{
			"node": target, "cursor": "1", "projects": []any{}, "worktrees": []any{}, "sessions": []any{}, "terminals": []any{},
			"session_lanes": map[string]any{}, "terminal_lanes": map[string]any{}, "unread_terminals": []any{}, "activity_order": []any{}, "hosts": []any{},
		}),
		"sailry/sdk.readActivityPreviews": constant([]any{}),
		"sailry/sdk.nextActivityHosts":    wait,
	}
	for _, name := range []string{"nextComposerChange", "nextModelContext", "nextProviderContexts", "nextBrowserChange", "nextBrowserShortcut", "nextLocationChange", "nextResourceTarget", "nextWindowActivation"} {
		operations["sailry/ui."+name] = wait
	}
	operations["sailry/artifacts.nextArtifactChange"] = wait
	operations["sailry/artifacts.nextArtifactEvent"] = wait
	operations["sailry/documents.nextDocumentChange"] = wait
	operations["sailry/file-transfers.nextTransferChange"] = wait
	return operations
}

func fixtureReads(info protocol.PluginInfo) map[string]desktop.HostOperation {
	values := map[string]json.RawMessage{}
	if info.Settings != nil {
		for name, field := range info.Settings.Properties {
			if field.Default != nil {
				values[name] = *field.Default
			}
			if field.Model {
				values[name] = json.RawMessage(`"fixture-model"`)
			}
			if field.Effort != nil {
				values[name] = json.RawMessage(`"none"`)
			}
		}
	}
	return map[string]desktop.HostOperation{
		"sailry/sdk.readSettings": constant(map[string]any{"package": packages.PackageReference(info), "values": values, "configured": []any{}, "keepable": []any{}, "ready": true}),
		"sailry/sdk.listModels": constant(map[string]any{"models": []any{map[string]any{
			"kind": "provider", "id": "fixture-model", "provider": "fixture", "model": "fixture-model", "authentication": "api_key",
			"default": true, "reasoning": false, "efforts": []any{"none"}, "default_effort": "none",
		}}}),
	}
}
