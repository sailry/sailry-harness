package compact

import (
	"fmt"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func BenchmarkVisibleRollingHistory(b *testing.B) {
	for _, count := range []int{1000, 10000, 100000} {
		b.Run(fmt.Sprint(count), func(b *testing.B) {
			turn := protocol.NewTurnID()
			records := make([]canonical.Record, 0, count+count/100)
			for i := range count {
				record := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Recorded progress")), time.Time{})
				records = append(records, record)
				if i%100 == 99 {
					records = append(records, canonical.NewRecord(turn, "compaction", &canonical.Compaction{
						Boundary: records[len(records)-11].ID,
						Summary:  canonical.NewMessage(canonical.User, canonical.Text(0, "Rolling handoff")),
					}, time.Time{}))
				}
			}
			b.ReportAllocs()
			b.ResetTimer()
			for b.Loop() {
				if len(Visible(records)) != 11 {
					b.Fatal("projection lost the summary or retained tail")
				}
			}
		})
	}
}
