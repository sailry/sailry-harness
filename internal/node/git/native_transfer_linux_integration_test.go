//go:build integration && linux

package git

import (
	"net/http/cgi"
	"net/http/httptest"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"github.com/go-git/go-git/v5/plumbing"
)

func TestLinuxSandboxGitTransfer(t *testing.T) {
	real, err := exec.LookPath("git")
	if err != nil {
		t.Skip("Git executable is required")
	}
	source, remote, commit := fixture(t)
	settings, err := remote.Config()
	if err != nil {
		t.Fatal(err)
	}
	settings.Raw.Section("http").SetOption("receivepack", "true")
	if err := remote.SetConfig(settings); err != nil {
		t.Fatal(err)
	}
	server := httptest.NewServer(&cgi.Handler{
		Path: real,
		Args: []string{"http-backend"},
		Env:  []string{"GIT_PROJECT_ROOT=" + filepath.Dir(source), "GIT_HTTP_EXPORT_ALL=1"},
	})
	defer server.Close()
	ctx := t.Context()
	s := New(Options{})
	alias := filepath.Join(t.TempDir(), "repository alias")
	if err := os.Symlink(source, alias); err != nil {
		t.Fatal(err)
	}
	for _, sourceURL := range []string{source, (&url.URL{Scheme: "file", Path: source}).String(), alias, (&url.URL{Scheme: "file", Path: alias}).String()} {
		target := filepath.Join(t.TempDir(), "local-checkout")
		if path, err := s.Clone(ctx, sourceURL, target, nil); err != nil || path != target {
			t.Fatalf("local clone to %s: %v\n%s", path, err, s.Output(filepath.Dir(target)))
		}
	}
	destination := filepath.Join(t.TempDir(), "checkout")
	root, err := s.Clone(ctx, server.URL+"/"+filepath.Base(source), destination, nil)
	if err != nil || root != destination {
		t.Fatalf("clone to %s: %v\n%s", root, err, s.Output(filepath.Dir(destination)))
	}
	local, err := open(root)
	if err != nil {
		t.Fatal(err)
	}
	if h, err := local.Head(); err != nil || h.Hash().String() != commit {
		t.Fatalf("cloned head = %v, %v", h, err)
	}
	upstream := plumbing.NewBranchReferenceName("upstream")
	if err := remote.Storer.SetReference(plumbing.NewHashReference(upstream, plumbing.NewHash(commit))); err != nil {
		t.Fatal(err)
	}
	if _, err := s.network(ctx, root, []string{"fetch", "origin"}); err != nil {
		t.Fatalf("fetch: %v\n%s", err, s.Output(root))
	}
	if ref, err := local.Reference(plumbing.NewRemoteReferenceName("origin", "upstream"), true); err != nil || ref.Hash().String() != commit {
		t.Fatalf("fetched upstream = %v, %v", ref, err)
	}
	pushed := plumbing.NewBranchReferenceName("published")
	if _, err := s.network(ctx, root, []string{"push", "origin", "refs/heads/main:" + pushed.String()}); err != nil {
		t.Fatalf("push: %v\n%s", err, s.Output(root))
	}
	if ref, err := remote.Reference(pushed, true); err != nil || ref.Hash().String() != commit {
		t.Fatalf("pushed branch = %v, %v", ref, err)
	}
}
