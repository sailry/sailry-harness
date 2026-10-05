import 'dart:convert';
import 'dart:typed_data';

import 'package:file_picker/file_picker.dart';
import 'package:sailry_bridge/api/transfers.dart';

import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../content/transfers.dart';

const _chunkBytes = 64 * 1024;

typedef PickedAttachment = ({
  Map<String, dynamic> attachment,
  Uint8List? preview,
});

Future<PickedAttachment?> pickAttachment(
  HostConnection host,
  String worktree,
) async {
  final file = await FilePicker.pickFile();
  if (file == null) return null;
  final length = await file.length();
  if (length == null || length > maximumAttachmentBytes) {
    throw StateError('Attachment is unreadable or exceeds the upload limit');
  }
  final bytes = await file.readAsBytes();
  if (bytes.length > maximumAttachmentBytes) {
    throw StateError('Attachment exceeds the upload limit');
  }
  return (
    attachment: await uploadAttachment(host, worktree, file.name, bytes),
    preview: mediaType(file.name).startsWith('image/') ? bytes : null,
  );
}

Future<Map<String, dynamic>> uploadAttachment(
  HostConnection host,
  String worktree,
  String name,
  Uint8List bytes,
) async {
  final digest = await Digest.newInstance();
  String revision;
  try {
    for (var offset = 0; offset < bytes.length; offset += _chunkBytes) {
      await digest.update(
        bytes: Uint8List.sublistView(
          bytes,
          offset,
          (offset + _chunkBytes).clamp(0, bytes.length),
        ),
      );
    }
    revision = await digest.revision();
  } finally {
    digest.dispose();
  }
  final descriptor = object(
    (await host.command('upload_attachment', {
      'worktree': worktree,
      'name': name,
      'media_type': mediaType(name),
      'size': bytes.length,
      'revision': revision,
    }))['data'],
  );
  final upload = await host.connection.uploadAttachment(
    upload: jsonEncode(descriptor),
  );
  try {
    for (var offset = 0; offset < bytes.length; offset += _chunkBytes) {
      await upload.write(
        bytes: Uint8List.sublistView(
          bytes,
          offset,
          (offset + _chunkBytes).clamp(0, bytes.length),
        ),
      );
    }
    await upload.finish();
    return object(
      (await host.command('finish_attachment_upload', {
        'worktree': worktree,
        'stream': descriptor['stream'],
      }))['data'],
    );
  } finally {
    await upload.close();
    upload.dispose();
  }
}
