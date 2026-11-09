// Package host is a host's page: the host's overview with its projects,
// runtime and running sessions, the update dialog, the port forwards, and the
// metrics in the right column. See docs/host.md.
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
	ID    string
	Name  string // an i18n key for the preview's hosts
	Local bool
	// OS names the platform's logo: "macos", "windows", "linux" or "" for the
	// generic one.
	OS     string
	Detail string // the platform line, an i18n key in the preview
	// Projects are the host's projects; the counts are its worktrees,
	// sessions and terminals.
	Projects                       []Project
	Worktrees, Sessions, Terminals int
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
}

// Project is a project on the host.
type Project struct{ ID, Name string }

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
type Runtime struct {
	Version, System, Timezone string
	Uptime                    time.Duration // < 0 unknown
	ProtocolMin, ProtocolMax  int           // 0 unknown
	Loading                   bool
	Error                     string
	// Known is whether a snapshot came at all (host_runtime_preview), or
	// the workspace only knows of the host (workspace_runtime_preview).
	Known bool
}

// Fact is a runtime row: its label's i18n key and its value, "" for
// unknown.
type Fact struct{ Label, Value string }

// Facts are the runtime's rows (Snapshot::facts); the uptime and the
// protocol are formatted by Uptime and Protocol.
func (r Runtime) Facts() []Fact {
	return []Fact{
		{"host_version", strings.TrimSpace(r.Version)},
		{"host_system_version", strings.TrimSpace(r.System)},
		{"host_timezone", strings.TrimSpace(r.Timezone)},
		{"host_uptime", ""},
		{"host_protocol", Protocol(r.ProtocolMin, r.ProtocolMax)},
	}
}

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
	Idle          Phase = "update_idle"
	Checking      Phase = "update_checking"
	Current       Phase = "update_current"
	Available     Phase = "update_available"
	Downloading   Phase = "update_downloading"
	Verifying     Phase = "update_verifying"
	Staged        Phase = "update_staged"
	Applying      Phase = "update_applying"
	RestartWait   Phase = "update_restart"
	UpdateFailed  Phase = "update_failed"
	UpdateLoading Phase = "update_loading"
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

// Metrics is a sample of the host's load.
type Metrics struct {
	CPU                   float64 // percent
	MemoryUsed, Memory    uint64  // bytes; Memory 0 unknown
	Received, Transmitted uint64  // bytes over Interval
	Interval              time.Duration
	Disks                 []Disk
	Processes             []Process
}

// Disk is a mounted volume.
type Disk struct {
	Mount, FileSystem  string
	Total, Available   uint64
	ReadBytes, Written uint64
}

// Process is a busy process.
type Process struct {
	Name   string
	CPU    float64
	Memory uint64
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
