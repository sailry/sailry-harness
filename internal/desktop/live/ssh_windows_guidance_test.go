package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSSHWindowsPairingGuidance(t *testing.T) {
	for _, uncertain := range []bool{false, true} {
		root := &Runtime{App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})}
		root.App.Settings.AddHost()
		err := &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: protocol.SSHWindowsDesktopRequired}}
		root.toastSSH(nil, err, false, uncertain)
		failure := root.App.Settings.ConnectionFailure
		if failure == nil {
			t.Fatal("Add host did not retain its failure")
		}
		if uncertain {
			if failure.Title != "host_install_unknown" || failure.Detail != protocol.SSHWindowsDesktopRequired {
				t.Fatalf("Windows guidance hid an earlier uncertain installation: %+v", failure)
			}
			continue
		}
		if failure.Title != "host_ssh_windows_pairing" || failure.Detail != "" {
			t.Fatalf("Windows guidance was replaced by a generic or duplicated failure: %+v", failure)
		}
		for language, want := range map[string]string{
			"en":    "On Windows, install and open Sailry Desktop, then add the host using its pairing code.",
			"zh-CN": "请在 Windows 主机安装并打开 Sailry 桌面版，再使用配对码添加。",
		} {
			if got := i18n.In(language, failure.Title); got != want {
				t.Fatalf("%s Windows guidance = %q, want %q", language, got, want)
			}
		}
	}
}
