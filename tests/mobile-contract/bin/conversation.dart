import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/conversation.dart';

void check(bool condition, String message) {
  if (!condition) throw StateError(message);
}

Future<Map<String, dynamic>> execute(
  Connection connection,
  String kind,
  Map<String, dynamic>? data,
) async {
  final request = await connection.prepare(
    command: jsonEncode({'kind': kind, if (data != null) 'data': data}),
  );
  final result = jsonDecode(await connection.execute(request: request));
  if (result['Ok'] == null)
    throw StateError('Conversation command failed: ${result['Err']}');
  return result['Ok'] as Map<String, dynamic>;
}

Future<Map<String, dynamic>> until(
  ConversationUpdates updates,
  bool Function(Map<String, dynamic>) ready,
) async {
  Map<String, dynamic>? last;
  return Future(() async {
    while (true) {
      final view = jsonDecode(await updates.next()) as Map<String, dynamic>;
      if (view['error'] != null)
        throw StateError('Conversation observer failed');
      final snapshot = view['snapshot'] as Map<String, dynamic>?;
      last = snapshot;
      if (snapshot != null && ready(snapshot)) return view;
    }
  }).timeout(
    const Duration(seconds: 10),
    onTimeout: () =>
        throw StateError('Conversation deadline: ${last?["page"]?["runs"]}'),
  );
}

class Fixture {
  final HttpServer server;
  final readTool = Platform.environment['SAILRY_READ_TOOL']!;
  final release = Completer<void>();
  int requests = 0;
  late String session;
  late String request;
  late String result;

  Fixture(this.server);

  Future<void> respond(HttpRequest request) async {
    final body = jsonDecode(await utf8.decoder.bind(request).join());
    check(
      body['model'] == 'ffi-fixture',
      'frozen model reaches execution Node',
    );
    requests++;
    final response = request.response;
    response.bufferOutput = false;
    response.headers.contentType = ContentType('text', 'event-stream');
    final results = (body['messages'] as List).where(
      (message) => message['role'] == 'tool',
    );
    if (results.isEmpty) {
      check(
        (body['tools'] as List).any(
          (tool) => tool['function']['name'] == readTool,
        ),
        'execution Node advertises read tool',
      );
      response.write(
        'data: ${jsonEncode({
          'id': 'ffi-tool',
          'object': 'chat.completion.chunk',
          'created': 1,
          'model': 'ffi-fixture',
          'choices': [
            {
              'index': 0,
              'delta': {
                'role': 'assistant',
                'tool_calls': [
                  {
                    'index': 0,
                    'id': 'ffi-call',
                    'type': 'function',
                    'function': {
                      'name': readTool,
                      'arguments': jsonEncode({'path': 'tool.txt'}),
                    },
                  },
                ],
              },
              'finish_reason': 'tool_calls',
            },
          ],
        })}\n\ndata: [DONE]\n\n',
      );
      await response.close();
      return;
    }
    check(
      jsonDecode(results.single['content'])['data']['text'] ==
          '中文 tool result 🙂',
      'tool result returns to actual model context',
    );
    final chunk = {
      'id': 'ffi-completion',
      'object': 'chat.completion.chunk',
      'created': 1,
      'model': 'ffi-fixture',
      'choices': [
        {
          'index': 0,
          'delta': {'role': 'assistant', 'content': 'FFI streaming answer'},
          'finish_reason': null,
        },
      ],
    };
    response.write('data: ${jsonEncode(chunk)}\n\n');
    await response.flush();
    await release.future;
    response.write(
      'data: ${jsonEncode({
        'id': 'ffi-completion',
        'object': 'chat.completion.chunk',
        'created': 1,
        'model': 'ffi-fixture',
        'choices': [
          {'index': 0, 'delta': {}, 'finish_reason': 'stop'},
        ],
        'usage': {'prompt_tokens': 8, 'completion_tokens': 4, 'total_tokens': 12},
      })}\n\ndata: [DONE]\n\n',
    );
    await response.close();
  }
}

