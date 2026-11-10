package core

import "context"

// PrepareUploadTarget applies the preparation deadline while capturing the
// destination, then transfers lifetime ownership to the durable publication.
func (files *Files) PrepareUploadTarget(ctx context.Context, path string, expected *string, limit uint64) (*FileTarget, error) {
	target, err := files.PrepareFileTarget(ctx, path, expected, limit)
	if err != nil {
		return nil, err
	}
	target.control.context = context.Background()
	return target, nil
}
