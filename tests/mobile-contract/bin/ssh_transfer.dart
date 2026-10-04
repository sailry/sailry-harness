import 'dart:convert';
import 'dart:io';
import 'package:sailry_bridge/api/connection.dart';
import 'conversation.dart' show check;

Future<({String request, String result})> exercise(
  Connection connection,
  Map<String, dynamic> profile,
  String worktree,
) async {
  Future<String> prepare(String direction, String path) => connection.prepare(
    command: jsonEncode({
      'kind': 'transfer_ssh',
      'data': {
        'profile': profile['id'],
        'expected_revision': profile['revision'],
        'transfer': {
          'worktree': worktree,
          'path': path,
          'remote_path': 'remote.bin',
          'direction': direction,
        },
        'timeout_ms': 5000,
      },
    }),
  );
  final upload = await prepare('upload', 'upload.bin');
  final uploaded = await connection.execute(request: upload);
  final outcome = jsonDecode(uploaded)['Ok']['data'];
  check(
    outcome['kind'] == 'transferred' && outcome['bytes'] == 128 * 1024,
    'SSH upload returns acknowledged byte count',
  );
  await File(
    '${Platform.environment['SAILRY_PROJECT_PATH']}/upload.bin',
  ).writeAsString('changed after upload');
  check(
    await connection.execute(request: upload) == uploaded,
    'same request does not send the modified source',
  );
  final download = await prepare('download', 'download.bin');
  check(
    await connection.execute(request: download) == uploaded,
    'SSH download preserves bytes',
  );
  check(
    await connection.execute(request: download) == uploaded,
    'completed download can be observed again',
  );
  final conflict = jsonDecode(
    await connection.execute(
      request: await prepare('download', 'download.bin'),
    ),
  );
  check(
    conflict['Err']['code'] == 'revision_conflict',
    'a new download does not overwrite its destination',
  );
  return (request: upload, result: uploaded);
}
