import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/terminal/session.dart';
import 'package:sailry_mobile/runtime/session.dart';

final controller = List.filled(32, 1);
final other = List.filled(32, 2);
const launch = {
  'viewport': {'columns': 80, 'rows': 24},
  'appearance': {'color_scheme': 'dark'},
};

Map<String, dynamic> info({
  String id = 'terminal-1',
  Object? owner,
  String access = 'unrestricted',
  String status = 'running',
  int revision = 3,
}) => {
  'id': id,
  'worktree': 'worktree-1',
  'access': access,
  'owner': owner ?? controller,
  'revision': revision,
  'status': {'kind': status},
};

class Fixture {
  final sent = <(String, Map<String, dynamic>?)>[];
  final recovered = <String>[];
  Map<String, dynamic> result = info();
  bool uncertain = false;

  TerminalSession page({
    String? terminal,
    bool createNew = false,
    String? sharedEntry,
  }) => TerminalSession(
    controller: sharedEntry == null ? jsonEncode(controller) : null,
    sharedEntry: sharedEntry,
    worktree: 'worktree-1',
    terminal: terminal,
    createNew: createNew,
    command: (kind, data) async {
      sent.add((kind, data));
      if (uncertain) {
        throw CommandFailure('outcome_unknown', request: 'original');
      }
      return {'data': result};
    },
    resolve: (request) async {
      recovered.add(request);
      return {'data': result};
    },
  );
}

void main() {
  test('manual creation requests the complete environment', () async {
    final fixture = Fixture();
    final page = fixture.page();
    final opened = await page.open(launch, []);
    expect(fixture.sent.single.$1, 'create_terminal');
    expect(fixture.sent.single.$2, {
      ...launch,
      'access': 'unrestricted',
      'worktree': 'worktree-1',
    });
    expect(page.terminal, 'terminal-1');
    expect(page.lease(opened), 3);
  });

  test('resource entry skips sandbox and specialized terminals', () async {
    final fixture = Fixture();
    await fixture.page().open(launch, [
      info(id: 'sandbox', access: 'workspace'),
      {...info(id: 'tool'), 'tool': 'codex'},
      {...info(id: 'ssh'), 'ssh': 'connection-1'},
      info(id: 'closed', status: 'closed'),
      info(id: 'stopped', status: 'stopped'),
      info(id: 'exited', status: 'exited'),
      info(id: 'failed', status: 'failed'),
      {...info(id: 'elsewhere'), 'worktree': 'worktree-2'},
    ]);
    expect(fixture.sent.single.$1, 'create_terminal');
    expect(fixture.sent.single.$2?['access'], 'unrestricted');
  });

  test('returning restores the existing controller lease', () async {
    final fixture = Fixture();
    final first = fixture.page();
    await first.open(launch, []);
    final returning = fixture.page();
    final opened = await returning.open(launch, [info()]);
    expect(fixture.sent.map((request) => request.$1), [
      'create_terminal',
      'open_terminal',
    ]);
    expect(fixture.sent.last.$2, {'terminal': first.terminal, ...launch});
    expect(returning.terminal, first.terminal);
    expect(returning.lease(opened), 3);
  });

  test('opening respects another controller taking over', () async {
    final fixture = Fixture()..result = info(owner: other, revision: 4);
    final page = fixture.page();
    final opened = await page.open(launch, [info()]);
    expect(page.lease(opened), isNull);
    expect(fixture.sent.single.$1, 'open_terminal');
    expect(opened['owner'], other);
  });

  test(
    'a routed entry lease is writable only after this page claims it',
    () async {
      final entry = List.filled(32, '02').join();
      final fixture = Fixture()..result = info(owner: other);
      final page = fixture.page(sharedEntry: entry);
      final opened = await page.open(launch, [fixture.result]);
      expect(page.lease(opened), isNull);
      expect(fixture.sent.single.$1, 'open_terminal');

      fixture.result = info(owner: other, revision: 4);
      await page.claim(3);
      expect(fixture.sent.last.$1, 'claim_terminal');
      expect(fixture.sent.last.$2, {
        'terminal': 'terminal-1',
        'expected_revision': 3,
      });
      expect(page.lease(opened), isNull);
      expect(page.lease(fixture.result), 4);
      page.observe(info(owner: controller, revision: 2));
      expect(page.lease(fixture.result), 4);
      expect(page.lease(info(owner: controller)), isNull);
      final changed = info(owner: other, revision: 5);
      page.observe(changed);
      expect(page.lease(changed), isNull);
      expect(page.revision, isNull);

      final returning = fixture.page(sharedEntry: entry);
      expect(returning.lease(await returning.open(launch, [opened])), isNull);
    },
  );

  test('a lost takeover reply resolves the original lease', () async {
    final fixture = Fixture()..result = info(owner: other);
    final page = fixture.page(sharedEntry: List.filled(32, '02').join());
    await page.open(launch, [fixture.result]);
    fixture.uncertain = true;
    await expectLater(page.claim(3), throwsA(isA<CommandFailure>()));
    fixture.result = info(owner: other, revision: 4);
    final newer = info(owner: other, revision: 5);
    await page.claim(5);
    expect(page.lease(newer), isNull);
    expect(
      fixture.sent.where((call) => call.$1 == 'claim_terminal'),
      hasLength(1),
    );
    expect(fixture.recovered, ['original']);
    page.observe(newer);
    expect(page.revision, isNull);
  });

  test('a routed page can use its own reconciled creation lease', () async {
    final fixture = Fixture()
      ..result = info(owner: other)
      ..uncertain = true;
    final page = fixture.page(sharedEntry: List.filled(32, '02').join());
    await expectLater(page.open(launch, []), throwsA(isA<CommandFailure>()));
    expect(page.lease(fixture.result), isNull);
    final opened = await page.open(launch, [fixture.result]);
    expect(page.lease(opened), 3);
    expect(fixture.sent.single.$1, 'create_terminal');
    expect(fixture.recovered, ['original']);
  });

  test('stopped terminals require a new launch identity', () async {
    final fixture = Fixture()..result = info(revision: 5);
    final page = fixture.page();
    final opened = await page.open(launch, [info(status: 'stopped')]);
    expect(fixture.sent.single.$1, 'create_terminal');
    expect(page.lease(opened), 5);
    expect(page.lease(info(status: 'exited')), isNull);
  });

  test('uncertain creation recovers the original request', () async {
    final fixture = Fixture()..uncertain = true;
    final page = fixture.page();
    await expectLater(page.open(launch, []), throwsA(isA<CommandFailure>()));
    final opened = await page.open(launch, [info()]);
    expect(fixture.sent.single.$1, 'create_terminal');
    expect(fixture.recovered, ['original']);
    expect(page.lease(opened), 3);
  });

  test('explicit terminal links preserve their access', () async {
    final fixture = Fixture()..result = info(access: 'workspace');
    final page = fixture.page(terminal: 'terminal-1');
    final opened = await page.open(launch, []);
    expect(fixture.sent.single.$1, 'open_terminal');
    expect(fixture.sent.single.$2, {'terminal': 'terminal-1', ...launch});
    expect(opened['access'], 'workspace');
  });

  test('explicit creation bypasses reusable terminals', () async {
    final fixture = Fixture();
    await fixture.page(createNew: true).open(launch, [info()]);
    expect(fixture.sent.single.$1, 'create_terminal');
  });
}
