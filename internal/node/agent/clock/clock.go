// Package clock provides the injected time boundary for agent policy deadlines.
package clock

import (
	"context"
	"time"
)

type Clock interface {
	Now() time.Time
	Sleep(context.Context, time.Duration) error
}

type System struct{}

func (System) Now() time.Time { return time.Now() }
func (System) Sleep(ctx context.Context, duration time.Duration) error {
	timer := time.NewTimer(duration)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-timer.C:
		return nil
	}
}
