package link

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRetireDirectoryRoutesPreservesDirectPairsAndHistory(t *testing.T) {
	source, imported, direct, inbound, removed := protocol.NodeID{1}, protocol.NodeID{2}, protocol.NodeID{3}, protocol.NodeID{4}, protocol.NodeID{5}
	values := []controllerPeer{
		{ID: imported, Direction: DirectionHost, Sources: []protocol.NodeID{source}},
		{ID: direct, Direction: DirectionHost, Direct: true, DirectDirection: DirectionHost, Sources: []protocol.NodeID{source}},
		{ID: inbound, Direction: DirectionBoth, Direct: true, DirectDirection: DirectionController, Sources: []protocol.NodeID{source}},
		{ID: removed, Direction: DirectionHost, Removed: true, Sources: []protocol.NodeID{source}},
	}
	if !retireControllerImportedRoutes(values) {
		t.Fatal("legacy routes were not migrated")
	}
	if !values[0].DirectoryOnly || len(values[0].Sources) != 1 || values[0].Removed {
		t.Fatal("imported trust retained execution or lost history")
	}
	if values[1].DirectoryOnly || values[1].Direction != DirectionHost || values[1].DirectDirection != DirectionHost {
		t.Fatal("direct pairing changed during migration")
	}
	if values[2].DirectoryOnly || values[2].Direction != DirectionController {
		t.Fatal("independent inbound pairing was not retained")
	}
	if !values[3].Removed || values[3].DirectoryOnly {
		t.Fatal("explicit removal history changed")
	}
	if retireControllerImportedRoutes(values) {
		t.Fatal("route migration was not idempotent")
	}
}
