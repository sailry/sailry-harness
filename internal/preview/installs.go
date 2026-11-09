package preview

import (
	"time"

	"github.com/sailry/sailry-go/internal/transfers"
)

// Installations is the preview's installation queue: a plugin
// downloading with its speed, a model queued, a language package
// counting its files on this app, and a failed and an uncertain request
// (installations/rows.rs's states). Nothing is installed.
func Installations() *transfers.Queue {
	return &transfers.Queue{Entries: []transfers.Entry{
		{Key: "i1", Kind: transfers.Plugin, Title: "Git worktrees", Status: transfers.Running, Phase: "installation_downloading",
			Percent: 45, Speed: 3.2 * 1048576, Connected: true, Actionable: true},
		{Key: "i2", Kind: transfers.Language, Title: "python 3.13.1", Status: transfers.Running, Phase: "installation_installing",
			Completed: 12, Total: 40, Controller: true, Connected: true, Actionable: true},
		{Key: "i3", Kind: transfers.Model, Title: "whisper-large-v3-turbo", Status: transfers.Queued, Percent: -1, Connected: true, Actionable: true},
		{Key: "i4", Kind: transfers.Plugin, Title: "Board games", Status: transfers.Failed, Percent: -1, Connected: true,
			Actionable: true, Retryable: true, Error: "The package signature is invalid"},
		{Key: "i5", Kind: transfers.Model, Title: "paddle-ocr-v5", Status: transfers.Uncertain, Percent: -1, Connected: false,
			Actionable: true, Retryable: true, Error: "The connection closed before the result arrived"},
	}}
}

// PlayDownload moves a Save as transfer along the frame clock: preparing,
// receiving its size over two seconds, publishing, finished. Nothing is
// read or written; the window says so as it finishes. It reports whether
// the transfer still moves.
func PlayDownload(d *transfers.Download, started, now time.Time) bool {
	if d.Status == transfers.Finished {
		return false
	}
	if d.Canceled {
		d.Status, d.Error = transfers.Finished, "files_download_cancelled"
		return false
	}
	t := now.Sub(started)
	switch {
	case t < 400*time.Millisecond:
		d.Status = transfers.Preparing
	case t < 2400*time.Millisecond:
		d.Status = transfers.Receiving
		d.Copied = d.Size * int64(t-400*time.Millisecond) / int64(2*time.Second)
	case t < 2800*time.Millisecond:
		d.Status, d.Copied = transfers.Publishing, d.Size
	default:
		d.Status = transfers.Finished
		return false
	}
	return true
}
