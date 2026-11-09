// Package transfers is the installations queue and the download dialog
// (sailry-public installations.rs, installations/{popover,rows}.rs,
// downloads.rs): the rail's installations button with its ring and its
// popover of requests, and the Save as dialog's progress. See
// docs/transfers.md.
package transfers

import (
	"fmt"
	"slices"
	"strconv"
)

// Kind is what an installation installs (installation::Resource).
type Kind int

const (
	Plugin Kind = iota
	Model
	Language
)

// Status is where an installation is (installations.rs Presentation and
// installation::State).
type Status int

const (
	Waiting   Status = iota // waiting for the Node to take it
	Starting                // sent, not yet observed
	Queued                  // the Node queued it
	Running                 // in Phase
	Failed                  // Error says why
	Uncertain               // the outcome is unknown; Error says why
)

// Entry is an installation request (rows.rs Row).
type Entry struct {
	Key   string
	Kind  Kind
	Title string // the plugin's name, the model's ID, "package version"
	Status
	// Phase is the running phase's label: installation_downloading,
	// _inspecting, _preparing, _installing or _validating.
	Phase string
	// Percent, or Completed of Total when counted; Percent < 0 and Total
	// 0 measure nothing. Speed is bytes a second, 0 for none.
	Percent          float64
	Completed, Total int
	Speed            float64
	// Controller is whether this app runs it, not the Node
	// (installation_controller, installation_node).
	Controller bool
	Connected  bool
	// Actionable entries can be removed; Retryable ones retried.
	Actionable, Retryable bool
	Error                 string
}

// Terminal reports whether the request ended without success, which the
// rail's button marks (attention).
func (e Entry) Terminal() bool { return e.Status == Failed || e.Status == Uncertain }

// Active reports whether it is starting or running.
func (e Entry) Active() bool { return e.Status == Starting || e.Status == Running }

// Label is the status's i18n key (rows.rs label).
func (e Entry) Label() string {
	switch e.Status {
	case Waiting:
		return "installation_waiting"
	case Starting:
		return "installation_starting"
	case Queued:
		return "installation_queued"
	case Failed:
		return "installation_failed"
	case Uncertain:
		return "installation_uncertain"
	}
	if e.Phase == "" {
		return "installation_installing"
	}
	return e.Phase
}

// Measured reports whether a running request has a measure, and its
// percentage (rows.rs percentage).
func (e Entry) Measured() (float64, bool) {
	switch {
	case e.Status != Running:
		return 0, false
	case e.Total > 0:
		return float64(e.Completed) / float64(e.Total) * 100, true
	case e.Percent >= 0:
		return e.Percent, true
	}
	return 0, false
}

// Measure is the measure as the row shows it: 45% or 3/10, "" for none.
func (e Entry) Measure() string {
	switch {
	case e.Status != Running:
		return ""
	case e.Total > 0:
		return strconv.Itoa(e.Completed) + "/" + strconv.Itoa(e.Total)
	case e.Percent >= 0:
		return fmt.Sprintf("%.0f%%", e.Percent)
	}
	return ""
}

// Overall is the rail ring's progress (popover.rs): the mean of the
// measured running requests, ok false when none is measured.
func Overall(entries []Entry) (float64, bool) {
	sum, n := 0.0, 0
	for _, e := range entries {
		if v, ok := e.Measured(); ok {
			sum += v
			n++
		}
	}
	if n == 0 {
		return 0, false
	}
	return sum / float64(n), true
}

// Queue is the installation requests in their order.
type Queue struct {
	Entries []Entry
}

// Remove drops a request, as its Remove does.
func (q *Queue) Remove(key string) {
	q.Entries = slices.DeleteFunc(q.Entries, func(e Entry) bool { return e.Key == key })
}

// Retry starts a retryable request again.
func (q *Queue) Retry(key string) {
	for i := range q.Entries {
		if e := &q.Entries[i]; e.Key == key && e.Retryable {
			e.Status, e.Error, e.Retryable = Starting, "", false
		}
	}
}

// DownloadStatus is a download's step (downloads/transfer.rs Status).
type DownloadStatus int

const (
	Preparing DownloadStatus = iota
	Receiving
	Publishing
	Finished
)

// Download is the Save as dialog's transfer (downloads.rs Form).
type Download struct {
	Open        bool
	Name        string // the file
	Destination string // where it goes
	Status      DownloadStatus
	Copied      int64
	Size        int64
	// Error is the i18n key of how it finished without success,
	// files_download_cancelled when canceled; "" for success.
	Error    string
	Canceled bool
}

// Key is the status line's i18n key, and the progress in 0..1, < 0 for
// none (downloads.rs render).
func (d *Download) Key() (string, float64) {
	switch d.Status {
	case Preparing:
		return "files_download_preparing", -1
	case Receiving:
		if d.Size == 0 {
			return "files_download_receiving", 1
		}
		return "files_download_receiving", float64(d.Copied) / float64(d.Size)
	case Publishing:
		return "files_download_publishing", -1
	}
	if d.Error != "" {
		return d.Error, -1
	}
	return "files_download_done", 1
}
