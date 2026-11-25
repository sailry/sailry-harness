import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/files.dart';
import 'package:sailry_mobile/features/conversations/live/question.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/features/conversations/live/transcript_index.dart';
import 'package:sailry_mobile/features/resources/contributions.dart';
import 'package:sailry_mobile/features/resources/file_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:url_launcher_platform_interface/link.dart';
import 'package:url_launcher_platform_interface/url_launcher_platform_interface.dart';

const _web = 'https://example.com/reply';
const _mail = 'mailto:hello@example.com?subject=Sailry';
const _knowledge = 'sailry-knowledge://docs/chunk?revision=3';
const _sessionLink = 'sailry://session/0b7c3e1a-5d2f-4c6b-9a8e-1f2d3c4b5a69';
const _worktree = 'captured-tree';
const _session = 'origin-session';

class _Launcher extends UrlLauncherPlatform {
  final calls = <(String, PreferredLaunchMode)>[];

  @override
  LinkDelegate? get linkDelegate => null;

  @override
  Future<bool> launchUrl(String url, LaunchOptions options) async {
    calls.add((url, options.mode));
    return true;
  }
}

_Launcher _fakeLauncher() {
  final original = UrlLauncherPlatform.instance;
  final launcher = _Launcher();
  UrlLauncherPlatform.instance = launcher;
  addTearDown(() => UrlLauncherPlatform.instance = original);
  return launcher;
}

HostConnection _host({String id = 'entry/source', CommandHandler? command}) {
  final host = HostConnection.test(
    id: id,
    label: 'Source Host',
    command: command ?? (kind, _) async => fail('Unexpected command: $kind'),
    snapshot: const {
      'worktrees': [
        {'id': _worktree, 'path': '/source'},
      ],
    },
  );
  addTearDown(host.dispose);
  return host;
}

TranscriptIndex _reply(String body, List<Map<String, dynamic>> citations) =>
    TranscriptIndex({
      'timeline': ['turn'],
      'snapshot': {
        'page': {
          'runs': [
            {
              'turn': 'turn',
              'kind': 'task',
              'status': 'completed',
              'worktree': _worktree,
            },
          ],
          'entries': [
            {
              'id': 'reply',
              'turn': 'turn',
              'author': 'assistant',
              'citations': citations,
              'parts': [
                {'kind': 'text', 'data': body},
              ],
            },
          ],
        },
      },
      'segments': {
        'turn': [
          {
            'rows': [
              {
                'source': {'entry': 'reply', 'index': 0},
                'end': 1,
                'text_offset': 0,
                'text_last': true,
              },
            ],
          },
        ],
      },
    });

Widget _liveReply(
  HostConnection host,
  ScrollController scroll,
  TranscriptIndex transcript, {
  bool Function(String)? onOpenSession,
}) => LiveTimeline(
  transcript: transcript,
  connected: true,
  controller: scroll,
  padding: EdgeInsets.zero,
  session: const {'id': _session, 'worktree': 'newer-tree'},
  host: host,
  command: (kind, _) async => fail('Unexpected command: $kind'),
  onOpenSession: onOpenSession,
);

