package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// LinkServices are the read and presentation capabilities of the source
// conversation. Connection lookups expose only referable display data.
type LinkServices struct {
	OpenKnowledge      func(*ui.Context, Source)
	OpenPackageDetails func(*ui.Context, string)
	ShowReference      func(label, subtitle, text string)
	SSHReference       func(protocol.SshID) (SSHReferenceEndpoint, bool)
	DatabaseReference  func(protocol.DatabaseID) (string, bool)
}

type SSHReferenceEndpoint struct {
	Username, Host string
	Port           uint16
}
