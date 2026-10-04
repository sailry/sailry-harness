import 'dart:convert';
import 'dart:typed_data';

import 'package:file_picker/file_picker.dart';
import 'package:sailry_bridge/api/transfers/download.dart';

import '../l10n/strings.dart';
import '../runtime/json.dart';
import '../runtime/session.dart';

const maximumAttachmentBytes = 64 * 1024 * 1024;

String mediaType(String name) => switch (name.split('.').last.toLowerCase()) {
  'png' => 'image/png',
  'jpg' || 'jpeg' => 'image/jpeg',
  'gif' => 'image/gif',
  'webp' => 'image/webp',
  'bmp' => 'image/bmp',
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
}) async {
  final spec = object(attachment['spec']);
  final length = spec['size'] as int;
  if (length > maximumAttachmentBytes) {
    throw StateError(tr('conversationAttachmentTooLarge'));
  }
  final response = image == null
      ? await host.command('download_attachment', {
          'worktree': spec['worktree'],
          'attachment': attachment['id'],
        })
      : await host.command('download_image', {
          'session': session,
          'image': image,
        });
  final download = await host.connection.downloadAttachment(
    download: jsonEncode(response['data']),
  );
  return _read(download);
}

Future<Uint8List> downloadFile(
  HostConnection host,
  String worktree,
  String path,
) async {
  final response = await host.command('download_file', {
    'worktree': worktree,
    'path': path,
  });
  final download = await host.connection.downloadFile(
    download: jsonEncode(response['data']),
  );
  return _read(download);
}

Future<Uint8List> _read(Download download) async {
  final bytes = BytesBuilder(copy: false);
  try {
    while (true) {
      final chunk = await download.next();
      if (chunk == null) break;
      if (bytes.length + chunk.length > maximumAttachmentBytes) {
        throw StateError(tr('conversationAttachmentTooLarge'));
      }
      bytes.add(chunk);
    }
    // Client verifies declared size and digest before returning the final EOF.
    return bytes.takeBytes();
  } finally {
    await download.close();
    download.dispose();
  }
}

Future<void> saveContent(Uint8List bytes, String name, String mime) async {
  await FilePicker.saveFile(fileName: name, bytes: bytes, mimeType: mime);
}
