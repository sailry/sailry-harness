import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute;
import 'terminal.dart' as terminal;
import 'ssh_transfer.dart' as transfer;

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
  var profile = (await execute(connection, 'save_ssh', {
    'profile': {
      'id': env['SAILRY_SSH_ID'],
      'revision': 0,
      'name': 'FFI SSH',
      'host': '127.0.0.1',
      'port': int.parse(env['SAILRY_SSH_PORT']!),
      'username': 'fixture',
      'authentication': 'password',
      'host_key': null,
    },
    'expected_revision': 0,
    'credential': {'kind': 'password', 'password': 'isolated-ssh-password'},
  }))['data'];
  check(
    !jsonEncode(profile).contains('isolated-ssh-password'),
    'saved profile excludes credentials',
  );
  final probe = (await execute(connection, 'check_ssh', {
    'profile': profile['id'],
    'expected_revision': profile['revision'],
  }))['data'];
  check(
    probe['kind'] == 'host_key_required' && probe['changed'] == false,
    'host key requires confirmation',
  );
  profile = (await execute(connection, 'trust_ssh', {
    'profile': profile['id'],
    'expected_revision': profile['revision'],
    'key': probe['key'],
  }))['data'];
  final updates = await connection.watch();
  final view = jsonDecode(
    await updates.next().timeout(const Duration(seconds: 10)),
  );
  check(
    view['snapshot']['ssh'].single['host_key'] != null,
    'shared Client projects trusted profiles',
  );
  check(
    !jsonEncode(view).contains('isolated-ssh-password'),
    'snapshot excludes credentials',
  );
  await updates.close();
  updates.dispose();
  final request = await connection.prepare(
    command: jsonEncode({
      'kind': 'run_ssh',
      'data': {
        'profile': profile['id'],
        'expected_revision': profile['revision'],
        'command': "printf x >> effects; printf 'FFI SSH result'",
        'timeout_ms': 5000,
      },
    }),
  );
  final result = await connection.execute(request: request);
  final output = jsonDecode(result)['Ok']['data'];
  check(
    output['kind'] == 'completed' && output['stdout'] == 'FFI SSH result',
    'execution Node returns command output',
  );
  await execute(connection, 'register_project', {
    'name': 'SSH terminal project',
    'path': env['SAILRY_PROJECT_PATH'],
  });
  final snapshot = (await execute(connection, 'snapshot', null))['data'];
  final worktree = snapshot['worktrees'].single['id'];
  final upload = await transfer.exercise(connection, profile, worktree);
  final color = {'red': 240, 'green': 241, 'blue': 242};
  final opened = (await execute(connection, 'open_ssh_terminal', {
    'profile': profile['id'],
    'expected_revision': profile['revision'],
    'launch': {
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
    },
  }))['data'];
  check(
    opened['kind'] == 'terminal' &&
        opened['ssh'] == profile['id'] &&
        opened['worktree'] == null,
    'SSH terminal belongs to its profile independently of local worktrees',
  );
  final id = opened['id'];
  final terminalUpdates = await connection.watchTerminal(terminal: id);
  await terminal.until(terminalUpdates, (_) => true);
  await execute(connection, 'input_terminal', {
    'terminal': id,
    'revision': opened['revision'],
    'input': {
      'paste': {
        'text': "printf x >> terminal-effects; printf '%s%s\\n' FFI _TERMINAL",
      },
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
    terminalUpdates,
    (snapshot) => terminal.content(snapshot).contains('FFI_TERMINAL'),
  );
  await terminalUpdates.close();
  terminalUpdates.dispose();
  await controller.close();
  connection.dispose();
  controller.dispose();
  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  check(
    await connection.execute(request: upload.request) == upload.result,
    'reconnect does not repeat SSH upload',
  );
  final recovered = await connection.watchTerminal(terminal: id);
  await terminal.until(
    recovered,
    (snapshot) => terminal.content(snapshot).contains('FFI_TERMINAL'),
  );
  await execute(connection, 'resize_terminal', {
    'terminal': id,
    'revision': opened['revision'],
    'viewport': {
      'columns': 100,
      'rows': 30,
      'pixel_width': 800,
      'pixel_height': 480,
    },
  });
  await terminal.until(
    recovered,
    (snapshot) => snapshot['screen']['columns'] == 100,
  );
  await execute(connection, 'close_ssh_terminal', {'terminal': id});
  await terminal.until(
    recovered,
    (snapshot) => snapshot['info']['status']['kind'] == 'closed',
  );
  await recovered.close();
  recovered.dispose();
  check(
    await connection.execute(request: request) == result,
    'reconnect does not repeat SSH command',
  );
  check(
    (await execute(connection, 'list_ssh', null))['data'].single['id'] ==
        profile['id'],
    'profile stays on execution Node',
  );
  await execute(connection, 'remove_ssh', {
    'profile': profile['id'],
    'expected_revision': profile['revision'],
  });
  check(
    (await execute(connection, 'list_ssh', null))['data'].isEmpty,
    'profile removed through shared command',
  );
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  print('Dart FFI SSH ownership and recovery passed');
}
