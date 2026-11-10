package backup

import (
	"bytes"
	"context"
	"errors"
	"io"
	"net/http"
	"net/url"
	"os"
	"strings"

	"github.com/minio/minio-go/v7"
	"github.com/minio/minio-go/v7/pkg/credentials"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/studio-b12/gowebdav"
)

// scopedTransport binds every request to its backup operation and refuses
// redirects, so credentials are sent only to the configured endpoint.
type scopedTransport struct {
	ctx  context.Context
	base http.RoundTripper
}

func (transport scopedTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	response, err := transport.base.RoundTrip(request.WithContext(transport.ctx))
	if err != nil {
		return nil, err
	}
	if response.StatusCode >= 300 && response.StatusCode < 400 && response.StatusCode != http.StatusNotModified {
		_ = response.Body.Close()
		return nil, errors.New("backup storage redirect refused")
	}
	return response, nil
}

type webdavStorage struct {
	client *gowebdav.Client
	prefix string
}

func openWebDAV(endpoint, prefix, identity, secret string, transport http.RoundTripper) storage {
	client := gowebdav.NewClient(endpoint, identity, secret)
	client.SetTransport(transport)
	return webdavStorage{client: client, prefix: prefix}
}

func (target webdavStorage) path(name string) string {
	if target.prefix == "" {
		return name
	}
	return target.prefix + "/" + name
}

func (target webdavStorage) createDirectory(name string) error {
	return webdavFault(target.client.MkdirAll(target.path(name)+"/", 0))
}

func (target webdavStorage) write(path string, data []byte) error {
	return webdavFault(target.client.Write(target.path(path), data, 0))
}

func (target webdavStorage) list(visit func(string, bool) bool) error {
	items, err := target.client.ReadDir(target.path(""))
	if err != nil {
		// A destination nothing was exported to yet has no directory.
		if code(webdavFault(err)) == protocol.ErrorCodeNotFound {
			return nil
		}
		return webdavFault(err)
	}
	for _, item := range items {
		if !visit(item.Name(), !item.IsDir()) {
			return nil
		}
	}
	return nil
}

func (target webdavStorage) stat(path string) (uint64, bool, error) {
	info, err := target.client.Stat(target.path(path))
	if err != nil {
		return 0, false, webdavFault(err)
	}
	return uint64(info.Size()), !info.IsDir(), nil
}

func (target webdavStorage) read(path string) (io.ReadCloser, error) {
	reader, err := target.client.ReadStream(target.path(path))
	return reader, webdavFault(err)
}

func (target webdavStorage) remove(path string) error {
	return webdavFault(target.client.Remove(target.path(path)))
}

func (target webdavStorage) removeDirectory(name string) error {
	return webdavFault(target.client.Remove(target.path(name) + "/"))
}

func (webdavStorage) close() {}

func webdavFault(err error) error {
	if err == nil {
		return nil
	}
	var failure *os.PathError
	if errors.As(err, &failure) {
		var response gowebdav.StatusError
		if errors.As(failure.Err, &response) {
			return status(response.Status)
		}
	}
	return status(0)
}

type s3Storage struct {
	ctx    context.Context
	client *minio.Client
	bucket string
	prefix string
}

func openS3(ctx context.Context, destination protocol.BackupDestination, prefix, identity, secret, token string, transport http.RoundTripper) (storage, error) {
	endpoint, err := url.Parse(destination.Endpoint)
	if err != nil || strings.Trim(endpoint.Path, "/") != "" {
		return nil, invalid("S3 endpoint must be a host without a path")
	}
	// One attempt per request: a repeated write could hide an uncertain outcome.
	client, err := minio.New(endpoint.Host, &minio.Options{
		Creds: credentials.NewStaticV4(identity, secret, token), Secure: endpoint.Scheme == "https",
		Transport: transport, Region: destination.Region, BucketLookup: minio.BucketLookupPath, MaxRetries: 1,
	})
	if err != nil {
		return nil, status(0)
	}
	return s3Storage{ctx: ctx, client: client, bucket: destination.Bucket, prefix: prefix}, nil
}

func (target s3Storage) key(name string) string {
	if target.prefix == "" {
		return name
	}
	return target.prefix + "/" + name
}

func (target s3Storage) createDirectory(name string) error {
	return target.write(name+"/", nil)
}

func (target s3Storage) write(path string, data []byte) error {
	_, err := target.client.PutObject(target.ctx, target.bucket, target.key(path), bytes.NewReader(data), int64(len(data)), minio.PutObjectOptions{})
	return s3Fault(err)
}

func (target s3Storage) list(visit func(string, bool) bool) error {
	ctx, cancel := context.WithCancel(target.ctx)
	defer cancel()
	prefix := target.key("")
	for object := range target.client.ListObjects(ctx, target.bucket, minio.ListObjectsOptions{Prefix: prefix}) {
		if object.Err != nil {
			return s3Fault(object.Err)
		}
		name := strings.TrimPrefix(object.Key, prefix)
		if name == "" {
			continue
		}
		if !visit(strings.TrimSuffix(name, "/"), !strings.HasSuffix(name, "/")) {
			return nil
		}
	}
	return nil
}

func (target s3Storage) stat(path string) (uint64, bool, error) {
	info, err := target.client.StatObject(target.ctx, target.bucket, target.key(path), minio.StatObjectOptions{})
	if err != nil {
		return 0, false, s3Fault(err)
	}
	return uint64(info.Size), true, nil
}

func (target s3Storage) read(path string) (io.ReadCloser, error) {
	object, err := target.client.GetObject(target.ctx, target.bucket, target.key(path), minio.GetObjectOptions{})
	return object, s3Fault(err)
}

func (target s3Storage) remove(path string) error {
	return s3Fault(target.client.RemoveObject(target.ctx, target.bucket, target.key(path), minio.RemoveObjectOptions{}))
}

func (target s3Storage) removeDirectory(name string) error { return target.remove(name + "/") }

func (s3Storage) close() {}

func s3Fault(err error) error {
	if err == nil {
		return nil
	}
	return status(minio.ToErrorResponse(err).StatusCode)
}
