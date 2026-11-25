package canonical

import "testing"

func TestCheckpointReadsValueAndPointer(t *testing.T) {
	for _, data := range []any{Compaction{Boundary: "b"}, &Compaction{Boundary: "b"}} {
		if checkpoint := (Record{Kind: "compaction", Data: data}).Checkpoint(); checkpoint == nil || checkpoint.Boundary != "b" {
			t.Fatalf("checkpoint of %T = %+v", data, checkpoint)
		}
	}
	if (Record{Kind: "message", Data: &Compaction{}}).Checkpoint() != nil {
		t.Fatal("a message record read as a checkpoint")
	}
}
