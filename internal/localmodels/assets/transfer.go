package assets

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"time"

	"github.com/shirou/gopsutil/v4/disk"
)

// readTimeout bounds each wait for response headers or body bytes.
const readTimeout = 60 * time.Second

// Client applies the acquisition redirect policy to the caller's transport,
// which carries the Node's proxy settings.
func Client(transport http.RoundTripper) *http.Client {
	return &http.Client{Transport: transport, CheckRedirect: func(request *http.Request, via []*http.Request) error {
		if request.URL.Scheme != "https" {
			return errors.New("redirect leaves HTTPS")
		}
		if len(via) >= 10 {
			return errors.New("stopped after 10 redirects")
		}
		return nil
	}}
}

// Prepare downloads the missing files of one model directory. Complete files
// are kept, partial files resume from their checkpoint, and every published
// file matches its pinned size and SHA-256.
func Prepare(ctx context.Context, client *http.Client, directory string, downloads []Download, notify func(Progress)) error {
	if ctx.Err() != nil {
		return ErrCancelled
	}
	files := make([]File, 0, len(downloads))
	for _, download := range downloads {
		address := download.URL
		if address.Scheme != "https" || address.Hostname() == "" || address.User != nil || address.Fragment != "" {
			return ErrInvalidAsset
		}
		files = append(files, download.File)
	}
	total, err := validate(files)
	if err != nil {
		return err
	}
	root, err := open(directory, true)
	if err != nil {
		return err
	}
	defer root.Close()
	started := time.Now()
	var completed, transferred uint64
	phase := Inspecting
	report := func(done uint64) { notify(progress(phase, done, total, transferred, time.Since(started))) }
	report(completed)
	for _, download := range downloads {
		specification := download.File
		if ctx.Err() != nil {
			return ErrCancelled
		}
		release, err := lock(ctx, directory, specification.Name)
		if err != nil {
			return err
		}
		err = func() error {
			defer release()
			if err := target(root, specification.Name); err != nil {
				return err
			}
			phase = Validating
			report(completed)
			cached := valid(ctx, root, specification)
			if ctx.Err() != nil {
				return ErrCancelled
			}
			if cached {
				completed += specification.Size
				report(completed)
				return nil
			}
			partial, err := openPart(ctx, root, download)
			if err != nil {
				return err
			}
			if ctx.Err() != nil {
				_ = partial.file.Close()
				return ErrCancelled
			}
			if !partial.complete(specification) {
				phase = Downloading
				report(completed + partial.received)
				err := room(directory, specification.Name, specification.Size-partial.received)
				if err == nil {
					err = receive(ctx, client, download, partial, func(received, bytes uint64) {
						transferred += bytes
						report(completed + received)
					})
				}
				if syncErr := partial.file.Sync(); syncErr != nil {
					_ = partial.file.Close()
					return storage(syncErr)
				}
				if err != nil {
					if errors.Is(err, ErrInvalidAsset) || partial.received == 0 {
						if discardErr := partial.discard(); discardErr != nil {
							return discardErr
						}
					} else {
						saveErr := partial.checkpoint()
						_ = partial.file.Close()
						if saveErr != nil {
							return saveErr
						}
					}
					return err
				}
				phase = Validating
				report(completed + partial.received)
				if !partial.complete(specification) {
					if err := partial.discard(); err != nil {
						return err
					}
					return ErrInvalidAsset
				}
				if ctx.Err() != nil {
					saveErr := partial.checkpoint()
					_ = partial.file.Close()
					if saveErr != nil {
						return saveErr
					}
					return ErrCancelled
				}
			}
			phase = Installing
			report(completed + partial.received)
			if err := partial.publish(specification.Name); err != nil {
				return err
			}
			completed += specification.Size
			report(completed)
			return nil
		}()
		if err != nil {
			return err
		}
	}
	if ctx.Err() != nil {
		return ErrCancelled
	}
	// Asset completion does not prove the consumer's native model validation.
	notify(Progress{Phase: phase, Percent: 100})
	return nil
}

