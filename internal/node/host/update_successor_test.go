package host

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCodeHostUpdateDoesNotDowngradeLegacyHost(t *testing.T) {
	id := protocol.NodeID{1}
	legacy := installedIdentity{Node: id, Version: officialVersionPrefix + "0.2.13"}
	if err := validateUpdate(legacy, id, "0.1.0"); err == nil {
		t.Fatal("Code's initial version reset was accepted as a managed downgrade")
	}
	code := installedIdentity{Node: id, Version: officialVersionPrefix + "0.1.0"}
	if err := validateUpdate(code, id, "0.1.1"); err != nil {
		t.Fatalf("subsequent Code Host upgrade was refused: %v", err)
	}
}
