package live

import (
	"math"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// A Node's sample shows as it was sampled: every value it carries in the
// page's units, and none it does not carry.
func TestHostMetrics(t *testing.T) {
	cpu, load := uint32(1250), uint32(325)
	sample := protocol.HostMetricsSample{
		SampledAtMs:     1_800_000_000_500,
		CpuBasisPoints:  &cpu,
		CoreBasisPoints: []uint32{1000, 1500},
		Load:            &protocol.HostMetricsLoadAverage{One: 125, Five: 50, Fifteen: 25},
		Memory:          &protocol.MemoryUsage{TotalBytes: 32, AvailableBytes: 20, FreeBytes: 8},
		Swap:            &protocol.HostMetricsSwapUsage{TotalBytes: 4, UsedBytes: 9},
		Disks: []protocol.HostMetricsDisk{
			{Mount: "/", FileSystem: "apfs", TotalBytes: 100, AvailableBytes: 40, Io: &protocol.HostMetricsIoRate{ReadBytesPerSec: 7, WrittenBytesPerSec: 9}},
			{Mount: "/dev", FileSystem: "devfs", TotalBytes: 1},
		},
		Network: &protocol.HostMetricsNetworkRate{ReceivedBytesPerSec: 2048, TransmittedBytesPerSec: 1024, ReceivedBytes: 5, TransmittedBytes: 6},
		Processes: []protocol.HostMetricsProcess{
			{Pid: 41, StartedAtSecs: 9, Name: "sailry-node", CpuBasisPoints: &load, MemoryBytes: 77},
			{Pid: 42, StartedAtSecs: 10, Name: "idle", MemoryBytes: 1},
		},
		ProcessCount: 512,
	}
	m := hostMetrics(sample)
	if !m.At.Equal(time.UnixMilli(1_800_000_000_500)) || m.CPU != 12.5 || len(m.Cores) != 2 || m.Cores[0] != 10 || m.Cores[1] != 15 {
		t.Errorf("the processor: at %v, %v, cores %v", m.At, m.CPU, m.Cores)
	}
	if m.Load == nil || *m.Load != [3]float64{1.25, 0.5, 0.25} {
		t.Errorf("the load averages: %v", m.Load)
	}
	// Used is what is not available; cached is what is available and not
	// free.
	if m.Memory != 32 || m.MemoryUsed != 12 || m.MemoryCached != 12 || m.Swap != 4 || m.SwapUsed != 4 {
		t.Errorf("the memory: %d of %d used, %d cached, swap %d of %d", m.MemoryUsed, m.Memory, m.MemoryCached, m.SwapUsed, m.Swap)
	}
	if m.Received != 2048 || m.Transmitted != 1024 || m.Interval != time.Second || !m.Totals || m.ReceivedTotal != 5 || m.TransmittedTotal != 6 {
		t.Errorf("the network: %+v", m)
	}
	// Every mount is carried: the page chooses which are volumes.
	if len(m.Disks) != 2 || m.Disks[0].Mount != "/" || m.Disks[0].ReadBytes != 7 || m.Disks[0].Written != 9 || m.Disks[0].Available != 40 || m.Disks[1].ReadBytes != 0 {
		t.Errorf("the disks: %+v", m.Disks)
	}
	if len(m.Volumes()) != 1 {
		t.Errorf("the volumes: %+v", m.Volumes())
	}
	if len(m.Processes) != 2 || m.ProcessCount != 512 {
		t.Fatalf("the processes: %+v of %d", m.Processes, m.ProcessCount)
	}
	if p := m.Processes[0]; p.Name != "sailry-node" || p.PID != 41 || p.Started != 9 || p.CPU != 3.25 || p.Memory != 77 {
		t.Errorf("a process: %+v", p)
	}
	if !math.IsNaN(m.Processes[1].CPU) {
		t.Errorf("a process without a load: %+v", m.Processes[1])
	}

	// A sample without values: nothing is made up, and a sample without
	// detail counts no process.
	bare := hostMetrics(protocol.HostMetricsSample{SampledAtMs: 1})
	if !math.IsNaN(bare.CPU) || bare.Load != nil || bare.Memory != 0 || bare.Interval != 0 || bare.Totals || bare.ProcessCount != -1 || len(bare.Disks) != 0 {
		t.Errorf("a bare sample: %+v", bare)
	}

	// The history follows the samples as the page's charts draw it.
	sample.SampledAtMs += 1000
	next := hostMetrics(sample)
	m.Follow(nil)
	next.Follow(m)
	if len(next.History) != 2 || next.History[1].CPU != 12.5 || next.History[1].Received != 2048 || len(next.Processes[0].History) != 2 {
		t.Errorf("the history: %+v", next.History)
	}
}
