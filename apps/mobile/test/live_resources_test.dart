import 'dart:async';
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/ports.dart';
import 'package:sailry_mobile/features/resources/file_draft.dart';
import 'package:sailry_mobile/runtime/session.dart';

class ForwardFixture implements Forwarding {
  final _values = StreamController<String>();
  late final _iterator = StreamIterator(_values.stream);
  bool closed = false;
  bool released = false;
  void stop() => _values.add(jsonEncode({'kind': 'closed'}));
  @override
  Future<int> localPort() async => 43123;
  @override
  Future<String> next() async {
    if (await _iterator.moveNext()) return _iterator.current;
    throw StateError('closed');
  }

  @override
  Future<void> close() async {
    if (closed) return;
    closed = true;
    unawaited(_values.close());
  }

  @override
  void dispose() => released = true;
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

class ConnectionFixture implements Connection {
  final requests = <String>[];
  Map<String, dynamic>? source;
  @override
  Future<Forwarding> forwardService({
    required String source,
    required int localPort,
  }) async {
    this.source = jsonDecode(source) as Map<String, dynamic>;
    return forwardPort(
      remotePort: (this.source!['service'] as Map)['port'] as int,
      localPort: localPort,
    );
  }

  final forward = ForwardFixture();
  (int, int)? forwarded;
  int forwardCalls = 0;
  Completer<Forwarding>? pendingForward;
  @override
  Future<String> execute({required String request}) async {
    requests.add(request);
    return jsonEncode({
      'Ok': {
        'kind': 'file_written',
        'data': {'revision': 'r2'},
      },
    });
  }

  @override
  Future<Forwarding> forwardPort({
    required int remotePort,
    required int localPort,
  }) async {
    forwardCalls++;
    forwarded = (remotePort, localPort);
    return pendingForward == null ? forward : await pendingForward!.future;
  }

  @override
  Future<void> close() async {}
  @override
  void dispose() {}
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  test('uncertain saves preserve the request and text', () async {
    final connection = ConnectionFixture();
    final commands = <Map<String, dynamic>>[];
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      command: (kind, data) async {
        commands.add(data!);
        throw const CommandFailure(
          'outcome_unknown',
          request: 'stable-save-request',
        );
      },
    );
    final draft = FileDraft({'text': 'original\r\n', 'revision': 'r1'});
    draft.text = '中文 😀\r\n';
    await expectLater(
      draft.save(host, 'tree', 'note.md'),
      throwsA(isA<CommandFailure>()),
    );
    expect(draft.dirty, isTrue);
    expect(draft.revision, 'r1');
    expect(
      () => draft.reload({'text': 'other', 'revision': 'r3'}),
      throwsStateError,
    );
    await draft.save(host, 'tree', 'note.md');
    expect(commands.single, {
      'worktree': 'tree',
      'path': 'note.md',
      'text': '中文 😀\r\n',
      'expected_revision': 'r1',
    });
    expect(connection.requests, ['stable-save-request']);
    expect(draft.text, '中文 😀\r\n');
    expect(draft.saved, draft.text);
    expect(draft.revision, 'r2');
    expect(draft.pending, isNull);
    expect(draft.dirty, isFalse);
  });
  test('partial previews cannot overwrite files', () async {
    var writes = 0;
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (_, _) async {
        writes++;
        return {};
      },
    );
    final draft = FileDraft({
      'text': 'preview',
      'revision': 'r1',
      'truncated': true,
    });
    draft.text = 'replacement';
    await expectLater(draft.save(host, 'tree', 'large.txt'), throwsStateError);
    expect(writes, 0);
  });

  test('host mappings survive consumers and close with their host', () async {
    final connection = ConnectionFixture();
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      command: (_, _) async => {},
    );
    final mapping = await host.ports.open(3000);
    expect(connection.forwarded, (3000, 0));
    expect(mapping.uri.toString(), 'http://127.0.0.1:43123');
    expect(await host.ports.open(3000), same(mapping));
    expect(connection.forwardCalls, 1);
    expect(connection.forward.closed, isFalse);
    await host.close();
    host.dispose();
    expect(connection.forward.closed, isTrue);
    expect(connection.forward.released, isTrue);
  });

  test('pending mappings deduplicate and release with their host', () async {
    final connection = ConnectionFixture()
      ..pendingForward = Completer<Forwarding>();
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      command: (_, _) async => {},
    );
    final opening = host.ports.open(3000);
    final repeated = host.ports.open(3000);
    expect(repeated, same(opening));
    await Future<void>.value();
    expect(connection.forwardCalls, 1);
    final rejected = expectLater(opening, throwsStateError);
    final closing = host.close();
    connection.pendingForward!.complete(connection.forward);
    await Future.wait([rejected, closing]);
    expect(connection.forward.closed, isTrue);
    expect(connection.forward.released, isTrue);
    expect(host.ports.entries, isEmpty);
    host.dispose();
  });

  test('service mappings preserve capture and URL components', () async {
    final connection = ConnectionFixture();
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      command: (_, _) async => {},
    );
    final source = {
      'session': 'session-a',
      'command': 'command-a',
      'service': {'port': 3000, 'url': 'http://127.0.0.1:3000/app?q=1'},
    };
    final mapping = await host.ports.open(3000, source: source);
    expect(connection.source, source);
    expect(mapping.uri.toString(), 'http://127.0.0.1:43123/app?q=1');
    expect(await host.ports.open(3000, source: source), same(mapping));
    expect(connection.forwardCalls, 1);
    final stopped = Completer<void>();
    mapping.listening.addListener(() {
      if (!mapping.listening.value && !stopped.isCompleted) stopped.complete();
    });
    connection.forward.stop();
    await stopped.future;
    expect(mapping.listening.value, isFalse);
    await host.ports.remove(3000);
    expect(connection.forward.closed, isTrue);
    expect(connection.forward.released, isTrue);
    expect(host.ports.entries, isEmpty);
    await host.close();
    host.dispose();
  });
}
