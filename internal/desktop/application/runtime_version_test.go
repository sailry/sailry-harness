package application

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/identity"
	"github.com/sailry/sailry-harness/internal/link"
)

func TestProductRuntimeVersionDoesNotRequireLinkerFlags(t *testing.T) {
	previous := link.ProductVersion
	t.Cleanup(func() { link.ProductVersion = previous })
	err := Run([]string{"--invalid-product-version-test-flag"}, Options{Identity: identity.Desktop{Version: "0.1.0"}})
	if err == nil {
		t.Fatal("invalid Desktop startup was accepted")
	}
	if link.ProductVersion != "0.1.0" {
		t.Fatalf("runtime version = %q, want product version", link.ProductVersion)
	}
}
