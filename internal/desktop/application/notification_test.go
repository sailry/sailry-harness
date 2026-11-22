package application

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"log"
	"slices"
	"strings"
	"testing"

	"github.com/egoist/mygo"
)

func TestSystemNotificationResult(t *testing.T) {
	for _, tc := range []struct {
		name        string
		err         error
		canceled    bool
		fallback    bool
		logDelivery bool
	}{
		{name: "unsupported", err: errors.ErrUnsupported, fallback: true},
		{name: "wrapped unsupported", err: fmt.Errorf("notification backend: %w", errors.ErrUnsupported), fallback: true},
		{name: "untyped failure", err: errors.New("not supported on this platform"), fallback: true, logDelivery: true},
		{name: "permission denied", err: mygo.ErrNotificationsDenied, fallback: true, logDelivery: true},
		{name: "delivered"},
		{name: "shutdown", err: context.Canceled, canceled: true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			var output bytes.Buffer
			previousWriter, previousFlags := log.Writer(), log.Flags()
			t.Cleanup(func() {
				log.SetOutput(previousWriter)
				log.SetFlags(previousFlags)
			})
			log.SetOutput(&output)
			log.SetFlags(0)

			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			if tc.canceled {
				cancel()
			}
			ids := []string{"first", "second"}
			var returned []string
			for _, id := range ids {
				completeSystemNotification(ctx, id, tc.err, func(id string) { returned = append(returned, id) })
			}

			var wantIDs []string
			if tc.fallback {
				wantIDs = ids
			}
			if !slices.Equal(returned, wantIDs) {
				t.Errorf("returned notices = %v, want %v", returned, wantIDs)
			}
			var wantLog string
			if tc.logDelivery {
				wantLog = strings.Repeat(fmt.Sprintf("desktop notification: %v\n", tc.err), len(ids))
			}
			if got := output.String(); got != wantLog {
				t.Errorf("log = %q, want %q", got, wantLog)
			}
		})
	}
}