Future<Fixture> create(
  Connection connection,
  String project,
  String worktree,
) async {
  final fixture = Fixture(
    await HttpServer.bind(InternetAddress.loopbackIPv4, 0),
  );
  fixture.server.listen((request) {
    unawaited(fixture.respond(request));
  });
  const provider = '00000000-0000-4000-8000-000000000071';
  await execute(connection, 'save_provider', {
    'provider': {
      'id': provider,
      'revision': 0,
      'name': 'FFI model fixture',
      'api': 'chat_completions',
      'authentication': 'api_key',
      'endpoint': 'http://127.0.0.1:${fixture.server.port}/v1',
      'enabled': true,
      'default_model': 'ffi-fixture',
      'models': [
        {
          'id': 'ffi-fixture',
          'context': 4096,
          'output': 128,
          'vision': false,
          'tools': true,
          'reasoning': false,
          'web_search': false,
          'generates': [],
          'efforts': [],
          'custom_efforts': false,
          'default_effort': 'default',
        },
      ],
    },
    'expected_revision': 0,
    'secret': null,
  });
  await execute(connection, 'write_file', {
    'worktree': worktree,
    'path': 'tool.txt',
    'text': '中文 tool result 🙂',
    'expected_revision': null,
  });
  final created = await execute(connection, 'create_session', {
    'project': project,
    'worktree': worktree,
    'config': {
      'provider': provider,
      'model': 'ffi-fixture',
      'effort': 'low',
      'mode': 'code',
      'permission': 'ask',
      'credential': null,
    },
  });
  fixture.session = created['data']['id'] as String;
  final updates = await connection.watchConversation(session: fixture.session);
  final empty = await until(updates, (_) => true);
  final emptyStatistics = empty['snapshot']['statistics'];
  check(
    emptyStatistics['turns'] == 0 &&
        emptyStatistics['responses'] == 0 &&
        emptyStatistics['usage'] == null,
    'empty conversation has unknown usage',
  );
  fixture.request = await connection.prepare(
    command: jsonEncode({
      'kind': 'submit_turn',
      'data': {
        'session': fixture.session,
        'expected_revision': 1,
        'message': {'text': 'FFI conversation fixture', 'attachments': []},
      },
    }),
  );
  fixture.result = await connection.execute(request: fixture.request);
  check(
    jsonDecode(fixture.result)['Ok']['kind'] == 'queued_turn',
    'durable turn admission over FFI',
  );
  final partial = await until(
    updates,
    (snapshot) => (snapshot['drafts'] as List).isNotEmpty,
  );
  check(
    partial['snapshot']['drafts'][0]['parts'][0]['data'] ==
        'FFI streaming answer',
    'shared transient projection over FFI',
  );
  check(
    (partial['snapshot']['page']['entries'] as List).every(
      (entry) => (entry['parts'] as List).every(
        (part) =>
            part['kind'] != 'text' || part['data'] != 'FFI streaming answer',
      ),
    ),
    'partial output is not committed history',
  );
  fixture.release.complete();
  final complete = await until(
    updates,
    (snapshot) =>
        (snapshot['page']['runs'] as List).last['status'] == 'completed',
  );
  check(
    (complete['snapshot']['drafts'] as List).isEmpty,
    'canonical commit replaces temporary output',
  );
  check(
    (complete['snapshot']['page']['entries'] as List)
            .last['parts'][0]['data'] ==
        'FFI streaming answer',
    'canonical answer over FFI',
  );
  check(
    (complete['calls'] as List).length == 1,
    'shared call association over FFI',
  );
  checkStatistics(complete['snapshot']);
  final call = complete['calls'][0];
  check(
    call['state'] == 'returned' &&
        call['id'] == 'ffi-call' &&
        call['name'] == fixture.readTool,
    'shared tool state over FFI',
  );
  final response = call['response'];
  final entry = (complete['snapshot']['page']['entries'] as List).singleWhere(
    (entry) => entry['id'] == response['entry'],
  );
  check(
    entry['parts'][response['index']]['data']['result']['data']['text'] ==
        '中文 tool result 🙂',
    'reference resolves to canonical tool output',
  );
  final pending = updates.next().then((_) => true, onError: (_) => true);
  await updates.close();
  await pending.timeout(const Duration(seconds: 2));
  var closed = false;
  try {
    await updates.next();
  } catch (_) {
    closed = true;
  }
  check(closed, 'closed conversation subscription rejects reads');
  updates.dispose();
  return fixture;
}

Future<void> restore(Connection connection, Fixture fixture) async {
  final updates = await connection.watchConversation(session: fixture.session);
  final snapshot = await until(updates, (_) => true);
  check(
    (snapshot['snapshot']['page']['runs'] as List).last['status'] ==
        'completed',
    'reconnected conversation restores canonical state',
  );
  check(
    await connection.execute(request: fixture.request) == fixture.result,
    'reopened controller deduplicates sent turn',
  );
  check(
    snapshot['calls'][0]['state'] == 'returned',
    'reopened FFI restores shared tool association',
  );
  check(fixture.requests == 2, 'recovery does not call model or tool again');
  checkStatistics(snapshot['snapshot']);
  await updates.close();
  updates.dispose();
  await fixture.server.close(force: true);
}

void checkStatistics(Map<String, dynamic> snapshot) {
  final statistics = snapshot['statistics'];
  final usage = statistics['usage'];
  check(
    statistics['turns'] == 1 && statistics['responses'] == 1,
    'tool calls and missing usage are not extra reported responses',
  );
  check(
    usage['input'] == 8 &&
        usage['output'] == 4 &&
        usage['cached_input'] == 0 &&
        usage['reasoning'] == 0,
    'shared conversation totals cross FFI and reconnect',
  );
}
