package testassembly

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func RegisterExecutionFixture(assembly *shell.Assembly) {
	assembly.ExecutionSettings = append(assembly.ExecutionSettings, func(host settings.ExecutionSettingHost) settings.ExecutionSetting {
		return &ExecutionFixture{host: host}
	})
}

type ExecutionFixture struct {
	settings.Stored[bool]
	Persistent *bool
	host       settings.ExecutionSettingHost
}

func (s *ExecutionFixture) Render(c *ui.Context) {
	settings.Switch(c, "execution-fixture", i18n.T(c, "settings_code_index"), "", &s.Draft, true)
}

func (s *ExecutionFixture) Read() {
	if s.Pending || s.host.Execute == nil {
		return
	}
	s.Pending = true
	s.host.Execute(protocol.Command{Kind: "read_code_index_settings"}, func(_ *ui.Context, out protocol.Output, _ error) {
		s.Pending = false
		if value, ok := out.Data.(protocol.CodeIndexSettings); ok {
			s.Saved, s.Draft, s.Revision, s.Persistent = value.Enabled, value.Enabled, value.Revision, value.Persistent
		}
	})
}

func (s *ExecutionFixture) SaveEnabled(enabled bool) {
	s.Draft = enabled
	s.save(nil)
}

func (s *ExecutionFixture) SavePersistent(enabled bool) { s.save(new(enabled)) }

func (s *ExecutionFixture) save(persistent *bool) {
	s.Pending = true
	command := protocol.Command{Kind: "save_code_index_settings", Data: protocol.CommandSaveCodeIndexSettingsPayload{
		Settings:         protocol.CodeIndexSettings{Revision: s.Revision, Enabled: s.Draft, Persistent: persistent},
		ExpectedRevision: s.Revision,
	}}
	s.host.Save(command, settings.Messages{Conflict: "code_index_settings_conflict"}, func(out protocol.Output, ok bool) {
		s.Pending = false
		if !ok {
			s.Draft = s.Saved
			s.Read()
			return
		}
		if value, valid := out.Data.(protocol.CodeIndexSettings); valid {
			s.Saved, s.Revision, s.Persistent = value.Enabled, value.Revision, value.Persistent
		}
	})
}

func RegisterWorktreeFixture(assembly *shell.Assembly) {
	assembly.PluginControlReplacements = append(assembly.PluginControlReplacements,
		shell.PluginControlReplacement{Package: "worktrees", Slot: protocol.PluginUiSlotProjectMenu, ID: "manage"},
		shell.PluginControlReplacement{Package: "worktrees", Slot: protocol.PluginUiSlotComposer, ID: "create"},
		shell.PluginControlReplacement{Package: "worktrees", Slot: protocol.PluginUiSlotComposer, ID: "fork"},
	)
	assembly.ProjectWorkflows = append(assembly.ProjectWorkflows, func(host shell.ProjectWorkflowHost) {
		host.RegisterMenu(sidebar.ProjectMenuAction{
			Order: 30,
			Context: func(c *ui.Context, m *ui.Menu, _ *viewmodel.Project) {
				m.Submenu(i18n.T(c, "worktree_manage"), func(child *ui.Menu) { child.Item(i18n.T(c, "worktree_create")) })
			},
			BeforeCopy: func(c *ui.Context, m *kit.Menu, _ *viewmodel.Project) { m.Item(i18n.T(c, "worktree_create")) },
			AfterCopy:  func(c *ui.Context, m *kit.Menu, _ *viewmodel.Project) { m.Item(i18n.T(c, "worktree_remove")) },
		})
		host.RegisterLocations(func(project shell.ProjectLocationReader) {
			project.Read("git-worktrees", protocol.Command{Kind: "list_worktrees", Data: protocol.CommandListWorktreesPayload{Worktree: project.Main}}, func(out protocol.Output) {
				if locations, ok := out.Data.(protocol.GitWorktrees); ok {
					project.Publish(locations)
				}
			})
		})
		host.RegisterWorktreeActions(shell.ConversationWorktreeActions{
			Create:   func(shell.WorkflowConversation) {},
			ForkTurn: func(shell.WorkflowConversation) {},
			ForkSession: func(conversation shell.WorkflowConversation) {
				if conversation.Session != nil {
					conversation.Execute("fixture-fork", protocol.Command{Kind: "read_conversation", Data: protocol.CommandReadConversationPayload{
						Session: conversation.Session.ID, Limit: 100,
					}}, func(*ui.Context, protocol.Output, error) {})
				}
			},
		})
	})
}
