package kit

import (
	"fmt"
	"testing"
)

// identity matches ui/identicon.rs, so a session keeps its mark across apps.
func TestIdentityMatchesReference(t *testing.T) {
	if shape, tint := identity("agent-7"); shape != 2 || tint != 0 {
		t.Fatalf("identity(agent-7) = %d, %d; want 2, 0", shape, tint)
	}
	seen := map[[2]int]bool{}
	for i := range 256 {
		shape, tint := identity(fmt.Sprintf("agent-%d", i))
		seen[[2]int{shape, tint}] = true
	}
	if len(seen) <= 32 {
		t.Fatalf("only %d identities", len(seen))
	}
}
