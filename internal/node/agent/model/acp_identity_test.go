package model

import (
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func cloneACPModel(t *testing.T, source *acpModel) *acpModel {
	t.Helper()
	result := *source
	var err error
	result.binding, err = Capture(source.binding.Provider, source.binding.Config, source.binding.Secret, source.binding.Session)
	if err != nil {
		t.Fatal(err)
	}
	// Copy declarations without duplicating the package-owned process pool.
	declarations := struct {
		Adapter protocol.PluginModelAdapterDeclaration
		Program protocol.PluginProgramDeclaration
	}{source.capture.Adapter, source.capture.Program}
	raw, err := json.Marshal(declarations)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(raw, &declarations); err != nil {
		t.Fatal(err)
	}
	result.capture.Adapter, result.capture.Program = declarations.Adapter, declarations.Program
	result.capture.Host = slices.Clone(source.capture.Host)
	return &result
}

func TestACPLeaseIdentity(t *testing.T) {
	for _, probe := range []struct {
		name   string
		mutate func(*testing.T, *acpModel)
	}{
		{"adapter", func(_ *testing.T, model *acpModel) {
			model.capture.Adapter.ID = "another-adapter"
			(**model.binding.Provider.Binding).Adapter.Adapter = model.capture.Adapter.ID
		}},
		{"adapter capabilities", func(_ *testing.T, model *acpModel) { model.capture.Adapter.Capabilities.Vision = false }},
		{"adapter session protocol", func(_ *testing.T, model *acpModel) { model.capture.Adapter.Acp.SystemMeta = nil }},
		{"package revision", func(_ *testing.T, model *acpModel) {
			(**model.binding.Provider.Binding).Package.Digest = strings.Repeat("b", 64)
		}},
		{"program identity", func(_ *testing.T, model *acpModel) {
			model.capture.Program.ID = "another-program"
			model.capture.Adapter.Acp.Program = model.capture.Program.ID
		}},
		{"program command", func(t *testing.T, model *acpModel) {
			bin := strings.TrimPrefix(model.capture.Host[0], "PATH=")
			self, err := os.Executable()
			if err != nil {
				t.Fatal(err)
			}
			if err := os.Symlink(self, filepath.Join(bin, "other-agent")); err != nil {
				t.Fatal(err)
			}
			model.capture.Program.Command = "other-agent"
		}},
		{"program arguments", func(_ *testing.T, model *acpModel) {
			model.capture.Program.Args = append(model.capture.Program.Args, "different")
		}},
		{"program environment", func(_ *testing.T, model *acpModel) { model.capture.Program.Env["FIXTURE_ACCOUNT"] = "another" }},
		{"program files", func(_ *testing.T, model *acpModel) { model.capture.Program.Files[0].Content = "different" }},
		{"program directory", func(t *testing.T, model *acpModel) { model.capture.Dir = t.TempDir() }},
		{"inherited environment", func(t *testing.T, model *acpModel) {
			model.capture.Host = append(model.capture.Host, "HOME="+t.TempDir())
		}},
		{"provider channel", func(_ *testing.T, model *acpModel) {
			model.binding.Provider.ID = protocol.NewProviderID()
			model.binding.Config.Provider = model.binding.Provider.ID
		}},
		{"provider endpoint", func(_ *testing.T, model *acpModel) {
			model.binding.Provider.Endpoint = "https://another.example.test/v1"
		}},
		{"credential reference", func(_ *testing.T, model *acpModel) {
			model.binding.Provider.Credential = &protocol.CredentialRef{Node: protocol.NodeID{1}, ID: protocol.NewCredentialID()}
		}},
		{"credential value", func(_ *testing.T, model *acpModel) {
			key := protocol.NewSecret("rotated-fixture-token")
			model.binding.Secret = &key
		}},
		{"credential removal", func(_ *testing.T, model *acpModel) { model.binding.Secret = nil }},
		{"client version", func(_ *testing.T, model *acpModel) { model.capture.Version = "0.0.1" }},
	} {
		t.Run(probe.name, func(t *testing.T) {
			fixture := acpFixtureModel(t, "meta", nil, func(capture *ACPCapture) { capture.Program.Env["FIXTURE_KEY"] = "{key}" })
			first, agent, err := fixture.model.lease(t.Context(), false)
			if err != nil {
				t.Fatal(err)
			}
			// A matching session must never outrank an incompatible identity.
			agent.claimed, agent.owner = true, fixture.model.binding.Session
			fixture.model.capture.Pool.Release(first, true)
			next := cloneACPModel(t, fixture.model)
			probe.mutate(t, next)
			language, err := NewACP(next.binding, next.capture)
			if err != nil {
				t.Fatal(err)
			}
			next = language.(*acpModel)
			second, other, err := next.lease(t.Context(), false)
			if err != nil {
				t.Fatal(err)
			}
			next.capture.Pool.Release(second, true)
			if first == second || agent == other || agent.identity == other.identity {
				t.Fatal("incompatible capture reused the previous process")
			}
			// Both entries are still owned by the package, including when
			// adapter/program/provider identities differ.
			next.capture.Pool.Invalidate(next.capture.Owner)
			for _, process := range []*acpAgent{agent, other} {
				select {
				case <-process.process.Done():
				default:
					t.Fatal("package invalidation retained a captured process")
				}
			}
		})
	}
}

func TestACPLeaseReusesCompatibleCapture(t *testing.T) {
	fixture := acpFixtureModel(t, "meta", nil, nil)
	entry, agent, err := fixture.model.lease(t.Context(), true)
	if err != nil {
		t.Fatal(err)
	}
	fixture.model.capture.Pool.Release(entry, true)
	next := cloneACPModel(t, fixture.model)
	// These changes do not affect a launch or the provider/channel identity.
	slices.Reverse(next.capture.Host)
	next.capture.Host = append(next.capture.Host, "NOT_INHERITED=changed")
	next.binding.Provider.Name = "Renamed provider"
	next.binding.Provider.Revision++
	next.binding.Config.Model = "another-session-selected-model"
	next.binding.Config.Effort = protocol.Effort{Kind: "high"}
	claimed, reused, err := next.lease(t.Context(), false)
	if err != nil {
		t.Fatal(err)
	}
	if claimed != entry || reused != agent {
		t.Fatal("compatible discovery process was not reused for inference")
	}
	agent.claimed, agent.owner = true, next.binding.Session
	next.capture.Pool.Release(claimed, true)
	continued, same, err := next.lease(t.Context(), false)
	if err != nil {
		t.Fatal(err)
	}
	next.capture.Pool.Release(continued, true)
	if continued != entry || same != agent || len(fixture.received("initialize")) != 1 {
		t.Fatal("the same session restarted its compatible process")
	}
}

func TestACPDiscoveryIdentityAndPackageLimit(t *testing.T) {
	fixture := acpFixtureModel(t, "meta", nil, nil)
	fixture.model.capture.Pool.Limit = 1
	entry, agent, err := fixture.model.lease(t.Context(), true)
	if err != nil {
		t.Fatal(err)
	}
	next := cloneACPModel(t, fixture.model)
	key := protocol.NewSecret("rotated-unused-fixture-token")
	next.binding.Secret = &key
	// The first lease still occupies the package's only slot, even though
	// this channel has a different credential and cannot reuse it.
	if _, _, err := next.lease(t.Context(), true); Classify(err) == nil || Classify(err).Kind != Unavailable {
		t.Fatal("another identity bypassed the package process limit")
	}
	fixture.model.capture.Pool.Release(entry, true)
	replaced, other, err := next.lease(t.Context(), true)
	if err != nil {
		t.Fatal(err)
	}
	next.capture.Pool.Release(replaced, true)
	if replaced == entry || other == agent {
		t.Fatal("discovery reused a process with an old credential")
	}
	select {
	case <-agent.process.Done():
	default:
		t.Fatal("the incompatible process was not replaced at the package limit")
	}
}
