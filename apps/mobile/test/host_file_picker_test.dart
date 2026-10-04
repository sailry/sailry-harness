import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/resources/host_file_picker.dart';
import 'package:sailry_mobile/features/resources/project_form.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';

void main() {
  testWidgets('project selection uses host directory pages', (tester) async {
    final requests = <Map<String, dynamic>>[];
    Map<String, dynamic>? saved;
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (kind, data) async {
        if (kind == 'register_project') {
          saved = data;
          return {};
        }
        expect(kind, 'browse_files');
        requests.add(data!);
        final nested = data['directory'] == '/remote/project';
        return {
          'kind': 'file_listing',
          'data': {
            'separator': '/',
            'locations': [
              {'name': 'Home', 'path': '/remote'},
            ],
            'parent': nested ? '/remote' : '/',
            'directory': {
              'path': nested ? '/remote/project' : '/remote',
              'entries': [
                if (!nested && data['after'] == null)
                  {'kind': 'file', 'name': 'note.txt'},
                if (!nested && data['after'] != null)
                  {'kind': 'directory', 'name': 'project'},
              ],
              'next': !nested && data['after'] == null
                  ? {'revision': 'r1', 'name': 'note.txt', 'directory': false}
                  : null,
            },
          },
        };
      },
    );
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.light),
        home: Scaffold(
          body: Builder(
            builder: (context) => TextButton(
              onPressed: () => showAppSheet(
                context,
                tr('hostRegisterProject'),
                child: ProjectForm(host: host),
              ),
              child: const Text('Open'),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('Open'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip(tr('hostChooseDirectory')));
    await tester.pumpAndSettle();
    expect(requests.single, {'directory': null, 'after': null});
    expect(
      tester
          .widget<ListTile>(find.widgetWithText(ListTile, 'note.txt'))
          .enabled,
      isFalse,
    );
    await tester.tap(find.text(tr('hostLoadMore')));
    await tester.pumpAndSettle();
    expect(requests.last['after'], {
      'revision': 'r1',
      'name': 'note.txt',
      'directory': false,
    });
    await tester.tap(find.text('project'));
    await tester.pumpAndSettle();
    expect(requests.last['directory'], '/remote/project');
    await tester.tap(
      find.widgetWithText(FilledButton, tr('hostChooseDirectory')),
    );
    await tester.pumpAndSettle();
    expect(find.text('/remote/project'), findsOneWidget);
    expect(find.text('project'), findsOneWidget);
    expect(saved, isNull);
    await tester.tap(find.widgetWithText(FilledButton, tr('save')));
    await tester.pumpAndSettle();
    expect(saved, {'name': 'project', 'path': '/remote/project'});
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await host.close();
    host.dispose();
  });

  testWidgets('host separators and retryable selection failures', (
    tester,
  ) async {
    var fail = true;
    String? selected;
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (_, _) async {
        if (fail) throw const CommandFailure('unavailable');
        return {
          'kind': 'file_listing',
          'data': {
            'separator': '\\',
            'parent': null,
            'locations': [],
            'directory': {
              'path': r'C:\keys',
              'entries': [
                {'name': 'key.pem', 'kind': 'file'},
              ],
              'next': null,
            },
          },
        };
      },
    );
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.light),
        home: Scaffold(
          body: Builder(
            builder: (context) => TextButton(
              onPressed: () async {
                selected = await pushPage<String>(
                  context,
                  HostFilePicker(host: host),
                );
              },
              child: const Text('Open'),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('Open'));
    await tester.pumpAndSettle();
    expect(find.text(tr('conversationFailed')), findsOneWidget);
    expect(find.byType(FailureState), findsOneWidget);
    expect(find.widgetWithText(FilledButton, tr('retry')), findsOneWidget);
    expect(tester.getSize(find.byType(FailureState)).height, greaterThan(400));
    fail = false;
    await tester.tap(find.text(tr('retry')));
    await tester.pumpAndSettle();
    expect(find.byType(FailureState), findsNothing);
    await tester.tap(find.text('key.pem'));
    await tester.pumpAndSettle();
    expect(selected, r'C:\keys\key.pem');
    await tester.pumpWidget(const SizedBox());
    await host.close();
    host.dispose();
  });
}
