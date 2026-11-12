package host

import (
	"cmp"
	"context"
	"math"
	"net"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/shirou/gopsutil/v4/cpu"
	"github.com/shirou/gopsutil/v4/disk"
	systemhost "github.com/shirou/gopsutil/v4/host"
	"github.com/shirou/gopsutil/v4/load"
	"github.com/shirou/gopsutil/v4/mem"
	systemnet "github.com/shirou/gopsutil/v4/net"
	"github.com/shirou/gopsutil/v4/process"
)

type processBaseline struct {
	started int64
	seconds float64
}

type Sampler struct {
	mu         sync.Mutex
	started    time.Time
	now        func() time.Time
	info       protocol.HostInfo
	infoAt     time.Time
	last       protocol.HostMetricsSample
	coreAt     time.Time
	detailsAt  time.Time
	cpus       []cpu.TimesStat
	processes  map[int32]processBaseline
	disks      map[string]disk.IOCountersStat
	interfaces map[string]systemnet.IOCountersStat
}

func NewSampler() *Sampler {
	return &Sampler{started: time.Now(), now: time.Now}
}

func (sampler *Sampler) Inspect(ctx context.Context, node protocol.NodeID) (protocol.HostInfo, error) {
	sampler.mu.Lock()
	defer sampler.mu.Unlock()
	now := sampler.now()
	if !sampler.infoAt.IsZero() && now.Sub(sampler.infoAt) < time.Second {
		return clone(sampler.info), nil
	}
	if now.UnixMilli() < 0 {
		return protocol.HostInfo{}, unavailable("host information is unavailable")
	}
	info := protocol.DefaultHostInfo()
	info.Node = node
	info.SampledAtMs = uint64(now.UnixMilli())
	info.NodeUptimeSecs = uint64(max(0, now.Sub(sampler.started).Seconds()))
	architecture := runtime.GOARCH
	info.Architecture = &architecture
	count := uint32(runtime.NumCPU())
	info.LogicalCpus = &count
	if system, err := systemhost.InfoWithContext(ctx); err == nil {
		info.Name = nonempty(system.Hostname)
		info.Os = nonempty(system.Platform)
		if info.Os == nil {
			info.Os = nonempty(system.OS)
		}
		info.OsVersion = nonempty(system.PlatformVersion)
		info.HostUptimeSecs = system.Uptime
	}
	info.Timezone = timezone(now)
	if memory, err := mem.VirtualMemoryWithContext(ctx); err == nil && memory.Total > 0 {
		info.Memory = &protocol.MemoryUsage{TotalBytes: memory.Total, AvailableBytes: min(memory.Available, memory.Total), FreeBytes: min(memory.Free, memory.Total)}
	}
	if err := ctx.Err(); err != nil {
		return protocol.HostInfo{}, err
	}
	sampler.info, sampler.infoAt = info, now
	return clone(info), nil
}

// timezone is the computer's time zone as its clock names it at now, with
// its offset from UTC: "CST +08:00". A zone the system has no name for is
// its offset alone, "UTC+08:00".
func timezone(now time.Time) *string {
	name, _ := now.Zone()
	offset := now.Format("-07:00")
	if name == "" || name[0] == '+' || name[0] == '-' {
		name = "UTC" + offset
	} else {
		name += " " + offset
	}
	return &name
}

