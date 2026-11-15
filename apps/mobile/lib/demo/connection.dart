import 'dart:async';
import 'dart:convert';

import 'package:sailry_bridge/api/commands.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/conversation.dart';
import 'package:sailry_bridge/api/terminal.dart';
import 'package:sailry_bridge/api/status.dart';
import 'package:sailry_bridge/api/usage.dart';

import '../l10n/strings.dart';
import '../runtime/json.dart';

/// An explicitly local fixture source for the review host. It implements the
/// presentation side of the Dart adapter without opening an FFI handle, Link,
/// Node, file, process or model. Unimplemented operations fail locally.
class DemoConnection implements Connection {
  DemoConnection({required this.translate}) {
    _files['README.md'] = translate('demoReadme');
    _files['src/styles/login.css'] =
        '.login-form {\n  display: flex;\n  gap: 20px;\n}\n\n'
        '.login-button {\n  min-height: 44px;\n}\n';
    _files['src/pages/Login.tsx'] =
        'export function Login() {\n'
        '  return <form className="login-form">\n'
        '    <input type="email" aria-label="Email" />\n'
        '    <input type="password" aria-label="Password" />\n'
        '    <button className="login-button">Sign in</button>\n'
        '  </form>;\n}\n';
    _files['docs/review.md'] = translate('demoReviewNotes');
    final session = _newSession(
      id: 'demo-login',
      project: 'demo-lumen',
      worktree: 'demo-lumen-main',
      title: translate('demoConversationTitle'),
    );
    _appendTurn(session, translate('demoUserMessage'), translate('demoReply'));
    final notes = _newSession(
      id: 'demo-notes',
      project: 'demo-notes',
      worktree: 'demo-notes-main',
      title: translate('demoNotesTitle'),
    );
    _appendTurn(
      notes,
      translate('demoNotesPrompt'),
      translate('demoReviewNotes'),
    );
    _terminalText = '${translate('demoTerminalWelcome')}\r\n';
  }

  static const pairingCode = '888888';
  static const hostId = 'local-demo';
  static const controllerId = 'demo-controller';
  static const _home = '/Users/alex';
  static const _projectPath = '$_home/Projects/lumen-web';
  final Translator translate;
  final _feeds = <_DemoUpdates>{};
  final _sessions = <Map<String, dynamic>>[];
  final _pages = <String, Map<String, dynamic>>{};
  final _files = <String, String>{};
  final _fileRevisions = <String, int>{};
  final _outcomes = <String, String>{};
  final _timers = <Timer>{};
  int _sequence = 0;
  int _request = 0;
  bool _closed = false;
  bool _staged = false;
  bool _committed = false;
  String _branch = 'feature/sign-in';
  String _terminalText = '';
  final _terminalInput = <int>[];
  final _terminalHistory = <String>[];
  int _terminalCursor = 0;
  int _terminalHistoryIndex = 0;
  String _terminalDraft = '';
  String _terminalDirectory = _projectPath;
  bool _terminalCR = false;
  final _terminalViewport = <String, int>{'columns': 80, 'rows': 24};
  Map<String, dynamic> _terminalAppearance = {};
  final int _terminalRevision = 1;

  Map<String, dynamic> get _config => {
    'mode': 'code',
    'permission': 'ask',
    'provider': 'openai',
    'model': 'gpt-6.1-sol',
    'effort': 'default',
    'options': <String, dynamic>{},
  };

  Map<String, dynamic> get _provider => {
    'id': 'openai',
    'name': 'OpenAI',
    'enabled': true,
    'revision': 1,
    'default_model': 'gpt-6.1-sol',
    'models': [
      {'id': 'gpt-6.1-sol', 'name': 'GPT-6.1 Sol', 'default_effort': 'default'},
    ],
  };

  Map<String, dynamic> get info => {
    'name': translate('demoHost'),
    'os': 'macOS',
    'os_version': '26.1',
    'architecture': 'arm64',
    'logical_cpus': 8,
    'host_uptime_secs': 86400,
    'node_uptime_secs': 7200,
    'memory': {
      'total_bytes': 17179869184,
      'free_bytes': 8589934592,
      'available_bytes': 10737418240,
    },
  };

