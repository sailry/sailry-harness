package live

import (
	"errors"
	"fmt"
	"reflect"
	"testing"
	"time"

	"github.com/egoist/mygo/plugins/permissionflow"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPermissionsMapping(t *testing.T) {
	const (
		disk          = protocol.PermissionsKindFullDisk
		screen        = protocol.PermissionsKindScreenRecording
		accessibility = protocol.PermissionsKindAccessibility
		microphone    = protocol.PermissionsKindMicrophone
	)
	for _, test := range []struct {
		goos     string
		kind     protocol.PermissionsKind
		status   permissionflow.Status
		err      error
		expected string
	}{
		{"darwin", disk, permissionflow.Granted, nil, "permission_granted"},
		{"darwin", disk, permissionflow.Denied, nil, "permission_required"},
		{"darwin", disk, permissionflow.Unknown, nil, "permission_unknown"},
		{"darwin", accessibility, permissionflow.Granted, nil, "permission_granted"},
		{"darwin", accessibility, permissionflow.Denied, nil, "permission_required"},
		{"darwin", screen, permissionflow.Granted, nil, "permission_granted"},
		{"darwin", screen, permissionflow.Denied, nil, "permission_required"},
		{"darwin", microphone, permissionflow.Granted, nil, "permission_granted"},
		{"darwin", microphone, permissionflow.NotDetermined, nil, "permission_required"},
		{"darwin", microphone, permissionflow.Denied, nil, "permission_denied"},
		{"darwin", microphone, permissionflow.Unknown, nil, "permission_unknown"},
		{"darwin", microphone, permissionflow.Granted, errors.New("framework"), "permission_unknown"},
		{"linux", accessibility, permissionflow.Unknown, errors.ErrUnsupported, "permission_unavailable"},
		{"windows", microphone, permissionflow.Granted, nil, "permission_unavailable"},
	} {
		if status := permissionStatus(test.goos, test.kind, test.status, test.err); status != test.expected {
			t.Errorf("%s %s %s: %s", test.goos, test.kind, test.status, status)
		}
	}
	for kind, names := range permissionKinds {
		if names.key == "" || names.flow == "" {
			t.Errorf("%s has no row or system permission", kind)
		}
	}

	local, remote := protocol.NodeID{1}, protocol.NodeID{2}
	handler := "check"
	helper := func(kind protocol.PermissionsKind, check, request bool) permissionEntry {
		entry := permissionEntry{declaration: protocol.PermissionsContribution{Kind: kind, Subject: "dev.sailry.computer"}}
		if check {
			entry.declaration.Check = &handler
		}
		if request {
			entry.declaration.Request = &handler
		}
		return entry
	}
	on := func(device protocol.NodeID, connected bool) func(permissionEntry) (protocol.NodeID, bool) {
		return func(permissionEntry) (protocol.NodeID, bool) { return device, connected }
	}
	statuses := func(cards []permissionCard) (values []string) {
		for _, card := range cards {
			values = append(values, card.subject+" "+string(card.kind)+" "+card.status)
		}
		return values
	}
	set := func(cards []permissionCard, subject string, kind protocol.PermissionsKind, status string) {
		for index := range cards {
			if cards[index].subject == subject && cards[index].kind == kind {
				cards[index].status = status
			}
		}
	}

	t.Run("cards", func(t *testing.T) {
		entries := []permissionEntry{helper(accessibility, true, true), helper(screen, true, true), helper(accessibility, true, true)}
		cards := permissionCards("darwin", local, entries, on(local, true), nil)
		if !reflect.DeepEqual(statuses(cards), []string{
			"sailry full_disk permission_unknown", "sailry accessibility permission_unknown", "sailry microphone permission_unknown",
			"dev.sailry.computer accessibility permission_unknown", "dev.sailry.computer screen_recording permission_unknown",
		}) {
			t.Fatal(statuses(cards))
		}
		if cards[0].request || !cards[0].opensSettings() || !cards[1].request || !cards[3].request || !cards[3].check || !cards[3].settings || cards[3].entry == nil {
			t.Fatal(cards)
		}
		// Known statuses survive a rebind; another device's grants are only read.
		set(cards, "dev.sailry.computer", screen, "permission_granted")
		cards = permissionCards("darwin", local, entries[:2], on(local, false), cards)
		if cards[4].status != "permission_granted" || cards[4].check || cards[4].request {
			t.Fatal(cards[4])
		}
		cards = permissionCards("darwin", local, entries[:2], on(remote, true), nil)
		if !cards[3].check || cards[3].request || cards[3].settings || cards[3].device != remote {
			t.Fatal(cards[3])
		}
		// A grant without a check handler is this process's own, or another device's.
		cards = permissionCards("darwin", local, []permissionEntry{helper(screen, false, false)}, on(local, true), nil)
		if cards[3].entry != nil || !cards[3].check || !cards[3].request || cards[3].subject != "dev.sailry.computer" {
			t.Fatal(cards[3])
		}
		cards = permissionCards("darwin", local, []permissionEntry{helper(screen, false, false)}, on(remote, true), nil)
		if cards[3].status != "permission_remote" || cards[3].actionable() {
			t.Fatal(cards[3])
		}
		cards = permissionCards("linux", local, nil, on(local, true), nil)
		for _, card := range cards {
			if card.status != "permission_unavailable" || card.check || card.actionable() {
				t.Fatal(card)
			}
		}
	})
	t.Run("rows", func(t *testing.T) {
		cards := permissionCards("darwin", local, []permissionEntry{helper(accessibility, true, true), helper(screen, true, true)}, on(local, true), nil)
		rows := func() map[string]viewmodel.Permission {
			values := map[string]viewmodel.Permission{}
			for _, row := range permissionRows(cards) {
				values[row.Resource] = row
			}
			return values
		}
		if len(permissionRows(cards)) != 4 || permissionRows(cards)[0].Resource != "permission_full_disk" {
			t.Fatal(permissionRows(cards))
		}
		// Full Disk Access is given only in the system's settings.
		if row := rows()["permission_full_disk"]; row.Status != "permission_unknown" || !row.Settings {
			t.Fatal(row)
		}
		set(cards, permissionApp, accessibility, "permission_required")
		set(cards, "dev.sailry.computer", accessibility, "permission_required")
		if row := rows()["permission_accessibility"]; row.Status != "permission_required" || row.Settings {
			t.Fatal(row)
		}
		set(cards, permissionApp, accessibility, "permission_granted")
		if row := rows()["permission_accessibility"]; row.Status != "permission_partial" || row.Settings {
			t.Fatal(row)
		}
		set(cards, "dev.sailry.computer", accessibility, "permission_granted")
		if row := rows()["permission_accessibility"]; row.Status != "permission_granted" || !row.Settings {
			t.Fatal(row)
		}
		set(cards, permissionApp, microphone, "permission_denied")
		if row := rows()["permission_microphone"]; row.Status != "permission_denied" || !row.Settings {
			t.Fatal(row)
		}
		set(cards, "dev.sailry.computer", screen, "permission_unavailable")
		if row := rows()["permission_screen"]; row.Status != "permission_unavailable" || row.Settings {
			t.Fatal(row)
		}
	})
	t.Run("requests", func(t *testing.T) {
		cards := permissionCards("darwin", local, []permissionEntry{helper(accessibility, true, true), helper(screen, true, false)}, on(local, true), nil)
		if requests := permissionRequests(cards, accessibility); len(requests) != 2 || requests[0].subject != permissionApp || requests[1].entry == nil {
			t.Fatal(requests)
		}
		set(cards, permissionApp, accessibility, "permission_granted")
		if requests := permissionRequests(cards, accessibility); len(requests) != 1 || requests[0].entry == nil {
			t.Fatal(requests)
		}
		// A package grant without a request handler is never asked for.
		if requests := permissionRequests(cards, screen); len(requests) != 0 {
			t.Fatal(requests)
		}
		set(cards, "dev.sailry.computer", accessibility, "permission_unavailable")
		if requests := permissionRequests(cards, accessibility); len(requests) != 0 {
			t.Fatal(requests)
		}
		if requests := permissionRequests(cards, disk); len(requests) != 1 || !requests[0].opensSettings() {
			t.Fatal(requests)
		}
	})
	t.Run("failed checks of one action toast once", func(t *testing.T) {
		failure := errors.New("framework")
		r := &Runtime{}
		r.permissions.probe = permissionProbe{goos: "darwin", check: func(permission permissionflow.Permission) (permissionflow.Status, error) {
			return permissionflow.Unknown, failure
		}}
		r.permissions.cards = permissionCards("darwin", local, nil, on(local, true), nil)
		var pending []func(*ui.Context)
		for _, card := range r.permissions.cards {
			pending = append(pending, r.permissionStep(card, false)())
		}
		var toasts []string
		tester := ui.NewTester(func(view *ui.Context) {
			for _, apply := range pending {
				apply(view)
			}
			if pending != nil {
				// Work still running holds the toast back.
				r.permissions.checking = true
				r.permissions.report(view)
				r.permissions.checking = false
				r.permissions.report(view)
				r.permissions.report(view)
			}
			pending = nil
			ui.ToastViewportBase(view, func(_ ui.Element, shown []ui.Toast) {
				toasts = toasts[:0]
				for _, toast := range shown {
					toasts = append(toasts, toast.Title)
				}
			})
		}, 320, 200)
		tester.SetLocale("en")
		tester.Frame()
		tester.Frame()
		if !reflect.DeepEqual(toasts, []string{i18n.In("en", "permission_failed")}) || r.permissions.cards[1].status != "permission_unknown" {
			t.Fatalf("failed checks: toasts %v, status %s", toasts, r.permissions.cards[1].status)
		}
		// A grant the system does not keep is an ordinary state.
		failure = fmt.Errorf("too old: %w", errors.ErrUnsupported)
		apply := r.permissionStep(r.permissions.cards[1], false)()
		tester.Advance(time.Minute)
		tester.Frame()
		pending = []func(*ui.Context){apply}
		tester.Frame()
		tester.Frame()
		if len(toasts) != 0 || r.permissions.cards[1].status != "permission_unavailable" {
			t.Fatalf("unsupported check: toasts %v, status %s", toasts, r.permissions.cards[1].status)
		}
	})
	t.Run("bundle", func(t *testing.T) {
		if bundle := permissionBundle("/Applications/Sailry.app/Contents/MacOS/sailry-desktop"); bundle != "/Applications/Sailry.app" {
			t.Fatal(bundle)
		}
		if bundle := permissionBundle("/work/bin/sailry-desktop"); bundle != "" {
			t.Fatal(bundle)
		}
	})
}