Future<void> _showReply(
  WidgetTester tester,
  HostConnection host,
  String body, {
  List<ProjectResourceFeature> resources = const [],
  List<Map<String, dynamic>> citations = const [],
  bool Function(String)? onOpenSession,
}) async {
  final scroll = ScrollController();
  addTearDown(scroll.dispose);
  await tester.pumpWidget(
    ProjectResourcesScope(
      features: resources,
      child: MaterialApp(
        home: Scaffold(
          body: _liveReply(
            host,
            scroll,
            _reply(body, citations),
            onOpenSession: onOpenSession,
          ),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

Future<void> _tapLink(WidgetTester tester, String label) async {
  await tester.tapOnText(find.textRange.ofSubstring(label));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets(
    'link priority survives reordering and restores on recomposition',
    (tester) async {
      final launcher = _fakeLauncher();
      final host = _host();
      final calls = <String>[];
      ProjectResourceFeature feature(String label, {int priority = 0}) =>
          ProjectResourceFeature(
            links: [
              ProjectLinkContribution.scheme('https', (_, _) async {
                calls.add(label);
              }, priority: priority),
            ],
          );
      final high = feature('high', priority: 7);
      final normal = feature('normal');
      final low = feature('low', priority: -2);
      final features = ValueNotifier([low, normal, high]);
      final scroll = ScrollController();
      addTearDown(features.dispose);
      addTearDown(scroll.dispose);
      await tester.pumpWidget(
        ValueListenableBuilder(
          valueListenable: features,
          child: MaterialApp(
            home: Scaffold(
              body: _liveReply(host, scroll, _reply('[Open web]($_web)', [])),
            ),
          ),
          builder: (_, current, child) =>
              ProjectResourcesScope(features: current, child: child!),
        ),
      );
      await tester.pumpAndSettle();
      await _tapLink(tester, 'Open web');
      final timeline = tester.state(find.byType(LiveTimeline));

      for (final next in [
        [high, normal, low],
        [low, normal],
        [low],
        <ProjectResourceFeature>[],
      ]) {
        features.value = next;
        await tester.pump();
        expect(tester.state(find.byType(LiveTimeline)), same(timeline));
        await _tapLink(tester, 'Open web');
      }
      expect(calls, ['high', 'high', 'normal']);
      expect(
        launcher.calls,
        List.filled(2, (_web, PreferredLaunchMode.externalApplication)),
      );
    },
  );

  testWidgets(
    'same priority in disjoint Host scopes uses the captured origin',
    (tester) async {
      final launcher = _fakeLauncher();
      final host = _host(id: 'entry-one/source');
      final other = _host(id: 'entry-two/source');
      final calls = <(String, HostConnection, String, String?)>[];
      const unknownScheme = 'sample://resource';
      ProjectLinkContribution claim(
        String label, {
        required ProjectLinkScope scope,
      }) => ProjectLinkContribution.scheme('sample', (_, link) async {
        calls.add((label, link.host, link.worktree, link.session));
      }, scope: scope);
      final features = [
        ProjectResourceFeature(
          links: [
            claim(
              'different worktree',
              scope: (host: host.id, worktree: 'newer-tree', session: _session),
            ),
            claim(
              'first',
              scope: (host: host.id, worktree: _worktree, session: _session),
            ),
            claim(
              'other',
              scope: (host: other.id, worktree: _worktree, session: _session),
            ),
          ],
        ),
      ];
      const reply = '[Open scoped link]($unknownScheme)';
      await _showReply(tester, host, reply, resources: features);
      await _tapLink(tester, 'Open scoped link');
      await _showReply(tester, other, reply, resources: features);
      await _tapLink(tester, 'Open scoped link');
      expect(calls, [
        ('first', host, _worktree, _session),
        ('other', other, _worktree, _session),
      ]);
      expect(launcher.calls, isEmpty);
    },
  );

  testWidgets('an offline scoped web handler does not launch externally', (
    tester,
  ) async {
    final launcher = _fakeLauncher();
    final host = _host();
    final attempts = <String>[];
    final resources = [
      ProjectResourceFeature(
        links: [
          ProjectLinkContribution.scheme('https', (_, _) async {
            attempts.add('scoped');
          }, scope: (host: host.id, worktree: _worktree, session: _session)),
          ProjectLinkContribution.scheme('https', (_, _) async {
            attempts.add('global');
          }, priority: -2),
        ],
      ),
    ];
    await _showReply(
      tester,
      host,
      '[Open scoped web]($_web)',
      resources: resources,
    );
    await host.close();
    await _tapLink(tester, 'Open scoped web');
    expect(attempts, isEmpty);
    expect(launcher.calls, isEmpty);
    expect(find.text(tr('fileLinkUnavailable')), findsOneWidget);
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
  });

  testWidgets('same-priority link claims cannot mount in one scope', (
    tester,
  ) async {
    final launcher = _fakeLauncher();
    final host = _host();
    final scroll = ScrollController();
    addTearDown(scroll.dispose);
    final scope = (host: host.id, worktree: _worktree, session: _session);
    Future<void> open(BuildContext _, ProjectResourceLink _) async =>
        fail('A conflicting claim must not open');
    await tester.pumpWidget(
      Builder(
        builder: (_) => ProjectResourcesScope(
          features: [
            ProjectResourceFeature(
              links: [
                ProjectLinkContribution.scheme(
                  'https',
                  open,
                  scope: scope,
                  priority: 5,
                ),
                ProjectLinkContribution.scheme(
                  'https',
                  open,
                  scope: scope,
                  priority: 1,
                ),
                ProjectLinkContribution.scheme(
                  'https',
                  open,
                  scope: scope,
                  priority: 1,
                ),
              ],
            ),
          ],
          child: MaterialApp(
            home: Scaffold(
              body: _liveReply(
                host,
                scroll,
                _reply('[Conflicting link]($_web)', []),
              ),
            ),
          ),
        ),
      ),
    );
    expect(
      tester.takeException(),
      isA<ArgumentError>().having(
        (error) => error.message,
        'message',
        'Duplicate project link route scheme/https at priority 1',
      ),
    );
    expect(find.byType(LiveTimeline), findsNothing);
    expect(launcher.calls, isEmpty);
  });

  testWidgets('a retired handler does not retry an in-flight click', (
    tester,
  ) async {
    final launcher = _fakeLauncher();
    final host = _host();
    final pending = Completer<void>();
    final attempts = <String>[];
    final high = ProjectResourceFeature(
      links: [
        ProjectLinkContribution.scheme('https', (_, _) {
          attempts.add('retired');
          return pending.future;
        }, priority: 1),
      ],
    );
    final lower = ProjectResourceFeature(
      links: [
        ProjectLinkContribution.scheme('https', (_, _) async {
          attempts.add('lower');
        }),
      ],
    );
    await _showReply(
      tester,
      host,
      '[Open pending]($_web)',
      resources: [high, lower],
    );
    await _tapLink(tester, 'Open pending');
    expect(attempts, ['retired']);

    await _showReply(tester, host, '[Open pending]($_web)', resources: [lower]);
    pending.completeError(const CommandFailure('unavailable'));
    await tester.pumpAndSettle();
    expect(attempts, ['retired']);
    expect(find.text(tr('failureUnavailable')), findsOneWidget);
    expect(launcher.calls, isEmpty);

    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    await _tapLink(tester, 'Open pending');
    expect(attempts, ['retired', 'lower']);
  });

  testWidgets('reply and source links use assembly targets and URI schemes', (
    tester,
  ) async {
    final launcher = _fakeLauncher();
    final host = _host();
    final links = <ProjectResourceLink>[];
    Future<void> record(BuildContext context, ProjectResourceLink link) async {
      links.add(link);
    }

    await _showReply(
      tester,
      host,
      '[Open file](file:///source/src/lib.txt#L4) | '
      '[Open directory](src/) | [Open web]($_web) | '
      '[Open session]($_sessionLink) | [Open mail]($_mail) | '
      '[Local anchor](#reply)',
      resources: [
        ProjectResourceFeature(
          links: [
            ProjectLinkContribution.scheme('file', (_, _) async {
              fail('A scoped file target takes precedence over its scheme');
            }, priority: 10),
            ProjectLinkContribution.target('file', record),
            ProjectLinkContribution.target('directory', record),
            ProjectLinkContribution.scheme('https', record),
            ProjectLinkContribution.scheme('sailry', record),
            ProjectLinkContribution.scheme('mailto', record),
            ProjectLinkContribution.scheme('sailry-knowledge', record),
          ],
        ),
      ],
      citations: const [
        {'uri': _knowledge, 'title': 'Knowledge citation'},
      ],
      onOpenSession: (_) => fail('Session contribution owns this link'),
    );

    for (final label in [
      'Open file',
      'Open directory',
      'Open web',
      'Open session',
      'Open mail',
      'Local anchor',
    ]) {
      await _tapLink(tester, label);
    }
    await tester.tap(find.text('Knowledge citation'));
    await tester.pumpAndSettle();

    expect(links.map((link) => (link.kind, link.data, link.uri?.toString())), [
      ('file', 'src/lib.txt', 'file:///source/src/lib.txt#L4'),
      ('directory', 'src', 'src/'),
      (null, null, _web),
      (null, null, _sessionLink),
      (null, null, _mail),
      (null, null, _knowledge),
    ]);
    for (final link in links) {
      expect(
        (link.host, link.worktree, link.session),
        (host, _worktree, _session),
      );
    }
    expect(launcher.calls, isEmpty);
    expect(find.text(tr('fileLinkUnavailable')), findsNothing);
    expect(find.byType(ResourceFilePage), findsNothing);
  });

  testWidgets('safe defaults win over lower priority handlers', (tester) async {
    final launcher = _fakeLauncher();
    final reads = <Map<String, dynamic>?>[];
    final host = _host(
      command: (kind, data) async {
        expect(kind, 'read_file');
        reads.add(data);
        return {
          'data': {'text': 'Remote file contents', 'revision': 'r1'},
        };
      },
    );
    final sessions = <String>[];
    await _showReply(
      tester,
      host,
      '[Open file](src/lib.txt#L4) | [Open web]($_web) | [Open mail]($_mail) | '
      '[Open session]($_sessionLink) | [Local anchor](#heading) | '
      '[Unknown scheme](unregistered://resource/x)',
      resources: [
        ProjectResourceFeature(
          links: [
            for (final scheme in ['mailto', 'sailry'])
              ProjectLinkContribution.scheme(scheme, (_, _) async {
                fail('Lower priority handler opened $scheme');
              }, priority: -2),
            ProjectLinkContribution.target('file', (_, _) async {
              fail('Lower priority handler opened the file');
            }, priority: -2),
          ],
        ),
      ],
      citations: const [
        {'uri': _knowledge, 'title': 'Knowledge citation'},
      ],
      onOpenSession: (link) {
        sessions.add(link);
        return true;
      },
    );

    await _tapLink(tester, 'Local anchor');
    expect(find.text(tr('fileLinkUnavailable')), findsNothing);
    await _tapLink(tester, 'Open web');
    await _tapLink(tester, 'Open mail');
    await _tapLink(tester, 'Open session');
    expect(sessions, [_sessionLink]);
    expect(launcher.calls, [
      (_web, PreferredLaunchMode.externalApplication),
      (_mail, PreferredLaunchMode.externalApplication),
    ]);
    await _tapLink(tester, 'Unknown scheme');
    expect(find.text(tr('fileLinkUnavailable')), findsOneWidget);
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Knowledge citation'));
    await tester.pumpAndSettle();
    expect(find.text(tr('fileLinkUnavailable')), findsOneWidget);
    expect(reads, isEmpty);
    expect(launcher.calls, hasLength(2));
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();

    await _tapLink(tester, 'Open file');
    final page = tester.widget<ResourceFilePage>(find.byType(ResourceFilePage));
    expect(
      (page.host, page.worktree, page.path),
      (host, _worktree, 'src/lib.txt'),
    );
    expect(reads, [
      {'worktree': _worktree, 'path': 'src/lib.txt'},
    ]);
  });

  testWidgets('unsafe reply links never reach contributions or the OS', (
    tester,
  ) async {
    final launcher = _fakeLauncher();
    final host = _host();
    var opened = 0;
    Future<void> record(BuildContext _, ProjectResourceLink _) async =>
        opened++;
    await _showReply(
      tester,
      host,
      '[Script URI](javascript:alert) | [Credentials](https://name@example.com/) | '
      '[No host](https:/path) | [Remote authority](file://other/source/x.txt) | '
      '[Scheme-relative](//other/source/x.txt) | [Outside](file:///elsewhere/x.txt) | '
      '[Mail headers](mailto:hello@example.com?subject=x%0d%0aBcc:test) | '
      '[Invalid citation](sailry-knowledge://docs/chunk?revision=3&revision=4) | '
      '[Bad session](sailry://session/$_session?node=other)',
      resources: [
        ProjectResourceFeature(
          links: [
            for (final scheme in [
              'javascript',
              'https',
              'file',
              'mailto',
              'sailry-knowledge',
              'sailry',
            ])
              ProjectLinkContribution.scheme(scheme, record, priority: 10),
            ProjectLinkContribution.target('file', record, priority: 10),
          ],
        ),
      ],
      onOpenSession: (_) => fail('Invalid session reached its fallback'),
    );

    for (final label in [
      'Script URI',
      'Credentials',
      'No host',
      'Remote authority',
      'Scheme-relative',
      'Outside',
      'Mail headers',
      'Invalid citation',
      'Bad session',
    ]) {
      await _tapLink(tester, label);
      expect(
        find.text(tr('fileLinkUnavailable')),
        findsOneWidget,
        reason: label,
      );
      await tester.pump(const Duration(seconds: 3));
      await tester.pumpAndSettle();
    }
    expect(opened, 0);
    expect(launcher.calls, isEmpty);
    expect(find.byType(ResourceFilePage), findsNothing);
  });

  testWidgets('a rejected handler is terminal for files, directories and web', (
    tester,
  ) async {
    final launcher = _fakeLauncher();
    final host = _host();
    final rejected = <ProjectResourceLink>[];
    final legacyAttempts = <(HostConnection, String, String)>[];
    var schemeAttempts = 0;
    var lowerAttempts = 0;
    Future<void> deny(BuildContext _, ProjectResourceLink link) async {
      rejected.add(link);
      throw const CommandFailure('permission_denied');
    }

    Future<void> lower(BuildContext _, ProjectResourceLink _) async {
      lowerAttempts++;
    }

    final resources = [
      ProjectResourceFeature(
        openDirectory: (_, host, worktree, directory) async {
          legacyAttempts.add((host, worktree, directory));
          throw const CommandFailure('permission_denied');
        },
        links: [
          ProjectLinkContribution.target('file', deny, priority: 4),
          ProjectLinkContribution.target('file', lower),
          ProjectLinkContribution.target('directory', lower, priority: -2),
          ProjectLinkContribution.scheme('https', deny),
          ProjectLinkContribution.scheme('https', lower, priority: -2),
          ProjectLinkContribution.scheme('file', (_, _) async {
            schemeAttempts++;
          }),
        ],
      ),
    ];
    await _showReply(
      tester,
      host,
      '[Rejected web]($_web) | [Rejected directory](file:///source/src/)',
      resources: resources,
    );
    await _tapLink(tester, 'Rejected directory');
    expect(legacyAttempts, [(host, _worktree, 'src')]);
    expect(schemeAttempts, 0);
    expect(find.text(tr('failureDenied')), findsOneWidget);
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();

    await _tapLink(tester, 'Rejected web');
    expect(find.text(tr('failureDenied')), findsOneWidget);
    expect(launcher.calls, isEmpty);
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();

    await tester.pumpWidget(
      ProjectResourcesScope(
        features: resources,
        child: MaterialApp(
          home: Scaffold(
            body: PublishedFiles(
              files: const [
                {'name': 'Published report', 'path': 'build/report.txt'},
              ],
              host: host,
              worktree: _worktree,
              session: _session,
              enabled: true,
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.text('Published report'));
    await tester.pumpAndSettle();
    expect(find.text(tr('failureDenied')), findsOneWidget);
    expect(find.byType(ResourceFilePage), findsNothing);
    expect(
      rejected.map((link) => (link.kind, link.data, link.uri?.toString())),
      [(null, null, _web), ('file', 'build/report.txt', null)],
    );
    expect(lowerAttempts, 0);
    expect(launcher.calls, isEmpty);
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
  });

  testWidgets('URL questions only acknowledge permitted external opens', (
    tester,
  ) async {
    final launcher = _fakeLauncher();
    final responses = <Map<String, dynamic>>[];
    for (final url in [
      'file:///source/secret.txt',
      'http://example.com/',
      _web,
    ]) {
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: ConversationQuestion(
              key: ValueKey(url),
              spec: {
                'input': {'kind': 'url', 'url': url},
              },
              question: const {'id': 'question'},
              session: const {'id': _session},
              command: (kind, data) async {
                expect(kind, 'resolve_question');
                responses.add(data);
                return {};
              },
            ),
          ),
        ),
      );
      await tester.tap(find.text(url));
      await tester.pumpAndSettle();
      if (url != _web) {
        expect(launcher.calls, isEmpty);
        expect(responses, isEmpty);
        expect(find.text(tr('conversationFailed')), findsOneWidget);
      }
    }
    expect(launcher.calls, [(_web, PreferredLaunchMode.externalApplication)]);
    expect(responses.single['response'], {
      'kind': 'answer',
      'data': {'kind': 'opened'},
    });
  });
}
