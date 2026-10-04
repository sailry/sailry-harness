import 'dart:convert';
import 'dart:typed_data';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/transfers.dart';
import 'package:sailry_bridge/api/transfers/upload.dart';
import '../conversation.dart' show check, execute;

const chunkSize = 64 * 1024;

Iterable<Uint8List> chunks(Uint8List bytes) sync* {
  for (var offset = 0; offset < bytes.length; offset += chunkSize) {
    final end = (offset + chunkSize).clamp(0, bytes.length);
    yield Uint8List.sublistView(bytes, offset, end);
  }
}

Future<String> revision(Uint8List bytes) async {
  final digest = await Digest.newInstance();
  try {
    for (final chunk in chunks(bytes)) {
      await digest.update(bytes: chunk);
    }
    return await digest.revision();
  } finally {
    digest.dispose();
  }
}

Future<Map<String, dynamic>> prepareUpload(
  Connection connection,
  String worktree,
  Uint8List bytes, {
  String name = '附件 🙂.bin',
  String mediaType = 'application/octet-stream',
}) async => (await execute(connection, 'upload_attachment', {
  'worktree': worktree,
  'name': name,
  'media_type': mediaType,
  'size': bytes.length,
  'revision': await revision(bytes),
}))['data'];

Future<void> write(Upload upload, Uint8List bytes) async {
  for (final chunk in chunks(bytes)) {
    await upload.write(bytes: chunk);
  }
  await upload.finish();
}

Future<Map<String, dynamic>> uploadAttachment(
  Connection connection,
  String worktree,
  Uint8List bytes, {
  required String name,
  required String mediaType,
}) async {
  final descriptor = await prepareUpload(
    connection,
    worktree,
    bytes,
    name: name,
    mediaType: mediaType,
  );
  final upload = await connection.uploadAttachment(
    upload: jsonEncode(descriptor),
  );
  try {
    await write(upload, bytes);
    return (await execute(connection, 'finish_attachment_upload', {
      'worktree': worktree,
      'stream': descriptor['stream'],
    }))['data'];
  } finally {
    await upload.close();
    upload.dispose();
  }
}

Future<Map<String, dynamic>> prepareDownload(
  Connection connection,
  Map<String, dynamic> attachment,
) async => (await execute(connection, 'download_attachment', {
  'worktree': attachment['spec']['worktree'],
  'attachment': attachment['id'],
}))['data'];

Future<Uint8List> downloadAttachment(
  Connection connection,
  Map<String, dynamic> descriptor,
) async {
  final download = await connection.downloadAttachment(
    download: jsonEncode(descriptor),
  );
  final bytes = BytesBuilder(copy: false);
  try {
    while (true) {
      final chunk = await download.next();
      if (chunk == null) break;
      check(chunk.isNotEmpty && chunk.length <= chunkSize, 'bounded FFI chunk');
      bytes.add(chunk);
    }
    check(await download.next() == null, 'verified EOF remains terminal');
    return bytes.takeBytes();
  } finally {
    await download.close();
    download.dispose();
  }
}

Future<void> fails(Future<dynamic> future, String reason) async {
  Object? failure;
  try {
    await future;
  } catch (error) {
    failure = error;
  }
  check(failure != null, reason);
}
