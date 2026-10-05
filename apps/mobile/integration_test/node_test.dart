import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart'
    show ExternalLibrary;
import 'package:integration_test/integration_test.dart';
import 'package:path_provider/path_provider.dart';
import 'package:sailry_bridge/bridge.dart';
import 'package:shared_preferences_platform_interface/in_memory_shared_preferences_async.dart';
import 'package:shared_preferences_platform_interface/shared_preferences_async_platform_interface.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/features/conversations/live/page.dart';
import 'package:sailry_mobile/features/conversations/live/tool_heading.dart';
import 'package:sailry_mobile/features/conversations/live/disclosure.dart';
import 'package:sailry_mobile/features/resources/projects_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/json.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/kit.dart';

import 'unassigned.dart';
import 'input.dart';
import '../test/support/terminal_grid.dart';

const _fixture = String.fromEnvironment('SAILRY_FLUTTER_FIXTURE');
const _project = String.fromEnvironment('SAILRY_PROJECT_PATH');
const _profile = String.fromEnvironment('SAILRY_CONTROLLER_PROFILE');
const _run = String.fromEnvironment('SAILRY_FLUTTER_RUN');
const _library = String.fromEnvironment('SAILRY_BRIDGE_LIBRARY');
const _nativeIme = bool.fromEnvironment('SAILRY_ANDROID_IME_ACCEPTANCE');
const _prompt = 'Read source.txt and report its contents';
const _content = 'Flutter reads this file 中文 🙂';

