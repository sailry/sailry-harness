package live

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

// TestMemoryGroups asserts the Experimental map reads every class the Go
// runtime names, grouped, with the heap holding live objects.
func TestMemoryGroups(t *testing.T) {
	tester := ui.NewTester(func(c *ui.Context) {
		groups := memoryGroups(c)
		if len(groups) != len(memoryClasses) {
			t.Fatalf("got %d groups, want %d", len(groups), len(memoryClasses))
		}
		heap := groups[0]
		if heap.ID != "ram_heap" || len(heap.Items) != 3 || heap.Items[0].Value <= 0 || heap.Detail == "" {
			t.Fatalf("heap group is %+v", heap)
		}
		if runtime := groups[2]; len(runtime.Items) != 3 || runtime.Items[0].ID != "ram_allocator" {
			t.Fatalf("allocator classes are not summed: %+v", runtime.Items)
		}
	}, 200, 200)
	tester.Frame()
}
