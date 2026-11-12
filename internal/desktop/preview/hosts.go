package preview

import (
	"math"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/host"
)

// HostPages are the host pages' data over the dataset: each host's
// projects, its running sessions, this host's runtime (its versions and
// uptimes), the paired host's update, the metrics and a port forward. The
// paired host's runtime is unknown, as the reference's preview leaves it.
func HostPages(d *Data) map[string]*host.Host {
	const gib = 1 << 30
	out := map[string]*host.Host{}
	for _, h := range d.Hosts {
		x := &host.Host{ID: h.ID, Name: h.Name, Local: h.Local, Detail: "remote_host_detail", OS: "linux"}
		if h.Local {
			x.Detail, x.OS = "local_host_detail", "macos"
		}
		for _, pr := range d.Projects {
			if pr.Host != h.ID {
				continue
			}
			x.Projects = append(x.Projects, host.Project{ID: pr.ID, Name: pr.Name})
			for _, cv := range pr.Conversations {
				if cv.Status != Running {
					continue
				}
				branch := pr.Branch
				if cv.Worktree != "" {
					branch = cv.Worktree
				}
				status, waiting := "turn_thinking", false
				if len(cv.Waiting.Approvals) > 0 {
					status, waiting = "approval_pending", true
				}
				x.Activity = append(x.Activity, host.Activity{
					Session: cv.ID, Title: cv.Title, Location: pr.Name + " / " + branch,
					Model: d.Models[0].Name, Status: status, Waiting: waiting,
				})
			}
		}
		used, total, cpu := uint64(3*gib), uint64(8*gib), 24.0
		mount, fs, cores := "/preview/remote-volume", "ext4", 4
		if h.Local {
			used, total, cpu, mount, fs, cores = 8*gib, 32*gib, 12.5, "/preview", "APFS", 10
			x.Runtime = host.Runtime{
				Version: "0.1.0-preview",
				Uptime:  3*24*time.Hour + 5*time.Hour + 12*time.Minute, NodeUptime: 42 * time.Minute,
				ProtocolMin: 1, ProtocolMax: 1, Known: true,
			}
		} else {
			x.Runtime = host.Runtime{Uptime: -1, NodeUptime: -1}
			x.Update = &host.Update{Phase: host.Idle, Current: "0.1.0-preview", Progress: -1}
		}
		x.Metrics = hostMetrics(cpu, used, total, cores, mount, fs)
		x.Ports = []host.Port{{Remote: 3000, Local: 13000, Listening: true}, {Remote: 8080, Local: 18080}}
		out[h.ID] = x
	}
	return out
}

// hostSampled is when the preview's last example sample was taken: a fixed
// time, so the charts' axes read the same in every run.
var hostSampled = time.Date(2026, time.November, 9, 9, 30, 0, 0, time.UTC)

// hostMetrics is a host's example load with a minute of history: the load
// and the transfers swing around their values, one sample a second, and
// end on them, so the monitor's charts have lines to draw. Nothing is
// sampled.
func hostMetrics(cpu float64, used, total uint64, cores int, mount, fs string) *host.Metrics {
	const gib = 1 << 30
	var last *host.Metrics
	for i := range host.HistoryLimit {
		swing := math.Sin(float64(host.HistoryLimit-1-i) / 6)
		m := &host.Metrics{
			At:  hostSampled.Add(time.Duration(i-host.HistoryLimit+1) * time.Second),
			CPU: cpu + 6*swing, Load: &[3]float64{float64(cores) * (cpu + 6*swing) / 100, float64(cores) * cpu / 100, float64(cores) * cpu / 120},
			MemoryUsed: uint64(int64(used) + int64(swing*float64(total)/64)), MemoryCached: total / 8, Memory: total, SwapUsed: gib / 2, Swap: 2 * gib,
			Received: uint64((1 + swing/2) * (512 << 10)), Transmitted: uint64((1 - swing/2) * (128 << 10)), Interval: 2 * time.Second,
			ReceivedTotal: 12 * gib, TransmittedTotal: 3 * gib, Totals: true,
			Disks: []host.Disk{{Mount: mount, FileSystem: fs, Total: 256 * gib, Available: 160 * gib,
				ReadBytes: uint64((1 + swing/2) * (8 << 20)), Written: uint64((1 - swing/2) * (2 << 20))}},
			Processes: []host.Process{
				{Name: "metrics_worker", CPU: 8 + 2*swing, Memory: 256 << 20},
				{Name: "metrics_indexer", CPU: 4.5 - swing, Memory: 128 << 20},
			},
			ProcessCount: 2,
		}
		for core := range cores {
			m.Cores = append(m.Cores, min(max(cpu+6*swing+float64((core*37)%23)-11, 0), 100))
		}
		m.Follow(last)
		last = m
	}
	return last
}
