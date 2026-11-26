package briefnode

import (
	"context"
	"io/fs"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type disabledBriefHost struct{ moduleapi.Host }

func (disabledBriefHost) PluginInstalled(context.Context, string) (bool, bool, error) {
	return true, false, nil
}

func (disabledBriefHost) PluginCaptured(*store.AgentInvocation, string) (protocol.PluginReference, bool) {
	return protocol.PluginReference{}, false
}

func TestBriefReplacementHonorsLegacyIdentityAndDisabledState(t *testing.T) {
	for _, digest := range []string{"2aa73c8ad133105bda550964a08d5c7719dd5d69be3c0a4e1973452cd2dd1c41", "b7404662a30f9133bc1a327e2ba5a387b79bad56ad8cfd0c1873612f19184f51"} {
		if !plugins.IsLegacyNativeToolPackage("brief", digest) || !plugins.IsLegacyDesktopPackage("brief", digest) {
			t.Fatalf("missing official brief identity %s", digest)
		}
	}
	if plugins.IsLegacyNativeToolPackage("brief", strings.Repeat("0", 64)) || plugins.IsLegacyDesktopPackage("brief", ArtifactReference().Digest) {
		t.Fatal("unrelated or native renderer mistaken for an old official plugin")
	}
	if tools, err := Module().ToolsForTurn(t.Context(), disabledBriefHost{}, &store.AgentInvocation{}); err != nil || len(tools) != 0 {
		t.Fatalf("disabled historical Brief contributed %d tools: %v", len(tools), err)
	}
}

func TestBriefModuleIdentity(t *testing.T) {
	t.Parallel()
	m := Module()
	assembly := moduleapi.Assembly{Modules: []moduleapi.Module{m}}
	if m.ID() != "brief" || !assembly.ReplacesPlugin("brief") || ArtifactReference().Name != "brief" {
		t.Fatalf("unexpected Brief identity: %s, %v, %+v", m.ID(), m.ReplacesPlugins(), ArtifactReference())
	}
	packages := assembly.NativeSkillPackages()
	if len(packages) != 1 || packages[0].Package != "brief" || len(packages[0].Skills) != 1 {
		t.Fatalf("native skills = %+v", packages)
	}
	skill := packages[0].Skills[0]
	if body, err := fs.ReadFile(packages[0].Tree, skill.Path); err != nil || skill.Name != "brief" || !strings.Contains(string(body), "# HTML artifacts") {
		t.Fatalf("bundled skill %q was unavailable: %v", skill.Name, err)
	}
	var session protocol.Session
	session.Delegation = new(new(protocol.Delegation))
	if tools, err := m.ToolsForTurn(t.Context(), nil, &store.AgentInvocation{Session: session}); err != nil || len(tools) != 0 {
		t.Fatalf("delegated session received %d Brief tools: %v", len(tools), err)
	}
}