  Map<String, dynamic> get _terminal => {
    'id': 'demo-terminal',
    'worktree': 'demo-lumen-main',
    'title': 'zsh',
    'access': 'unrestricted',
    'status': {'kind': 'running'},
    'owner': controllerId,
    'revision': _terminalRevision,
  };

  Map<String, dynamic> get snapshot => {
    'projects': [
      {
        'id': 'demo-lumen',
        'name': 'lumen-web',
        'path': _projectPath,
        'icon': 'code',
        'color': 'violet',
      },
      {
        'id': 'demo-notes',
        'name': 'growth-notes',
        'path': '$_home/Projects/growth-notes',
        'icon': 'file',
        'color': 'amber',
      },
    ],
    'worktrees': [
      {'id': 'demo-lumen-main', 'project': 'demo-lumen', 'path': _projectPath},
      {
        'id': 'demo-notes-main',
        'project': 'demo-notes',
        'path': '$_home/Projects/growth-notes',
      },
      {
        'id': 'demo-workspace',
        'project': null,
        'path': '$_home/Library/Application Support/Sailry/workspaces/chats',
      },
    ],
    'sessions': _sessions,
    'providers': [_provider],
    'terminals': [_terminal],
    'roles': <dynamic>[],
    'teams': <dynamic>[],
    'defaults': {'revision': 1, 'config': _config},
  };

  Map<String, dynamic> _newSession({
    required String id,
    String? project,
    String worktree = 'demo-workspace',
    String? title,
    Map<String, dynamic>? config,
  }) {
    final session = <String, dynamic>{
      'id': id,
      'project': project,
      'worktree': worktree,
      'revision': 1,
      'archived': false,
      'config': config ?? _config,
      'activity': {'title': title ?? translate('demoConversationTitle')},
    };
    _sessions.add(session);
    _pages[id] = {
      'revision': 1,
      'runs': <Map<String, dynamic>>[],
      'entries': <Map<String, dynamic>>[],
    };
    return session;
  }

  Map<String, dynamic> _session(Object? id) => _sessions.firstWhere(
    (session) => session['id'] == id,
    orElse: () => throw StateError('Unknown demo conversation'),
  );

  Map<String, dynamic> _appendTurn(
    Map<String, dynamic> session,
    String input,
    String reply, {
    bool running = false,
  }) {
    final turn = 'demo-turn-${++_sequence}';
    final now = DateTime.now().millisecondsSinceEpoch;
    final run = <String, dynamic>{
      'id': turn,
      'turn': turn,
      'kind': 'task',
      'revision': 1,
      'session': session['id'],
      'worktree': session['worktree'],
      'status': running ? 'running' : 'completed',
      'started_ms': now - (running ? 0 : 17000),
      if (!running) 'finished_ms': now,
      'message': {'text': input, 'attachments': <dynamic>[]},
    };
    final page = _pages[session['id']]!;
    (page['runs'] as List<Map<String, dynamic>>).add(run);
    (page['entries'] as List<Map<String, dynamic>>).addAll([
      {
        'id': '$turn-user',
        'turn': turn,
        'author': 'user',
        'sequence': ++_sequence,
        'parts': [
          {'kind': 'text', 'data': input},
        ],
      },
      {
        'id': '$turn-answer',
        'turn': turn,
        'author': 'assistant',
        'sequence': ++_sequence,
        'parts': [
          {'kind': 'text', 'data': reply},
        ],
      },
    ]);
    page['revision'] = number(page['revision']).toInt() + 1;
    session['revision'] = number(session['revision']).toInt() + 1;
    session['activity'] = {...object(session['activity']), 'run': run};
    return run;
  }