void main({bool host = false}) {
  WidgetController.hitTestWarningShouldBeFatal = true;
  final binding = IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  if (host) binding.testTextInput.register();
  gridTests();
  testWidgets(
    'remote conversation creation and resume',
    (tester) async {
      if (host) {
        // flutter-tester has no platform preference plugin. Only these local
        // preferences are in memory; Controller, Link and Node remain real.
        SharedPreferencesAsyncPlatform.instance =
            InMemorySharedPreferencesAsync.empty();
        await initializeBridge(library: ExternalLibrary.open(_library));
      }
      final http = HttpClient();
      final unassignedResponse = await (await http.getUrl(
        Uri.parse('$_fixture/unassigned'),
      )).close();
      expect(unassignedResponse.statusCode, 200);
      final unassignedInvitation = text(
        object(
          jsonDecode(await utf8.decoder.bind(unassignedResponse).join()),
        )['invitation'],
      );
      final baseProfile = host
          ? _profile
          : '${(await getApplicationSupportDirectory()).path}/acceptance-$_run';
      final internet = !host && Platform.isAndroid;
      await conversationWithoutProject(
        tester,
        invitation: unassignedInvitation,
        profile: '$baseProfile-unassigned',
        internet: internet,
        nativeIme: _nativeIme,
        nativeInput: () async {
          final response = await (await http.getUrl(
            Uri.parse('$_fixture/ime'),
          )).close();
          expect(response.statusCode, 200);
          await response.drain<void>();
        },
        until: until,
      );
      final response = await (await http.getUrl(Uri.parse(_fixture))).close();
      expect(response.statusCode, 200);
      final invitation =
          object(
                jsonDecode(await utf8.decoder.bind(response).join()),
              )['invitation']
              as String;
      http.close();
      final profile = baseProfile;
      var app = AppSession();
      addTearDown(() => app.close());
      // Android's emulator has its own network namespace. Exercise the real
      // application's network binding while execution stays on the fixture Node.
      await app.start(path: profile, internet: internet);
      expect(app.error, isNull, reason: 'isolated native controller startup');
      expect(app.ready, isTrue, reason: 'native shared controller initializes');
      await app.pair(invitation);
      await tester.pumpWidget(SailryApp(session: app));
      await until(tester, () => app.hosts.single.connected);
      final node = app.hosts.single;
      expect(objects(node.snapshot['projects']), isEmpty);
      expect(objects(node.snapshot['sessions']), isEmpty);

      final navigation = find.byType(FloatingNavigation);
      await tester.tap(
        find.descendant(of: navigation, matching: find.text(tr('hosts'))),
      );
      await tester.pump();
      final projects = find.byKey(const ValueKey('host-projects'));
      await tester.ensureVisible(projects);
      await tester.tap(projects);
      await tester.pumpAndSettle();
      expect(find.byType(ProjectsPage), findsOneWidget);
      await tester.tap(find.byTooltip(tr('hostRegisterProject')));
      await tester.pumpAndSettle();
      await edit(
        tester,
        find.widgetWithText(TextField, tr('hostProjectName')),
        'Flutter fixture',
      );
      await edit(
        tester,
        find.widgetWithText(TextField, tr('hostProjectPath')),
        _project,
      );
      await tester.tap(find.widgetWithText(FilledButton, tr('save')));
      await until(tester, () => objects(node.snapshot['projects']).length == 1);
      await tester.pumpAndSettle();
      expect(objects(node.snapshot['projects']).single['path'], _project);
      await tester.tap(find.byTooltip(tr('back')));
      await tester.pumpAndSettle();

      await tester.tap(
        find.descendant(of: navigation, matching: find.text(tr('chat'))),
      );
      await tester.pump();
      await tester.tap(find.byTooltip(tr('newConversation')));
      await tester.pumpAndSettle();
      expect(objects(node.snapshot['sessions']), isEmpty);
      await edit(tester, find.byType(TextField), _prompt);
      await tester.pumpAndSettle();
      await tester.tap(find.byTooltip(tr('send')));
      await until(
        tester,
        () => find.byType(LiveConversationPage).evaluate().isNotEmpty,
      );
      await until(
        tester,
        () =>
            find.text('answer-flutter-fixture').evaluate().isNotEmpty &&
            object(
                  object(
                    objects(node.snapshot['sessions']).firstOrNull?['activity'],
                  )['run'],
                )['status'] ==
                'completed',
      );
      expect(objects(node.snapshot['sessions']), hasLength(1));
      expect(find.text(_prompt), findsWidgets);
      if (find.byType(ToolHeading).evaluate().isEmpty) {
        await tester.tap(
          find.byWidgetPredicate(
            (widget) => widget is WorkDisclosure && widget.framed,
          ),
        );
        await tester.pumpAndSettle();
      }
      final heading = tester.widget<ToolHeading>(find.byType(ToolHeading));
      final resolved = object(heading.call['resolved']);
      expect(object(resolved['label'])['label'], 'Read file');
      expect(object(resolved['label'])['locales'], {'zh-CN': '读取文件'});
      expect(
        object(object(resolved['input'])['summary'])['text'],
        'source.txt',
      );
      expect(object(resolved['output'])['preview'], _content);
      expect(object(resolved['output'])['paths'], ['source.txt']);
      expect(toolPart(heading.page, heading.call['source'])['arguments'], {
        'path': 'source.txt',
      });
      final label = find.text('读取文件 source.txt');
      expect(label, findsOneWidget);
      await tester.tap(label);
      await tester.pumpAndSettle();
      expect(find.textContaining(_content), findsOneWidget);
      expect(tester.takeException(), isNull);

      // Reopen the same trusted controller and canonical conversation, with no
      // new invitation, model request, input submission, or execution replay.
      await tester.pumpWidget(const SizedBox());
      await app.close();
      app.dispose();
      app = AppSession();
      await app.start(path: profile, internet: internet);
      await tester.pumpWidget(SailryApp(session: app));
      await until(
        tester,
        () =>
            app.hosts.length == 1 &&
            app.hosts.single.connected &&
            objects(app.hosts.single.snapshot['sessions']).length == 1,
      );
      await tester.tap(find.text(_prompt));
      await until(
        tester,
        () => find.text('answer-flutter-fixture').evaluate().isNotEmpty,
      );
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
      await app.close();
      app.dispose();
    },
    skip: _fixture.isEmpty,
    timeout: const Timeout(Duration(minutes: 5)),
  );
}

Future<void> until(WidgetTester tester, bool Function() ready) async {
  final deadline = DateTime.now().add(const Duration(seconds: 45));
  while (!ready()) {
    if (DateTime.now().isAfter(deadline)) {
      fail('Flutter Node observation deadline');
    }
    await tester.pump(const Duration(milliseconds: 100));
  }
  await tester.pump();
}
