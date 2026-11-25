package live

import (
	"context"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func railPagesSession(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
	defer cancel()
	profile := filepath.Join(t.TempDir(), "profile")
	owner, err := node.StartWithOptions(ctx, profile, node.Options{})
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Stop(context.Background())
	controller := client.New(link.NewLocal(owner.ID(), owner.ID(), owner))
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()

	const mainLabel, detailsLabel = "Rail workspace: ", "Rail details workspace: "
	assembly := shell.Assembly{WorkspacePages: []shell.NativePage{{
		Page: shell.FilesPage, Resource: panel.Files,
		Feature: shell.Feature{ID: string(shell.FilesPage), Label: "files", Icon: icons.Folder, DefaultPinned: true},
		Render: func(c *ui.Context, app *shell.App) {
			app.RenderResource(c, func(project *panel.Project) { ui.Text(c, mainLabel+project.ID) })
		},
		RenderDetails: func(c *ui.Context, _ *shell.App, project *panel.Project) {
			ui.Text(c, detailsLabel+project.ID)
		},
	}}}
	r, err := NewWithAssembly(ctx, controller, preferences.ForStartup(profile), assembly)
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	var listings atomic.Int32
	controller.OnDispatch(func(kind string) {
		if kind == "list_directory" {
			listings.Add(1)
		}
	})
	app := r.App
	app.SetWelcome(true)
	app.Effects = false
	tester := ui.NewTesterWith(app.View, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	tester.SetScale(0.125)
	tester.SetLocale("en")
	wait := func(what string, done func() bool) {
		t.Helper()
		ticker := time.NewTicker(time.Millisecond)
		defer ticker.Stop()
		for {
			tester.Frame()
			if done() {
				return
			}
			select {
			case <-ticker.C:
			case <-ctx.Done():
				t.Fatalf("waiting for %s: %v; visible text: %q", what, ctx.Err(), tester.Texts())
			}
		}
	}
	do := func(command protocol.Command) protocol.Output {
		t.Helper()
		output, err := controller.Do(ctx, command)
		if err != nil {
			t.Fatalf("%s: %v", command.Kind, err)
		}
		return output
	}
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name, provider.Enabled = protocol.NewProviderID(), "Rail fixture", true
	provider.Api, provider.Authentication, provider.Endpoint = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "http://127.0.0.1:9/v1"
	model := protocol.DefaultConversationModel()
	model.ID, model.Context, model.Output = "fixture", 32000, 8000
	provider.DefaultModel, provider.Models = model.ID, []protocol.ConversationModel{model}
	secret := protocol.NewSecret("fixture-key")
	do(protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Effort = provider.ID, model.ID, model.DefaultEffort
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionFull
	var previous protocol.WorktreeID
	for range 2 {
		session, valid := detail[protocol.Session](do(protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &config}}).Data)
		if !valid || session.Project != nil || session.Worktree == (protocol.WorktreeID{}) || session.Worktree == previous {
			t.Fatalf("expected a session with its own worktree and no project: %+v", session)
		}
		r.OpenSession(session.ID.String())
		wait("the selected session and its worktree", func() bool {
			return app.Page == shell.ConversationPage && app.Sidebar.Selected == session.ID.String() && r.workspace.selected == session.Worktree
		})
		var rail ui.Rect
		wait("the contributed Files rail item", func() bool {
			for _, entry := range tester.AccessAll() {
				b := entry.Bounds
				if entry.Role == ui.RoleButton && entry.Name == i18n.In("en", "files") && !entry.Disabled && !entry.Offscreen && b.W > 0 && b.H > 0 && b.X+b.W <= shell.RailWidth {
					rail = b
					return true
				}
			}
			return false
		})
		tester.ClickAt(rail.X+rail.W/2, rail.Y+rail.H/2)
		wait("the contributed page bound to the session", func() bool {
			return app.Page == shell.FilesPage && tester.HasText(mainLabel+session.Worktree.String())
		})
		app.RevealDetails()
		wait("the contributed details bound to the same worktree", func() bool {
			return tester.HasText(detailsLabel + session.Worktree.String())
		})
		shown := r.shownResourceWorkspace()
		if shown == nil || shown != r.selectedWorkspace() || shown.owner.ID != session.Worktree || shown.runtime.controller.Target() != owner.ID() || app.ResourceTarget() != (shell.ResourceTarget{}) || app.Sidebar.Selected != session.ID.String() {
			t.Fatal("opening the rail contributed page did not follow the active session scope")
		}
		if previous != (protocol.WorktreeID{}) && (tester.HasText(mainLabel+previous.String()) || tester.HasText(detailsLabel+previous.String())) {
			t.Fatal("the rail page retained another session's worktree")
		}
		if _, watching := controller.Workspace(session.Worktree, false); watching || listings.Load() != 0 {
			t.Fatal("rendering a scoped contribution started a directory read or watch")
		}
		previous = session.Worktree
	}
}