  Map<String, dynamic> _conversation(String id) {
    final page = _pages[id];
    if (page == null) throw StateError('Unknown demo conversation');
    final runs = objects(page['runs']);
    final entries = objects(page['entries']);
    return {
      'connected': true,
      'snapshot': {'page': page, 'drafts': <dynamic>[]},
      'timeline': runs.map((run) => run['turn']).toList(),
      'segments': {
        for (final run in runs)
          run['turn'] as String: [
            {
              'inputs': [
                {
                  'turn': run['turn'],
                  'message': run['message'],
                  'canonical': true,
                  'attachments': <dynamic>[],
                },
              ],
              'rows': [
                for (final entry in entries.where(
                  (entry) =>
                      entry['turn'] == run['turn'] &&
                      entry['author'] == 'assistant',
                ))
                  for (final (index, part) in objects(entry['parts']).indexed)
                    {
                      'source': {'entry': entry['id'], 'index': index},
                      'part': part,
                      'work': false,
                      'text_offset': 0,
                      'text_last': true,
                    },
              ],
            },
          ],
      },
      'calls': <dynamic>[],
      'statistics': {'turns': runs.length, 'responses': runs.length},
    };
  }

  _DemoUpdates _watch(Map<String, dynamic> Function() value) {
    if (_closed) throw StateError('demo connection is closed');
    final feed = _DemoUpdates(value, (feed) => _feeds.remove(feed));
    _feeds.add(feed);
    feed.publish();
    return feed;
  }

  void _publish() {
    if (_closed) return;
    for (final feed in _feeds.toList()) {
      feed.publish();
    }
  }

  @override
  Future<Updates> watch() async => _watch(
    () => {
      'connected': true,
      'connection': {'route': 'p2p', 'rtt_ms': 3},
      'snapshot': snapshot,
    },
  );

  @override
  Future<ConversationUpdates> watchConversation({
    required String session,
  }) async => _watch(() => _conversation(session));

  @override
  Future<CommandUpdates> watchCommands({required String session}) async =>
      _watch(
        () => {
          'connected': true,
          'snapshot': {'commands': <dynamic>[]},
        },
      );

  @override
  Future<TerminalUpdates> watchTerminal({required String terminal}) async =>
      _watch(
        () => {
          'connected': true,
          'reset': true,
          'snapshot': {
            'info': _terminal,
            'screen': {
              ..._terminalAppearance,
              'vt': {
                'viewport': _terminalViewport,
                'bytes': utf8.encode(_terminalScreen),
              },
            },
          },
        },
      );

  @override
  Future<UsageUpdates> watchUsage({required String query}) async => _watch(
    () => {
      'connected': true,
      'report': {
        'points': <dynamic>[],
        'groups': <dynamic>[],
        'totals': {'responses': 0},
      },
      'summary': {
        'points': <dynamic>[],
        'groups': <dynamic>[],
        'totals': {'responses': 0},
      },
      'complete': true,
    },
  );

  @override
  Future<String> searchConversation({
    required String session,
    required String query,
  }) async {
    final search = text(object(jsonDecode(query))['text']).toLowerCase();
    final matches = <Map<String, dynamic>>[];
    for (final entry in objects(_pages[session]?['entries'])) {
      final snippet = objects(entry['parts'])
          .where((part) => part['kind'] == 'text')
          .map((part) => text(part['data']))
          .join('');
      final index = snippet.toLowerCase().indexOf(search);
      if (index >= 0) {
        matches.add({
          'sequence': entry['sequence'],
          'turn': entry['turn'],
          'author': entry['author'],
          'snippet': snippet,
          'highlight': {
            'start': utf8.encode(snippet.substring(0, index)).length,
            'end': utf8
                .encode(snippet.substring(0, index + search.length))
                .length,
          },
        });
      }
    }
    return jsonEncode({
      'Ok': {
        'session': session,
        'revision': _pages[session]?['revision'],
        'matches': matches,
      },
    });
  }

  @override
  Future<StatusUpdates> watchStatus({required String scope}) async =>
      _watch(() => {'connected': true, 'entries': <dynamic>[]});

  @override
  Future<String> prepare({required String command}) async {
    if (_closed) throw StateError('demo connection is closed');
    return jsonEncode({
      'demo_request': ++_request,
      'command': jsonDecode(command),
    });
  }

