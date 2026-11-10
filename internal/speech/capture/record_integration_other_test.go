//go:build integration && !darwin

package capture

import (
	"context"
	"testing"
)

// answered has nothing to ask: only macOS gates the microphone per app.
func answered(*testing.T, context.Context) {}
