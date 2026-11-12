// Package host is a host's page: the header with the host's status and
// uptimes, its platform and versions, the monitor (processor, memory,
// utilization, network, disks, storage and processes) and the running
// sessions, with the update dialog, the port forwards and the metrics
// summary in the right column.
package host

import (
	"fmt"
	"math"
	"strconv"
	"strings"
	"time"
)

// Host is a host page's view model: one computer running a Node.
type Host struct {
	Live  bool
	ID    string
	Name  string // an i18n key for the preview's hosts
	Local bool
	// OS names the platform's logo: "macos", "windows", "linux" or "" for the
	// generic one.
	OS string
	// Detail is the platform line, an i18n key in the preview. The header
	// shows it before the Runtime's versions.
	Detail string
	// Offline is whether a live host's Node is not connected: its page keeps
	// what is known and samples nothing.
	Offline bool
	// Activity is the host's running and waiting sessions.
	Activity []Activity
	Runtime  Runtime
	// Update is nil for a host that does not update from here (this one).
	Update *Update
	// Metrics is nil while the first sample loads; MetricsError says why
	// it failed.
	Metrics      *Metrics
	MetricsError string
	// Ports are the port forwards to this computer.
	Ports []Port
	// Revoking is whether the removal of the host's pairing runs.
	Revoking bool
	// Stopping is whether a request to end one of the host's processes
	// runs: no other is sent meanwhile.
	Stopping bool
}

// Activity is a running or waiting session.
type Activity struct {
	Session  string // the conversation it opens
	Title    string
	Location string // project / branch
	Model    string // "" for none
	Status   string // i18n key: approval_pending or the phase's label
	Waiting  bool
}

// Runtime is what the host's Node tells of itself: empty fields are unknown.
// The header shows the version and the protocol after the platform, and the
// two uptimes as its facts.
type Runtime struct {
	Version string
	// Uptime is the computer's, since it started; NodeUptime is Sailry's on
	// it, since its Node started. Each is < 0 when unknown.
	Uptime, NodeUptime       time.Duration
	ProtocolMin, ProtocolMax int // 0 unknown
	Error                    string
	// Known is whether a snapshot came at all.
	Known bool
}

// Fact is a row of a process's details: its label's i18n key and its value.
type Fact struct{ Label, Value string }

// UptimeParts are the uptime's two largest nonzero units, days, hours and
// minutes, as (i18n key, count) pairs; minutes when both others are zero. nil
// for unknown.
func UptimeParts(d time.Duration) [][2]any {
	if d < 0 {
		return nil
	}
	s := int64(d / time.Second)
	var parts [][2]any
	for _, u := range []struct {
		key string
		n   int64
	}{{"host_uptime_days", s / 86400}, {"host_uptime_hours", s / 3600 % 24}, {"host_uptime_minutes", s / 60 % 60}} {
		if u.n > 0 || u.key == "host_uptime_minutes" && len(parts) == 0 {
			parts = append(parts, [2]any{u.key, u.n})
		}
	}
	return parts[:min(len(parts), 2)]
}

// Protocol is the protocol range: v3, v1-v3, or "" when unknown or inverted.
func Protocol(lo, hi int) string {
	switch {
	case lo <= 0 || hi <= 0 || lo > hi:
		return ""
	case lo == hi:
		return "v" + strconv.Itoa(lo)
	}
	return fmt.Sprintf("v%d-v%d", lo, hi)
}

// Phase is an update's step, named by its label's i18n key.
type Phase string

const (
	Idle         Phase = "update_idle"
	Checking     Phase = "update_checking"
	Current      Phase = "update_current"
	Available    Phase = "update_available"
	Downloading  Phase = "update_downloading"
	Verifying    Phase = "update_verifying"
	Staged       Phase = "update_staged"
	Applying     Phase = "update_applying"
	RestartWait  Phase = "update_restart"
	UpdateFailed Phase = "update_failed"
)

// Action is what the update dialog's button does.
type Action int

const (
	Check Action = iota
	Download
	Apply
	Status
)

// Update is a host's update.
type Update struct {
	Phase    Phase
	Current  string
	Target   string // "" for none
	Progress int    // percent, < 0 for none
	Note     string // why updates are unavailable, "" for none
	Error    string // what failed, "" for none
	Retry    Action
}

// Headline is the dialog's headline key: update_unavailable for an idle
// host with a note, else the phase.
func (u *Update) Headline() string {
	if u.Phase == Idle && u.Note != "" {
		return "update_unavailable"
	}
	return string(u.Phase)
}

// Primary is the dialog's button: its action and label's i18n key, ok false
// for none.
func (u *Update) Primary() (Action, string, bool) {
	if u.Error != "" {
		if u.Phase == UpdateFailed {
			return Check, "update_retry", true
		}
		return u.Retry, "update_retry", true
	}
	switch u.Phase {
	case Idle:
		if u.Note != "" {
			return 0, "", false
		}
		return Check, "update_check", true
	case Current:
		return Check, "update_check", true
	case Available:
		return Download, "update_download", true
	case Staged:
		return Apply, "update_apply", true
	case UpdateFailed:
		return Check, "update_retry", true
	}
	return 0, "", false
}

