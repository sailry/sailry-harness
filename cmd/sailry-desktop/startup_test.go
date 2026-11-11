package main

import (
	"context"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
)

// TestStartupScope covers the network a Node without a saved selection
// starts on: --relay, else the preferences' pinned relays, else public.
func TestStartupScope(t *testing.T) {
	t.Parallel()
	options, err := parseOptions([]string{"--relay", "https://one.example.test", "--data-dir", "/profile", "--relay", "https://two.example.test"})
	if err != nil || !reflect.DeepEqual(options.Relays, []string{"https://one.example.test", "https://two.example.test"}) || options.DataDir != "/profile" {
		t.Fatalf("repeated --relay parsed as %+v, %v", options, err)
	}
	if _, err := parseOptions([]string{"--relay"}); err == nil {
		t.Fatal("--relay without a URL was accepted")
	}
	stored := []string{"https://stored.example.test"}
	for _, current := range []struct {
		name     string
		explicit []string
		stored   *[]string
		want     link.NetworkScope
	}{
		{"public relays by default", nil, nil, link.NetworkScope{Internet: true}},
		{"the preferences' relays", nil, &stored, link.NetworkScope{Relays: stored}},
		{"--relay over the preferences", options.Relays, &stored, link.NetworkScope{Relays: options.Relays}},
	} {
		if got := startupScope(current.explicit, current.stored); !reflect.DeepEqual(*got, current.want) {
			t.Errorf("%s: %+v, want %+v", current.name, *got, current.want)
		}
	}
}

func TestPreviewFlagNoNode(t *testing.T) {
	t.Parallel()
	for _, explicit := range []bool{false, true} {
		path := filepath.Join(t.TempDir(), "must-stay-absent")
		args := []string{"--preview"}
		if explicit {
			args = append(args, "--data-dir", path)
		}
		options, err := parseOptions(args)
		if err != nil {
			t.Fatal(err)
		}
		backend, err := openBackend(context.Background(), options)
		if err != nil {
			t.Fatal(err)
		}
		if backend.Node != nil || backend.Client != nil || backend.Preferences != nil || backend.Path != "" {
			t.Fatal("preview opened live state")
		}
		if err := backend.Close(); err != nil {
			t.Fatal(err)
		}
		if _, err := os.Stat(path); !os.IsNotExist(err) {
			t.Fatalf("preview changed profile path: %v", err)
		}
	}
}