// room refuses, before any request, a file whose remaining bytes the volume of
// the model directory cannot hold. A volume that does not report its free space
// refuses nothing: the write itself reports a full one.
func room(directory, name string, remaining uint64) error {
	usage, err := disk.Usage(directory)
	if err != nil || usage.Free >= remaining {
		return nil
	}
	return storageMessage(fmt.Sprintf("not enough free space for %s: %d bytes needed, %d free", name, remaining, usage.Free))
}

// receive appends the response body to the partial file. A rejected range
// restarts once from the beginning.
func receive(ctx context.Context, client *http.Client, item Download, partial *part, notify func(received, bytes uint64)) error {
	size := item.File.Size
	for attempt := range 2 {
		scoped, cancel := context.WithCancel(ctx)
		idle := time.AfterFunc(readTimeout, cancel)
		interrupted := func(err error) error {
			if ctx.Err() != nil {
				return ErrCancelled
			}
			if scoped.Err() != nil {
				return fmt.Errorf("%w: response timed out", ErrDownload)
			}
			return download(err)
		}
		request, err := http.NewRequestWithContext(scoped, http.MethodGet, item.URL.String(), nil)
		if err != nil {
			idle.Stop()
			cancel()
			return ErrInvalidAsset
		}
		request.Header.Set("User-Agent", "Sailry/0.1.0")
		request.Header.Set("Accept-Encoding", "identity")
		validator, resuming := partial.resume()
		if resuming {
			request.Header.Set("Range", fmt.Sprintf("bytes=%d-", partial.received))
			request.Header.Set("If-Range", validator)
		}
		response, err := client.Do(request)
		if err != nil {
			idle.Stop()
			failure := interrupted(err)
			cancel()
			return failure
		}
		retry, err := func() (bool, error) {
			defer cancel()
			defer idle.Stop()
			defer response.Body.Close()
			switch {
			case response.StatusCode == http.StatusPartialContent && resuming && ranged(response, partial.received, size) && partial.validator.matches(response):
			case response.StatusCode == http.StatusOK:
				if err := partial.reset(); err != nil {
					return false, err
				}
				if response.ContentLength >= 0 && uint64(response.ContentLength) != size {
					return false, ErrInvalidAsset
				}
			case response.StatusCode == http.StatusPartialContent || response.StatusCode == http.StatusRequestedRangeNotSatisfiable:
				if attempt != 0 || !resuming {
					return false, ErrInvalidAsset
				}
				if response.StatusCode == http.StatusRequestedRangeNotSatisfiable && response.Header.Get("Content-Range") != fmt.Sprintf("bytes */%d", size) {
					return false, ErrInvalidAsset
				}
				return true, partial.reset()
			default:
				return false, fmt.Errorf("%w: HTTP %d for %s", ErrDownload, response.StatusCode, item.File.Name)
			}
			if encoding := response.Header.Get("Content-Encoding"); encoding != "" && encoding != "identity" {
				return false, ErrInvalidAsset
			}
			partial.headers(response)
			if err := partial.checkpoint(); err != nil {
				return false, err
			}
			notify(partial.received, 0)
			saved := partial.received
			buffer := make([]byte, 64*1024)
			for {
				count, err := response.Body.Read(buffer)
				if count > 0 {
					idle.Reset(readTimeout)
					received := partial.received + uint64(count)
					if received > size {
						return false, ErrInvalidAsset
					}
					// Finish each bounded body write before recording its prefix hash.
					if _, err := partial.file.Write(buffer[:count]); err != nil {
						return false, storage(err)
					}
					partial.received = received
					partial.hash.Write(buffer[:count])
					if received-saved >= 8*1024*1024 {
						if err := partial.checkpoint(); err != nil {
							return false, err
						}
						saved = received
					}
					notify(received, uint64(count))
				}
				if err == io.EOF {
					return false, nil
				}
				if err != nil {
					return false, interrupted(err)
				}
			}
		}()
		if err != nil || !retry {
			return err
		}
	}
	return ErrInvalidAsset
}
