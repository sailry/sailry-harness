import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:package_info_plus/package_info_plus.dart';
import 'package:sailry_mobile/features/updates/presentation.dart';
import 'package:sailry_mobile/features/updates/service.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/ui/theme.dart';

const _version = String.fromEnvironment('SAILRY_APP_VERSION');
const _published = bool.fromEnvironment('SAILRY_RELEASE_PUBLISHED');

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('native version and live release feedback', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.light),
        home: const Scaffold(body: CircularProgressIndicator()),
      ),
    );
    await tester.pump();
    expect(
      _version,
      isNotEmpty,
      reason: 'Supply the expected application version',
    );
    final info = await PackageInfo.fromPlatform();
    expect(info.packageName, 'com.sailry.sailry_mobile.acceptance');
    expect(info.version, _version);

    // Only distribution metadata is accessed, never a Node or controller profile.
    final updates = AppUpdates();
    addTearDown(updates.dispose);
    expect(
      await updates.check(),
      _published ? UpdateResult.current : UpdateResult.unpublished,
    );
    expect(updates.version, _version);
    expect(updates.available, isNull);
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.light),
        locale: const Locale('en'),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        home: Scaffold(
          body: ListView(children: [UpdateSettings(updates: updates)]),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text(_version), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('check-updates')));
    final deadline = DateTime.now().add(const Duration(seconds: 30));
    while (updates.checking) {
      if (DateTime.now().isAfter(deadline)) fail('Release feedback timed out');
      await tester.pump(const Duration(milliseconds: 100));
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    await tester.pump();
    final context = tester.element(find.byType(UpdateSettings));
    final message = context.tr(
      _published ? 'updatesCurrent' : 'updatesUnpublished',
    );
    expect(find.text(message), findsOneWidget);
    expect(find.text(_version), findsOneWidget);
    expect(find.byKey(const ValueKey('download-update')), findsNothing);
    await tester.pump(const Duration(seconds: 5));
    expect(find.text(message), findsNothing);
    expect(find.text(_version), findsOneWidget);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
