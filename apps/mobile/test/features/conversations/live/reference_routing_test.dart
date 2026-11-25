import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/assembly/lean.dart';
import 'package:sailry_mobile/assembly/pages.dart';
import 'package:sailry_mobile/features/conversations/live/references.dart';
import 'package:sailry_mobile/features/resources/file_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/shell/assembly.dart';

const _worktree = 'tree';

void main() {
  testWidgets('lean references preview files but do not open directories', (
    tester,
  ) async {
    final calls = <(String, Map<String, dynamic>?)>[];
    final host = HostConnection.test(
      id: 'entry/fixture',
      label: 'Fixture host',
      command: (kind, data) async {
        calls.add((kind, data));
        return {
          'data': kind == 'read_file'
              ? {'text': 'fixture contents', 'revision': 'v1'}
              : <String, dynamic>{},
        };
      },
      snapshot: const {
        'worktrees': [
          {'id': _worktree, 'path': '/fixture'},
        ],
      },
    );
    final session = AppSession.test(hosts: [host]);
    final assembly = MobileAssembly(
      defaultRoute: chatRoute,
      resources: leanAssembly.resources,
      pages: [
        MobilePage(
          id: chatRoute,
          icon: 'chat',
          label: 'chat',
          build: (_) => Center(
            child: MessageReferences(
              host: host,
              worktree: _worktree,
              references: const [
                {
                  'label': 'Source directory',
                  'target': {'kind': 'directory', 'data': 'src'},
                },
                {
                  'label': 'Source file',
                  'target': {'kind': 'file', 'data': 'src/lib.txt'},
                },
              ],
            ),
          ),
        ),
      ],
    );
    await tester.pumpWidget(SailryApp(assembly: assembly, session: session));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Source directory'));
    await tester.pumpAndSettle();
    final context = tester.element(find.byType(MessageReferences));
    expect(find.text(context.tr('fileLinkUnavailable')), findsOneWidget);
    expect(Navigator.of(context).canPop(), isFalse);
    expect(calls.map((call) => call.$1), isNot(contains('list_directory')));
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();

    await tester.tap(find.text('Source file'));
    await tester.pumpAndSettle();
    final file = tester.widget<ResourceFilePage>(find.byType(ResourceFilePage));
    expect(
      (file.host, file.worktree, file.path),
      (host, _worktree, 'src/lib.txt'),
    );
    expect(calls.singleWhere((call) => call.$1 == 'read_file').$2, {
      'worktree': _worktree,
      'path': 'src/lib.txt',
    });
    expect(file.document.draft?.text, 'fixture contents');

    await tester.pumpWidget(const SizedBox.shrink());
    await session.close();
    session.dispose();
  });
}
