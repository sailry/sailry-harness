package live

import (
	"context"
	"errors"
	"math"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) hostMetricsFrame(c *ui.Context, state *liveHost, demand, detailed bool) {
	if state.owner == nil || !state.owner.controller.Connected() {
		state.view.Metrics = nil
		state.view.MetricsError = "Host is disconnected"
	}
	if !demand || state.owner == nil {
		if state.metricsCancel != nil {
			state.metricsCancel()
			state.metricsCancel = nil
		}
		if state.metricsDemand {
			state.metricsEpoch++
			state.metricsPending = false
		}
		state.metricsDemand = false
		return
	}
	if !state.metricsDemand || detailed && !state.metricsDetail {
		state.metricsNext = time.Time{}
	}
	state.metricsDemand, state.metricsDetail = true, detailed
	if !state.metricsPending && !c.Now().Before(state.metricsNext) {
		r.readHostMetrics(state, detailed)
	}
	c.After(time.Second)
}

func (r *Runtime) readHostMetrics(state *liveHost, detailed bool) {
	owner := state.owner
	ctx, cancel := context.WithCancel(owner.ctx)
	state.metricsCancel = cancel
	state.metricsPending = true
	epoch := state.metricsEpoch
	r.async(func() {
		defer cancel()
		output, err := owner.controller.Do(ctx, protocol.Command{Kind: "read_host_metrics", Data: protocol.CommandReadHostMetricsPayload{Detailed: detailed}})
		sample, ok := detail[protocol.HostMetricsSample](output.Data)
		if err == nil && (output.Kind != "host_metrics" || !ok || sample.Node != owner.controller.Target()) {
			err = errors.New("Host metrics response belongs to another Node")
		}
		r.enqueue(func(c *ui.Context) {
			if r.hosts[state.view.ID] != state || state.owner != owner || state.metricsEpoch != epoch {
				return
			}
			state.metricsPending, state.metricsCancel = false, nil
			state.metricsNext = c.Now().Add(time.Second)
			if err != nil {
				state.view.Metrics = nil
				state.view.MetricsError = err.Error()
				if state.metricsError != err.Error() {
					kit.Error(c, i18n.T(c, "live_metrics_error"), err.Error())
				}
				state.metricsError = err.Error()
				return
			}
			if state.sample != nil && sample.SampledAtMs <= state.sample.SampledAtMs {
				return
			}
			state.sample = &sample
			metrics := hostMetrics(sample)
			metrics.Follow(state.metrics)
			state.metrics = metrics
			state.view.Metrics, state.view.MetricsError = metrics, ""
			state.metricsError = ""
		})
	})
}

// hostMetrics is a Node's sample as the host page shows it: every value the
// Node sends, and none it does not.
func hostMetrics(sample protocol.HostMetricsSample) *host.Metrics {
	metrics := &host.Metrics{At: time.UnixMilli(int64(min(sample.SampledAtMs, uint64(math.MaxInt64)))), CPU: math.NaN(), ProcessCount: -1}
	if sample.CpuBasisPoints != nil {
		metrics.CPU = float64(*sample.CpuBasisPoints) / 100
	}
	for _, core := range sample.CoreBasisPoints {
		metrics.Cores = append(metrics.Cores, float64(core)/100)
	}
	if load := sample.Load; load != nil {
		metrics.Load = &[3]float64{float64(load.One) / 100, float64(load.Five) / 100, float64(load.Fifteen) / 100}
	}
	if memory := sample.Memory; memory != nil {
		available := min(memory.AvailableBytes, memory.TotalBytes)
		metrics.Memory = memory.TotalBytes
		metrics.MemoryUsed = memory.TotalBytes - available
		metrics.MemoryCached = available - min(memory.FreeBytes, available)
	}
	if swap := sample.Swap; swap != nil {
		metrics.Swap, metrics.SwapUsed = swap.TotalBytes, min(swap.UsedBytes, swap.TotalBytes)
	}
	if network := sample.Network; network != nil {
		metrics.Interval = time.Second
		metrics.Received, metrics.Transmitted = network.ReceivedBytesPerSec, network.TransmittedBytesPerSec
		metrics.ReceivedTotal, metrics.TransmittedTotal, metrics.Totals = network.ReceivedBytes, network.TransmittedBytes, true
	}
	for _, gpu := range sample.Gpus {
		row := host.GPU{ID: gpu.ID, Name: gpu.Name, Usage: math.NaN()}
		if gpu.UsageBasisPoints != nil {
			row.Usage = float64(*gpu.UsageBasisPoints) / 100
		}
		metrics.GPUs = append(metrics.GPUs, row)
	}
	for _, disk := range sample.Disks {
		row := host.Disk{Mount: disk.Mount, FileSystem: disk.FileSystem, Total: disk.TotalBytes, Available: disk.AvailableBytes}
		if disk.Io != nil {
			row.ReadBytes, row.Written = disk.Io.ReadBytesPerSec, disk.Io.WrittenBytesPerSec
		}
		metrics.Disks = append(metrics.Disks, row)
	}
	for _, process := range sample.Processes {
		row := host.Process{Name: process.Name, PID: process.Pid, Started: process.StartedAtSecs, CPU: math.NaN(), Memory: process.MemoryBytes}
		if process.CpuBasisPoints != nil {
			row.CPU = float64(*process.CpuBasisPoints) / 100
		}
		metrics.Processes = append(metrics.Processes, row)
	}
	// A sample without detail lists no process and counts none.
	if len(sample.Processes) > 0 || sample.ProcessCount > 0 {
		metrics.ProcessCount = int(min(sample.ProcessCount, uint64(math.MaxInt32)))
	}
	return metrics
}

func (r *Runtime) projectSidebarMetrics(id string, enabled bool) {
	if !enabled {
		r.App.Sidebar.Metrics = nil
		return
	}
	display := &sidebar.Metrics{}
	if state := r.hosts[id]; state != nil && state.owner != nil && state.owner.controller.Connected() && state.view.MetricsError == "" && state.sample != nil {
		if state.sample.CpuBasisPoints != nil {
			cpu := float64(*state.sample.CpuBasisPoints) / 100
			display.CPU = &cpu
		}
		if memory := state.sample.Memory; memory != nil {
			if percent, ok := host.Usage(memory.TotalBytes-min(memory.AvailableBytes, memory.TotalBytes), memory.TotalBytes); ok {
				display.Memory = &percent
			}
		}
	}
	r.App.Sidebar.Metrics = display
}
