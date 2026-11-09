package transfers

import "testing"

func TestEntries(t *testing.T) {
	run := Entry{Status: Running, Phase: "installation_downloading", Percent: 45}
	count := Entry{Status: Running, Completed: 3, Total: 12, Percent: -1}
	none := Entry{Status: Running, Percent: -1}
	if run.Label() != "installation_downloading" || run.Measure() != "45%" || count.Measure() != "3/12" || none.Measure() != "" {
		t.Error("labels and measures")
	}
	if (Entry{Status: Queued}).Label() != "installation_queued" || !(Entry{Status: Uncertain}).Terminal() {
		t.Error("statuses")
	}
	if v, ok := Overall([]Entry{run, count, none, {Status: Failed}}); !ok || v != 35 {
		t.Errorf("overall %v %v", v, ok)
	}
	if _, ok := Overall([]Entry{none}); ok {
		t.Error("an unmeasured queue has a measure")
	}
	q := &Queue{Entries: []Entry{{Key: "a", Status: Failed, Retryable: true, Error: "e"}, {Key: "b"}}}
	q.Retry("a")
	if q.Entries[0].Status != Starting || q.Entries[0].Error != "" {
		t.Error("retry")
	}
	q.Remove("b")
	if len(q.Entries) != 1 {
		t.Error("remove")
	}
}

func TestDownloadKey(t *testing.T) {
	d := &Download{Status: Receiving, Copied: 1, Size: 4}
	if k, v := d.Key(); k != "files_download_receiving" || v != 0.25 {
		t.Errorf("%s %v", k, v)
	}
	d.Status, d.Error = Finished, "files_download_cancelled"
	if k, v := d.Key(); k != "files_download_cancelled" || v >= 0 {
		t.Errorf("%s %v", k, v)
	}
}
