import 'dart:convert';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/terminal.dart';

Future<Map<String, dynamic>> execute(
  Connection connection,
  String kind,
  Map<String, dynamic> data,
) async {
  final request = await connection.prepare(
    command: jsonEncode({'kind': kind, 'data': data}),
  );
  final result = jsonDecode(await connection.execute(request: request));
  if (result['Ok'] == null)
    throw StateError('Terminal command failed: ${result['Err']}');
  return result['Ok'] as Map<String, dynamic>;
}

void check(bool condition, String message) {
  if (!condition) throw StateError(message);
}

Future<Map<String, dynamic>> until(
  TerminalUpdates updates,
  bool Function(Map<String, dynamic>) predicate,
) async {
  return Future(() async {
    while (true) {
      final view = jsonDecode(await updates.next()) as Map<String, dynamic>;
      if (view['error'] != null)
        throw StateError('Terminal subscription failed');
      final snapshot = view['snapshot'] as Map<String, dynamic>?;
      if (snapshot != null && predicate(snapshot)) return snapshot;
    }
  }).timeout(const Duration(seconds: 10));
}

String content(Map<String, dynamic> snapshot) {
  final screen = snapshot['screen'];
  return [
    ...screen['scrollback'],
    ...screen['rows'],
  ].expand((line) => line['spans'] as List).map((span) => span['text']).join();
}

Future<String> create(Connection connection, String worktree) async {
  final color = {'red': 240, 'green': 241, 'blue': 242};
  final created = await execute(connection, 'create_terminal', {
    'worktree': worktree,
    'viewport': {
      'columns': 80,
      'rows': 24,
      'pixel_width': 640,
      'pixel_height': 384,
    },
    'appearance': {
      'foreground': color,
      'background': {'red': 20, 'green': 21, 'blue': 22},
      'palette': List.filled(16, color),
      'color_scheme': 'dark',
    },
  });
  final info = created['data'];
  final id = info['id'] as String;
  final revision = info['revision'];
  final updates = await connection.watchTerminal(terminal: id);
  final first = await until(updates, (_) => true);
  check(first['screen']['columns'] == 80, 'terminal snapshot over FFI');
  await execute(connection, 'resize_terminal', {
    'terminal': id,
    'revision': revision,
    'viewport': {
      'columns': 100,
      'rows': 30,
      'pixel_width': 800,
      'pixel_height': 480,
    },
  });
  await until(
    updates,
    (snapshot) =>
        snapshot['screen']['columns'] == 100 &&
        snapshot['screen']['rows'].length == 30,
  );
  await execute(connection, 'input_terminal', {
    'terminal': id,
    'revision': revision,
    'input': {
      'paste': {
        'text':
            "printf '%s\\n' ffi-run >> ffi-terminal.txt; printf 'ffi-%s\\n' ready",
      },
    },
  });
  await execute(connection, 'input_terminal', {
    'terminal': id,
    'revision': revision,
    'input': {
      'key': {
        'event': {
          'key': 'enter',
          'action': 'press',
          'modifiers': {
            'shift': false,
            'control': false,
            'alt': false,
            'super_key': false,
            'caps_lock': false,
            'num_lock': false,
          },
          'utf8': null,
          'unshifted_codepoint': null,
        },
      },
    },
  });
  final ready = await until(
    updates,
    (snapshot) => content(snapshot).contains('ffi-ready'),
  );
  check(
    ready['sequence'] > first['sequence'],
    'terminal updates reduced by Rust Client',
  );
  // A buffered update may win the race; cancellation must still release a blocked next.
  final pending = updates.next().then((_) {}, onError: (_) {});
  await updates.close();
  await pending.timeout(const Duration(seconds: 2));
  var closed = false;
  try {
    await updates.next();
  } catch (_) {
    closed = true;
  }
  check(closed, 'closed terminal subscription rejects next');
  updates.dispose();
  return id;
}

Future<void> restore(Connection connection, String id, String worktree) async {
  final updates = await connection.watchTerminal(terminal: id);
  final snapshot = await until(
    updates,
    (snapshot) => content(snapshot).contains('ffi-ready'),
  );
  check(
    snapshot['info']['status']['kind'] == 'running',
    'controller disposal preserves PTY',
  );
  final file = await execute(connection, 'read_file', {
    'worktree': worktree,
    'path': 'ffi-terminal.txt',
  });
  check(
    file['data']['text'] == 'ffi-run\n',
    'reconnect never replays terminal input',
  );
  await execute(connection, 'close_terminal', {
    'worktree': worktree,
    'terminal': id,
  });
  await until(
    updates,
    (snapshot) => snapshot['info']['status']['kind'] == 'closed',
  );
  await updates.close();
  updates.dispose();
}