  @override
  Future<String> execute({required String request}) async {
    if (_closed) throw StateError('demo connection is closed');
    final previous = _outcomes[request];
    if (previous != null) return previous;
    final command = object(object(jsonDecode(request))['command']);
    try {
      final result = await _command(
        text(command['kind']),
        object(command['data']),
      );
      return _outcomes[request] = jsonEncode({
        'Ok': {'kind': 'demo', 'data': result},
      });
    } catch (_) {
      return _outcomes[request] = jsonEncode({
        'Err': {
          'code': 'invalid_request',
          'message': translate('demoUnavailable'),
        },
      });
    }
  }

  @override
  Future<String> resolve({required String request}) async =>
      _outcomes[request] ??
      jsonEncode({
        'Err': {'code': 'not_found', 'message': translate('demoUnavailable')},
        'released': true,
      });

  Future<Object?> _command(String kind, Map<String, dynamic> data) async {
    switch (kind) {
      case 'inspect_host':
        return info;
      case 'read_host_metrics':
        return {
          'cpu_basis_points': 1250,
          'core_basis_points': [900, 1300, 1500, 700, 1600, 1100, 1400, 1500],
          'memory': info['memory'],
          'load': {'one': 1.24, 'five': 1.08, 'fifteen': .95},
          'network': {'received_bytes_per_sec': 0, 'sent_bytes_per_sec': 0},
          'process_count': 2,
          'processes': [
            {
              'pid': 101,
              'name': 'sailry-host',
              'cpu_basis_points': 120,
              'memory_bytes': 134217728,
            },
            {
              'pid': 102,
              'name': 'lumen-web',
              'cpu_basis_points': 320,
              'memory_bytes': 268435456,
            },
          ],
          'disks': <dynamic>[],
        };
      case 'read_conversation':
        return {'page': _pages[data['session']], 'missing': <dynamic>[]};
      case 'create_session':
        final session = _newSession(
          id: 'demo-session-${++_sequence}',
          project: data['project'] as String?,
          worktree: text(data['worktree'], 'demo-workspace'),
          config: object(data['config']),
        );
        _publish();
        return session;
      case 'submit_turn':
        final session = _session(data['session']);
        final run = _appendTurn(
          session,
          text(object(data['message'])['text']),
          '',
          running: true,
        );
        _publish();
        final answer = translate('demoSimulatedReply');
        var length = 0;
        late final Timer timer;
        timer = Timer.periodic(const Duration(milliseconds: 45), (_) {
          if (_closed || run['status'] != 'running') {
            timer.cancel();
            _timers.remove(timer);
            return;
          }
          length = (length + 8).clamp(0, answer.length);
          final entry =
              (_pages[session['id']]!['entries'] as List<Map<String, dynamic>>)
                  .firstWhere(
                    (entry) => entry['id'] == '${run['turn']}-answer',
                  );
          entry['parts'] = [
            {'kind': 'text', 'data': answer.substring(0, length)},
          ];
          if (length == answer.length) {
            run['status'] = 'completed';
            run['finished_ms'] = DateTime.now().millisecondsSinceEpoch;
            session['revision'] = number(session['revision']).toInt() + 1;
            timer.cancel();
            _timers.remove(timer);
          }
          _publish();
        });
        _timers.add(timer);
        return run;
      case 'stop_turn':
        for (final page in _pages.values) {
          for (final run in page['runs'] as List<Map<String, dynamic>>) {
            if (run['turn'] == data['turn']) {
              run['status'] = 'stopped';
              run['finished_ms'] = DateTime.now().millisecondsSinceEpoch;
            }
          }
        }
        _publish();
        return <String, dynamic>{};
      case 'set_session_config':
      case 'set_session_archived':
      case 'set_session_read':
        final session = _session(data['session']);
        if (kind == 'set_session_config') session['config'] = data['config'];
        if (kind == 'set_session_archived') {
          session['archived'] = data['archived'];
        }
        session['revision'] = number(session['revision']).toInt() + 1;
        _publish();
        return session;
      case 'search_files':
        final query = text(object(data['options'])['query']).toLowerCase();
        return {
          'matches': [
            for (final file in _files.entries)
              for (final (index, line) in file.value.split('\n').indexed)
                if (line.toLowerCase().contains(query))
                  {'path': file.key, 'line_number': index + 1, 'line': line},
          ],
          'truncated': false,
        };
      case 'list_directory':
        final directory = text(data['path']);
        final prefix = directory.isEmpty ? '' : '$directory/';
        final names = <String>{};
        return {
          'path': directory,
          'entries': [
            for (final file in _files.entries)
              if (file.key.startsWith(prefix))
                if (names.add(
                  file.key.substring(prefix.length).split('/').first,
                ))
                  {
                    'name': file.key.substring(prefix.length).split('/').first,
                    'path':
                        '$prefix${file.key.substring(prefix.length).split('/').first}',
                    'kind': file.key.substring(prefix.length).contains('/')
                        ? 'directory'
                        : 'file',
                    'size': utf8.encode(file.value).length,
                    'modified_ms':
                        DateTime.now().millisecondsSinceEpoch - 3600000,
                  },
          ],
          'truncated': false,
        };
      case 'read_file':
        final path = text(data['path']);
        if (!_files.containsKey(path)) throw StateError('Unknown demo file');
        return {
          'text': _files[path],
          'revision': '${_fileRevisions[path] ?? 1}',
          'truncated': false,
        };
      case 'write_file':
        final path = text(data['path']);
        if (!_files.containsKey(path) ||
            data['expected_revision'] != '${_fileRevisions[path] ?? 1}') {
          throw StateError('Invalid demo file revision');
        }
        _files[path] = text(data['text']);
        _fileRevisions[path] = (_fileRevisions[path] ?? 1) + 1;
        return {'revision': '${_fileRevisions[path]}'};
      case 'inspect_git':
        return {
          'kind': 'repository',
          'branch': _branch,
          'head': 'a12b34c56d78e90f',
          'index_revision': 'demo-1',
          'entries': [
            if (!_committed)
              {
                'path': 'src/styles/login.css',
                if (_staged) 'staged': 'modified' else 'unstaged': 'modified',
                'diff': {'additions': 2, 'deletions': 2},
                '${_staged ? 'staged' : 'unstaged'}_diff': {
                  'additions': 2,
                  'deletions': 2,
                },
              },
          ],
        };
      case 'list_git_branches':
        return {
          'current': _branch,
          'entries': [
            {'name': _branch, 'current': true, 'remote': false},
            {'name': 'main', 'current': _branch == 'main', 'remote': false},
          ],
          'sync': {
            'upstream': 'origin/$_branch',
            'ahead': _committed ? 2 : 1,
            'behind': 0,
          },
        };
      case 'read_git_diff':
        return {
          'path': 'src/styles/login.css',
          'text':
              '@@ -1,4 +1,4 @@\n .login-form {\n-  gap: 12px;\n+  gap: 20px;\n }\n-  height: 36px;\n+  min-height: 44px;\n',
          'additions': 2,
          'deletions': 2,
          'format': 'unified',
        };
      case 'read_git_log':
        return {
          'entries': [
            {
              'id': 'a12b34c56d78e90f',
              'message': translate('demoConversationTitle'),
              'author': 'Alex Rivera',
              'timestamp': DateTime.now().millisecondsSinceEpoch ~/ 1000 - 3600,
              'references': ['heads/$_branch'],
            },
          ],
        };
      case 'update_git_index':
        _staged = data['operation'] == 'stage';
        return <String, dynamic>{};
      case 'create_git_commit':
        _committed = true;
        return <String, dynamic>{};
      case 'switch_git_branch':
        _branch = text(data['name'], 'main');
        return <String, dynamic>{};
      case 'list_providers':
        return [_provider];
      case 'read_provider':
        return _provider;
      case 'list_channel_presets':
      case 'read_model_catalog':
        return <dynamic>[];
      case 'open_terminal':
      case 'create_terminal':
        _resizeTerminal(data['viewport']);
        _terminalAppearance = object(data['appearance']);
        return _terminal;
      case 'claim_terminal':
        return _terminal;
      case 'resize_terminal':
        _resizeTerminal(data['viewport']);
        _publish();
        return <String, dynamic>{};
      case 'set_terminal_appearance':
        _terminalAppearance = object(data['appearance']);
        return <String, dynamic>{};
      case 'input_terminal':
        for (final input in objects(data['inputs'])) {
          _terminalInsert(text(object(input['text'])['text']));
          _terminalInsert(text(object(input['paste'])['text']));
          _terminalKey(object(object(input['key'])['event']));
        }
        _publish();
        return <String, dynamic>{};
      default:
        throw UnsupportedError(
          'Operation is unavailable in the local demonstration',
        );
    }
  }