// Advance runs the button on the update shown as expected: a stale click, on
// an update that changed meanwhile, does nothing. The preview moves between
// example states; no updater runs.
func (u *Update) Advance(expected Update) {
	if *u != expected {
		return
	}
	a, _, ok := u.Primary()
	if !ok {
		return
	}
	u.Error = ""
	switch a {
	case Check:
		*u = Update{Phase: Available, Current: u.Current, Target: "0.2.0-preview", Progress: -1}
	case Download:
		u.Phase, u.Progress = Staged, 100
	case Apply:
		u.Phase, u.Progress = RestartWait, -1
	}
}

// Metrics is a sample of the host's load, with the history of the samples
// before it (Follow).
type Metrics struct {
	// At is when the Node took the sample.
	At  time.Time
	CPU float64 // percent, NaN unknown
	// Cores are the logical processors' loads in percent, empty when the
	// sample has none.
	Cores []float64
	// Load is the 1, 5 and 15 minute load averages, nil where the platform
	// has none.
	Load *[3]float64
	// MemoryUsed is what is not available of Memory, and MemoryCached what
	// is available but not free; Memory 0 is unknown.
	MemoryUsed, MemoryCached, Memory uint64
	// SwapUsed of Swap; Swap 0 is none or unknown.
	SwapUsed, Swap uint64
	// Received and Transmitted are bytes over Interval; the totals are
	// since the computer started, when Totals is set.
	Received, Transmitted           uint64
	ReceivedTotal, TransmittedTotal uint64
	Totals                          bool
	Interval                        time.Duration
	// GPUs are the accelerators the sample lists, none where the Node reads
	// none (it reads macOS's).
	GPUs []GPU
	// Disks are the volumes the sample lists: the Node leaves the device and
	// system mounts out.
	Disks     []Disk
	Processes []Process
	// ProcessCount is how many processes run, < 0 unknown: Processes are
	// the busiest of them.
	ProcessCount int
	// History are the last samples' points, oldest first, this sample's
	// last.
	History []Point
}

// GPU is an accelerator and its load.
type GPU struct {
	ID, Name string
	Usage    float64 // percent, NaN unknown
}

// Disk is a mounted volume.
type Disk struct {
	Mount, FileSystem  string
	Total, Available   uint64
	ReadBytes, Written uint64
}

// Process is a busy process.
type Process struct {
	Name string
	// PID is 0 and Started 0 for the preview's examples.
	PID     uint32
	Started uint64
	CPU     float64 // percent, NaN unknown
	Memory  uint64
	// History are the process's loads over the last samples, oldest first.
	History []float64
}

// Bytes writes a size in binary units: 512 B, 1.0 KiB; "" for a value that is
// not one.
func Bytes(v float64) string {
	if math.IsNaN(v) || math.IsInf(v, 0) || v < 0 {
		return ""
	}
	units := []string{"B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"}
	u := 0
	for v >= 1024 && u < len(units)-1 {
		v /= 1024
		u++
	}
	if u == 0 {
		return fmt.Sprintf("%.0f %s", v, units[u])
	}
	return fmt.Sprintf("%.1f %s", v, units[u])
}

// Percent writes a load: 12.5%; "" when not one.
func Percent(v float64) string {
	if math.IsNaN(v) || math.IsInf(v, 0) || v < 0 {
		return ""
	}
	return fmt.Sprintf("%.1f%%", v)
}

// Rate is bytes over an interval per second, "" for no interval.
func Rate(v uint64, d time.Duration) string {
	if d <= 0 {
		return ""
	}
	return Bytes(float64(v) / d.Seconds())
}

// Usage is used of total in percent, clamped to 0..100; ok false when the
// total is unknown.
func Usage(used, total uint64) (float64, bool) {
	if total == 0 {
		return 0, false
	}
	return min(max(float64(used)/float64(total)*100, 0), 100), true
}

// Port is a port forward (sailry_client::ports::Forwarder).
type Port struct {
	Remote, Local int
	Listening     bool
}

// Address is the forward's local address.
func (p Port) Address() string { return fmt.Sprintf("http://127.0.0.1:%d", p.Local) }

// Route writes the forward as the picker lists it: 3000 → 127.0.0.1:3000.
func (p Port) Route() string { return fmt.Sprintf("%d → 127.0.0.1:%d", p.Remote, p.Local) }

// AddPort checks a new forward: the host's port 1.65535 and the local one
// 0.65535, 0 choosing one; a host port already forwarded is port_exists. It
// returns the problem's i18n key, or "" and the forward.
func AddPort(ports []Port, remote, local string) (Port, string) {
	r, err := strconv.Atoi(strings.TrimSpace(remote))
	if err != nil || r < 1 || r > 65535 {
		return Port{}, "port_invalid"
	}
	l := 0
	if s := strings.TrimSpace(local); s != "" {
		if l, err = strconv.Atoi(s); err != nil || l < 0 || l > 65535 {
			return Port{}, "port_invalid"
		}
	}
	for _, p := range ports {
		if p.Remote == r {
			return Port{}, "port_exists"
		}
	}
	if l == 0 {
		l = r + 10000
		if l > 65535 {
			l = r
		}
	}
	return Port{Remote: r, Local: l, Listening: true}, ""
}