func (sampler *Sampler) Sample(ctx context.Context, node protocol.NodeID, detailed bool) (protocol.HostMetricsSample, error) {
	sampler.mu.Lock()
	defer sampler.mu.Unlock()
	now := sampler.now()
	if now.UnixMilli() < 0 {
		return protocol.HostMetricsSample{}, unavailable("host information is unavailable")
	}
	sample := sampler.last
	if sampler.coreAt.IsZero() {
		sample = protocol.DefaultHostMetricsSample()
		sample.Node = node
	}
	refreshCore := sampler.coreAt.IsZero() || now.Sub(sampler.coreAt) >= time.Second
	refreshDetails := detailed && (sampler.detailsAt.IsZero() || now.Sub(sampler.detailsAt) >= time.Second)
	if refreshCore {
		current, err := cpu.TimesWithContext(ctx, true)
		ready := !sampler.coreAt.IsZero() && now.Sub(sampler.coreAt) < 5*time.Second
		sample.CpuBasisPoints = nil
		sample.CoreBasisPoints = []uint32{}
		if err == nil {
			if ready && len(current) == len(sampler.cpus) && len(current) > 0 {
				var total, busy float64
				for index, value := range current {
					elapsed, occupied := cpuDelta(sampler.cpus[index], value)
					total += elapsed
					busy += occupied
					if index < 1024 {
						sample.CoreBasisPoints = append(sample.CoreBasisPoints, utilization(occupied, elapsed))
					}
				}
				if total > 0 {
					value := utilization(busy, total)
					sample.CpuBasisPoints = &value
				}
			}
			sampler.cpus = current
		}
		if memory, err := mem.VirtualMemoryWithContext(ctx); err == nil && memory.Total > 0 {
			sample.Memory = &protocol.MemoryUsage{TotalBytes: memory.Total, AvailableBytes: min(memory.Available, memory.Total), FreeBytes: min(memory.Free, memory.Total)}
		} else {
			sample.Memory = nil
		}
		if swap, err := mem.SwapMemoryWithContext(ctx); err == nil && swap.Total > 0 {
			sample.Swap = &protocol.HostMetricsSwapUsage{TotalBytes: swap.Total, UsedBytes: min(swap.Used, swap.Total)}
		} else {
			sample.Swap = nil
		}
		sampler.coreAt = now
	}
	if refreshDetails {
		sampler.details(ctx, &sample, now)
		sampler.detailsAt = now
	}
	if err := ctx.Err(); err != nil {
		return protocol.HostMetricsSample{}, err
	}
	if refreshCore || refreshDetails {
		sample.SampledAtMs = uint64(now.UnixMilli())
	}
	sampler.last = sample
	if detailed {
		return clone(sample), nil
	}
	result := protocol.DefaultHostMetricsSample()
	result.Node, result.SampledAtMs = node, sample.SampledAtMs
	result.CpuBasisPoints, result.Memory = sample.CpuBasisPoints, sample.Memory
	return clone(result), nil
}

func (sampler *Sampler) details(ctx context.Context, sample *protocol.HostMetricsSample, now time.Time) {
	interval := now.Sub(sampler.detailsAt)
	ready := !sampler.detailsAt.IsZero() && interval < 5*time.Second
	sample.Load = nil
	if averages, err := load.AvgWithContext(ctx); err == nil {
		sample.Load = &protocol.HostMetricsLoadAverage{One: basisPoints(averages.Load1), Five: basisPoints(averages.Load5), Fifteen: basisPoints(averages.Load15)}
	}
	sampler.sampleNetwork(ctx, sample, interval, ready)
	sampler.sampleDisks(ctx, sample, interval, ready)
	sample.Gpus = sampleGPUs()
	sampler.sampleProcesses(ctx, sample, interval, ready)
}

func (sampler *Sampler) sampleNetwork(ctx context.Context, sample *protocol.HostMetricsSample, interval time.Duration, ready bool) {
	sample.Network = nil
	current, err := systemnet.IOCountersWithContext(ctx, true)
	if err != nil {
		sampler.interfaces = nil
		return
	}
	next := make(map[string]systemnet.IOCountersStat, len(current))
	total := protocol.HostMetricsNetworkRate{}
	var received, sent uint64
	valid := false
	for _, value := range current {
		device, err := net.InterfaceByName(value.Name)
		if err != nil || device.Flags&net.FlagLoopback != 0 || device.Flags&net.FlagUp == 0 {
			continue
		}
		addresses, err := device.Addrs()
		if err != nil || len(addresses) == 0 {
			continue
		}
		next[value.Name] = value
		total.ReceivedBytes += value.BytesRecv
		total.TransmittedBytes += value.BytesSent
		if previous, exists := sampler.interfaces[value.Name]; exists && ready && value.BytesRecv >= previous.BytesRecv && value.BytesSent >= previous.BytesSent {
			received += value.BytesRecv - previous.BytesRecv
			sent += value.BytesSent - previous.BytesSent
			valid = true
		}
	}
	if valid {
		total.ReceivedBytesPerSec = bytesPerSecond(received, interval)
		total.TransmittedBytesPerSec = bytesPerSecond(sent, interval)
		sample.Network = &total
	}
	sampler.interfaces = next
}

