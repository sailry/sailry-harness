package application

import (
	"context"
	"errors"
	"log"
)

func completeSystemNotification(ctx context.Context, id string, err error, undelivered func(string)) {
	if err == nil || ctx.Err() != nil {
		return
	}
	if !errors.Is(err, errors.ErrUnsupported) {
		log.Printf("desktop notification: %v", err)
	}
	undelivered(id)
}
