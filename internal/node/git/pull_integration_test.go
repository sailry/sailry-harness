//go:build integration

package git

import (
	"context"
	"os/exec"
	"reflect"
	"testing"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/config"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Native Git resolves configuration here; custom targets push only to a local
// temporary bare repository.
func TestNativeBranchPushTarget(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("Git executable is required")
	}
	for _, probe := range []struct {
		name, mode, upstream, pushRemote, destination, pushSpec string
	}{
		{"default simple", "", "main", "", "refs/heads/main", ""},
		{"explicit simple", "simple", "main", "", "refs/heads/main", ""},
		{"simple rejects renamed upstream", "simple", "release", "", "", ""},
		{"renamed upstream", "upstream", "release", "", "refs/heads/release", ""},
		{"triangular", "simple", "release", "publish", "refs/heads/main", ""},
		{"current", "current", "release", "", "refs/heads/main", ""},
		{"nothing", "nothing", "main", "", "", ""},
		{"custom namespace", "simple", "main", "", "refs/for/main", "refs/heads/main:refs/for/main"},
	} {
		t.Run(probe.name, func(t *testing.T) {
			root, r, commit := fixture(t)
			configurePullUpstream(t, r)
			cfg, err := r.Config()
			if err != nil {
				t.Fatal(err)
			}
			cfg.Branches["main"].Merge = plumbing.NewBranchReferenceName(probe.upstream)
			if probe.mode != "" {
				cfg.Raw.Section("push").SetOption("default", probe.mode)
			}
			if probe.pushRemote != "" {
				cfg.Remotes[probe.pushRemote] = &config.RemoteConfig{Name: probe.pushRemote, URLs: []string{t.TempDir()}}
				cfg.Raw.Section("branch").Subsection("main").SetOption("pushRemote", probe.pushRemote)
			}
			var target *gogit.Repository
			if probe.pushSpec != "" {
				path := t.TempDir()
				target, err = gogit.PlainInit(path, true)
				if err != nil {
					t.Fatal(err)
				}
				cfg.Remotes["origin"].URLs = []string{path}
				cfg.Raw.Section("remote").Subsection("origin").SetOption("push", probe.pushSpec)
			}
			if err := r.SetConfig(cfg); err != nil {
				t.Fatal(err)
			}
			for _, remote := range []string{"origin", "publish"} {
				for _, branch := range []string{"main", "release"} {
					if err := r.Storer.SetReference(plumbing.NewHashReference(plumbing.NewRemoteReferenceName(remote, branch), plumbing.NewHash(commit))); err != nil {
						t.Fatal(err)
					}
				}
			}
			s := New(Options{})
			args, err := s.branchPush(context.Background(), root, "main")
			if probe.destination == "" {
				requireFault(t, err, protocol.ErrorCodeNotConfigured)
				return
			}
			remote := "origin"
			if probe.pushRemote != "" {
				remote = probe.pushRemote
			}
			want := []string{"push", remote, "refs/heads/main:" + probe.destination}
			if err != nil || !reflect.DeepEqual(args, want) {
				t.Fatalf("native destination=%q error=%v want=%q", args, err, want)
			}
			if target != nil {
				if _, err := s.network(context.Background(), root, args); err != nil {
					t.Fatal(err)
				}
				pushed, err := target.Reference(plumbing.ReferenceName(probe.destination), true)
				if err != nil || pushed.Hash().String() != commit {
					t.Fatalf("custom target did not receive the selected commit: %v, %v", pushed, err)
				}
			}
		})
	}
}
