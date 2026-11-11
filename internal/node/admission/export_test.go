package admission

import "time"

// SetSettleRetry shortens the pause between result writes for a test.
func SetSettleRetry(interval time.Duration) (restore func()) {
	previous := settleRetry
	settleRetry = interval
	return func() { settleRetry = previous }
}
