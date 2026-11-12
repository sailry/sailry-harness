package host

import (
	"context"
	"encoding/json"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSampler(t *testing.T) {
	t.Parallel()
	node := protocol.NodeID{1}

	// The disks are the volumes the user stores files on: no device or
	// kernel file system, none of the system's hidden mounts, none of no
	// size.
	t.Run("disks", func(t *testing.T) {
		t.Parallel()
		sample, err := NewSampler().Sample(context.Background(), node, true)
		if err != nil {
			t.Fatal(err)
		}
		if len(sample.Disks) == 0 {
			t.Fatal("the sample lists no volume")
		}
		for _, disk := range sample.Disks {
			if disk.TotalBytes == 0 || disk.Mount == "/dev" || strings.HasPrefix(disk.Mount, "/System/Volumes/") ||
				strings.HasPrefix(disk.Mount, "/proc") || strings.HasPrefix(disk.Mount, "/sys") || strings.EqualFold(disk.FileSystem, "devfs") {
				t.Errorf("the sample lists the mount %+v", disk)
			}
		}
	})

	// macOS publishes its accelerators' load; the sample names each once.
	t.Run("gpus", func(t *testing.T) {
		t.Parallel()
		sample, err := NewSampler().Sample(context.Background(), node, true)
		if err != nil {
			t.Fatal(err)
		}
		if sample.Gpus == nil {
			t.Fatal("the sample's accelerators are null")
		}
		seen := map[string]bool{}
		for _, gpu := range sample.Gpus {
			if gpu.ID == "" || gpu.Name == "" || seen[gpu.ID] || gpu.UsageBasisPoints != nil && *gpu.UsageBasisPoints > 10000 {
				t.Errorf("the sample lists the accelerator %+v", gpu)
			}
			seen[gpu.ID] = true
		}
		if runtime.GOOS != "darwin" && len(sample.Gpus) != 0 {
			t.Errorf("%s lists accelerators: %+v", runtime.GOOS, sample.Gpus)
		}
		t.Logf("accelerators: %s", func() []byte { text, _ := json.Marshal(sample.Gpus); return text }())
	})

	// The information names the computer's time zone with its offset, and a
	// Node that sends none decodes as before.
	t.Run("timezone", func(t *testing.T) {
		t.Parallel()
		zone := func(name string, offset int) string {
			return *timezone(time.Date(2026, 1, 2, 3, 4, 5, 0, time.FixedZone(name, offset)))
		}
		for _, row := range []struct {
			name   string
			offset int
			want   string
		}{{"CST", 8 * 3600, "CST +08:00"}, {"UTC", 0, "UTC +00:00"}, {"+0530", 19800, "UTC+05:30"}, {"", -3 * 3600, "UTC-03:00"}} {
			if got := zone(row.name, row.offset); got != row.want {
				t.Errorf("the zone %q at %d is %q, want %q", row.name, row.offset, got, row.want)
			}
		}
		info, err := NewSampler().Inspect(context.Background(), node)
		if err != nil {
			t.Fatal(err)
		}
		if info.Timezone == nil || *info.Timezone != *timezone(time.Now()) {
			t.Errorf("the information's time zone is %v", info.Timezone)
		}
		info.Timezone = nil
		text, err := json.Marshal(info)
		if err != nil || strings.Contains(string(text), "timezone") {
			t.Fatalf("information without a time zone encodes as %s, %v", text, err)
		}
		var earlier protocol.HostInfo
		if err := json.Unmarshal(text, &earlier); err != nil || earlier.Timezone != nil || earlier.NodeUptimeSecs != info.NodeUptimeSecs {
			t.Errorf("information without a time zone decodes as %+v, %v", earlier, err)
		}
	})
}
