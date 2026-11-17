import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:file_picker/file_picker.dart';
import 'package:sailry_bridge/api/transfers/download.dart';

import '../runtime/json.dart';
import '../runtime/session.dart';

const maximumAttachmentBytes = 64 * 1024 * 1024;

String mediaType(String name) => switch (name.split('.').last.toLowerCase()) {
  'png' => 'image/png',
  'jpg' || 'jpeg' => 'image/jpeg',
  'gif' => 'image/gif',
  'webp' => 'image/webp',
  'bmp' => 'image/bmp',
  'svg' => 'image/svg+xml',
  'pdf' => 'application/pdf',
  'json' => 'application/json',
  'md' || 'markdown' => 'text/markdown',
  'txt' ||
  'rs' ||
  'dart' ||
  'js' ||
  'ts' ||
  'css' ||
  'html' ||
  'py' ||
  'yaml' ||
  'yml' ||
  'toml' ||
  'tsx' ||
  'jsx' ||
  'vue' ||
  'svelte' ||
  'sh' ||
  'sql' ||
  'xml' ||
  'csv' ||
  'log' ||
  'ini' ||
  'conf' ||
  'gitignore' ||
  'lock' ||
  'c' ||
  'h' ||
  'cpp' ||
  'go' ||
  'java' ||
  'kt' ||
  'swift' => 'text/plain',
  _ => 'application/octet-stream',
};

Future<Uint8List> downloadAttachment(
  HostConnection host,
  Map<String, dynamic> attachment, {
  Map<String, dynamic>? image,
  String? session,
  DownloadCancellation? cancellation,
}) async {
  cancellation?.check();
  final spec = object(attachment['spec']);
  _checkDownloadSize(spec['size'], maximumAttachmentBytes);
  final response = image == null
      ? await host.command('download_attachment', {
          'worktree': spec['worktree'],
          'attachment': attachment['id'],
        })
      : await host.command('download_image', {
          'session': session,
          'image': image,
        });
  final data = object(response['data']);
  return _downloadReceipt(
    host,
    data,
    () => host.connection.downloadAttachment(download: jsonEncode(data)),
    cancellation: cancellation,
    validate: () {
      final received = object(data['attachment']);
      final receivedSpec = object(received['spec']);
      if (received['id'] != attachment['id'] ||
          receivedSpec['worktree'] != spec['worktree'] ||
          receivedSpec['revision'] != spec['revision'] ||
          receivedSpec['size'] != spec['size']) {
        throw const FormatException(
          'Attachment response does not match its source',
        );
      }
      _checkDownloadSize(receivedSpec['size'], maximumAttachmentBytes);
    },
  );
}

Future<Uint8List> downloadFile(
  HostConnection host,
  String worktree,
  String path, {
  DownloadCancellation? cancellation,
}) async => (await downloadFileContent(
  host,
  worktree,
  path,
  cancellation: cancellation,
)).bytes;

class DownloadedFile {
  const DownloadedFile(this.bytes, this.revision, this.stamp);
  final Uint8List bytes;
  final String revision;
  final String stamp;
}

Future<DownloadedFile> downloadFileContent(
  HostConnection host,
  String worktree,
  String path, {
  DownloadCancellation? cancellation,
  int maximumBytes = maximumAttachmentBytes,
}) async {
  cancellation?.check();
  final response = await host.command('download_file', {
    'worktree': worktree,
    'path': path,
  });
  final data = object(response['data']);
  final bytes = await _downloadReceipt(
    host,
    data,
    () => host.connection.downloadFile(download: jsonEncode(data)),
    cancellation: cancellation,
    maximumBytes: maximumBytes,
    validate: () {
      if (data['worktree'] != worktree || data['path'] != path) {
        throw const FormatException('File response does not match its source');
      }
      _checkDownloadSize(data['size'], maximumBytes);
    },
  );
  return DownloadedFile(bytes, text(data['revision']), text(data['stamp']));
}

void _checkDownloadSize(Object? value, int maximumBytes) {
  if (value is! int ||
      value < 0 ||
      value > maximumBytes ||
      value > maximumAttachmentBytes) {
    throw StateError('Attachment exceeds the download limit');
  }
}

/// A cancelled or rejected receipt still owns a Node stream until it is closed.
/// Once Client.Download starts, closing its native handle cancels that stream.
Future<Uint8List> _downloadReceipt(
  HostConnection host,
  Map<String, dynamic> data,
  Future<Download> Function() open, {
  required void Function() validate,
  DownloadCancellation? cancellation,
  int maximumBytes = maximumAttachmentBytes,
}) async {
  final stream = text(data['stream']);
  Future<void>? aborting;
  Future<void> abort() => aborting ??= () async {
    if (stream.isEmpty) return;
    try {
      await host.command('cancel_file_transfer', {'stream': stream});
    } catch (_) {
      // An unavailable source cannot be cleaned up through a different host.
    }
  }();
  var opened = false;
  cancellation?._closing.add(abort);
  try {
    cancellation?.check();
    validate();
    final download = await open();
    opened = true;
    cancellation?._closing.remove(abort);
    return await _read(
      download,
      cancellation: cancellation,
      maximumBytes: maximumBytes,
    );
  } catch (_) {
    if (!opened) await abort();
    rethrow;
  } finally {
    cancellation?._closing.remove(abort);
  }
}

/// A document releases its streams when it closes; late command results never
/// start another read. Commands still use their original request identity.
class DownloadCancellation {
  bool _cancelled = false;
  final _closing = <Future<void> Function()>{};

  void check() {
    if (_cancelled) throw StateError('Download cancelled');
  }

  void cancel() {
    if (_cancelled) return;
    _cancelled = true;
    for (final close in List.of(_closing)) {
      unawaited(close().catchError((Object _) {}));
    }
    _closing.clear();
  }
}

Future<Uint8List> _read(
  Download download, {
  DownloadCancellation? cancellation,
  int maximumBytes = maximumAttachmentBytes,
}) async {
  final bytes = BytesBuilder(copy: false);
  Future<void>? closing;
  Future<void> close() =>
      closing ??= download.close().whenComplete(download.dispose);
  cancellation?._closing.add(close);
  try {
    cancellation?.check();
    while (true) {
      final chunk = await download.next();
      cancellation?.check();
      if (chunk == null) break;
      if (bytes.length + chunk.length > maximumBytes) {
        throw StateError('Attachment exceeds the download limit');
      }
      bytes.add(chunk);
    }
    // Client verifies declared size and digest before returning the final EOF.
    return bytes.takeBytes();
  } finally {
    cancellation?._closing.remove(close);
    await close();
  }
}

Future<void> saveContent(Uint8List bytes, String name, String mime) async {
  await FilePicker.saveFile(fileName: name, bytes: bytes, mimeType: mime);
}