func (sampler *Sampler) sampleDisks(ctx context.Context, sample *protocol.HostMetricsSample, interval time.Duration, ready bool) {
	sample.Disks = []protocol.HostMetricsDisk{}
	partitions, err := disk.PartitionsWithContext(ctx, false)
	if err != nil {
		sampler.disks = nil
		return
	}
	counters, _ := disk.IOCountersWithContext(ctx)
	// Only the volumes the user stores files on, as the file browser's
	// devices (files.IsVolume): gopsutil lists /dev and the system's hidden
	// volumes on macOS. A volume of no size is not one either.
	partitions = slices.DeleteFunc(partitions, func(partition disk.PartitionStat) bool { return !files.IsVolume(runtime.GOOS, partition) })
	for _, partition := range partitions[:min(len(partitions), 128)] {
		usage, err := disk.UsageWithContext(ctx, partition.Mountpoint)
		if err != nil || usage.Total == 0 {
			continue
		}
		item := protocol.HostMetricsDisk{Mount: partition.Mountpoint, Name: partition.Device, FileSystem: partition.Fstype, TotalBytes: usage.Total, AvailableBytes: min(usage.Free, usage.Total)}
		device := filepath.Base(partition.Device)
		current, exists := counters[device]
		previous, seen := sampler.disks[device]
		if ready && exists && seen && current.ReadBytes >= previous.ReadBytes && current.WriteBytes >= previous.WriteBytes && (current.ReadBytes > 0 || current.WriteBytes > 0) {
			item.Io = &protocol.HostMetricsIoRate{ReadBytesPerSec: bytesPerSecond(current.ReadBytes-previous.ReadBytes, interval), WrittenBytesPerSec: bytesPerSecond(current.WriteBytes-previous.WriteBytes, interval)}
		}
		sample.Disks = append(sample.Disks, item)
	}
	sampler.disks = counters
}

func (sampler *Sampler) sampleProcesses(ctx context.Context, sample *protocol.HostMetricsSample, interval time.Duration, ready bool) {
	sample.Processes = []protocol.HostMetricsProcess{}
	current, err := process.ProcessesWithContext(ctx)
	if err != nil {
		sample.ProcessCount = 0
		sampler.processes = nil
		return
	}
	sample.ProcessCount = uint64(len(current))
	next := make(map[int32]processBaseline, len(current))
	for _, item := range current {
		started, err := item.CreateTimeWithContext(ctx)
		if err != nil || started < 0 {
			continue
		}
		name, _ := item.NameWithContext(ctx)
		name = string([]rune(name)[:min(len([]rune(name)), 256)])
		result := protocol.HostMetricsProcess{Pid: uint32(item.Pid), StartedAtSecs: uint64(started / 1000), Name: name}
		if memory, err := item.MemoryInfoWithContext(ctx); err == nil {
			result.MemoryBytes = memory.RSS
		}
		if times, err := item.TimesWithContext(ctx); err == nil {
			seconds := times.User + times.System
			if previous, exists := sampler.processes[item.Pid]; exists && ready && previous.started == started && seconds >= previous.seconds {
				value := basisPoints(100 * (seconds - previous.seconds) / interval.Seconds())
				result.CpuBasisPoints = &value
			}
			next[item.Pid] = processBaseline{started: started, seconds: seconds}
		}
		sample.Processes = append(sample.Processes, result)
	}
	slices.SortFunc(sample.Processes, func(left, right protocol.HostMetricsProcess) int {
		if (left.CpuBasisPoints == nil) != (right.CpuBasisPoints == nil) {
			if left.CpuBasisPoints == nil {
				return 1
			}
			return -1
		}
		if left.CpuBasisPoints != nil {
			if order := cmp.Compare(*right.CpuBasisPoints, *left.CpuBasisPoints); order != 0 {
				return order
			}
		}
		if order := cmp.Compare(right.MemoryBytes, left.MemoryBytes); order != 0 {
			return order
		}
		return cmp.Compare(left.Pid, right.Pid)
	})
	sample.Processes = sample.Processes[:min(len(sample.Processes), 2048)]
	sampler.processes = next
}

func cpuDelta(previous, current cpu.TimesStat) (float64, float64) {
	total := func(value cpu.TimesStat) float64 {
		return value.User + value.System + value.Nice + value.Idle + value.Iowait + value.Irq + value.Softirq + value.Steal
	}
	elapsed := max(0, total(current)-total(previous))
	idle := max(0, current.Idle+current.Iowait-previous.Idle-previous.Iowait)
	return elapsed, min(elapsed, max(0, elapsed-idle))
}

func utilization(busy, total float64) uint32 {
	if total <= 0 {
		return 0
	}
	return min(10000, basisPoints(100*busy/total))
}

func basisPoints(value float64) uint32 {
	if math.IsNaN(value) || value <= 0 {
		return 0
	}
	return uint32(min(float64(math.MaxUint32), math.Round(value*100)))
}

func bytesPerSecond(bytes uint64, interval time.Duration) uint64 {
	if interval <= 0 {
		return 0
	}
	return uint64(min(float64(math.MaxUint64), float64(bytes)/interval.Seconds()))
}

func nonempty(value string) *string {
	value = strings.TrimSpace(value)
	if value == "" {
		return nil
	}
	return &value
}
