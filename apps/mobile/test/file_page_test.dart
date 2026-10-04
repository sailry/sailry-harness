import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/resources/files_page.dart';
import 'package:sailry_mobile/features/resources/file_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';

import 'live_resources_test.dart' as fixture;
import 'live_conversations_test.dart' as chat;
import 'package:sailry_mobile/features/conversations/live/timeline.dart';

void main() {
  testWidgets('binary files keep the external open action', (tester) async {
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (kind, data) async {
        throw const CommandFailure('invalid_request');
      },
    );
    final app = AppSession.test(hosts: [host]);
    await fixture.mount(
      tester,
      ResourceFilePage(
        host: host,
        worktree: 'tree',
        path: 'archive.zip',
        document: FileDocument(),
        onOpenFile: (_) async {},
      ),
      app,
    );
    expect(find.text(tr('fileOpenExternal')), findsOneWidget);
    expect(find.byTooltip(tr('edit')), findsNothing);
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });
  for (final path in [
    'index.php',
    'app.rb',
    '.env',
    'Dockerfile',
    'module.mjs',
  ]) {
    testWidgets('opens UTF-8 source in $path', (tester) async {
      final host = HostConnection.test(
        id: 'node',
        label: 'Node',
        command: (kind, data) async {
          expect(kind, 'read_file');
          expect(data!['path'], path);
          return {
            'data': {'text': 'Source text', 'revision': 'r1'},
          };
        },
      );
      final app = AppSession.test(hosts: [host]);
      await fixture.mount(
        tester,
        ResourceFilePage(
          host: host,
          worktree: 'tree',
          path: path,
          document: FileDocument(),
          onOpenFile: (_) async {},
        ),
        app,
      );
      expect(find.text('Source text'), findsOneWidget);
      await tester.tap(find.byTooltip(tr('edit')));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('resource-file-input')), findsOneWidget);
      await tester.pumpWidget(const SizedBox());
      app.dispose();
    });
  }

  testWidgets('reply file links retain drafts in the sent turn worktree', (
    tester,
  ) async {
    final reads = <Map<String, dynamic>?>[];
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      snapshot: {
        'worktrees': [
          {'id': 'captured', 'path': '/project'},
          fixture.workspace,
        ],
      },
      command: (kind, data) async {
        expect(kind, 'read_file');
        reads.add(data);
        return {
          'data': {'text': '# From the captured worktree', 'revision': 'r1'},
        };
      },
    );
    final app = AppSession.test(hosts: [host]);
    final view = chat.view(
      entries: [
        {
          'id': 'prompt',
          'turn': 'turn',
          'author': 'user',
          'parts': [
            {
              'kind': 'reference',
              'data': {
                'label': 'Attached document',
                'target': {'kind': 'file', 'data': 'docs/note.md'},
              },
            },
          ],
        },
        {
          'id': 'reply',
          'turn': 'turn',
          'author': 'assistant',
          'parts': [
            {
              'kind': 'text',
              'data': '[Document](file:///project/docs/note.md:42)',
            },
          ],
        },
      ],
    );
    view['snapshot']['page']['runs'][0]['worktree'] = 'captured';
    await fixture.mount(
      tester,
      Scaffold(
        body: SingleChildScrollView(
          child: LiveTimeline(
            view: view,
            session: chat.session(),
            host: host,
            command: (_, _) async => {},
          ),
        ),
      ),
      app,
    );
    await tester.tap(find.text('Document'));
    await tester.pumpAndSettle();
    expect(reads, [
      {'worktree': 'captured', 'path': 'docs/note.md'},
    ]);
    expect(find.text('From the captured worktree'), findsOneWidget);
    await tester.tap(find.byTooltip(tr('edit')));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const ValueKey('resource-file-input')),
      'Unsaved draft',
    );
    await tester.tap(find.byTooltip(tr('back')));
    await tester.pumpAndSettle();
    expect(find.text('Document'), findsOneWidget);
    await tester.tap(find.text('Document'));
    await tester.pumpAndSettle();
    expect(find.text('Unsaved draft'), findsOneWidget);
    await tester.tap(find.byTooltip(tr('back')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Attached document'));
    await tester.pumpAndSettle();
    expect(find.text('Unsaved draft'), findsOneWidget);
    expect(reads.length, 1);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('reopening an unchanged file refreshes its preview', (
    tester,
  ) async {
    var version = 1;
    var reads = 0;
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      snapshot: {
        'worktrees': [fixture.workspace],
      },
      command: (kind, data) async {
        if (kind == 'read_file') {
          reads++;
          return {
            'data': {'text': '# Version $version', 'revision': 'r$version'},
          };
        }
        return {
          'data': {
            'entries': [
              {'name': 'note.md', 'kind': 'file'},
            ],
          },
        };
      },
    );
    final app = AppSession.test(hosts: [host]);
    await fixture.mount(
      tester,
      const FilesPage(hostId: 'node', worktreeId: 'tree'),
      app,
    );
    await tester.tap(find.text('note.md'));
    await tester.pumpAndSettle();
    expect(find.text('Version 1'), findsOneWidget);
    await tester.tap(find.byTooltip(tr('back')));
    await tester.pumpAndSettle();
    version = 2;
    await tester.tap(find.text('note.md'));
    await tester.pumpAndSettle();
    expect(reads, 2);
    expect(find.text('Version 2'), findsOneWidget);
    expect(find.text('Version 1'), findsNothing);
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('linked files inherit the worktree and return to the document', (
    tester,
  ) async {
    final reads = <String>[];
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      snapshot: {
        'worktrees': [fixture.workspace],
      },
      command: (kind, data) async {
        expect(data!['worktree'], 'tree');
        if (kind == 'read_file') {
          final path = data['path'] as String;
          reads.add(path);
          return {
            'data': {
              'text': path == 'docs/start.md'
                  ? '[Next](next.md)\n\n[Report](../report.pdf)\n\n[Outside](../../private.md)'
                  : '# Linked page',
              'revision': 'r1',
            },
          };
        }
        return {
          'data': {
            'entries': [
              if (data['path'] == '')
                {'name': 'docs', 'kind': 'directory'}
              else
                {'name': 'start.md', 'kind': 'file'},
            ],
          },
        };
      },
    );
    final app = AppSession.test(hosts: [host]);
    await fixture.mount(
      tester,
      const FilesPage(hostId: 'node', worktreeId: 'tree'),
      app,
    );
    await tester.tap(find.text('docs'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('start.md'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Next'));
    await tester.pumpAndSettle();
    expect(find.text('Linked page'), findsOneWidget);
    expect(reads.last, 'docs/next.md');
    await tester.tap(find.byTooltip(tr('back')));
    await tester.pumpAndSettle();
    expect(find.text('start.md'), findsOneWidget);
    await tester.tap(find.text('Report'));
    await tester.pumpAndSettle();
    expect(find.text('report.pdf'), findsOneWidget);
    expect(find.text(tr('fileOpenExternal')), findsOneWidget);
    await tester.tap(find.byTooltip(tr('back')));
    await tester.pumpAndSettle();
    final count = reads.length;
    await tester.tap(find.text('Outside'));
    await tester.pumpAndSettle();
    expect(find.text(tr('fileLinkUnavailable')), findsOneWidget);
    expect(reads.length, count);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('returning during loading cannot overwrite a reopened draft', (
    tester,
  ) async {
    final pending = Completer<Map<String, dynamic>>();
    var reads = 0;
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      snapshot: {
        'worktrees': [fixture.workspace],
      },
      command: (kind, data) async {
        if (kind == 'read_file') {
          if (++reads == 1) return pending.future;
          return {
            'data': {'text': '# Current', 'revision': 'r2'},
          };
        }
        return {
          'data': {
            'entries': [
              {'name': 'slow.md', 'kind': 'file'},
            ],
          },
        };
      },
    );
    final app = AppSession.test(hosts: [host]);
    await fixture.mount(
      tester,
      const FilesPage(hostId: 'node', worktreeId: 'tree'),
      app,
    );
    await tester.tap(find.text('slow.md'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 500));
    await tester.tap(find.byTooltip(tr('back')));
    await tester.pumpAndSettle();
    expect(find.byType(ResourceFilePage), findsNothing);
    await tester.tap(find.text('slow.md'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip(tr('edit')));
    await tester.pumpAndSettle();
    final field = find.byKey(const ValueKey('resource-file-input'));
    await tester.enterText(field, '# Keep my draft');
    pending.complete({
      'data': {'text': '# Finished', 'revision': 'r1'},
    });
    await tester.pumpAndSettle();
    await tester.tap(find.text(tr('preview')));
    await tester.pumpAndSettle();
    expect(find.text('Keep my draft'), findsOneWidget);
    expect(find.text('Finished'), findsNothing);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets(
    'back preserves drafts and sharing does not bypass a failed save',
    (tester) async {
      final commands = <String>[];
      final host = HostConnection.test(
        id: 'node',
        label: 'Node',
        snapshot: {
          'worktrees': [fixture.workspace],
        },
        command: (kind, data) async {
          commands.add(kind);
          return switch (kind) {
            'list_directory' => {
              'data': {
                'entries': [
                  {'name': 'note.md', 'kind': 'file'},
                ],
              },
            },
            'read_file' => {
              'data': {'text': '# Original', 'revision': 'r1'},
            },
            'write_file' => throw const CommandFailure('revision_conflict'),
            _ => throw StateError('Unexpected command: $kind'),
          };
        },
      );
      final app = AppSession.test(hosts: [host]);
      await fixture.mount(
        tester,
        const FilesPage(hostId: 'node', worktreeId: 'tree'),
        app,
      );
      tester.view.physicalSize = const Size(320, 740);
      await tester.tap(find.text('note.md'));
      await tester.pumpAndSettle();
      expect(find.byType(ResourceFilePage), findsOneWidget);
      expect(find.byType(BottomSheet), findsNothing);
      await tester.tap(find.byTooltip(tr('edit')));
      await tester.pumpAndSettle();
      final field = find.byKey(const ValueKey('resource-file-input'));
      await tester.enterText(field, '# Draft');
      await tester.tap(find.byTooltip(tr('back')));
      await tester.pumpAndSettle();
      expect(find.byType(ResourceFilePage), findsNothing);
      await tester.tap(find.text('note.md'));
      await tester.pumpAndSettle();
      expect(tester.widget<TextField>(field).controller!.text, '# Draft');
      expect(commands.where((kind) => kind == 'read_file'), hasLength(1));
      await tester.tap(find.byTooltip(tr('send')));
      await tester.pumpAndSettle();
      expect(find.text(tr('fileSaveBeforeShare')), findsOneWidget);
      await tester.tap(find.text(tr('cancel')));
      await tester.pumpAndSettle();
      expect(commands, isNot(contains('write_file')));
      await tester.tap(find.byTooltip(tr('send')));
      await tester.pumpAndSettle();
      await tester.tap(find.text(tr('save')));
      await tester.pumpAndSettle();
      expect(find.textContaining(tr('failureConflict')), findsOneWidget);
      expect(tester.widget<TextField>(field).controller!.text, '# Draft');
      expect(commands, isNot(contains('download_file')));
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
      app.dispose();
    },
  );

  testWidgets(
    'deletion confirms its target and retries the same request after back',
    (tester) async {
      final connection = fixture.ConnectionFixture();
      var deletions = 0;
      var listings = 0;
      final host = HostConnection.test(
        id: 'node',
        label: 'Node',
        connection: connection,
        snapshot: {
          'worktrees': [fixture.workspace],
        },
        command: (kind, data) async {
          switch (kind) {
            case 'list_directory':
              listings++;
              return {
                'data': {
                  'entries': [
                    {'name': 'report.pdf', 'kind': 'file'},
                  ],
                },
              };
            case 'trash_entry':
              expect(data, {'worktree': 'tree', 'path': 'report.pdf'});
              deletions++;
              throw const CommandFailure(
                'outcome_unknown',
                request: 'stable-trash-request',
              );
            default:
              throw StateError('Unexpected command: $kind');
          }
        },
      );
      final app = AppSession.test(hosts: [host]);
      await fixture.mount(
        tester,
        const FilesPage(hostId: 'node', worktreeId: 'tree'),
        app,
      );
      await tester.tap(find.text('report.pdf'));
      await tester.pumpAndSettle();
      expect(find.text(tr('fileOpenExternal')), findsOneWidget);
      expect(find.byTooltip(tr('edit')), findsNothing);
      await tester.tap(find.byTooltip(tr('delete')));
      await tester.pumpAndSettle();
      expect(find.textContaining('report.pdf'), findsNWidgets(2));
      await tester.tap(find.text(tr('cancel')));
      await tester.pumpAndSettle();
      expect(deletions, 0);
      await tester.tap(find.byTooltip(tr('delete')));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(FilledButton, tr('delete')));
      await tester.pumpAndSettle();
      expect(find.text(tr('fileTrashUncertain')), findsOneWidget);
      expect(find.byType(ResourceFilePage), findsOneWidget);
      await tester.tap(find.byTooltip(tr('back')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('report.pdf'));
      await tester.pumpAndSettle();
      await tester.tap(find.byTooltip(tr('retry')));
      await tester.pumpAndSettle();
      expect(deletions, 1);
      expect(connection.requests, ['stable-trash-request']);
      expect(find.byType(ResourceFilePage), findsNothing);
      expect(listings, 3);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
      await app.close();
      app.dispose();
    },
  );
}
