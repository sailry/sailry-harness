package dispatch

import (
	"context"
	"sync"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Recheck bounds the wait for the next due time. A timer counts monotonic
// time, which stops while the system sleeps and ignores a wall-clock change,
// so the runner reads the wall clock again at least this often: an occurrence
// that became due during sleep runs within Recheck of the wake.
const Recheck = 30 * time.Second

type Work struct {
	Job        protocol.JobID
	Request    protocol.Request
	Completion protocol.DispatchCompletion
}

type Batch struct {
	Work    []Work
	Pending bool
	NextMs  *int64
}

type Runner struct {
	Clock    clockwork.Clock
	Wake     <-chan struct{}
	Tick     func(context.Context, bool) (Batch, error)
	Execute  func(context.Context, Work) error
	Finished func(context.Context, protocol.JobID, error) error
	Report   func(error)
}

func (runner Runner) Serve(ctx context.Context) {
	completed := make(chan struct{}, 1)
	var running sync.WaitGroup
	defer running.Wait()
	recovering := true
	for ctx.Err() == nil {
		batch, err := runner.Tick(ctx, recovering)
		if err != nil {
			if ctx.Err() != nil {
				return
			}
			if runner.Report != nil {
				runner.Report(err)
			}
			timer := runner.Clock.NewTimer(time.Second)
			select {
			case <-ctx.Done():
				timer.Stop()
				return
			case <-timer.Chan():
			}
			continue
		}
		recovering = false
		for _, work := range batch.Work {
			running.Add(1)
			go func() {
				defer running.Done()
				result := runner.Execute(ctx, work)
				if ctx.Err() != nil {
					return
				}
				if err := runner.Finished(ctx, work.Job, result); err != nil && runner.Report != nil {
					runner.Report(err)
				}
				select {
				case completed <- struct{}{}:
				default:
				}
			}()
		}
		if batch.Pending {
			continue
		}
		var timer clockwork.Timer
		var due <-chan time.Time
		if batch.NextMs != nil {
			delay := min(max(time.UnixMilli(*batch.NextMs).Sub(runner.Clock.Now()), time.Millisecond), Recheck)
			timer = runner.Clock.NewTimer(delay)
			due = timer.Chan()
		}
		select {
		case <-ctx.Done():
		case <-runner.Wake:
		case <-completed:
		case <-due:
		}
		if timer != nil {
			timer.Stop()
		}
	}
}
