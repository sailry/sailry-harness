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

type completedWork struct {
	job    protocol.JobID
	result error
}

func (runner Runner) Serve(ctx context.Context) {
	completed := make(chan completedWork, 1)
	var running sync.WaitGroup
	defer running.Wait()
	recovering := true
	var ending *completedWork
	for ctx.Err() == nil {
		if ending == nil {
			select {
			case result := <-completed:
				ending = &result
			default:
			}
		}
		var batch Batch
		var err error
		if ending != nil {
			// Keep the completed effect's owner until its end is durable. A
			// failed write uses the tick's existing retry wait below; only
			// Finished repeats, never Execute.
			err = runner.Finished(ctx, ending.job, ending.result)
			if err == nil {
				ending = nil
			}
		}
		if err == nil {
			batch, err = runner.Tick(ctx, recovering)
		}
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
				select {
				case completed <- completedWork{job: work.Job, result: result}:
				case <-ctx.Done():
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
		case result := <-completed:
			ending = &result
		case <-due:
		}
		if timer != nil {
			timer.Stop()
		}
	}
}
