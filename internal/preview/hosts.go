package preview

import (
	"time"

	"github.com/sailry/sailry-go/internal/host"
)

// HostPages are the host pages' data over the dataset (workspace/model.rs
// State::default): each host's projects and counts, its running sessions,
// this host's runtime, the paired host's update, the
// metrics (metrics/data.rs Snapshot::sample) and a port forward. The
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
			x.Worktrees += max(len(pr.Worktrees), 1)
			x.Sessions += len(pr.Conversations)
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
		mount, fs := "/preview/remote-volume", "ext4"
		if h.Local {
			used, total, cpu, mount, fs = 8*gib, 32*gib, 12.5, "/preview", "APFS"
			x.Terminals = 1
			x.Runtime = host.Runtime{
				Version: "0.1.0-preview", System: "macOS 26.1", Timezone: "Asia/Shanghai",
				Uptime: 3*24*time.Hour + 5*time.Hour + 12*time.Minute, ProtocolMin: 1, ProtocolMax: 1, Known: true,
			}
		} else {
			x.Runtime = host.Runtime{Uptime: -1}
			x.Update = &host.Update{Phase: host.Idle, Current: "0.1.0-preview", Progress: -1}
		}
		x.Metrics = &host.Metrics{
			CPU: cpu, MemoryUsed: used, Memory: total, Received: 512 << 10, Transmitted: 128 << 10, Interval: 2 * time.Second,
			Disks: []host.Disk{{Mount: mount, FileSystem: fs, Total: 256 * gib, Available: 160 * gib, ReadBytes: 8 << 20, Written: 2 << 20}},
			Processes: []host.Process{
				{Name: "metrics_worker", CPU: 8, Memory: 256 << 20},
				{Name: "metrics_indexer", CPU: 4.5, Memory: 128 << 20},
			},
		}
		x.Ports = []host.Port{{Remote: 3000, Local: 13000, Listening: true}, {Remote: 8080, Local: 18080}}
		out[h.ID] = x
	}
	return out
}
