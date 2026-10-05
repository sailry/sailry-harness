import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fluttertoast/fluttertoast.dart';
import 'package:http/http.dart' as http;
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/features/updates/presentation.dart';
import 'package:sailry_mobile/features/updates/service.dart';
import 'package:sailry_mobile/l10n/language.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';
import 'package:url_launcher_platform_interface/url_launcher_platform_interface.dart';
import 'package:url_launcher_platform_interface/link.dart';

import 'support/updates.dart';

class Launcher extends UrlLauncherPlatform {
  final urls = <String>[];
  bool succeeds = true;

  @override
  LinkDelegate? get linkDelegate => null;

  @override
  Future<bool> launchUrl(String url, LaunchOptions options) async {
    expect(options.mode, PreferredLaunchMode.externalApplication);
    urls.add(url);
    return succeeds;
  }
}

Future<void> mount(
  WidgetTester tester,
  AppUpdates service, {
  bool automatic = false,
  Brightness brightness = Brightness.light,
  AppLanguage language = AppLanguage.english,
}) async {
  tester.view.physicalSize = const Size(320, 740);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  Widget page = Scaffold(body: UpdateSettings(updates: service));
  if (automatic) page = UpdateDelivery(updates: service, child: page);
  await tester.pumpWidget(
    MaterialApp(
      theme: SailryTheme.of(brightness),
      locale: language.locale,
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      home: page,
    ),
  );
  await tester.pumpAndSettle();
}

Future<void> clear(WidgetTester tester) async {
  FToast().removeQueuedCustomToasts();
  await tester.pumpWidget(const SizedBox.shrink());
  await tester.pump();
}

