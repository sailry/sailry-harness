import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/conversation_frame.dart';
import 'package:sailry_mobile/features/resources/live_resources.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';

import 'live_resources_test.dart' as fixture;

void main() {
  testWidgets('secondary titles and icons go back while honoring pop guards', (
    tester,
  ) async {
    final allowed = ValueNotifier(false);
    addTearDown(allowed.dispose);
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.light),
        home: Builder(
          builder: (context) => PageFrame(
            title: 'Resources',
            child: TextButton(
              onPressed: () => pushPage(
                context,
                Builder(
                  builder: (context) => PageFrame(
                    title: 'Files',
                    child: TextButton(
                      onPressed: () => pushPage(
                        context,
                        ValueListenableBuilder(
                          valueListenable: allowed,
                          builder: (context, canLeave, _) => PopScope(
                            canPop: canLeave,
                            child: const PageFrame(
                              title: 'Preview',
                              child: SizedBox(),
                            ),
                          ),
                        ),
                      ),
                      child: const Text('Open preview'),
                    ),
                  ),
                ),
              ),
              child: const Text('Open files'),
            ),
          ),
        ),
      ),
    );
    expect(find.byTooltip(tr('back')), findsNothing);
    await tester.tap(find.text('Open files'));
    await tester.pumpAndSettle();
    expect(find.byTooltip(tr('back')), findsOneWidget);
    await tester.tap(find.text('Open preview'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Preview'));
    await tester.pumpAndSettle();
    expect(find.text('Preview'), findsOneWidget);
    allowed.value = true;
    await tester.pump();
    await tester.tap(find.text('Preview'));
    await tester.pumpAndSettle();
    expect(find.text('Files'), findsOneWidget);
    final arrow = find.descendant(
      of: find.byTooltip(tr('back')),
      matching: find.byType(AppIcon),
    );
    await tester.tap(arrow);
    await tester.pumpAndSettle();
    expect(find.text('Resources'), findsOneWidget);
    expect(find.byTooltip(tr('back')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('conversation titles return without removing settings', (
    tester,
  ) async {
    var settings = 0;
    tester.view.physicalSize = const Size(320, 740);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.dark),
        home: Builder(
          builder: (context) => Scaffold(
            body: TextButton(
              onPressed: () => pushPage(
                context,
                ConversationFrame(
                  title: 'A conversation with a long title',
                  leading: RoundButton(
                    icon: 'settings',
                    tooltip: 'Settings',
                    onPressed: () => settings++,
                  ),
                  actions: const [],
                  composer: const SizedBox(height: 48),
                  bodyBuilder: (_, _) => const SizedBox(),
                ),
              ),
              child: const Text('Open'),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('Open'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Settings'));
    expect(settings, 1);
    await tester.tap(find.text('A conversation with a long title'));
    await tester.pumpAndSettle();
    expect(find.text('Open'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'resource children inherit the selected worktree without switchers',
    (tester) async {
      final requests = <(String, String)>[];
      final host = HostConnection.test(
        id: 'node',
        label: 'Node',
        snapshot: {
          'worktrees': [
            {'id': 'default', 'project': 'project', 'path': '/default'},
            {'id': 'selected', 'project': 'project', 'path': '/selected'},
          ],
        },
        command: (kind, data) async {
          requests.add((kind, data!['worktree'] as String));
          return {'data': <String, dynamic>{}};
        },
      );
      final app = AppSession.test(hosts: [host]);
      await fixture.mount(
        tester,
        const LiveResourcesPage(hostId: 'node', worktreeId: 'selected'),
        app,
      );
      expect(find.byTooltip(tr('selectHost')), findsOneWidget);
      await tester.tap(find.text(tr('files')));
      await tester.pumpAndSettle();
      expect(find.byTooltip(tr('selectWorkspace')), findsNothing);
      await tester.tap(find.byTooltip(tr('refresh')));
      await tester.pumpAndSettle();
      await tester.tap(find.text(tr('files')));
      await tester.pumpAndSettle();
      expect(find.byTooltip(tr('selectHost')), findsOneWidget);
      await tester.tap(find.text(tr('git')));
      await tester.pumpAndSettle();
      expect(find.byTooltip(tr('selectWorkspace')), findsNothing);
      expect(
        requests.where((item) => item.$1 == 'list_directory'),
        hasLength(2),
      );
      expect(requests.where((item) => item.$1 == 'inspect_git'), hasLength(1));
      expect(requests.every((item) => item.$2 == 'selected'), isTrue);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
      app.dispose();
    },
  );
}
