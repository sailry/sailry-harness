import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute;
import 'terminal.dart' as terminal;

Future<void> main() async {
  final env = Platform.environment;
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  final path = env['SAILRY_CONTROLLER_PROFILE']!;
  var controller = await Controller.open(
    path: path,
    internet: false,
    relays: [],
  );
  final address = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  var connection = await controller.connect(address: address);
  await execute(connection, 'register_project', {
    'name': 'FFI CLI',
    'path': env['SAILRY_PROJECT_PATH']!,
  });
  final snapshotRequest = await connection.prepare(
    command: '{"kind":"snapshot"}',
  );
  final snapshot = jsonDecode(
    await connection.execute(request: snapshotRequest),
  )['Ok']['data'];
  final worktree = snapshot['worktrees'].single['id'];
  await execute(connection, 'save_terminal_settings', {
    'revision': 0,
    'shell': '',
    'environment': {'PATH': env['SAILRY_TOOL_PATH']!},
  });
  final tools =
      (await execute(connection, 'list_terminal_tools', {
            'worktree': worktree,
          }))['data']
          as List;
  check(
    tools.where((entry) => entry['available'] == true).single['tool'] ==
        'codex',
    'CLI discovery uses the execution Node',
  );
  final color = {'red': 200, 'green': 200, 'blue': 200};
  final request = await connection.prepare(
    command: jsonEncode({
      'kind': 'open_tool_terminal',
      'data': {
        'tool': 'codex',
        'launch': {
          'worktree': worktree,
          'viewport': {
            'columns': 80,
            'rows': 24,
            'pixel_width': 640,
            'pixel_height': 384,
          },
          'appearance': {
            'foreground': color,
            'background': color,
            'palette': List.filled(16, color),
            'color_scheme': 'dark',
          },
        },
      },
    }),
  );
  final opened = jsonDecode(
    await connection.execute(request: request),
  )['Ok']['data'];
  final repeated = jsonDecode(
    await connection.execute(request: request),
  )['Ok']['data'];
  check(
    opened['id'] == repeated['id'] && opened['tool'] == 'codex',
    'CLI launch retains its original receipt and tool',
  );
  final id = opened['id'] as String;
  var updates = await connection.watchTerminal(terminal: id);
  await terminal.until(
    updates,
    (snapshot) => terminal.content(snapshot).contains('FFI tool ready'),
  );
  await execute(connection, 'input_terminal', {
    'terminal': id,
    'revision': opened['revision'],
    'input': {
      'paste': {'text': 'mobile-input'},
    },
  });
  await execute(connection, 'input_terminal', {
    'terminal': id,
    'revision': opened['revision'],
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
  await terminal.until(
    updates,
    (snapshot) =>
        terminal.content(snapshot).contains('FFI tool result: mobile-input'),
  );
  await updates.close();
  updates.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  updates = await connection.watchTerminal(terminal: id);
  final recovered = await terminal.until(
    updates,
    (snapshot) =>
        terminal.content(snapshot).contains('FFI tool result: mobile-input'),
  );
  check(
    recovered['info']['tool'] == 'codex',
    'shared terminal projection restores tool identity',
  );
  await execute(connection, 'close_terminal', {
    'worktree': worktree,
    'terminal': id,
  });
  await updates.close();
  updates.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  print('Dart CLI terminal lifecycle passed');
}
