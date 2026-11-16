package hostctl

import (
	"io"
	"testing"
)

func TestIdentityCLIKeepsInspectionAndRevocationOnly(t *testing.T) {
	for _, args := range [][]string{{"users", "list"}, {"devices", "list"}, {"devices", "revoke", "0100000000000000000000000000000000000000000000000000000000000000"}} {
		if _, err := parseIdentity(args); err != nil {
			t.Fatalf("supported identity operation rejected: %v %v", args, err)
		}
	}
	for _, args := range [][]string{{"users", "administrator", "00000000-0000-0000-0000-000000000001", "on"}, {"users", "assign", "00000000-0000-0000-0000-000000000001", "00000000-0000-0000-0000-000000000002"}} {
		if _, err := parseIdentity(args); err == nil {
			t.Fatalf("retired user remapping operation accepted: %v", args)
		}
	}
	for _, args := range [][]string{{"--kind", "my_device"}, {"--user", "00000000-0000-0000-0000-000000000001"}} {
		if err := RunShare(t.Context(), args, io.Discard, io.Discard); err == nil {
			t.Fatalf("retired invitation scope accepted: %v", args)
		}
	}
}