void main() {
  late Launcher launcher;
  late UrlLauncherPlatform previous;
  setUp(() {
    final binding = TestWidgetsFlutterBinding.ensureInitialized();
    binding.platformDispatcher.accessibilityFeaturesTestValue =
        const FakeAccessibilityFeatures(disableAnimations: true);
    previous = UrlLauncherPlatform.instance;
    launcher = Launcher();
    UrlLauncherPlatform.instance = launcher;
  });
  tearDown(() {
    TestWidgetsFlutterBinding.ensureInitialized().platformDispatcher
        .clearAccessibilityFeaturesTestValue();
    FToast().removeQueuedCustomToasts();
    UrlLauncherPlatform.instance = previous;
  });

  for (final brightness in Brightness.values) {
    for (final language in AppLanguage.values) {
      testWidgets('manual update in ${brightness.name}/${language.name}', (
        tester,
      ) async {
        final service = updates(releases: [release('0.1.0-alpha.2')]);
        addTearDown(service.dispose);
        await mount(
          tester,
          service,
          brightness: brightness,
          language: language,
        );
        await tester.tap(find.byKey(const ValueKey('check-updates')));
        await tester.pumpAndSettle();
        final toast = find.byKey(const ValueKey('update-toast'));
        expect(toast, findsOneWidget);
        expect(find.text('0.1.0-alpha.1'), findsOneWidget);
        final action = find.descendant(
          of: toast,
          matching: find.byType(TextButton),
        );
        await tester.tap(action);
        await tester.pumpAndSettle();
        expect(launcher.urls.single, service.available!.download.toString());
        expect(toast, findsNothing);
        expect(find.byKey(const ValueKey('download-update')), findsOneWidget);
        expect(tester.takeException(), isNull);
        await clear(tester);
      });
    }
  }

  testWidgets('startup announces a new version once', (tester) async {
    var requests = 0;
    final service = updates(
      request: (_) async {
        requests++;
        return http.Response(jsonEncode([release('0.1.0-alpha.2')]), 200);
      },
    );
    addTearDown(service.dispose);
    await mount(tester, service, automatic: true);
    expect(find.byKey(const ValueKey('update-toast')), findsOneWidget);
    await tester.pump(const Duration(seconds: 11));
    await tester.pumpAndSettle();
    service.notifyListeners();
    await tester.pumpAndSettle();
    expect(requests, 1);
    expect(find.byKey(const ValueKey('update-toast')), findsNothing);
    await clear(tester);
  });

  for (final (name, records, status, key) in [
    ('current', [release('0.1.0-alpha.1')], 200, 'updatesCurrent'),
    ('unpublished', <Map<String, dynamic>>[], 200, 'updatesUnpublished'),
    ('failure', <Map<String, dynamic>>[], 503, 'updatesCheckFailed'),
  ]) {
    testWidgets('$name is quiet at startup and uses a manual toast', (
      tester,
    ) async {
      final service = status == 200
          ? updates(releases: records)
          : updates(request: (_) async => http.Response('', status));
      addTearDown(service.dispose);
      await mount(tester, service, automatic: true);
      expect(find.byKey(const ValueKey('app-toast')), findsNothing);
      await tester.tap(find.byKey(const ValueKey('check-updates')));
      await tester.pumpAndSettle();
      final toast = find.byKey(const ValueKey('app-toast'));
      final context = tester.element(find.byType(UpdateSettings));
      expect(find.text(context.tr(key)), findsOneWidget);
      expect(
        find.descendant(of: toast, matching: find.text(context.tr(key))),
        findsOneWidget,
      );
      expect(find.byType(SnackBar), findsNothing);
      await clear(tester);
    });
  }

  testWidgets('pending check disables repeated admission', (tester) async {
    final response = Completer<http.Response>();
    var requests = 0;
    final service = updates(
      request: (_) {
        requests++;
        return response.future;
      },
    );
    addTearDown(service.dispose);
    await mount(tester, service);
    final row = find.byKey(const ValueKey('check-updates'));
    await tester.tap(row);
    await tester.pump();
    expect(tester.widget<ListTile>(row).enabled, isFalse);
    expect(tester.widget<ListTile>(row).onTap, isNull);
    await tester.tap(row);
    await tester.pump();
    expect(requests, 1);
    response.complete(http.Response('[]', 200));
    await tester.pumpAndSettle();
    expect(tester.widget<ListTile>(row).enabled, isTrue);
    await clear(tester);
  });

  testWidgets('download failure is a toast and retains the action', (
    tester,
  ) async {
    final service = updates(releases: [release('0.1.0-alpha.2')]);
    addTearDown(service.dispose);
    await service.check();
    launcher.succeeds = false;
    await mount(tester, service);
    await tester.tap(find.byKey(const ValueKey('download-update')));
    await tester.pumpAndSettle();
    final context = tester.element(find.byType(UpdateSettings));
    expect(find.text(context.tr('updatesOpenFailed')), findsOneWidget);
    expect(find.byKey(const ValueKey('download-update')), findsOneWidget);
    expect(launcher.urls.length, 1);
    await clear(tester);
  });

  testWidgets('preview performs no checks even with an injected service', (
    tester,
  ) async {
    var requests = 0;
    final service = updates(
      request: (_) async {
        requests++;
        return http.Response('[]', 200);
      },
    );
    addTearDown(service.dispose);
    await tester.pumpWidget(SailryApp(preview: true, updates: service));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('tab-3')));
    await tester.pumpAndSettle();
    final row = tester.widget<ListTile>(
      find.byKey(const ValueKey('check-updates')),
    );
    expect(row.enabled, isFalse);
    expect(requests, 0);
    expect(service.version, isNull);
    await clear(tester);
  });

  testWidgets('app checks and downloads without a paired Node', (tester) async {
    final session = AppSession.test();
    final service = updates(releases: [release('0.1.0-alpha.2')]);
    addTearDown(session.dispose);
    addTearDown(service.dispose);
    await tester.pumpWidget(SailryApp(session: session, updates: service));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('update-toast')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('tab-3')));
    await tester.pumpAndSettle();
    await tester.pump(const Duration(seconds: 11));
    await tester.pumpAndSettle();
    final action = find.byKey(const ValueKey('download-update'));
    await tester.ensureVisible(action);
    await tester.tap(action);
    await tester.pumpAndSettle();
    expect(launcher.urls.single, service.available!.download.toString());
    expect(session.hosts, isEmpty);
    expect(tester.takeException(), isNull);
    await clear(tester);
  });
}
