import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/runtime/background.dart';

import 'runtime_test.dart' show Preferences;

void main() {
  testWidgets('one controller until background opt-out', (tester) async {
    const channel = MethodChannel('flutter_foreground_task/methods');
    var running = false;
    var starts = 0;
    var stops = 0;
    Map<dynamic, dynamic>? options;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(channel, (
      call,
    ) async {
      switch (call.method) {
        case 'isRunningService':
          return running;
        case 'checkNotificationPermission':
          return 0;
        case 'startService':
          starts++;
          options = call.arguments as Map;
          running = true;
          return null;
        case 'stopService':
          stops++;
          running = false;
          return null;
        default:
          throw StateError('Unexpected platform call: ${call.method}');
      }
    });
    addTearDown(
      () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        channel,
        null,
      ),
    );
    final preferences = Preferences();
    final background = BackgroundConnection(
      supported: true,
      preferences: preferences,
    );
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await background.initialize();
    expect(starts, 0);
    background.update(hasHosts: true);
    await tester.pumpAndSettle();
    expect(starts, 1);
    expect(background.running, isTrue);
    expect(options!['callbackHandle'], isNull);
    expect(options!.containsKey('stopWithTask'), isFalse);
    expect(options!['autoRunOnBoot'], isFalse);
    for (final state in [
      AppLifecycleState.inactive,
      AppLifecycleState.paused,
      AppLifecycleState.resumed,
    ]) {
      tester.binding.handleAppLifecycleStateChanged(state);
      await tester.pumpAndSettle();
    }
    expect(starts, 1);
    expect(stops, 0);
    await background.setEnabled(false);
    expect(preferences.enabled, isFalse);
    expect(stops, 1);
    expect(background.running, isFalse);
    await background.setEnabled(true);
    expect(starts, 2);
    await background.close();
    expect(stops, 2);
    background.dispose();
  });

  testWidgets('foreground startup and visible platform failures', (
    tester,
  ) async {
    const channel = MethodChannel('flutter_foreground_task/methods');
    var starts = 0;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(channel, (
      call,
    ) async {
      if (call.method == 'isRunningService') return false;
      if (call.method == 'checkNotificationPermission') return 0;
      if (call.method == 'startService') {
        starts++;
        throw PlatformException(code: 'service_denied');
      }
      throw StateError('Unexpected platform call: ${call.method}');
    });
    addTearDown(
      () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        channel,
        null,
      ),
    );
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    final background = BackgroundConnection(
      supported: true,
      preferences: Preferences(),
    );
    background.update(hasHosts: true);
    await background.initialize();
    expect(starts, 0);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pumpAndSettle();
    expect(starts, 1);
    expect(background.running, isFalse);
    expect(background.error, isA<PlatformException>());
    await background.close();
    background.dispose();
  });
}
