package shell

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestSidebarHostNamesStayLiteral(t *testing.T) {
	for _, test := range []struct {
		name, title string
		local       bool
	}{
		{name: "local catalog key", title: "settings", local: true},
		{name: "remote catalog key", title: "settings"},
		{name: "local hostname", title: "build-node.local", local: true},
		{name: "local placeholder", title: "local_host_name", local: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			data := &viewmodel.Data{Workspace: &viewmodel.Workspace{Hosts: []viewmodel.Host{{ID: "device", Name: test.title, Local: test.local}}}}
			app := NewLive(data)
			tester := ui.NewTesterWith(app.View, 900, 700, opaque)
			tester.SetLocale("zh-CN")
			tester.Frame()
			title := test.title
			if title == "local_host_name" {
				title = i18n.In("zh-CN", title)
			}
			if _, present := tester.AccessByRole(ui.RoleButton, title); !present {
				t.Fatalf("host row %q is missing: %q", title, tester.Texts())
			}
		})
	}
}
