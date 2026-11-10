package ports

import (
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPortLeaseOwnershipAndBounds(t *testing.T) {
	t.Parallel()
	now := time.Unix(100, 0)
	service := New(t.Context(), Options{Now: func() time.Time { return now }})
	defer service.Close()
	caller, stranger := protocol.NodeID{1}, protocol.NodeID{2}
	if _, err := service.Prepare(caller, 0); err == nil {
		t.Fatal("zero port was accepted")
	}
	var first protocol.StreamID
	for index := 0; index < 32; index++ {
		id, err := service.Prepare(caller, uint16(5000+index))
		if err != nil {
			t.Fatal(err)
		}
		if index == 0 {
			first = id
		}
	}
	if _, err := service.Prepare(caller, 6000); err == nil {
		t.Fatal("port lease capacity was exceeded")
	}
	if _, found, err := service.Open(t.Context(), stranger, first); !found || err == nil {
		t.Fatal("another caller opened a captured port")
	}
	if err := service.Cancel(stranger, first); err == nil {
		t.Fatal("another caller cancelled a captured port")
	}
	if err := service.Cancel(caller, first); err != nil {
		t.Fatal(err)
	}
	if _, found, err := service.Open(t.Context(), caller, first); found || err != nil {
		t.Fatal("cancelled lease still claimed its stream")
	}
	if _, err := service.Prepare(caller, 6000); err != nil {
		t.Fatal(err)
	}
	now = now.Add(30 * time.Second)
	if _, err := service.Prepare(caller, 7000); err != nil || len(service.entries) != 1 {
		t.Fatalf("expired inactive leases were retained: %v", err)
	}
	if err := service.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := service.Prepare(caller, 8000); err == nil {
		t.Fatal("closed service admitted a lease")
	}
}
