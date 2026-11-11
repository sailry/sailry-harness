package host

import (
	"context"
	"crypto/ed25519"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/ssh"
	cryptossh "golang.org/x/crypto/ssh"
)

func TestSSHInstallationContracts(t *testing.T) {
	t.Parallel()
	for _, row := range []struct {
		platform string
		target   string
	}{
		{"Linux\nx86_64\n", "linux-amd64"},
		{"Linux\naarch64\n", "linux-arm64"},
		{"Linux\narm64\n", "linux-arm64"},
		{"Darwin\narm64\n", "darwin-arm64"},
		{"Darwin\nx86_64\n", "darwin-amd64"},
		{"Windows\nx86_64\n", ""},
	} {
		target, _, err := targetPlatform(row.platform)
		if target != row.target || (err == nil) != (row.target != "") {
			t.Fatalf("platform %q mapped to %q: %v", row.platform, target, err)
		}
	}
	node := protocol.NodeID{1}
	for _, row := range []struct {
		identity installedIdentity
		target   string
		ok       bool
	}{
		{installedIdentity{Node: node, Version: "Sailry Host 0.1.0"}, "0.1.1", true},
		{installedIdentity{Node: node, Version: "Sailry Host 0.1.0+old"}, "0.1.0+new", true},
		{installedIdentity{Node: protocol.NodeID{2}, Version: "Sailry Host 0.1.0"}, "0.1.1", false},
		{installedIdentity{Node: node, Version: "Sailry Host 0.1.2"}, "0.1.1", false},
		{installedIdentity{Node: node, Version: "invalid"}, "0.1.1", false},
	} {
		if err := validateUpdate(row.identity, node, row.target); (err == nil) != row.ok {
			t.Fatalf("update identity=%+v target=%s err=%v", row.identity, row.target, err)
		}
	}
	release := "0.1.0+build"
	if !sameRelease(&release, "0.1.0+other") || sameRelease(&release, "0.1.1") || sameRelease(nil, "0.1.0") {
		t.Fatal("update precedence did not ignore build metadata")
	}
	quoted, err := shellQuote("https://relay.example/a'\"$x;$(touch marker)\r\n")
	if err != nil || quoted != "'https://relay.example/a'\"'\"'\"$x;$(touch marker)\r\n'" {
		t.Fatalf("shell argument was not retained literally: %q %v", quoted, err)
	}
	if _, err := shellQuote("value\x00"); err == nil {
		t.Fatal("accepted a null shell argument")
	}
	owner := NewInstallations(DeploymentOptions{})
	request := protocol.NewRequest(node, protocol.Command{Kind: "install_host", Data: protocol.CommandInstallHostPayload{}})
	ctx, finish, err := owner.Begin(context.Background(), node, request)
	if err != nil {
		t.Fatal(err)
	}
	defer finish()
	owner.Progress(node, request.ID, "installing")
	if progress, err := owner.Read(node, request.ID, nil); err != nil || progress.Kind != "installing" {
		t.Fatal("progress lost its original request scope")
	}
	if _, err := owner.Read(protocol.NodeID{2}, request.ID, nil); err == nil {
		t.Fatal("progress escaped its caller scope")
	}
	if err := owner.Cancel(node, request.ID, nil); err != nil || !errors.Is(ctx.Err(), context.Canceled) {
		t.Fatal("cancellation did not stop only its captured operation")
	}
	finish()
	if _, err := owner.Read(node, request.ID, nil); err == nil {
		t.Fatal("completed installation retained active progress")
	}
	seed := make([]byte, ed25519.SeedSize)
	public := ed25519.NewKeyFromSeed(seed).Public()
	key, err := cryptossh.NewPublicKey(public)
	if err != nil {
		t.Fatal(err)
	}
	var required *ssh.HostKeyRequired
	if err := ssh.VerifyHostKey(nil, key); !errors.As(err, &required) || required.Changed {
		t.Fatal("first-use SSH key did not require explicit trust")
	}
	trusted := required.Key
	if err := ssh.VerifyHostKey(&trusted, key); err != nil {
		t.Fatal("explicitly trusted SSH key was rejected")
	}
	trusted.Fingerprint = "SHA256:changed"
	if err := ssh.VerifyHostKey(&trusted, key); !errors.As(err, &required) || !required.Changed {
		t.Fatal("changed SSH key did not require explicit trust")
	}
	for _, row := range []struct {
		bytes    uint64
		interval time.Duration
		expected uint64
	}{
		{100, time.Second, 100},
		{100, 2 * time.Second, 50},
		{100, 0, 0},
	} {
		if actual := bytesPerSecond(row.bytes, row.interval); actual != row.expected {
			t.Fatalf("rate=%d expected=%d", actual, row.expected)
		}
	}
	paths := DeploymentOptions{ProgramRoot: "/program", Profile: "/profile", ServicePath: "/service", CommandPath: "/command"}
	selected := protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{"https://relay.example/a'\"$x"}}}
	script, err := deploymentScript("/tmp/sailry-bootstrap-fixture", "libsailry_iroh.so", strings.Repeat("a", 64), strings.Repeat("b", 64), paths, false, &selected)
	if err != nil || !strings.Contains(script, "set -- '--relay' 'https://relay.example/a'\"'\"'\"$x'") || strings.Contains(script, "curl ") || strings.Contains(script, "releases/latest") {
		t.Fatal("local deployment changed network arguments or fetched a mutable release")
	}
	// The release feed installs only into the installer's own managed paths.
	if err := NewInstallations(paths).installRelease(context.Background(), nil, node, request.ID, nil, nil); err == nil {
		t.Fatal("release installation accepted isolated managed paths")
	}
	t.Run("bootstrap pairing", func(t *testing.T) {
		t.Parallel()
		refused := &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "pairing rejected"}
		paired := link.EndpointAddress{ID: protocol.NodeID{7}}
		for _, row := range []struct {
			name    string
			tickets []string
			results []error
			tried   []string
			message string
		}{
			{"first ticket", []string{"", "", "a"}, []error{nil}, []string{"a"}, ""},
			{"refused once", []string{"a", "a", "b"}, []error{refused, refused, nil}, []string{"a", "a", "b"}, ""},
			{"unreachable once", []string{"a", "a"}, []error{errors.New("no route"), nil}, []string{"a", "a"}, ""},
			{"always refused", []string{"a", "b", "c", "d", "e", "f"}, []error{refused, refused, refused, refused, refused, nil}, []string{"a", "b", "c", "d", "e"}, "Host installed but pairing could not be confirmed"},
			{"trust unconfirmed", []string{"a", "b"}, []error{unknown("trust could not be confirmed"), nil}, []string{"a"}, "trust could not be confirmed"},
			{"never published", nil, nil, nil, "Host service started but pairing is unavailable; inspect the host service"},
		} {
			t.Run(row.name, func(t *testing.T) {
				reads, waits := 0, 0
				var tried []string
				address, err := pairBootstrap(context.Background(), func() (string, error) {
					reads++
					if reads > len(row.tickets) {
						return "", nil
					}
					return row.tickets[reads-1], nil
				}, func(ticket string) (link.EndpointAddress, error) {
					tried = append(tried, ticket)
					if err := row.results[len(tried)-1]; err != nil {
						return link.EndpointAddress{}, err
					}
					return paired, nil
				}, func(context.Context) error { waits++; return nil })
				if strings.Join(tried, ",") != strings.Join(row.tried, ",") {
					t.Fatalf("tickets tried: %v", tried)
				}
				if row.message == "" {
					if err != nil || address.ID != paired.ID {
						t.Fatalf("pairing: %+v, %v", address, err)
					}
					return
				}
				var fault *protocol.Fault
				if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeOutcomeUnknown || fault.Message != row.message {
					t.Fatalf("pairing failure: %v", err)
				}
				if reads > 90 || waits > 90 {
					t.Fatalf("pairing was not bounded: %d reads, %d waits", reads, waits)
				}
			})
		}
	})
}
