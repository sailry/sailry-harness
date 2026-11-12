package host

import (
	"fmt"
	"math"
	"slices"
	"strings"
	"time"
)

// HistoryLimit is how many samples the monitor's charts keep: a minute at
// the sampler's one sample a second (sailry_client::host::HISTORY_LIMIT).
const HistoryLimit = 60

// historyGap is the pause between two samples after which the history
// starts again, as the reference's View::accept: a chart never joins the
// values around a disconnection.
const historyGap = 5 * time.Second

// Point is one sample in the monitor's history. A value the sample did not
// have is NaN, which breaks a chart's line there.
type Point struct {
	At time.Time
	// CPU and Memory are percentages.
	CPU, Memory float64
	// Received and Transmitted are bytes a second.
	Received, Transmitted float64
	// Disks are the read and written bytes a second of each volume, by its
	// mount.
	Disks map[string][2]float64
	// GPUs are the accelerators' loads in percent, by their IDs: one whose
	// load the sample did not have is absent.
	GPUs map[string]float64
}

// Follow continues the history of the sample before this one: its points and
// its processes' loads, each followed by this sample's, at most HistoryLimit
// of them. A sample that is not newer keeps the earlier history as it is; one
// that comes after a pause, or the first, starts the history.
func (m *Metrics) Follow(prev *Metrics) {
	var history []Point
	loads := map[processKey][]float64{}
	if prev != nil && !m.At.IsZero() && !prev.At.IsZero() {
		if !m.At.After(prev.At) {
			m.History = prev.History
			for i := range m.Processes {
				m.Processes[i].History = prev.history(m.Processes[i])
			}
			return
		}
		if m.At.Sub(prev.At) <= historyGap {
			history = prev.History
			for _, x := range prev.Processes {
				loads[processKey{x.PID, x.Started}] = x.History
			}
		}
	}
	point := Point{At: m.At, CPU: m.CPU, Memory: math.NaN(), Received: math.NaN(), Transmitted: math.NaN(), Disks: map[string][2]float64{}, GPUs: map[string]float64{}}
	if v, ok := Usage(m.MemoryUsed, m.Memory); ok {
		point.Memory = v
	}
	if m.Interval > 0 {
		point.Received = float64(m.Received) / m.Interval.Seconds()
		point.Transmitted = float64(m.Transmitted) / m.Interval.Seconds()
	}
	for _, d := range m.Disks {
		point.Disks[d.Mount] = [2]float64{float64(d.ReadBytes), float64(d.Written)}
	}
	for _, g := range m.GPUs {
		if !math.IsNaN(g.Usage) {
			point.GPUs[g.ID] = g.Usage
		}
	}
	m.History = trim(append(slices.Clone(history), point))
	for i, x := range m.Processes {
		m.Processes[i].History = trim(append(slices.Clone(loads[processKey{x.PID, x.Started}]), x.CPU))
	}
}

// processKey names a process across samples: a PID is reused, its start is
// not.
type processKey struct {
	pid     uint32
	started uint64
}

func (m *Metrics) history(x Process) []float64 {
	for _, old := range m.Processes {
		if old.PID == x.PID && old.Started == x.Started {
			return old.History
		}
	}
	return nil
}

func trim[T any](values []T) []T {
	if len(values) > HistoryLimit {
		return values[len(values)-HistoryLimit:]
	}
	return values
}

// Volume is the volume the disk card follows: the one chosen, or the root,
// or the first (Monitor::accept). ok is false when the host has none. The
// Node lists only the volumes the user stores files on.
func (m *Metrics) Volume(chosen string) (Disk, bool) {
	volumes := m.Disks
	for _, want := range []string{chosen, "/", `C:\`} {
		for _, d := range volumes {
			if want != "" && d.Mount == want {
				return d, true
			}
		}
	}
	if len(volumes) == 0 {
		return Disk{}, false
	}
	return volumes[0], true
}

// GPU is the accelerator the GPU card follows: the one chosen, or the first
// (Monitor::accept). ok is false when the sample has none.
func (m *Metrics) GPU(chosen string) (GPU, bool) {
	for _, g := range m.GPUs {
		if g.ID == chosen {
			return g, true
		}
	}
	if len(m.GPUs) == 0 {
		return GPU{}, false
	}
	return m.GPUs[0], true
}

// Used is the volume's used space, and its share of the whole in percent.
func (d Disk) Used() (uint64, float64) {
	used := d.Total - min(d.Available, d.Total)
	percent, _ := Usage(used, d.Total)
	return used, percent
}

// Scaled writes a size as its number and its binary unit apart, for a
// figure whose unit is set smaller: "12.5", "GiB".
func Scaled(v float64) (string, string) {
	text := Bytes(v)
	if number, unit, ok := strings.Cut(text, " "); ok {
		return number, unit
	}
	return text, ""
}

// Clock writes a sample's time as a chart's axis shows it.
func Clock(t time.Time) string { return t.Format("15:04:05") }

// Load writes a load average: 1.25.
func Load(v float64) string { return fmt.Sprintf("%.2f", v) }

// SortProcesses orders processes by a column of the table, "name", "cpu",
// "memory" or "pid", and keeps those whose name or PID contains query. An
// unknown load sorts below every known one.
func SortProcesses(rows []Process, query, column string, descending bool) []Process {
	query = strings.ToLower(strings.TrimSpace(query))
	var out []Process
	for _, x := range rows {
		if query == "" || strings.Contains(strings.ToLower(x.Name), query) || x.PID != 0 && strings.Contains(fmt.Sprint(x.PID), query) {
			out = append(out, x)
		}
	}
	load := func(x Process) float64 {
		if math.IsNaN(x.CPU) {
			return -1
		}
		return x.CPU
	}
	slices.SortStableFunc(out, func(a, b Process) int {
		order := 0
		switch column {
		case "name":
			order = strings.Compare(strings.ToLower(a.Name), strings.ToLower(b.Name))
		case "memory":
			order = compare(a.Memory, b.Memory)
		case "pid":
			order = compare(a.PID, b.PID)
		default:
			order = compare(load(a), load(b))
		}
		if descending {
			order = -order
		}
		return order
	})
	return out
}

func compare[T int | uint32 | uint64 | float64](a, b T) int {
	switch {
	case a < b:
		return -1
	case a > b:
		return 1
	}
	return 0
}