  void _resizeTerminal(Object? value) {
    final viewport = object(value);
    if (viewport.isEmpty) return;
    _terminalViewport['columns'] = number(
      viewport['columns'],
      _terminalViewport['columns']!,
    ).toInt().clamp(1, 500);
    _terminalViewport['rows'] = number(
      viewport['rows'],
      _terminalViewport['rows']!,
    ).toInt().clamp(1, 200);
  }

  String get _terminalPrompt {
    final directory = _terminalDirectory == _home
        ? '~'
        : _terminalDirectory == '/'
        ? '/'
        : _terminalDirectory.split('/').last;
    return 'alex@MacBook-Pro $directory % ';
  }

  String get _terminalScreen {
    final input = String.fromCharCodes(_terminalInput);
    final left = _terminalInput.length - _terminalCursor;
    return '\x1b[2J\x1b[H$_terminalText$_terminalPrompt$input'
        '${left > 0 ? '\x1b[${left}D' : ''}';
  }

  void _terminalInsert(String value) {
    for (final codepoint in value.runes) {
      if (codepoint == 10 && _terminalCR) {
        _terminalCR = false;
        continue;
      }
      _terminalCR = codepoint == 13;
      if (codepoint == 10 || codepoint == 13) {
        _terminalSubmit();
      } else if (codepoint == 3) {
        _terminalCancel();
      } else if (codepoint == 8 || codepoint == 127) {
        _terminalBackspace();
      } else if (codepoint >= 32) {
        _terminalInput.insert(_terminalCursor++, codepoint);
      }
    }
  }

  void _terminalBackspace() {
    if (_terminalCursor > 0) {
      _terminalInput.removeAt(--_terminalCursor);
    }
  }

  void _terminalCancel() {
    _terminalText +=
        '$_terminalPrompt${String.fromCharCodes(_terminalInput)}^C\r\n';
    _terminalInput.clear();
    _terminalCursor = 0;
  }

  void _terminalHistoryMove(int direction) {
    if (_terminalHistory.isEmpty) return;
    if (_terminalHistoryIndex == _terminalHistory.length) {
      _terminalDraft = String.fromCharCodes(_terminalInput);
    }
    _terminalHistoryIndex = (_terminalHistoryIndex + direction).clamp(
      0,
      _terminalHistory.length,
    );
    final value = _terminalHistoryIndex == _terminalHistory.length
        ? _terminalDraft
        : _terminalHistory[_terminalHistoryIndex];
    _terminalInput
      ..clear()
      ..addAll(value.runes);
    _terminalCursor = _terminalInput.length;
  }

  void _terminalKey(Map<String, dynamic> event) {
    if (event['action'] == 'release') return;
    final key = event['key'];
    final character = text(event['utf8']);
    if (object(event['modifiers'])['control'] == true) {
      switch (character.toLowerCase()) {
        case 'c':
          _terminalCancel();
        case 'l':
          _terminalText = '';
        case 'u':
          _terminalInput.removeRange(0, _terminalCursor);
          _terminalCursor = 0;
        case 'a':
          _terminalCursor = 0;
        case 'e':
          _terminalCursor = _terminalInput.length;
      }
      return;
    }
    switch (key) {
      case 'enter':
        _terminalSubmit();
      case 'backspace':
        _terminalBackspace();
      case 'delete':
        if (_terminalCursor < _terminalInput.length) {
          _terminalInput.removeAt(_terminalCursor);
        }
      case 'arrow_left':
        _terminalCursor = (_terminalCursor - 1).clamp(0, _terminalInput.length);
      case 'arrow_right':
        _terminalCursor = (_terminalCursor + 1).clamp(0, _terminalInput.length);
      case 'home':
        _terminalCursor = 0;
      case 'end':
        _terminalCursor = _terminalInput.length;
      case 'arrow_up':
        _terminalHistoryMove(-1);
      case 'arrow_down':
        _terminalHistoryMove(1);
      default:
        final codepoint = object(object(key)['character'])['codepoint'];
        if (character.isNotEmpty) {
          _terminalInsert(character);
        } else if (codepoint is int) {
          _terminalInsert(String.fromCharCode(codepoint));
        }
    }
  }

  void _terminalSubmit() {
    final line = String.fromCharCodes(_terminalInput);
    _terminalText += '$_terminalPrompt$line\r\n';
    _terminalInput.clear();
    _terminalCursor = 0;
    if (line.trim().isNotEmpty) _terminalHistory.add(line);
    _terminalHistoryIndex = _terminalHistory.length;
    _terminalDraft = '';
    final result = _terminalCommand(line.trim());
    if (result.isNotEmpty) {
      _terminalText +=
          '${result.replaceAll('\r\n', '\n').replaceAll('\n', '\r\n')}\r\n';
    }
  }

  String _terminalPath(String input) {
    var value = input;
    if (value == '~' || value.startsWith('~/')) {
      value = '$_home${value.substring(1)}';
    }
    final parts = <String>[];
    for (final part
        in (value.startsWith('/') ? value : '$_terminalDirectory/$value').split(
          '/',
        )) {
      if (part.isEmpty || part == '.') continue;
      if (part == '..') {
        if (parts.isNotEmpty) parts.removeLast();
      } else {
        parts.add(part);
      }
    }
    return '/${parts.join('/')}';
  }

  List<String>? _terminalEntries(String path) {
    if (path == '/') return ['Applications', 'Library', 'System', 'Users'];
    if (path == '/Users') return ['alex'];
    if (path == _home) {
      return ['Desktop', 'Documents', 'Downloads', 'Library', 'Projects'];
    }
    if (path == '$_home/Projects') return ['growth-notes', 'lumen-web'];
    if (path != _projectPath && !path.startsWith('$_projectPath/')) {
      return null;
    }
    final relative = path == _projectPath
        ? ''
        : path.substring(_projectPath.length + 1);
    final prefix = relative.isEmpty ? '' : '$relative/';
    final entries = {
      for (final file in _files.keys)
        if (file.startsWith(prefix))
          file.substring(prefix.length).split('/').first,
    }.toList()..sort();
    return entries.isEmpty && relative.isNotEmpty ? null : entries;
  }

  String _terminalCommand(String line) {
    if (line.isEmpty) return '';
    final words = line.split(RegExp(r'\s+'));
    final command = words.first;
    switch (command) {
      case 'clear':
        _terminalText = '';
        return '';
      case 'pwd':
        return _terminalDirectory;
      case 'whoami':
        return 'alex';
      case 'hostname':
        return 'MacBook-Pro.local';
      case 'uname':
        return words.length > 1
            ? 'Darwin MacBook-Pro.local 25.1.0 Darwin Kernel Version 25.1.0 arm64'
            : 'Darwin';
      case 'date':
        return 'Sun Nov 15 09:00:00 CST 2026';
      case 'echo':
        var value = line.substring(command.length).trimLeft();
        if (value.length > 1 &&
            ((value.startsWith('"') && value.endsWith('"')) ||
                (value.startsWith("'") && value.endsWith("'")))) {
          value = value.substring(1, value.length - 1);
        }
        return value;
      case 'cd':
        final target = _terminalPath(words.length == 1 ? '~' : words[1]);
        if (_terminalEntries(target) == null) {
          return 'cd: no such file or directory: ${words.skip(1).join(' ')}';
        }
        _terminalDirectory = target;
        return '';
      case 'ls':
        final arguments = words.skip(1).where((word) => !word.startsWith('-'));
        final path = _terminalPath(arguments.isEmpty ? '.' : arguments.first);
        final entries = _terminalEntries(path);
        return entries?.join('  ') ??
            'ls: ${arguments.first}: No such file or directory';
      case 'cat':
        if (words.length < 2) return '';
        final output = <String>[];
        for (final name in words.skip(1)) {
          final path = _terminalPath(name);
          final relative = path.startsWith('$_projectPath/')
              ? path.substring(_projectPath.length + 1)
              : null;
          output.add(
            _files[relative] ?? 'cat: $name: No such file or directory',
          );
        }
        return output.join('\n');
      case 'git':
        final action = words.length > 1 ? words[1] : '';
        if (action == 'status') {
          if (_committed) {
            return 'On branch $_branch\nnothing to commit, working tree clean';
          }
          if (words.contains('--short') || words.contains('-s')) {
            return '${_staged ? 'M ' : ' M'} src/styles/login.css';
          }
          return 'On branch $_branch\nYour branch is ahead of origin/$_branch by 1 commit.\n\n'
              '${_staged ? 'Changes to be committed:' : 'Changes not staged for commit:'}\n'
              '  modified:   src/styles/login.css';
        }
        if (action == 'diff') {
          if (_committed || words.contains('--cached') != _staged) return '';
          return 'diff --git a/src/styles/login.css b/src/styles/login.css\n'
              '--- a/src/styles/login.css\n+++ b/src/styles/login.css\n'
              '@@ -1,4 +1,4 @@\n .login-form {\n-  gap: 12px;\n+  gap: 20px;\n }\n'
              '-  height: 36px;\n+  min-height: 44px;';
        }
        if (action == 'branch') {
          return words.contains('--show-current')
              ? _branch
              : '* $_branch\n  main';
        }
        if (action == 'log') {
          return words.contains('--oneline')
              ? 'a12b34c Improve sign-in form spacing and accessibility'
              : 'commit a12b34c56d78e90f\nAuthor: Alex Rivera <alex@example.com>\n\n'
                    '    Improve sign-in form spacing and accessibility';
        }
        return 'git: \'$action\' is not a git command. See \'git --help\'.';
      case 'npm':
        if (words.skip(1).join(' ') == 'run build') {
          return '> lumen-web@1.0.0 build\n> vite build\n\n'
              'building client bundle for production...\n'
              '42 modules transformed.\n'
              'dist/index.html                   0.46 kB\n'
              'dist/assets/index.js             68.23 kB\n'
              'built in 428ms';
        }
        if (words.length > 1 && words[1] == 'test') {
          return '> lumen-web@1.0.0 test\n\n'
              ' PASS  src/pages/Login.test.tsx\n'
              '  ✓ submits a valid email address\n'
              '  ✓ keeps the primary action accessible\n\n'
              ' Test Files  1 passed\n      Tests  2 passed';
        }
        return 'npm error Missing script: "${words.skip(1).join(' ')}"';
      case 'help':
        return 'Shell commands: pwd, ls, cd, cat, echo, clear, whoami, hostname, uname, date\n'
            'Project commands: git status, git diff, git branch, git log, npm test, npm run build';
      default:
        return 'zsh: command not found: $command';
    }
  }

  @override
  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    for (final timer in _timers) {
      timer.cancel();
    }
    _timers.clear();
    for (final feed in _feeds.toList()) {
      await feed.close();
    }
  }

  @override
  void dispose() => unawaited(close());

  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnsupportedError(
    'Operation is unavailable in the local demonstration',
  );
}

class _DemoUpdates
    implements
        Updates,
        ConversationUpdates,
        CommandUpdates,
        TerminalUpdates,
        UsageUpdates,
        StatusUpdates {
  _DemoUpdates(this._value, this._removed);
  final Map<String, dynamic> Function() _value;
  final void Function(_DemoUpdates) _removed;
  final _values = StreamController<String>();
  late final _iterator = StreamIterator(_values.stream);
  bool _closed = false;

  void publish() {
    if (!_closed) _values.add(jsonEncode(_value()));
  }

  @override
  Future<String> next() async {
    if (await _iterator.moveNext()) return _iterator.current;
    throw StateError('demo observation is closed');
  }

  @override
  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    _removed(this);
    unawaited(_values.close());
  }

  @override
  Future<void> refresh() async => publish();
  @override
  Future<void> loadOlder() async {}
  @override
  Future<void> loadThrough({required BigInt sequence}) async {}
  @override
  Future<void> trackTools({required String selection}) async {}
  @override
  void dispose() => unawaited(close());
  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnsupportedError(
    'Operation is unavailable in the local demonstration',
  );
}
